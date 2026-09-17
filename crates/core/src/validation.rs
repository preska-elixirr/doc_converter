//! Local, bounded veraPDF execution. Never infer conformance from XMP alone.
use crate::{check_cancel, message, OutputFormat, Result};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct Validator {
    pub home: PathBuf,
    pub java: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidationReport {
    pub profile: String,
    pub passed: bool,
    pub human_review_required: bool,
    pub issues: Vec<String>,
}

impl Validator {
    /// Detect trusted administrator/developer configuration; never supplied by IPC.
    pub fn detect() -> Option<Self> {
        let mut roots = Vec::new();
        if let Some(home) = std::env::var_os("DOC_CONVERTER_VERAPDF") {
            roots.push(PathBuf::from(home));
        }
        if let Ok(exe) = std::env::current_exe() {
            for dir in exe.ancestors().skip(1).take(6) {
                roots.push(dir.join("engines/verapdf"));
                roots.push(dir.join(".tools/verapdf"));
            }
        }
        for home in roots {
            let has_cli = std::fs::read_dir(home.join("bin"))
                .ok()
                .is_some_and(|entries| {
                    entries.flatten().any(|e| {
                        e.file_name().to_string_lossy().starts_with("cli-")
                            && e.path().extension().is_some_and(|x| x == "jar")
                    })
                });
            if !has_cli {
                continue;
            }
            let executable = if cfg!(windows) { "java.exe" } else { "java" };
            let mut candidates = Vec::new();
            if let Some(java) = std::env::var_os("DOC_CONVERTER_JAVA") {
                candidates.push(PathBuf::from(java));
            }
            candidates.push(home.join("jre/bin").join(executable));
            if let Some(root) = home.parent() {
                if let Ok(entries) = std::fs::read_dir(root.join("pdf-standards/java")) {
                    candidates.extend(
                        entries
                            .flatten()
                            .map(|e| e.path().join("bin").join(executable)),
                    );
                }
            }
            if let Some(root) = std::env::var_os("JAVA_HOME") {
                candidates.push(PathBuf::from(root).join("bin").join(executable));
            }
            if let Some(path) = std::env::var_os("PATH") {
                candidates.extend(std::env::split_paths(&path).map(|p| p.join(executable)));
            }
            if let Some(java) = candidates.into_iter().find(|p| p.is_file()) {
                return Some(Self { home, java });
            }
        }
        None
    }

    pub fn validate(
        &self,
        pdf: &Path,
        format: OutputFormat,
        cancel: &AtomicBool,
    ) -> Result<ValidationReport> {
        check_cancel(cancel)?;
        let flavour = format
            .validation_flavour()
            .ok_or_else(|| message("Select a PDF/A or PDF/UA profile."))?;
        let report_file = tempfile::NamedTempFile::new()?;
        let mut cmd = Command::new(&self.java);
        cmd.arg("-Xmx512m")
            .arg("-Djava.awt.headless=true")
            .arg("-Dfile.encoding=UTF8")
            .arg("-cp")
            .arg(self.home.join("bin/*"))
            .arg("org.verapdf.apps.GreenfieldCliWrapper")
            .args([
                "--format",
                "xml",
                "--flavour",
                flavour,
                "--maxfailuresdisplayed",
                "5",
            ])
            .arg(pdf.canonicalize()?)
            .stdin(Stdio::null())
            .stdout(report_file.reopen()?)
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| message(format!("Could not start local veraPDF: {e}")))?;
        #[cfg(windows)]
        let job = crate::office::job::JobObject::new(&child);
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            let oversized = report_file.as_file().metadata()?.len() > 8 * 1024 * 1024;
            if check_cancel(cancel).is_err()
                || started.elapsed() > Duration::from_secs(120)
                || oversized
            {
                #[cfg(windows)]
                if let Some(job) = &job {
                    job.kill();
                }
                let _ = child.kill();
                let _ = child.wait();
                check_cancel(cancel)?;
                return Err(message(
                    "Local PDF validation exceeded its time or report-size limit.",
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        check_cancel(cancel)?;
        if !status.success() && status.code() != Some(1) {
            return Err(message(format!(
                "Local veraPDF exited with {status}; conformance was not confirmed."
            )));
        }
        if report_file.as_file().metadata()?.len() > 8 * 1024 * 1024 {
            return Err(message("Validation report is too large."));
        }
        let report = parse_report(&std::fs::read_to_string(report_file.path())?, format)?;
        // veraPDF uses exit 1 for a completed, non-compliant validation too.
        // Never promote a nonzero exit to a successful validation.
        if !status.success() && report.passed {
            return Err(message("veraPDF exit status contradicts its report."));
        }
        Ok(report)
    }
}

fn parse_report(xml: &str, format: OutputFormat) -> Result<ValidationReport> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut reports = Vec::new();
    let mut issues = Vec::new();
    let mut summary_ok = false;
    let mut failed_rule = false;
    loop {
        match reader.read_event().map_err(message)? {
            Event::Start(e) | Event::Empty(e) => {
                let attrs = e
                    .attributes()
                    .map(|a| {
                        a.map(|a| {
                            (
                                a.key.as_ref().to_string(),
                                a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                    .map(|v| v.into_owned()),
                            )
                        })
                    })
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(message)?;
                let attr = |name: &str| {
                    attrs
                        .iter()
                        .find(|(k, _)| k == name)
                        .and_then(|(_, v)| v.as_ref().ok())
                        .map(String::as_str)
                };
                match e.local_name().as_ref() {
                    "validationReport" => {
                        let profile = attr("profileName").unwrap_or("");
                        if !profile.starts_with(&format!("{} ", format.label())) {
                            return Err(message("Validator reported an unexpected PDF profile."));
                        }
                        reports.push(match attr("isCompliant") {
                            Some("true") => true,
                            Some("false") => false,
                            _ => return Err(message("Missing validation result.")),
                        });
                    }
                    "batchSummary" => {
                        summary_ok = attr("totalJobs") == Some("1")
                            && [
                                "failedToParse",
                                "encrypted",
                                "outOfMemory",
                                "veraExceptions",
                            ]
                            .iter()
                            .all(|k| attr(k) == Some("0"));
                    }
                    "rule" => {
                        failed_rule = attr("status") == Some("failed");
                    }
                    "description" if failed_rule && issues.len() < 20 => {
                        let value = reader.read_text(e.name()).map_err(message)?;
                        issues.push(value.chars().take(500).collect());
                    }
                    _ => {}
                }
            }
            Event::End(e) if e.local_name().as_ref() == "rule" => failed_rule = false,
            Event::Eof => break,
            _ => {}
        }
    }
    if reports.len() != 1 || !summary_ok {
        return Err(message(
            "Incomplete veraPDF report; conformance was not confirmed.",
        ));
    }
    Ok(ValidationReport {
        profile: format.label().into(),
        passed: reports[0],
        human_review_required: format == OutputFormat::Pdfua1,
        issues,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validation_reports_fail_closed() {
        let report = |profile: &str, passed: &str, failed: &str| {
            format!(
                r#"<report><validationReport profileName="{profile} validation profile" isCompliant="{passed}"><details><rule status="failed"><description>Missing alternative text</description></rule></details></validationReport><batchSummary totalJobs="1" failedToParse="{failed}" encrypted="0" outOfMemory="0" veraExceptions="0"/></report>"#
            )
        };
        let valid = report("PDF/A-2b", "true", "0");
        assert!(parse_report(&valid, OutputFormat::Pdfa2b).unwrap().passed);
        assert!(parse_report(&valid, OutputFormat::Pdfa3b).is_err());
        assert!(parse_report("<report/>", OutputFormat::Pdfa2b).is_err());
        assert!(parse_report(&report("PDF/A-2b", "true", "1"), OutputFormat::Pdfa2b).is_err());
        let ua = parse_report(&report("PDF/UA-1", "false", "0"), OutputFormat::Pdfua1).unwrap();
        assert!(!ua.passed);
        assert!(ua.human_review_required);
        assert_eq!(ua.issues, ["Missing alternative text"]);
    }
}
