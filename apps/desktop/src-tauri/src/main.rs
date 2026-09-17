#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Tauri shell: the only IPC surface. The page never sees a filesystem path;
//! it works with opaque input IDs and typed options.

use converter_core::{
    capability::{capabilities, Availability, Engines},
    images::{inspect_image, ImageInfo},
    job::{self, Action, Batch, Destination, ImageSettings, ItemReport, Status, Task},
    office::OfficeEngine,
    pdf::{self, PdfInfo},
    secret, InputKind, Layout, OutputFormat,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tauri::{Emitter, Manager, State};

struct Entry {
    path: PathBuf,
    kind: InputKind,
}

struct AppState {
    inspections: Mutex<HashMap<String, String>>,
    files: Mutex<HashMap<String, Entry>>,
    next: AtomicU64,
    busy: AtomicBool,
    cancel: AtomicBool,
    /// `None` until detection has finished.
    engines: Mutex<Option<Engines>>,
    /// Runs previews one at a time and keeps them out of a batch's way.
    preview_gate: tauri::async_runtime::Mutex<()>,
    /// Cancel flag of the preview that is running or waiting, if any.
    preview_cancel: Mutex<Option<Arc<AtomicBool>>>,
}

#[derive(Serialize)]
struct Asset {
    id: String,
    name: String,
    bytes: u64,
    kind: InputKind,
    label: String,
    /// Convert-tab outputs for this kind, with reasons when unavailable.
    outputs: Vec<Availability>,
    pdf: Option<PdfInfo>,
    image: Option<ImageInfo>,
}

#[derive(Serialize)]
struct EngineStatus {
    ready: bool,
    office: Option<OfficeEngine>,
    validator: bool,
}

#[derive(Serialize)]
struct AssetOutputs {
    id: String,
    outputs: Vec<Availability>,
}

#[derive(Deserialize)]
struct RequestItem {
    id: String,
    /// Convert: output format. Images: image format.
    format: Option<String>,
    #[serde(default)]
    page_breaks: Vec<usize>,
}

#[derive(Deserialize)]
struct BatchRequest {
    mode: String,
    items: Vec<RequestItem>,
    #[serde(default)]
    merge: bool,
    #[serde(default)]
    merge_name: String,
    #[serde(default)]
    layout: Layout,
    #[serde(default)]
    password: String,
    #[serde(default)]
    protect: bool,
    #[serde(default)]
    watermark: Option<String>,
    /// Encrypt tab: `pdf`, `file` (password) or `key` (public keys).
    #[serde(default)]
    encryption: String,
    /// Encrypt tab, `key`: public keys, one per line.
    #[serde(default)]
    recipients: String,
    /// Decrypt tab: the secret key for `.age` files encrypted to a public key.
    #[serde(default)]
    identity: String,
    #[serde(default)]
    image: ImageSettings,
    #[serde(default)]
    attachments: Vec<AttachmentRequest>,
}

#[derive(Deserialize)]
struct AttachmentRequest {
    id: String,
    #[serde(default)]
    relationship: converter_core::pdf_standards::Relationship,
    #[serde(default)]
    description: String,
}

fn resolve_attachments(
    state: &AppState,
    request: &BatchRequest,
) -> Result<Vec<converter_core::pdf_standards::Attachment>, String> {
    if request.attachments.len() > converter_core::pdf_standards::MAX_ATTACHMENTS {
        return Err("At most 20 attachments are allowed.".into());
    }
    request
        .attachments
        .iter()
        .map(|a| {
            let (source, _) = lookup(state, &a.id)?;
            Ok(converter_core::pdf_standards::Attachment {
                source,
                relationship: a.relationship,
                description: a.description.clone(),
            })
        })
        .collect()
}

#[derive(Serialize)]
struct BatchOutcome {
    /// `saved`, `cancelled` (dialog), or `nothing`.
    result: String,
    reports: Vec<ItemReport>,
}

fn engines_or_default(state: &AppState) -> Engines {
    state
        .engines
        .lock()
        .ok()
        .and_then(|e| e.clone())
        .unwrap_or_default()
}

fn register(state: &AppState, path: &Path) -> Result<Option<Asset>, String> {
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let metadata = path.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Ok(None);
    }
    let kind = converter_core::inspect(&path);
    let engines = engines_or_default(state);
    let id = state.next.fetch_add(1, Ordering::Relaxed).to_string();
    let asset = Asset {
        id: id.clone(),
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        bytes: metadata.len(),
        kind,
        label: kind.label().to_string(),
        outputs: if kind.is_document() {
            capabilities(kind, &engines)
        } else {
            Vec::new()
        },
        pdf: (kind == InputKind::Pdf)
            .then(|| pdf::info(&path).ok())
            .flatten(),
        image: kind.is_image().then(|| inspect_image(&path).ok()).flatten(),
    };
    state
        .files
        .lock()
        .map_err(|_| "File state unavailable")?
        .insert(id, Entry { path, kind });
    Ok(Some(asset))
}

