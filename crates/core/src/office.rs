//! LibreOffice adapter. Runs `soffice` headless with a private user profile,
//! inside a Windows job object so cancellation kills the whole process tree.
//! Passwords never go through this adapter; PDF protection happens in `pdf`.

use crate::{message, Result, CANCELLED};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize)]
pub struct OfficeEngine {
    pub path: PathBuf,
    pub version: String,
    /// LibreOffice has the Markdown import/export filter (25.8 and later).
    pub markdown: bool,
    #[serde(skip)]
    pub profile: PathBuf,
}

/// What LibreOffice should write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub ext: &'static str,
    /// `--convert-to` argument, for example `txt:Text (encoded):UTF8`.
    pub convert_to: &'static str,
}

impl Target {
    pub const PDF: Target = Target {
        ext: "pdf",
        convert_to: "pdf",
    };
    pub const DOCX: Target = Target {
        ext: "docx",
        convert_to: "docx",
    };
    pub const TXT: Target = Target {
        ext: "txt",
        convert_to: "txt:Text (encoded):UTF8",
    };
    /// Writer HTML with images embedded as data URIs, so one file holds everything.
    pub const HTML: Target = Target {
        ext: "html",
        convert_to: "html:HTML (StarWriter):EmbedImages",
    };
    /// Calc HTML export; the job pipeline embeds its image sidecars afterwards.
    pub const HTML_CALC: Target = Target {
        ext: "html",
        convert_to: "html",
    };
    pub const MD: Target = Target {
        ext: "md",
        convert_to: "md:Markdown",
    };
}

fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut url = String::from("file:///");
    for byte in text.trim_start_matches('/').bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'.' | b'_' | b'-' => {
                url.push(byte as char)
            }
            _ => url.push_str(&format!("%{byte:02X}")),
        }
    }
    url
}

const PROFILE_XCU: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<oor:items xmlns:oor="http://openoffice.org/2001/registry" xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
<item oor:path="/org.openoffice.Office.Common/Security/Scripting"><prop oor:name="MacroSecurityLevel" oor:op="fuse"><value>3</value></prop></item>
<item oor:path="/org.openoffice.Office.Jobs/Jobs/org.openoffice.Office.Jobs:Job['UpdateCheck']/Arguments"><prop oor:name="AutoCheckEnabled" oor:op="fuse"><value>false</value></prop></item>
<item oor:path="/org.openoffice.Office.Common/Misc"><prop oor:name="ShowTipOfTheDay" oor:op="fuse"><value>false</value></prop></item>
<item oor:path="/org.openoffice.Office.Common/Misc"><prop oor:name="CrashReport" oor:op="fuse"><value>false</value></prop></item>
</oor:items>
"#;

impl OfficeEngine {
    /// Places to look, most specific first.
    pub fn candidates() -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(env) = std::env::var_os("DOC_CONVERTER_SOFFICE") {
            out.push(PathBuf::from(env));
        }
        if let Ok(exe) = std::env::current_exe() {
            let mut dir = exe.parent().map(Path::to_path_buf);
            let mut hops = 0;
            while let Some(d) = dir {
                out.push(d.join("engines/libreoffice/program/soffice.exe"));
                out.push(d.join(".tools/libreoffice/program/soffice.exe"));
                dir = d.parent().map(Path::to_path_buf);
                hops += 1;
                if hops > 4 {
                    break;
                }
            }
        }
        for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
            if let Some(root) = std::env::var_os(var) {
                out.push(PathBuf::from(root).join("LibreOffice/program/soffice.exe"));
            }
        }
        if let Some(path) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path) {
                out.push(dir.join("soffice.exe"));
                out.push(dir.join("soffice"));
            }
        }
        out
    }

    /// Finds an installed LibreOffice without starting it.
    pub fn detect(profile: &Path) -> Option<OfficeEngine> {
        let path = Self::candidates().into_iter().find(|p| p.is_file())?;
        let program = path.parent()?;
        let version = std::fs::read_to_string(program.join("bootstrap.ini"))
            .ok()
            .and_then(|ini| {
                ini.lines()
                    .find_map(|l| l.strip_prefix("ProductKey=").map(|v| v.trim().to_string()))
            })
            .unwrap_or_else(|| "LibreOffice".to_string());
        let markdown = std::fs::read_to_string(program.join("../share/registry/writer.xcd"))
            .map(|xcd| {
                xcd.find("<node oor:name=\"Markdown\"")
                    .map(|pos| {
                        xcd[pos..]
                            .get(..2000)
                            .unwrap_or("")
                            .contains("IMPORT EXPORT")
                    })
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        Some(OfficeEngine {
            path,
            version,
            markdown,
            profile: profile.to_path_buf(),
        })
    }

    fn prepare_profile(&self) -> Result<()> {
        let user = self.profile.join("user");
        std::fs::create_dir_all(&user)?;
        let xcu = user.join("registrymodifications.xcu");
        if !xcu.exists() {
            std::fs::write(&xcu, PROFILE_XCU)?;
        }
        // A stale lock from a killed instance would make soffice exit silently.
        let _ = std::fs::remove_file(self.profile.join(".lock"));
        Ok(())
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.path);
        cmd.arg("--headless")
            .arg("--norestore")
            .arg("--nologo")
            .arg("--nodefault")
            .arg("--nolockcheck")
            .arg("--nofirststartwizard")
            .arg(format!("-env:UserInstallation={}", file_url(&self.profile)))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd
    }

    /// Creates the profile so the first real conversion is not slow.
    pub fn warm_up(&self) -> Result<()> {
        self.prepare_profile()?;
        let mut cmd = self.command();
        cmd.arg("--version");
        let child = cmd
            .spawn()
            .map_err(|e| message(format!("Could not start LibreOffice: {e}")))?;
        wait(child, Duration::from_secs(180), &AtomicBool::new(false))?;
        Ok(())
    }

    /// Converts `inputs` and returns the produced files in the same order.
    /// `outdir` must be empty and input stems must be distinct.
    pub fn convert(
        &self,
        inputs: &[PathBuf],
        target: Target,
        infilter: Option<&str>,
        outdir: &Path,
        cancel: &AtomicBool,
    ) -> Result<Vec<PathBuf>> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        self.prepare_profile()?;
        std::fs::create_dir_all(outdir)?;
        let mut cmd = self.command();
        if let Some(filter) = infilter {
            cmd.arg(format!("--infilter={filter}"));
        }
        cmd.arg("--convert-to").arg(target.convert_to);
        cmd.arg("--outdir").arg(outdir);
        for input in inputs {
            cmd.arg(input);
        }
        let child = cmd
            .spawn()
            .map_err(|e| message(format!("Could not start LibreOffice: {e}")))?;
        let timeout =
            Duration::from_secs(120 + 30 * inputs.len() as u64).min(Duration::from_secs(900));
        let stderr = wait(child, timeout, cancel)?;
        let mut outputs = Vec::new();
        let mut missing = Vec::new();
        for input in inputs {
            let expected = outdir.join(format!("{}.{}", crate::stem(input), target.ext));
            if expected.is_file() && expected.metadata().map(|m| m.len() > 0).unwrap_or(false) {
                outputs.push(expected);
            } else {
                missing.push(
                    input
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                );
            }
        }
        if !missing.is_empty() {
            let detail = stderr
                .lines()
                .rev()
                .find(|l| l.contains("Error") || l.contains("error"))
                .map(|l| format!(" ({})", l.trim()))
                .unwrap_or_default();
            return Err(message(format!(
                "LibreOffice could not convert {}{detail}",
                missing.join(", ")
            )));
        }
        Ok(outputs)
    }
}

