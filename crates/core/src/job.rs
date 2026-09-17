//! Batch execution: every item is prepared in a private temp directory, then
//! committed to the destination without overwriting. Progress is reported per
//! item so the UI can show a live status column.

use crate::{
    capability::{route, Engines, OutputFormat, Route},
    check_cancel, copy_file, docx, images, layout, message,
    office::Target,
    pdf, stem, text, unique_path, InputKind, Layout, Result, SecretString,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
use tempfile::TempDir;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Action {
    Convert { format: OutputFormat },
    Image { format: String },
    Protect,
    Unlock,
    Encrypt,
    Decrypt,
}

#[derive(Clone, Debug)]
pub struct Task {
    pub source: PathBuf,
    pub name: String,
    pub kind: InputKind,
    pub action: Action,
    /// Blocks or paragraphs of this document that start a new page.
    pub page_breaks: Vec<usize>,
}

impl Task {
    /// The batch layout with this document's own page breaks.
    pub fn layout(&self, batch: &Batch) -> Layout {
        Layout {
            page_breaks: self.page_breaks.clone(),
            ..batch.layout.clone()
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageSettings {
    pub max_edge: u32,
    pub quality: u8,
}

pub struct Batch {
    pub tasks: Vec<Task>,
    /// Combine every task into one PDF, in task order.
    pub merge: bool,
    pub layout: Layout,
    /// Open password for PDF outputs, `.age` passphrase, or the unlock password.
    pub password: Option<SecretString>,
    /// Convert mode: add `password` to every PDF output.
    pub protect: bool,
    pub image: ImageSettings,
}

#[derive(Clone, Debug)]
pub enum Destination {
    Folder(PathBuf),
    File(PathBuf),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Queued,
    Working,
    Done,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
pub struct ItemReport {
    pub index: usize,
    pub status: Status,
    pub detail: String,
    pub output: Option<String>,
}

/// Output file name for a task, used for save dialogs and folder outputs.
pub fn output_name(task: &Task, batch: &Batch) -> String {
    let stem = stem(&task.source);
    match &task.action {
        Action::Convert { format } => format!("{stem}.{}", format.ext()),
        Action::Image { format } => format!("{stem}.{format}"),
        Action::Protect => format!("{stem}-protected.pdf"),
        Action::Unlock => format!("{stem}-unlocked.pdf"),
        Action::Encrypt => format!("{}.age", task.name),
        Action::Decrypt => {
            let name = task.name.strip_suffix(".age").unwrap_or(&task.name);
            let _ = batch;
            format!("restored-{name}")
        }
    }
}

fn split_name(name: &str) -> (String, String) {
    match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && !e.is_empty() => (s.to_string(), e.to_string()),
        _ => (name.to_string(), String::new()),
    }
}

fn final_path(destination: &Destination, name: &str) -> PathBuf {
    match destination {
        Destination::File(path) => path.clone(),
        Destination::Folder(dir) => {
            let (stem, ext) = split_name(name);
            if ext.is_empty() {
                unique_path(dir, &stem, "")
                    .to_string_lossy()
                    .trim_end_matches('.')
                    .into()
            } else {
                unique_path(dir, &stem, &ext)
            }
        }
    }
}

fn read_text(source: &Path, kind: InputKind) -> Result<text::Doc> {
    let content = text::read_text_file(source)?;
    Ok(match kind {
        InputKind::Md => text::parse_markdown(&content),
        InputKind::Html => text::parse_markdown(&text::html_to_markdown(&content)?),
        _ => text::parse_text(&content),
    })
}

/// A DOCX copy with the layout applied, or the source when nothing changes.
fn docx_source(
    source: &Path,
    layout: &Layout,
    work: &Path,
    tag: &str,
    cancel: &AtomicBool,
) -> Result<PathBuf> {
    if layout.is_default() {
        return Ok(source.to_path_buf());
    }
    let edited = work.join(format!("{tag}-layout.docx"));
    docx::rewrite(source, &edited, layout, cancel)?;
    Ok(edited)
}

fn office_infilter(kind: InputKind, engines: &Engines) -> Option<&'static str> {
    match kind {
        InputKind::Html => Some("HTML (StarWriter)"),
        InputKind::Md if engines.office.as_ref().map(|o| o.markdown).unwrap_or(false) => {
            Some("Markdown")
        }
        _ => None,
    }
}

fn target_for(format: OutputFormat, kind: InputKind) -> Target {
    match format {
        OutputFormat::Pdf => Target::PDF,
        OutputFormat::Docx => Target::DOCX,
        OutputFormat::Txt => Target::TXT,
        OutputFormat::Html if kind == InputKind::Xlsx => Target::HTML_CALC,
        OutputFormat::Html => Target::HTML,
        OutputFormat::Md => Target::MD,
    }
}

fn run_office(
    engines: &Engines,
    input: &Path,
    kind: InputKind,
    target: Target,
    work: &Path,
    tag: &str,
    cancel: &AtomicBool,
) -> Result<PathBuf> {
    let office = engines
        .office
        .as_ref()
        .ok_or_else(|| message(crate::capability::NO_OFFICE))?;
    let outdir = work.join(format!("{tag}-office"));
    let produced = office.convert(
        &[input.to_path_buf()],
        target,
        office_infilter(kind, engines),
        &outdir,
        cancel,
    )?;
    produced
        .into_iter()
        .next()
        .ok_or_else(|| message("LibreOffice produced no output."))
}

/// Converts one document into `work` and returns the produced file.
#[allow(clippy::too_many_arguments)]
pub fn convert_document(
    source: &Path,
    kind: InputKind,
    format: OutputFormat,
    layout: &Layout,
    engines: &Engines,
    work: &Path,
    tag: &str,
    cancel: &AtomicBool,
) -> Result<PathBuf> {
    check_cancel(cancel)?;
    let title = stem(source);
    let out = work.join(format!("{tag}.{}", format.ext()));
    match route(kind, format, engines) {
        Route::Unavailable(reason) => Err(message(reason)),
        Route::Copy => {
            if kind == InputKind::Docx && !layout.is_default() {
                docx::rewrite(source, &out, layout, cancel)?;
            } else {
                copy_file(source, &out, cancel)?;
            }
            Ok(out)
        }
        Route::PdfText => {
            pdf::extract_text(source, &out, cancel)?;
            Ok(out)
        }
        Route::Office => {
            let input = if kind == InputKind::Docx {
                docx_source(source, layout, work, tag, cancel)?
            } else {
                source.to_path_buf()
            };
            let produced = run_office(
                engines,
                &input,
                kind,
                target_for(format, kind),
                work,
                tag,
                cancel,
            )?;
            if kind == InputKind::Xlsx && format == OutputFormat::Html {
                crate::html::embed_export_images(&produced, &out, cancel)?;
                Ok(out)
            } else {
                Ok(produced)
            }
        }
        Route::TextToPdf => {
            let doc = read_text(source, kind)?;
            let rendered = layout::render_pdf(&doc, layout)?;
            crate::write_bytes(&out, &rendered.pdf, cancel)?;
            Ok(out)
        }
        Route::TextToHtml => {
            let doc = read_text(source, kind)?;
            crate::write_bytes(&out, text::to_html(&doc, &title).as_bytes(), cancel)?;
            Ok(out)
        }
        Route::TextToTxt | Route::HtmlToTxtBuiltin => {
            let doc = read_text(source, kind)?;
            crate::write_bytes(&out, text::to_text(&doc).as_bytes(), cancel)?;
            Ok(out)
        }
        Route::HtmlToMd => {
            let html = text::read_text_file(source)?;
            crate::write_bytes(&out, text::html_to_markdown(&html)?.as_bytes(), cancel)?;
            Ok(out)
        }
        Route::HtmlToPdfBuiltin => {
            let doc = read_text(source, kind)?;
            let rendered = layout::render_pdf(&doc, layout)?;
            crate::write_bytes(&out, &rendered.pdf, cancel)?;
            Ok(out)
        }
        Route::OfficeToMdViaHtml => {
            let html = run_office(engines, source, kind, Target::HTML, work, tag, cancel)?;
            let content = text::read_text_file(&html)?;
            crate::write_bytes(&out, text::html_to_markdown(&content)?.as_bytes(), cancel)?;
            Ok(out)
        }
        Route::TextToOfficeViaHtml => {
            let doc = read_text(source, kind)?;
            let html = work.join(format!("{tag}-bridge.html"));
            crate::write_bytes(&html, text::to_html(&doc, &title).as_bytes(), cancel)?;
            run_office(
                engines,
                &html,
                InputKind::Html,
                target_for(format, InputKind::Html),
                work,
                tag,
                cancel,
            )
        }
    }
}

/// PDF bytes for the preview panel, produced through the same route as a real
/// conversion to `target`. A DOCX target from a text source is first written as
/// DOCX, then rendered to PDF, so the preview shows what will be saved.
pub fn preview_pdf(
    source: &Path,
    kind: InputKind,
    target: OutputFormat,
    layout: &Layout,
    engines: &Engines,
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    if kind.is_image() {
        let decoded = images::decode(source, cancel)?;
        return pdf::image_pdf(&[decoded], 80);
    }
    let work = work_dir()?;
    let produced = if target == OutputFormat::Docx && kind != InputKind::Docx {
        let docx = convert_document(
            source,
            kind,
            OutputFormat::Docx,
            layout,
            engines,
            work.path(),
            "preview-docx",
            cancel,
        )?;
        convert_document(
            &docx,
            InputKind::Docx,
            OutputFormat::Pdf,
            &Layout::default(),
            engines,
            work.path(),
            "preview",
            cancel,
        )?
    } else {
        convert_document(
            source,
            kind,
            OutputFormat::Pdf,
            layout,
            engines,
            work.path(),
            "preview",
            cancel,
        )?
    };
    Ok(std::fs::read(produced)?)
}

/// Blocks a user can move to the next page: DOCX body paragraphs or text blocks.
pub fn outline(source: &Path, kind: InputKind) -> Result<Vec<text::OutlineEntry>> {
    match kind {
        InputKind::Docx => docx::outline(source),
        InputKind::Md | InputKind::Txt | InputKind::Html => Ok(read_text(source, kind)?.outline()),
        _ => Ok(Vec::new()),
    }
}

const WORK_PREFIX: &str = "doc-converter-";
const LOCK_NAME: &str = ".in-use";

/// A private work folder that is removed when dropped. While it lives, a lock
/// file inside it is held open so another instance's cleanup leaves it alone.
pub struct WorkDir {
    // Declared first so the lock closes before the folder is removed.
    _lock: std::fs::File,
    dir: TempDir,
}

impl WorkDir {
    pub fn path(&self) -> &Path {
        self.dir.path()
    }
}

#[cfg(windows)]
fn hold_lock(dir: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .share_mode(0)
        .open(dir.join(LOCK_NAME))
}

#[cfg(not(windows))]
fn hold_lock(dir: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::create(dir.join(LOCK_NAME))
}

/// True when another running instance still holds the folder's lock.
fn in_use(dir: &Path) -> bool {
    let lock = dir.join(LOCK_NAME);
    if !lock.exists() {
        return false;
    }
    #[cfg(windows)]
    {
        std::fs::OpenOptions::new().read(true).open(&lock).is_err()
    }
    #[cfg(not(windows))]
    {
        lock.metadata()
            .and_then(|m| m.modified())
            .map(|t| t.elapsed().map(|e| e.as_secs() < 3600).unwrap_or(true))
            .unwrap_or(false)
    }
}

pub fn work_dir() -> Result<WorkDir> {
    let dir = tempfile::Builder::new()
        .prefix(WORK_PREFIX)
        .tempdir()
        .map_err(|e| message(format!("Could not create a temporary folder: {e}")))?;
    let lock = hold_lock(dir.path())
        .map_err(|e| message(format!("Could not lock the temporary folder: {e}")))?;
    Ok(WorkDir { _lock: lock, dir })
}

fn clean_stale_work_dirs_in(root: &Path) -> usize {
    let mut removed = 0;
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if name.starts_with(WORK_PREFIX)
                && path.is_dir()
                && !in_use(&path)
                && std::fs::remove_dir_all(&path).is_ok()
            {
                removed += 1;
            }
        }
    }
    removed
}

/// Removes work folders left by earlier runs that ended abnormally. Folders a
/// running instance still uses are skipped.
pub fn clean_stale_work_dirs() -> usize {
    clean_stale_work_dirs_in(&std::env::temp_dir())
}

fn run_item(
    task: &Task,
    index: usize,
    batch: &Batch,
    destination: &Destination,
    engines: &Engines,
    work: &Path,
    cancel: &AtomicBool,
) -> Result<PathBuf> {
    let tag = format!("item{index}");
    let target = final_path(destination, &output_name(task, batch));
    match &task.action {
        Action::Convert { format } => {
            let produced = convert_document(
                &task.source,
                task.kind,
                *format,
                &task.layout(batch),
                engines,
                work,
                &tag,
                cancel,
            )?;
            match (&batch.password, *format) {
                (Some(password), OutputFormat::Pdf) if batch.protect => {
                    pdf::protect(&produced, &target, password, cancel)?
                }
                _ => copy_file(&produced, &target, cancel)?,
            }
        }
        Action::Image { format } => images::convert_image(
            &task.source,
            &target,
            format,
            batch.image.max_edge,
            batch.image.quality,
            cancel,
        )?,
        Action::Protect => {
            let password = batch
                .password
                .as_ref()
                .ok_or_else(|| message("Enter a password."))?;
            pdf::protect(&task.source, &target, password, cancel)?
        }
        Action::Unlock => {
            let password = batch
                .password
                .as_ref()
                .ok_or_else(|| message("Enter the password."))?;
            pdf::unlock(&task.source, &target, password, cancel)?
        }
        Action::Encrypt => {
            let password = batch
                .password
                .clone()
                .ok_or_else(|| message("Enter a password."))?;
            crate::encrypt(&task.source, &target, password, cancel)?
        }
        Action::Decrypt => {
            let password = batch
                .password
                .clone()
                .ok_or_else(|| message("Enter the password."))?;
            crate::decrypt(&task.source, &target, password, cancel)?
        }
    }
    Ok(target)
}

fn run_merge(
    batch: &Batch,
    destination: &Destination,
    engines: &Engines,
    work: &Path,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(ItemReport),
) -> Result<PathBuf> {
    let Destination::File(target) = destination else {
        return Err(message("A combined document needs a file name."));
    };
    let mut parts = Vec::new();
    for (index, task) in batch.tasks.iter().enumerate() {
        progress(ItemReport {
            index,
            status: Status::Working,
            detail: "Converting to PDF…".into(),
            output: None,
        });
        let pdf = convert_document(
            &task.source,
            task.kind,
            OutputFormat::Pdf,
            &task.layout(batch),
            engines,
            work,
            &format!("part{index}"),
            cancel,
        )
        .map_err(|e| message(format!("{}: {e}", task.name)))?;
        progress(ItemReport {
            index,
            status: Status::Done,
            detail: "Ready to merge".into(),
            output: None,
        });
        parts.push(pdf);
    }
    check_cancel(cancel)?;
    let mut bytes = pdf::merge_to_bytes(&parts, cancel)?;
    if batch.protect {
        if let Some(password) = &batch.password {
            bytes = pdf::protect_bytes(&bytes, password)?;
        }
    }
    crate::write_bytes(target, &bytes, cancel)?;
    Ok(target.clone())
}

/// Runs the whole batch. Each report is sent through `progress` as it happens
/// and the final list is returned.
pub fn run(
    batch: &Batch,
    destination: &Destination,
    engines: &Engines,
    cancel: &AtomicBool,
    mut progress: impl FnMut(ItemReport),
) -> Vec<ItemReport> {
    let work = match work_dir() {
        Ok(dir) => dir,
        Err(e) => {
            return batch
                .tasks
                .iter()
                .enumerate()
                .map(|(index, _)| ItemReport {
                    index,
                    status: Status::Failed,
                    detail: e.to_string(),
                    output: None,
                })
                .collect()
        }
    };
    let mut reports = Vec::new();
    if batch.merge {
        let result = run_merge(
            batch,
            destination,
            engines,
            work.path(),
            cancel,
            &mut progress,
        );
        let (status, detail, output) = match result {
            Ok(path) => (
                Status::Done,
                "Combined".to_string(),
                Some(path.to_string_lossy().into_owned()),
            ),
            Err(e) if e.to_string() == crate::CANCELLED => (Status::Cancelled, e.to_string(), None),
            Err(e) => (Status::Failed, e.to_string(), None),
        };
        for index in 0..batch.tasks.len() {
            let report = ItemReport {
                index,
                status,
                detail: detail.clone(),
                output: output.clone(),
            };
            progress(report.clone());
            reports.push(report);
        }
        return reports;
    }
    let mut cancelled = false;
    for (index, task) in batch.tasks.iter().enumerate() {
        if cancelled {
            let report = ItemReport {
                index,
                status: Status::Cancelled,
                detail: crate::CANCELLED.into(),
                output: None,
            };
            progress(report.clone());
            reports.push(report);
            continue;
        }
        progress(ItemReport {
            index,
            status: Status::Working,
            detail: "Working…".into(),
            output: None,
        });
        let report = match run_item(
            task,
            index,
            batch,
            destination,
            engines,
            work.path(),
            cancel,
        ) {
            Ok(path) => ItemReport {
                index,
                status: Status::Done,
                detail: "Saved".into(),
                output: Some(path.to_string_lossy().into_owned()),
            },
            Err(e) if e.to_string() == crate::CANCELLED => {
                cancelled = true;
                ItemReport {
                    index,
                    status: Status::Cancelled,
                    detail: e.to_string(),
                    output: None,
                }
            }
            Err(e) => ItemReport {
                index,
                status: Status::Failed,
                detail: e.to_string(),
                output: None,
            },
        };
        progress(report.clone());
        reports.push(report);
    }
    reports
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret;

    fn task(source: PathBuf, action: Action) -> Task {
        let kind = crate::inspect(&source);
        let name = source.file_name().unwrap().to_string_lossy().into_owned();
        Task {
            source,
            name,
            kind,
            action,
            page_breaks: Vec::new(),
        }
    }

    fn batch(tasks: Vec<Task>) -> Batch {
        Batch {
            tasks,
            merge: false,
            layout: Layout::default(),
            password: None,
            protect: false,
            image: ImageSettings {
                max_edge: 0,
                quality: 85,
            },
        }
    }

    #[test]
    fn builtin_routes_run_without_office() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("notes.md");
        std::fs::write(&md, "# Notes\n\nSome *text*.\n\n- a\n- b\n").unwrap();
        let html = dir.path().join("page.html");
        std::fs::write(&html, "<h1>Page</h1><p>Body <b>bold</b></p>").unwrap();
        let png = dir.path().join("pic.png");
        image::RgbImage::from_pixel(12, 8, image::Rgb([1, 2, 3]))
            .save(&png)
            .unwrap();
        let out = dir.path().join("out");
        std::fs::create_dir(&out).unwrap();
        let engines = Engines::default();
        let cancel = AtomicBool::new(false);
        let mut b = batch(vec![
            task(
                md.clone(),
                Action::Convert {
                    format: OutputFormat::Pdf,
                },
            ),
            task(
                md.clone(),
                Action::Convert {
                    format: OutputFormat::Html,
                },
            ),
            task(
                md.clone(),
                Action::Convert {
                    format: OutputFormat::Txt,
                },
            ),
            task(
                html.clone(),
                Action::Convert {
                    format: OutputFormat::Md,
                },
            ),
            task(
                html.clone(),
                Action::Convert {
                    format: OutputFormat::Pdf,
                },
            ),
            task(
                md.clone(),
                Action::Convert {
                    format: OutputFormat::Docx,
                },
            ),
            task(
                png.clone(),
                Action::Image {
                    format: "webp".into(),
                },
            ),
            task(
                png.clone(),
                Action::Image {
                    format: "pdf".into(),
                },
            ),
            task(md.clone(), Action::Encrypt),
        ]);
        b.password = Some(secret("a long enough passphrase".into()));
        b.protect = true;
        let mut seen = Vec::new();
        let reports = run(
            &b,
            &Destination::Folder(out.clone()),
            &engines,
            &cancel,
            |r| seen.push(r),
        );
        assert!(seen.len() >= reports.len());
        let statuses: Vec<Status> = reports.iter().map(|r| r.status).collect();
        assert_eq!(
            statuses,
            vec![
                Status::Done,
                Status::Done,
                Status::Done,
                Status::Done,
                Status::Done,
                Status::Failed,
                Status::Done,
                Status::Done,
                Status::Done
            ],
            "{reports:?}"
        );
        assert!(reports[5].detail.contains("LibreOffice"));
        assert!(pdf::info(&out.join("notes.pdf")).unwrap().encrypted);
        assert!(std::fs::read_to_string(out.join("notes.html"))
            .unwrap()
            .contains("<h1>Notes</h1>"));
        assert!(std::fs::read_to_string(out.join("page.md"))
            .unwrap()
            .contains("# Page"));
        assert!(out.join("pic.webp").exists());
        assert!(out.join("notes.md.age").exists());

        // Merge md + html into one protected PDF, then unique naming on rerun.
        let mut m = batch(vec![
            task(
                md.clone(),
                Action::Convert {
                    format: OutputFormat::Pdf,
                },
            ),
            task(
                html.clone(),
                Action::Convert {
                    format: OutputFormat::Pdf,
                },
            ),
        ]);
        m.merge = true;
        m.password = Some(secret("merge password".into()));
        m.protect = true;
        let combined = out.join("combined.pdf");
        let reports = run(
            &m,
            &Destination::File(combined.clone()),
            &engines,
            &cancel,
            |_| {},
        );
        assert!(
            reports.iter().all(|r| r.status == Status::Done),
            "{reports:?}"
        );
        let info = pdf::info(&combined).unwrap();
        assert!(info.encrypted);
        let unlocked = out.join("combined-open.pdf");
        pdf::unlock(
            &combined,
            &unlocked,
            &secret("merge password".into()),
            &cancel,
        )
        .unwrap();
        assert_eq!(pdf::info(&unlocked).unwrap().pages, 2);

        let again = run(
            &batch(vec![task(
                md.clone(),
                Action::Convert {
                    format: OutputFormat::Html,
                },
            )]),
            &Destination::Folder(out.clone()),
            &engines,
            &cancel,
            |_| {},
        );
        assert_eq!(
            again[0]
                .output
                .as_deref()
                .map(|p| p.ends_with("notes (2).html")),
            Some(true)
        );

        let preview = preview_pdf(
            &md,
            InputKind::Md,
            OutputFormat::Pdf,
            &Layout::default(),
            &engines,
            &cancel,
        )
        .unwrap();
        assert!(preview.starts_with(b"%PDF"));
        assert_eq!(outline(&md, InputKind::Md).unwrap().len(), 3);

        let stopped = run(
            &batch(vec![
                task(
                    md.clone(),
                    Action::Convert {
                        format: OutputFormat::Pdf,
                    },
                ),
                task(
                    md.clone(),
                    Action::Convert {
                        format: OutputFormat::Pdf,
                    },
                ),
            ]),
            &Destination::Folder(out.clone()),
            &engines,
            &AtomicBool::new(true),
            |_| {},
        );
        assert!(
            stopped.iter().all(|r| r.status == Status::Cancelled),
            "{stopped:?}"
        );
    }

    #[test]
    fn spreadsheet_html_keeps_images_when_office_is_available() {
        let dir = tempfile::tempdir().unwrap();
        let Some(office) = crate::office::OfficeEngine::detect(&dir.path().join("profile")) else {
            eprintln!("LibreOffice not found; skipping");
            return;
        };
        let source = dir.path().join("sheet.xlsx");
        std::fs::write(
            &source,
            include_bytes!("../tests/fixtures/spreadsheet-with-image.xlsx"),
        )
        .unwrap();
        let output = dir.path().join("saved.html");
        let reports = run(
            &batch(vec![task(
                source,
                Action::Convert {
                    format: OutputFormat::Html,
                },
            )]),
            &Destination::File(output.clone()),
            &Engines {
                office: Some(office),
            },
            &AtomicBool::new(false),
            |_| {},
        );
        assert_eq!(reports[0].status, Status::Done, "{reports:?}");
        // run() has already dropped its work directory and all image sidecars.
        let html = std::fs::read_to_string(output).unwrap();
        let encoded = html
            .split("data:image/png;base64,")
            .nth(1)
            .expect("embedded PNG")
            .split('"')
            .next()
            .unwrap();
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        let image = image::load_from_memory(&bytes).unwrap();
        assert_eq!((image.width(), image.height()), (40, 40));
        assert!(html.contains("Image export"));
        assert!(!html.contains("src=\"sheet_html_"));
    }

    #[test]
    fn cleanup_skips_folders_that_are_still_in_use() {
        let root = tempfile::tempdir().unwrap();
        let active = root.path().join(format!("{WORK_PREFIX}active"));
        let stale = root.path().join(format!("{WORK_PREFIX}stale"));
        let other = root.path().join("unrelated");
        for dir in [&active, &stale, &other] {
            std::fs::create_dir(dir).unwrap();
            std::fs::write(dir.join("part.pdf"), b"x").unwrap();
        }
        let lock = hold_lock(&active).unwrap();
        std::fs::write(stale.join(LOCK_NAME), b"").unwrap();
        assert_eq!(clean_stale_work_dirs_in(root.path()), 1);
        assert!(other.exists());
        assert!(!stale.exists());
        if cfg!(windows) {
            assert!(
                active.join("part.pdf").exists(),
                "active folder must survive"
            );
        }
        drop(lock);
        assert_eq!(clean_stale_work_dirs_in(root.path()), 1);
        assert!(!active.exists());
        let work = work_dir().unwrap();
        let path = work.path().to_path_buf();
        assert!(path.join(LOCK_NAME).exists());
        drop(work);
        assert!(!path.exists(), "dropping the work dir must remove it");
    }

    #[test]
    fn output_names_follow_the_action() {
        let b = batch(vec![]);
        let t = |name: &str, action: Action| Task {
            source: PathBuf::from(format!("C:/x/{name}")),
            name: name.to_string(),
            kind: InputKind::Other,
            action,
            page_breaks: Vec::new(),
        };
        assert_eq!(
            output_name(
                &t(
                    "a.b.docx",
                    Action::Convert {
                        format: OutputFormat::Pdf
                    }
                ),
                &b
            ),
            "a.b.pdf"
        );
        assert_eq!(output_name(&t("a.docx", Action::Encrypt), &b), "a.docx.age");
        assert_eq!(
            output_name(&t("a.docx.age", Action::Decrypt), &b),
            "restored-a.docx"
        );
        assert_eq!(
            output_name(&t("a.pdf", Action::Protect), &b),
            "a-protected.pdf"
        );
        assert_eq!(
            output_name(
                &t(
                    "a.png",
                    Action::Image {
                        format: "jpg".into()
                    }
                ),
                &b
            ),
            "a.jpg"
        );
    }
}