#[tauri::command]
async fn pick_files(state: State<'_, AppState>) -> Result<Vec<Asset>, String> {
    if state.busy.load(Ordering::SeqCst) {
        return Err("A job is running".into());
    }
    let picked = rfd::AsyncFileDialog::new()
        .pick_files()
        .await
        .unwrap_or_default();
    let mut result = Vec::new();
    for file in picked {
        if let Some(asset) = register(&state, file.path())? {
            result.push(asset);
        }
    }
    Ok(result)
}

/// Paths come from the webview drag-and-drop event, which the native layer
/// produces; they are still validated and canonicalized here.
#[tauri::command]
fn add_paths(state: State<'_, AppState>, paths: Vec<String>) -> Result<Vec<Asset>, String> {
    if state.busy.load(Ordering::SeqCst) {
        return Err("A job is running".into());
    }
    let mut result = Vec::new();
    for path in paths.iter().take(500) {
        if let Some(asset) = register(&state, Path::new(path))? {
            result.push(asset);
        }
    }
    Ok(result)
}

#[tauri::command]
fn engine_status(state: State<'_, AppState>) -> EngineStatus {
    let engines = state.engines.lock().ok().and_then(|e| e.clone());
    EngineStatus {
        ready: engines.is_some(),
        office: engines.and_then(|e| e.office),
        validator: converter_core::validation::Validator::detect().is_some(),
    }
}

#[tauri::command]
fn cancel_job(state: State<'_, AppState>) {
    state.cancel.store(true, Ordering::SeqCst);
}

