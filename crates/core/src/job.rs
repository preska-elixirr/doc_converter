//! Batch execution: every item is prepared in a private temp directory, then
//! committed to the destination without overwriting. Progress is reported per
//! item so the UI can show a live status column.

use crate::{
    capability::{route, Engines, OutputFormat, Route},
    check_cancel, copy_file,
    crypto::{Identity, Recipient},
    docx, images, layout, message,
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
    Convert {
        format: OutputFormat,
    },
    Image {
        format: String,
    },
    Protect,
    Unlock,
    Encrypt,
    /// `.age` output for the batch's public keys; no password involved.
    EncryptFor,
    Decrypt,
    Clean,
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
    /// Optional text overlay on every page of ordinary PDF conversion outputs.
    pub watermark: Option<String>,
    pub image: ImageSettings,
    pub attachments: Vec<crate::pdf_standards::Attachment>,
    /// Encrypt for recipients: the public keys that can open the `.age` outputs.
    pub recipients: Vec<Recipient>,
    /// Decrypt: the secret key for `.age` files encrypted to a public key.
    pub identity: Option<Identity>,
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
    pub validation: Option<crate::validation::ValidationReport>,
}

/// Output file name for a task, used for save dialogs and folder outputs.
pub fn output_name(task: &Task, batch: &Batch) -> String {
    let stem = stem(&task.source);
    match &task.action {
        Action::Convert { format } => format!("{stem}.{}", format.ext()),
        Action::Image { format } => format!("{stem}.{format}"),
        Action::Clean => format!("{stem}-clean.{}", crate::clean::extension(task.kind)),
        Action::Protect => format!("{stem}-protected.pdf"),
        Action::Unlock => format!("{stem}-unlocked.pdf"),
        Action::Encrypt | Action::EncryptFor => format!("{}.age", task.name),
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
        format if format.is_standard_pdf() => Target::standard(kind, format),
        OutputFormat::Docx => Target::DOCX,
        OutputFormat::Txt => Target::TXT,
        OutputFormat::Html if kind == InputKind::Xlsx => Target::HTML_CALC,
        OutputFormat::Html => Target::HTML,
        OutputFormat::Md => Target::MD,
        _ => unreachable!("standard PDF handled above"),
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
            if format.is_standard_pdf() {
                crate::office::check_standard_output(
                    &produced,
                    if format == OutputFormat::Pdfa4f {
                        OutputFormat::Pdfa4
                    } else {
                        format
                    },
                )?;
            }
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
            if target.is_standard_pdf() {
                target
            } else {
                OutputFormat::Pdf
            },
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
) -> Result<(PathBuf, Option<crate::validation::ValidationReport>)> {
    let mut validation = None;
    let tag = format!("item{index}");
    let target = final_path(destination, &output_name(task, batch));
    match &task.action {
        Action::Clean => crate::clean::clean(&task.source, &target, cancel)?,
        Action::Convert { format } => {
            let validator = if format.is_standard_pdf() {
                Some(crate::validation::Validator::detect().ok_or_else(|| {
                    message("Local veraPDF and Java are required for PDF/A and PDF/UA export.")
                })?)
            } else {
                None
            };
            let mut produced = convert_document(
                &task.source,
                task.kind,
                *format,
                &task.layout(batch),
                engines,
                work,
                &tag,
                cancel,
            )?;
            if !batch.attachments.is_empty() {
                let attached = work.join(format!("{tag}-attached.pdf"));
                crate::pdf_standards::embed(
                    &produced,
                    &attached,
                    *format,
                    &batch.attachments,
                    cancel,
                )?;
                produced = attached;
            }
            if format.is_standard_pdf() {
                crate::office::check_standard_output(&produced, *format)?;
                let report = validator
                    .as_ref()
                    .expect("standard profile validator")
                    .validate(&produced, *format, cancel)?;
                if !report.passed {
                    return Err(crate::Error::Validation(report));
                }
                validation = Some(report);
            }
            if let Some(text) = &batch.watermark {
                let marked = work.join(format!("{tag}-watermarked.pdf"));
                let bytes = crate::watermark::apply(&std::fs::read(&produced)?, text, cancel)?;
                crate::write_bytes(&marked, &bytes, cancel)?;
                produced = marked;
            }
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
        Action::EncryptFor => {
            crate::crypto::encrypt_for(&task.source, &target, &batch.recipients, cancel)?
        }
        Action::Decrypt => crate::decrypt(
            &task.source,
            &target,
            batch.password.as_ref(),
            batch.identity.as_ref(),
            cancel,
        )?,
    }
    Ok((target, validation))
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
            validation: None,
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
            validation: None,
        });
        parts.push(pdf);
    }
    check_cancel(cancel)?;
    let mut bytes = pdf::merge_to_bytes(&parts, cancel)?;
    if let Some(text) = &batch.watermark {
        bytes = crate::watermark::apply(&bytes, text, cancel)?;
    }
    if batch.protect {
        if let Some(password) = &batch.password {
            bytes = pdf::protect_bytes(&bytes, password)?;
        }
    }
    crate::write_bytes(target, &bytes, cancel)?;
    Ok(target.clone())
}