/// Waits for the child, polling the cancel flag. Returns captured stderr.
fn wait(mut child: Child, timeout: Duration, cancel: &AtomicBool) -> Result<String> {
    #[cfg(windows)]
    let job = job::JobObject::new(&child);
    let stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut stderr) = stderr {
            use std::io::Read;
            let _ = stderr.read_to_string(&mut text);
        }
        text
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if cancel.load(Ordering::Relaxed) || started.elapsed() > timeout {
            #[cfg(windows)]
            if let Some(job) = &job {
                job.kill();
            }
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let text = reader.join().unwrap_or_default();
    match status {
        Some(status) if status.success() => Ok(text),
        Some(status) => Err(message(format!(
            "LibreOffice exited with {status}. {}",
            text.lines().last().unwrap_or("").trim()
        ))),
        None if cancel.load(Ordering::Relaxed) => Err(message(CANCELLED)),
        None => Err(message("LibreOffice took too long and was stopped.")),
    }
}

#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
    };

    /// Owns every process LibreOffice spawns; closing the handle kills them all.
    pub struct JobObject(HANDLE);

    impl JobObject {
        pub fn new(child: &Child) -> Option<Self> {
            // SAFETY: plain Win32 calls with valid pointers; the handle is closed on drop.
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle.is_null() {
                    return None;
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const std::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if AssignProcessToJobObject(handle, child.as_raw_handle() as HANDLE) == 0 {
                    CloseHandle(handle);
                    return None;
                }
                Some(JobObject(handle))
            }
        }
        pub fn kill(&self) {
            // SAFETY: the handle is a live job object owned by this struct.
            unsafe {
                TerminateJobObject(self.0, 1);
            }
        }
    }

    impl Drop for JobObject {
        fn drop(&mut self) {
            // SAFETY: closes the handle this struct owns.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_url_escapes_spaces() {
        let url = file_url(Path::new(
            r"C:\Users\Some One\AppData\Local\Doc Converter\lo",
        ));
        assert_eq!(
            url,
            "file:///C:/Users/Some%20One/AppData/Local/Doc%20Converter/lo"
        );
    }

    /// Runs only when LibreOffice is present; see BUILD_AND_VERIFICATION.md.
    #[test]
    fn converts_text_to_pdf_when_available() {
        let dir = tempfile::tempdir().unwrap();
        let Some(engine) = OfficeEngine::detect(&dir.path().join("profile")) else {
            eprintln!("LibreOffice not found; skipping");
            return;
        };
        let input = dir.path().join("sample.txt");
        std::fs::write(&input, "Hello from Doc Converter\n\nSecond paragraph.\n").unwrap();
        let out = dir.path().join("out");
        let produced = engine
            .convert(
                &[input.clone()],
                Target::PDF,
                None,
                &out,
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(produced.len(), 1);
        assert!(std::fs::read(&produced[0]).unwrap().starts_with(b"%PDF"));
        let cancelled = engine.convert(
            &[input],
            Target::DOCX,
            None,
            &dir.path().join("cancelled"),
            &AtomicBool::new(true),
        );
        assert!(cancelled.is_err());
    }
}