/// Convert options for files that were added before engine detection finished.
#[tauri::command]
fn refresh_outputs(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<Vec<AssetOutputs>, String> {
    let engines = engines_or_default(&state);
    let files = state.files.lock().map_err(|_| "File state unavailable")?;
    Ok(ids
        .iter()
        .filter_map(|id| {
            files.get(id).map(|entry| AssetOutputs {
                id: id.clone(),
                outputs: if entry.kind.is_document() {
                    capabilities(entry.kind, &engines)
                } else {
                    Vec::new()
                },
            })
        })
        .collect())
}

/// Stops the preview that is running or waiting so a newer request or a batch
/// does not queue behind it.
fn cancel_preview(state: &AppState) {
    if let Ok(mut current) = state.preview_cancel.lock() {
        if let Some(flag) = current.take() {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

fn lookup(state: &AppState, id: &str) -> Result<(PathBuf, InputKind), String> {
    state
        .files
        .lock()
        .map_err(|_| "File state unavailable")?
        .get(id)
        .map(|e| (e.path.clone(), e.kind))
        .ok_or_else(|| "Select the file again".to_string())
}

fn parse_format(value: Option<&str>) -> Result<OutputFormat, String> {
    match value.unwrap_or("pdf") {
        "pdf" => Ok(OutputFormat::Pdf),
        "pdfa1b" => Ok(OutputFormat::Pdfa1b),
        "pdfa2b" => Ok(OutputFormat::Pdfa2b),
        "pdfa3b" => Ok(OutputFormat::Pdfa3b),
        "pdfa4f" => Ok(OutputFormat::Pdfa4f),
        "pdfua1" => Ok(OutputFormat::Pdfua1),
        "pdfa4" => Ok(OutputFormat::Pdfa4),
        "docx" => Ok(OutputFormat::Docx),
        "txt" => Ok(OutputFormat::Txt),
        "html" => Ok(OutputFormat::Html),
        "md" => Ok(OutputFormat::Md),
        other => Err(format!("Unknown output format {other}")),
    }
}

fn build_tasks(state: &AppState, request: &BatchRequest) -> Result<Vec<Task>, String> {
    if !request.attachments.is_empty()
        && (request.mode != "convert"
            || request.merge
            || request.items.iter().any(|item| {
                !parse_format(item.format.as_deref()).is_ok_and(|f| f.supports_attachments())
            }))
    {
        return Err("Attachments require separate PDF/A-3b or PDF/A-4f outputs.".into());
    }
    if request.mode == "convert"
        && request.attachments.is_empty()
        && request
            .items
            .iter()
            .any(|item| item.format.as_deref() == Some("pdfa4f"))
    {
        return Err("PDF/A-4f requires at least one attachment.".into());
    }
    let mut tasks = Vec::new();
    for item in &request.items {
        if request.mode == "convert"
            && (request.merge || request.protect)
            && parse_format(item.format.as_deref())?.is_standard_pdf()
        {
            return Err(
                "PDF/A or PDF/UA export cannot be combined with merging or password protection."
                    .into(),
            );
        }
        let (source, kind) = lookup(state, &item.id)?;
        let name = source
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let action = match request.mode.as_str() {
            "clean" if converter_core::clean::supported(kind) => Action::Clean,
            "clean" => return Err("Cleaning supports DOCX, PDF and static photos.".into()),
            "convert" => Action::Convert {
                format: if request.merge {
                    OutputFormat::Pdf
                } else {
                    parse_format(item.format.as_deref())?
                },
            },
            "images" => Action::Image {
                format: item.format.clone().unwrap_or_else(|| "png".into()),
            },
            "encrypt" if request.encryption == "pdf" => Action::Protect,
            "encrypt" if request.encryption == "key" => Action::EncryptFor,
            "encrypt" => Action::Encrypt,
            "decrypt" if kind == InputKind::Pdf => Action::Unlock,
            "decrypt" => Action::Decrypt,
            other => return Err(format!("Unknown mode {other}")),
        };
        tasks.push(Task {
            source,
            name,
            kind,
            action,
            page_breaks: item.page_breaks.clone(),
        });
    }
    Ok(tasks)
}

fn check_password(request: &BatchRequest) -> Result<(), String> {
    let chars = request.password.chars().count();
    // Convert only needs a new password when a PDF will actually be protected.
    let protects_pdf = request.mode == "convert"
        && request.protect
        && (request.merge
            || request
                .items
                .iter()
                .any(|item| item.format.as_deref().unwrap_or("pdf") == "pdf"));
    let needs_new = (request.mode == "encrypt" && request.encryption != "key") || protects_pdf;
    if needs_new && chars < 12 {
        return Err("Use a password with at least 12 characters.".into());
    }
    if request.mode == "decrypt" && chars == 0 && request.identity.trim().is_empty() {
        return Err("Enter the password or the secret key.".into());
    }
    Ok(())
}

type Keys = (
    Vec<converter_core::crypto::Recipient>,
    Option<converter_core::crypto::Identity>,
);

/// Public keys for an encrypt-for-recipients batch and the secret key for a
/// decrypt batch, parsed before any dialog opens so a typo fails fast.
fn parse_keys(request: &BatchRequest) -> Result<Keys, String> {
    let recipients = if request.mode == "encrypt" && request.encryption == "key" {
        converter_core::crypto::parse_recipients(&request.recipients).map_err(|e| e.to_string())?
    } else {
        Vec::new()
    };
    let identity = if request.mode == "decrypt" && !request.identity.trim().is_empty() {
        Some(converter_core::crypto::parse_identity(&request.identity).map_err(|e| e.to_string())?)
    } else {
        None
    };
    Ok((recipients, identity))
}

#[tauri::command]
async fn run_batch(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: BatchRequest,
) -> Result<BatchOutcome, String> {
    if request.items.is_empty() {
        return Err("Select at least one file.".into());
    }
    check_password(&request)?;
    let (recipients, identity) = parse_keys(&request)?;
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err("A job is already running".into());
    }
    state.cancel.store(false, Ordering::SeqCst);
    let result = async {
        cancel_preview(&state);
        let _gate = state.preview_gate.lock().await;
        let tasks = build_tasks(&state, &request)?;
        if tasks
            .iter()
            .any(|t| matches!(t.action, Action::Convert {format} if format.is_standard_pdf()))
            && converter_core::validation::Validator::detect().is_none()
        {
            return Err("Local veraPDF and Java are required for PDF/A and PDF/UA export.".into());
        }
        let attachments = resolve_attachments(&state, &request)?;
        let password = (!request.password.is_empty()).then(|| secret(request.password.clone()));
        let batch = Batch {
            tasks,
            merge: request.mode == "convert" && request.merge,
            layout: request.layout.clone(),
            password,
            protect: request.protect,
            watermark: request.watermark.clone(),
            image: request.image.clone(),
            attachments,
            recipients,
            identity,
        };
        job::validate_watermark(&batch).map_err(|e| e.to_string())?;
        let destination = if batch.merge {
            let mut name = request.merge_name.trim().to_string();
            if name.is_empty() {
                name = "Combined documents".into();
            }
            if !name.to_ascii_lowercase().ends_with(".pdf") {
                name.push_str(".pdf");
            }
            match rfd::AsyncFileDialog::new()
                .set_file_name(&name)
                .save_file()
                .await
            {
                Some(file) => Destination::File(file.path().to_owned()),
                None => {
                    return Ok(BatchOutcome {
                        result: "cancelled".into(),
                        reports: Vec::new(),
                    })
                }
            }
        } else if batch.tasks.len() == 1 {
            let suggested = job::output_name(&batch.tasks[0], &batch);
            match rfd::AsyncFileDialog::new()
                .set_file_name(&suggested)
                .save_file()
                .await
            {
                Some(file) => Destination::File(file.path().to_owned()),
                None => {
                    return Ok(BatchOutcome {
                        result: "cancelled".into(),
                        reports: Vec::new(),
                    })
                }
            }
        } else {
            match rfd::AsyncFileDialog::new()
                .set_title("Choose a folder for the new files")
                .pick_folder()
                .await
            {
                Some(folder) => Destination::Folder(folder.path().to_owned()),
                None => {
                    return Ok(BatchOutcome {
                        result: "cancelled".into(),
                        reports: Vec::new(),
                    })
                }
            }
        };
        if let Destination::File(path) = &destination {
            if path.exists() {
                return Err("Output already exists. Choose a new filename.".into());
            }
        }
        let engines = engines_or_default(&state);
        let emitter = app.clone();
        let reports = tauri::async_runtime::spawn_blocking(move || {
            let state = emitter.state::<AppState>();
            job::run(&batch, &destination, &engines, &state.cancel, |report| {
                let _ = emitter.emit("batch-progress", &report);
            })
        })
        .await
        .map_err(|e| e.to_string())?;
        let result = if reports.iter().any(|r| r.status == Status::Done) {
            "saved"
        } else {
            "nothing"
        };
        Ok(BatchOutcome {
            result: result.into(),
            reports,
        })
    }
    .await;
    state.busy.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
async fn outline(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<converter_core::text::OutlineEntry>, String> {
    let (source, kind) = lookup(&state, &id)?;
    tauri::async_runtime::spawn_blocking(move || job::outline(&source, kind))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn inspect_pdf(
    state: State<'_, AppState>,
    id: String,
) -> Result<converter_core::pdf_security::SecurityReport, String> {
    let (source, kind) = lookup(&state, &id)?;
    if kind != InputKind::Pdf {
        return Err("Select a PDF file.".into());
    }
    let _gate = state.preview_gate.lock().await;
    if state.busy.load(Ordering::SeqCst) {
        return Err("A job is running".into());
    }
    state
        .inspections
        .lock()
        .map_err(|_| "File state unavailable")?
        .remove(&id);
    let report = tauri::async_runtime::spawn_blocking(move || {
        converter_core::pdf_security::inspect(&source)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    if let Some(fingerprint) = &report.fingerprint {
        state
            .inspections
            .lock()
            .map_err(|_| "File state unavailable")?
            .insert(id, fingerprint.clone());
    }
    Ok(report)
}

#[tauri::command]
async fn open_inspected_pdf(
    state: State<'_, AppState>,
    id: String,
    fingerprint: String,
) -> Result<tauri::ipc::Response, String> {
    let (source, kind) = lookup(&state, &id)?;
    let _gate = state.preview_gate.lock().await;
    if state.busy.load(Ordering::SeqCst) {
        return Err("A job is running".into());
    }
    if kind != InputKind::Pdf
        || state
            .inspections
            .lock()
            .map_err(|_| "File state unavailable")?
            .get(&id)
            != Some(&fingerprint)
    {
        return Err("Inspect this PDF before opening the preview.".into());
    }
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        converter_core::pdf_security::reviewed_bytes(&source, &fingerprint)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
async fn validate_pdf(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    format: String,
) -> Result<converter_core::validation::ValidationReport, String> {
    let (source, kind) = lookup(&state, &id)?;
    if kind != InputKind::Pdf {
        return Err("Select an existing PDF to validate.".into());
    }
    let profile = parse_format(Some(&format))?;
    if !profile.is_standard_pdf() {
        return Err("Select a PDF/A or PDF/UA validation profile.".into());
    }
    let validator = converter_core::validation::Validator::detect()
        .ok_or("Local veraPDF and Java were not found.")?;
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err("A job is already running".into());
    }
    state.cancel.store(false, Ordering::SeqCst);
    cancel_preview(&state);
    let _gate = state.preview_gate.lock().await;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        validator
            .validate(&source, profile, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string());
    state.busy.store(false, Ordering::SeqCst);
    result?
}

/// PDF bytes of the real output for the preview panel. A newer preview or a
/// batch cancels the one in flight; the gate runs them one at a time.
#[tauri::command]
async fn preview(
    state: State<'_, AppState>,
    id: String,
    format: String,
    layout: Layout,
    watermark: Option<String>,
) -> Result<tauri::ipc::Response, String> {
    let (source, kind) = lookup(&state, &id)?;
    let target = parse_format(Some(&format))?;
    if let Some(text) = &watermark {
        converter_core::watermark::validate(text).map_err(|e| e.to_string())?;
        if target != OutputFormat::Pdf {
            return Err("Watermarks require ordinary PDF output in Convert mode.".into());
        }
    }
    if kind == InputKind::Pdf || converter_core::inspect(&source) == InputKind::Pdf {
        return Err(
            "Use the security inspector to review this PDF before opening its preview.".into(),
        );
    }
    if state.busy.load(Ordering::SeqCst) {
        return Err("A job is already running".into());
    }
    cancel_preview(&state);
    let flag = Arc::new(AtomicBool::new(false));
    if let Ok(mut current) = state.preview_cancel.lock() {
        *current = Some(flag.clone());
    }
    let _gate = state.preview_gate.lock().await;
    if state.busy.load(Ordering::SeqCst) {
        return Err("A job is already running".into());
    }
    if flag.load(Ordering::SeqCst) {
        return Err(converter_core::CANCELLED.into());
    }
    let engines = engines_or_default(&state);
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        let bytes = job::preview_pdf(&source, kind, target, &layout, &engines, &flag)?;
        match watermark {
            Some(text) => converter_core::watermark::apply(&bytes, &text, &flag),
            None => Ok(bytes),
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[derive(Serialize)]
struct KeyPair {
    public_key: String,
    /// Where the secret key file was saved, for display only.
    path: String,
}

/// Creates an age key pair. The secret key goes straight into a file the
/// user picks in a save dialog; only the public key and that file's
/// location return to the page. Holds `busy` like a batch, so no job can
/// start while the dialog is open.
#[tauri::command]
async fn create_key_pair(state: State<'_, AppState>) -> Result<Option<KeyPair>, String> {
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err("A job is already running".into());
    }
    let result = async {
        let Some(file) = rfd::AsyncFileDialog::new()
            .set_title("Save your secret key")
            .set_file_name("age-secret-key.txt")
            .save_file()
            .await
        else {
            return Ok(None);
        };
        let path = file.path().to_owned();
        if path.exists() {
            return Err("Output already exists. Choose a new filename.".into());
        }
        let public_key = tauri::async_runtime::spawn_blocking({
            let path = path.clone();
            move || converter_core::crypto::write_identity_file(&path, &AtomicBool::new(false))
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
        Ok(Some(KeyPair {
            public_key,
            path: path.to_string_lossy().into_owned(),
        }))
    }
    .await;
    state.busy.store(false, Ordering::SeqCst);
    result
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let profile = app
                .path()
                .app_local_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("doc-converter"))
                .join("lo-profile");
            app.manage(AppState {
                inspections: Mutex::new(HashMap::new()),
                files: Mutex::new(HashMap::new()),
                next: AtomicU64::new(0),
                busy: AtomicBool::new(false),
                cancel: AtomicBool::new(false),
                engines: Mutex::new(None),
                preview_gate: tauri::async_runtime::Mutex::new(()),
                preview_cancel: Mutex::new(None),
            });
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                job::clean_stale_work_dirs();
                let office = OfficeEngine::detect(&profile);
                let state = handle.state::<AppState>();
                if let Ok(mut engines) = state.engines.lock() {
                    *engines = Some(Engines {
                        office: office.clone(),
                    });
                }
                let _ = handle.emit("engines-ready", ());
                if let Some(office) = office {
                    // Creates the private profile so the first conversion is fast.
                    let _ = office.warm_up();
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pick_files,
            add_paths,
            engine_status,
            run_batch,
            cancel_job,
            refresh_outputs,
            outline,
            inspect_pdf,
            open_inspected_pdf,
            preview,
            validate_pdf,
            create_key_pair
        ])
        .run(tauri::generate_context!())
        .expect("Unable to start Doc Converter");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure(keys: Result<Keys, String>) -> String {
        match keys {
            Ok(_) => panic!("expected the keys to be refused"),
            Err(message) => message,
        }
    }

    #[test]
    fn archival_requests_are_not_silently_normalized_to_plain_pdf() {
        let state = AppState {
            inspections: Mutex::new(HashMap::new()),
            files: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            busy: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            engines: Mutex::new(None),
            preview_gate: tauri::async_runtime::Mutex::new(()),
            preview_cancel: Mutex::new(None),
        };
        for format in ["pdfa1b", "pdfa2b", "pdfa3b", "pdfa4", "pdfua1"] {
            assert!(parse_format(Some(format)).unwrap().is_standard_pdf());
            for (protect, merge) in [(true, false), (false, true)] {
                let result =
                    build_tasks(&state, &request("convert", &[format], protect, merge, ""));
                assert!(result
                    .unwrap_err()
                    .contains("PDF/A or PDF/UA export cannot"));
            }
        }
    }

    fn request(
        mode: &str,
        formats: &[&str],
        protect: bool,
        merge: bool,
        password: &str,
    ) -> BatchRequest {
        BatchRequest {
            mode: mode.into(),
            items: formats
                .iter()
                .enumerate()
                .map(|(i, f)| RequestItem {
                    id: i.to_string(),
                    format: Some((*f).to_string()),
                    page_breaks: Vec::new(),
                })
                .collect(),
            merge,
            merge_name: String::new(),
            layout: Layout::default(),
            password: password.into(),
            protect,
            watermark: None,
            encryption: "file".into(),
            recipients: String::new(),
            identity: String::new(),
            image: ImageSettings::default(),
            attachments: Vec::new(),
        }
    }

    #[test]
    fn password_is_required_only_when_a_pdf_gets_protected() {
        assert!(check_password(&request("clean", &["pdf", "docx"], true, true, "")).is_ok());
        assert!(check_password(&request("convert", &["txt", "html"], true, false, "")).is_ok());
        assert!(check_password(&request("convert", &["txt", "pdf"], true, false, "")).is_err());
        assert!(check_password(&request(
            "convert",
            &["txt", "pdf"],
            true,
            false,
            "twelve chars!"
        ))
        .is_ok());
        assert!(check_password(&request("convert", &["txt"], true, true, "short")).is_err());
        assert!(check_password(&request("convert", &["pdf"], false, false, "")).is_ok());
        assert!(check_password(&request("encrypt", &[], false, false, "short")).is_err());
        assert!(check_password(&request("decrypt", &[], false, false, "")).is_err());
        assert!(check_password(&request("decrypt", &[], false, false, "x")).is_ok());
    }

    #[test]
    fn public_key_mode_needs_keys_instead_of_a_password() {
        let dir =
            std::env::temp_dir().join(format!("doc-converter-key-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("key.txt");
        let _ = std::fs::remove_file(&file);
        let public =
            converter_core::crypto::write_identity_file(&file, &AtomicBool::new(false)).unwrap();
        let key_file = std::fs::read_to_string(&file).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        let mut r = request("encrypt", &[], false, false, "");
        r.encryption = "key".into();
        assert!(check_password(&r).is_ok(), "recipients need no password");
        assert!(failure(parse_keys(&r)).contains("at least one public key"));
        r.recipients = format!("# colleague\n{public}\n");
        assert_eq!(parse_keys(&r).unwrap().0.len(), 1);
        r.recipients = key_file.clone();
        assert!(failure(parse_keys(&r)).contains("secret key"));
        r.encryption = "file".into();
        assert!(check_password(&r).is_err(), "password mode still needs one");
        assert!(
            parse_keys(&r).unwrap().0.is_empty(),
            "keys are ignored outside key mode"
        );

        let mut d = request("decrypt", &[], false, false, "");
        assert!(check_password(&d).unwrap_err().contains("secret key"));
        d.identity = public.clone();
        assert!(check_password(&d).is_ok());
        assert!(failure(parse_keys(&d)).contains("public key"));
        d.identity = key_file;
        let identity = parse_keys(&d).unwrap().1.unwrap();
        assert_eq!(identity.to_public().to_string(), public);
        d.identity = String::new();
        d.password = "x".into();
        assert!(parse_keys(&d).unwrap().1.is_none());
    }
}