/// Validate watermark text and eligibility before opening a destination dialog.
pub fn validate_watermark(batch: &Batch) -> Result<()> {
    if let Some(text) = &batch.watermark {
        crate::watermark::validate(text)?;
        if batch.tasks.iter().any(|task| {
            !matches!(
                task.action,
                Action::Convert {
                    format: OutputFormat::Pdf
                }
            )
        }) {
            return Err(message(
                "Watermarks require ordinary PDF output in Convert mode.",
            ));
        }
    }
    Ok(())
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
    if let Err(error) = validate_watermark(batch) {
        return batch
            .tasks
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let report = ItemReport {
                    index,
                    status: Status::Failed,
                    detail: error.to_string(),
                    output: None,
                    validation: None,
                };
                progress(report.clone());
                report
            })
            .collect();
    }
    let attachment_error = if batch.attachments.len() > crate::pdf_standards::MAX_ATTACHMENTS {
        Some("At most 20 attachments are allowed.")
    } else if !batch.attachments.is_empty()
        && (batch.merge
            || batch.tasks.iter().any(
                |t| !matches!(t.action, Action::Convert {format} if format.supports_attachments()),
            ))
    {
        Some("Attachments can only be added to separate PDF/A-3b or PDF/A-4f outputs.")
    } else if batch.attachments.is_empty()
        && batch.tasks.iter().any(|t| {
            matches!(
                t.action,
                Action::Convert {
                    format: OutputFormat::Pdfa4f
                }
            )
        })
    {
        Some("PDF/A-4f requires at least one attachment.")
    } else {
        None
    };
    if let Some(detail) = attachment_error {
        return batch
            .tasks
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let report = ItemReport {
                    index,
                    status: Status::Failed,
                    detail: detail.into(),
                    output: None,
                    validation: None,
                };
                progress(report.clone());
                report
            })
            .collect();
    }
    if (batch.merge || batch.protect)
        && batch
            .tasks
            .iter()
            .any(|t| matches!(t.action, Action::Convert { format } if format.is_standard_pdf()))
    {
        return batch
            .tasks
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let report = ItemReport {
                    index,
                    status: Status::Failed,
                    detail: "PDF/A or PDF/UA export cannot be combined with merging or password protection."
                        .into(),
                    output: None,
                    validation: None,
                };
                progress(report.clone());
                report
            })
            .collect();
    }
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
                    validation: None,
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
                validation: None,
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
                validation: None,
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
            validation: None,
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
            Ok((path, validation)) => ItemReport {
                index,
                status: Status::Done,
                detail: if validation.as_ref().is_some_and(|v| v.human_review_required) {
                    "Saved; PDF/UA machine checks passed. Human review required.".into()
                } else if validation.is_some() {
                    "Saved; PDF/A validation passed.".into()
                } else {
                    "Saved".into()
                },
                output: Some(path.to_string_lossy().into_owned()),
                validation,
            },
            Err(e) if e.to_string() == crate::CANCELLED => {
                cancelled = true;
                ItemReport {
                    index,
                    status: Status::Cancelled,
                    detail: e.to_string(),
                    output: None,
                    validation: None,
                }
            }
            Err(crate::Error::Validation(validation)) => ItemReport {
                index,
                status: Status::Failed,
                detail: "PDF validation failed. No output was saved.".into(),
                output: None,
                validation: Some(validation),
            },
            Err(e) => ItemReport {
                index,
                status: Status::Failed,
                detail: e.to_string(),
                output: None,
                validation: None,
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
            watermark: None,
            attachments: Vec::new(),
            recipients: Vec::new(),
            identity: None,
            image: ImageSettings {
                max_edge: 0,
                quality: 85,
            },
        }
    }

    #[test]
    fn watermark_batches_preserve_sources_protect_merge_and_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        std::fs::write(&source, "Original document\n\nSecond page").unwrap();
        let original = std::fs::read(&source).unwrap();
        let mut t = task(
            source.clone(),
            Action::Convert {
                format: OutputFormat::Pdf,
            },
        );
        t.page_breaks = vec![1];
        let mut b = batch(vec![t.clone()]);
        b.watermark = Some("Confidential".into());
        let out = dir.path().join("out.pdf");
        let engines = Engines::default();
        let cancel = AtomicBool::new(false);
        let reports = run(
            &b,
            &Destination::File(out.clone()),
            &engines,
            &cancel,
            |_| {},
        );
        assert_eq!(reports[0].status, Status::Done, "{reports:?}");
        let bytes = std::fs::read(&out).unwrap();
        let check = |doc: &lopdf::Document, count| {
            assert_eq!(doc.get_pages().len(), count);
            for page in doc.get_pages().into_values() {
                assert!(String::from_utf8_lossy(&doc.get_page_content(page))
                    .contains("/DocConverterWatermark Do"));
            }
        };
        check(&lopdf::Document::load_mem(&bytes).unwrap(), 2);
        // Existing PDFs take the same copy route and retain the original bytes.
        let mut existing = batch(vec![task(
            out.clone(),
            Action::Convert {
                format: OutputFormat::Pdf,
            },
        )]);
        existing.watermark = Some("Željko Čović".into());
        let copy = dir.path().join("recipient.pdf");
        assert_eq!(
            run(
                &existing,
                &Destination::File(copy.clone()),
                &engines,
                &cancel,
                |_| {}
            )[0]
            .status,
            Status::Done
        );
        check(&lopdf::Document::load(&copy).unwrap(), 2);
        assert_eq!(std::fs::read(&out).unwrap(), bytes);
        assert_eq!(
            run(
                &b,
                &Destination::File(out.clone()),
                &engines,
                &cancel,
                |_| {}
            )[0]
            .status,
            Status::Failed
        );
        assert_eq!(std::fs::read(&out).unwrap(), bytes);
        b.protect = true;
        b.password = Some(secret("watermark password".into()));
        for merge in [false, true] {
            b.merge = merge;
            if merge {
                b.tasks.push(t.clone());
            }
            let target = dir.path().join(format!("protected-{merge}.pdf"));
            let reports = run(
                &b,
                &Destination::File(target.clone()),
                &engines,
                &cancel,
                |_| {},
            );
            assert!(
                reports.iter().all(|r| r.status == Status::Done),
                "{reports:?}"
            );
            let doc = lopdf::Document::load_with_password(&target, "watermark password").unwrap();
            check(&doc, if merge { 4 } else { 2 });
        }
        let cancelled = dir.path().join("cancelled.pdf");
        run(
            &b,
            &Destination::File(cancelled.clone()),
            &engines,
            &AtomicBool::new(true),
            |_| {},
        );
        assert!(!cancelled.exists());
        assert_eq!(std::fs::read(&source).unwrap(), original);
    }

    #[test]
    fn watermark_rejects_ineligible_batches_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        std::fs::write(&source, "source").unwrap();
        for action in [
            Action::Convert {
                format: OutputFormat::Docx,
            },
            Action::Convert {
                format: OutputFormat::Pdfa2b,
            },
            Action::Clean,
            Action::Image {
                format: "pdf".into(),
            },
            Action::Encrypt,
        ] {
            let mut b = batch(vec![task(source.clone(), action)]);
            b.watermark = Some("Confidential".into());
            for merge in [false, true] {
                b.merge = merge;
                let out = dir.path().join("out.pdf");
                let reports = run(
                    &b,
                    &Destination::File(out.clone()),
                    &Engines::default(),
                    &AtomicBool::new(false),
                    |_| {},
                );
                assert_eq!(reports[0].status, Status::Failed);
                assert!(reports[0].detail.contains("ordinary PDF"));
                assert!(!out.exists());
            }
        }
    }

    #[test]
    fn all_pdf_standards_and_attachments_validate_locally() {
        use crate::pdf_standards::{Attachment, Relationship};
        let dir = tempfile::tempdir().unwrap();
        let Some(office) = crate::office::OfficeEngine::detect(&dir.path().join("profile")) else {
            eprintln!("SKIP: LibreOffice unavailable");
            return;
        };
        let Some(validator) = crate::validation::Validator::detect() else {
            eprintln!("SKIP: local veraPDF/Java unavailable");
            return;
        };
        if !office.supports_pdfa() {
            eprintln!("SKIP: LibreOffice 25.8+ required");
            return;
        }
        let engines = Engines {
            office: Some(office),
        };
        let html = dir.path().join("document.html");
        let content = "<!DOCTYPE html><html lang=\"en-US\"><head><meta charset=\"utf-8\"><title>Accessible archive</title></head><body><h1>Accessible archive</h1><p>A short document with meaningful text.</p></body></html>";
        std::fs::write(&html, content).unwrap();
        let attachment = dir.path().join("račun & podatci.xml");
        let attachment_bytes = b"<data><amount>42</amount></data>";
        std::fs::write(&attachment, attachment_bytes).unwrap();
        let out = dir.path().join("output");
        std::fs::create_dir(&out).unwrap();
        let cancel = AtomicBool::new(false);
        for format in [
            OutputFormat::Pdfa1b,
            OutputFormat::Pdfa2b,
            OutputFormat::Pdfa3b,
            OutputFormat::Pdfa4,
            OutputFormat::Pdfa4f,
            OutputFormat::Pdfua1,
        ] {
            let mut b = batch(vec![task(html.clone(), Action::Convert { format })]);
            if format.supports_attachments() {
                b.attachments.push(Attachment {
                    source: attachment.clone(),
                    relationship: Relationship::Data,
                    description: "Sample data, not a certified e-invoice".into(),
                });
            }
            let reports = run(
                &b,
                &Destination::Folder(out.clone()),
                &engines,
                &cancel,
                |_| {},
            );
            eprintln!("{}: {reports:?}", format.label());
            assert_eq!(reports[0].status, Status::Done, "{reports:?}");
            let report = reports[0].validation.as_ref().unwrap();
            assert!(report.passed);
            assert_eq!(report.human_review_required, format == OutputFormat::Pdfua1);
            let saved = PathBuf::from(reports[0].output.as_ref().unwrap());
            if format.supports_attachments() {
                let doc = lopdf::Document::load(&saved).unwrap();
                let af = doc
                    .catalog()
                    .unwrap()
                    .get(b"AF")
                    .unwrap()
                    .as_array()
                    .unwrap();
                assert_eq!(af.len(), 1);
                let file = doc.get_dictionary(af[0].as_reference().unwrap()).unwrap();
                assert_eq!(
                    file.get(b"AFRelationship").unwrap().as_name().unwrap(),
                    b"Data"
                );
                let ef = file
                    .get(b"EF")
                    .unwrap()
                    .as_dict()
                    .unwrap()
                    .get(b"F")
                    .unwrap()
                    .as_reference()
                    .unwrap();
                let stream = doc.get_object(ef).unwrap().as_stream().unwrap();
                assert_eq!(&stream.content, attachment_bytes);
            }
            assert_eq!(std::fs::read_to_string(&html).unwrap(), content);
            assert_eq!(std::fs::read(&attachment).unwrap(), attachment_bytes);
        }
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 6);
        let image_path = dir.path().join("image.png");
        image::RgbImage::from_pixel(10, 10, image::Rgb([100, 20, 10]))
            .save(&image_path)
            .unwrap();
        let bad_html = dir.path().join("missing-alt.html");
        let image_url = format!(
            "file:///{}",
            image_path.to_string_lossy().replace('\\', "/")
        );
        std::fs::write(&bad_html, format!("<html lang=\"en-US\"><head><title>Missing image description</title></head><body><h1>Photo</h1><img src=\"{image_url}\"></body></html>")).unwrap();
        let rejected = run(
            &batch(vec![task(
                bad_html,
                Action::Convert {
                    format: OutputFormat::Pdfua1,
                },
            )]),
            &Destination::Folder(out.clone()),
            &engines,
            &cancel,
            |_| {},
        );
        assert_eq!(
            rejected[0].status,
            Status::Failed,
            "Missing-alt export must fail: {rejected:?}"
        );
        assert!(
            rejected[0]
                .validation
                .as_ref()
                .is_some_and(|v| !v.passed && !v.issues.is_empty()),
            "{rejected:?}"
        );
        assert!(!out.join("missing-alt.pdf").exists());
        let ordinary = dir.path().join("ordinary.pdf");
        std::fs::write(
            &ordinary,
            crate::pdf::image_pdf(&[image::DynamicImage::new_rgb8(3, 3)], 85).unwrap(),
        )
        .unwrap();
        let report = validator
            .validate(&ordinary, OutputFormat::Pdfa2b, &cancel)
            .unwrap();
        assert!(!report.passed);
        assert!(!report.issues.is_empty());
        assert!(validator
            .validate(&ordinary, OutputFormat::Pdfa2b, &AtomicBool::new(true))
            .unwrap_err()
            .to_string()
            .contains("Cancelled"));
    }

    #[test]
    fn archival_rejects_merge_and_protection_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("paper.html");
        std::fs::write(&input, "<html><body>Archive me</body></html>").unwrap();
        let output = dir.path().join("out.pdf");
        for format in [OutputFormat::Pdfa2b, OutputFormat::Pdfa4] {
            for (merge, protect) in [(true, false), (false, true)] {
                let mut b = batch(vec![task(input.clone(), Action::Convert { format })]);
                b.merge = merge;
                b.protect = protect;
                let reports = run(
                    &b,
                    &Destination::File(output.clone()),
                    &Engines::default(),
                    &AtomicBool::new(false),
                    |_| {},
                );
                assert_eq!(reports[0].status, Status::Failed);
                assert!(reports[0].detail.contains("PDF/A"));
                assert!(!output.exists());
            }
        }
    }

    #[test]
    fn archival_exports_when_office_is_available() {
        if crate::validation::Validator::detect().is_none() {
            eprintln!("Local veraPDF/Java unavailable; skipping PDF/A integration");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let Some(office) = crate::office::OfficeEngine::detect(&dir.path().join("profile")) else {
            eprintln!("LibreOffice not found; skipping PDF/A integration");
            return;
        };
        if !office.supports_pdfa() {
            eprintln!("LibreOffice 25.8+ required; skipping PDF/A integration");
            return;
        }
        eprintln!("PDF/A integration using {}", office.version);
        let engines = Engines {
            office: Some(office),
        };
        let html = dir.path().join("paper.html");
        let sheet = dir.path().join("sheet.xlsx");
        std::fs::write(&html, "<!DOCTYPE html><html><body><h1>Archive sample</h1><p>Preserve this text.</p></body></html>").unwrap();
        std::fs::write(
            &sheet,
            include_bytes!("../tests/fixtures/spreadsheet-with-image.xlsx"),
        )
        .unwrap();
        let cancel = AtomicBool::new(false);
        let out = dir.path().join("out");
        std::fs::create_dir(&out).unwrap();
        for source in [&html, &sheet] {
            let original = std::fs::read(source).unwrap();
            for format in [OutputFormat::Pdfa2b, OutputFormat::Pdfa4] {
                let b = batch(vec![task(source.clone(), Action::Convert { format })]);
                let reports = run(
                    &b,
                    &Destination::Folder(out.clone()),
                    &engines,
                    &cancel,
                    |_| {},
                );
                assert_eq!(reports[0].status, Status::Done, "{reports:?}");
                let saved = PathBuf::from(reports[0].output.as_ref().unwrap());
                crate::office::check_archival_output(&saved, format == OutputFormat::Pdfa4)
                    .unwrap();
                assert!(crate::office::check_archival_output(
                    &saved,
                    format != OutputFormat::Pdfa4
                )
                .is_err());
                let bytes = std::fs::read(&saved).unwrap();
                let again = run(
                    &b,
                    &Destination::File(saved.clone()),
                    &engines,
                    &cancel,
                    |_| {},
                );
                assert_eq!(again[0].status, Status::Failed);
                assert_eq!(std::fs::read(&saved).unwrap(), bytes);
                assert_eq!(std::fs::read(source).unwrap(), original);
            }
        }
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 4);
        let b = batch(vec![task(
            html,
            Action::Convert {
                format: OutputFormat::Pdfa2b,
            },
        )]);
        let reports = run(
            &b,
            &Destination::Folder(out),
            &engines,
            &AtomicBool::new(true),
            |_| {},
        );
        assert_eq!(reports[0].status, Status::Cancelled);
    }

    #[test]
    fn cleaning_batch_reports_failures_and_numbers_copies() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("photo.jpg");
        image::RgbImage::from_pixel(7, 5, image::Rgb([20, 40, 60]))
            .save(&photo)
            .unwrap();
        let pdf = dir.path().join("paper.pdf");
        let pixels = image::open(&photo).unwrap();
        std::fs::write(&pdf, crate::pdf::image_pdf(&[pixels], 85).unwrap()).unwrap();
        let unsupported = dir.path().join("notes.txt");
        std::fs::write(&unsupported, "Keep me").unwrap();
        let out = dir.path().join("out");
        std::fs::create_dir(&out).unwrap();
        let b = batch(vec![
            task(photo.clone(), Action::Clean),
            task(pdf.clone(), Action::Clean),
            task(unsupported, Action::Clean),
        ]);
        let cancel = AtomicBool::new(false);
        let destination = Destination::Folder(out.clone());
        let reports = run(&b, &destination, &Engines::default(), &cancel, |_| {});
        assert_eq!(
            reports.iter().map(|r| r.status).collect::<Vec<_>>(),
            vec![Status::Done, Status::Done, Status::Failed]
        );
        assert!(out.join("photo-clean.png").exists());
        assert!(out.join("paper-clean.pdf").exists());
        assert_eq!(
            image::open(out.join("photo-clean.png")).unwrap().to_rgb8(),
            image::open(photo).unwrap().to_rgb8()
        );
        run(&b, &destination, &Engines::default(), &cancel, |_| {});
        assert!(out.join("photo-clean (2).png").exists());
        assert!(out.join("paper-clean (2).pdf").exists());
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let reports = run(&b, &destination, &Engines::default(), &cancel, |_| {});
        assert!(reports.iter().all(|r| r.status == Status::Cancelled));
        assert!(!out.join("photo-clean (3).png").exists());
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
    fn public_key_encryption_runs_through_the_batch() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        std::fs::create_dir(&out).unwrap();
        let source = dir.path().join("notes.md");
        std::fs::write(&source, "# Private notes\n").unwrap();
        let engines = Engines::default();
        let cancel = AtomicBool::new(false);
        let recipient = crate::crypto::Identity::generate();

        // Without keys the item fails; a password never stands in for them.
        let mut b = batch(vec![task(source.clone(), Action::EncryptFor)]);
        b.password = Some(secret("a long enough passphrase".into()));
        let folder = Destination::Folder(out.clone());
        let reports = run(&b, &folder, &engines, &cancel, |_| {});
        assert_eq!(reports[0].status, Status::Failed, "{reports:?}");
        assert!(reports[0].detail.contains("public key"), "{reports:?}");
        assert!(!out.join("notes.md.age").exists());

        b.recipients = vec![recipient.to_public()];
        let reports = run(&b, &folder, &engines, &cancel, |_| {});
        assert_eq!(reports[0].status, Status::Done, "{reports:?}");
        let sealed = out.join("notes.md.age");
        assert!(sealed.exists());

        // The password alone does not open it; the secret key does.
        let mut d = batch(vec![task(sealed, Action::Decrypt)]);
        d.password = Some(secret("a long enough passphrase".into()));
        let reports = run(&d, &folder, &engines, &cancel, |_| {});
        assert_eq!(reports[0].status, Status::Failed, "{reports:?}");
        assert!(reports[0].detail.contains("secret key"), "{reports:?}");
        d.password = None;
        d.identity = Some(recipient);
        let reports = run(&d, &folder, &engines, &cancel, |_| {});
        assert_eq!(reports[0].status, Status::Done, "{reports:?}");
        assert_eq!(
            std::fs::read_to_string(out.join("restored-notes.md")).unwrap(),
            "# Private notes\n"
        );
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
            output_name(&t("a.docx", Action::EncryptFor), &b),
            "a.docx.age"
        );
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
