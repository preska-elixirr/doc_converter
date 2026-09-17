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
    /// Encrypt tab: `pdf` or `file`.
    #[serde(default)]
    encryption: String,
    #[serde(default)]
    image: ImageSettings,
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
        "docx" => Ok(OutputFormat::Docx),
        "txt" => Ok(OutputFormat::Txt),
        "html" => Ok(OutputFormat::Html),
        "md" => Ok(OutputFormat::Md),
        other => Err(format!("Unknown output format {other}")),
    }
}

fn build_tasks(state: &AppState, request: &BatchRequest) -> Result<Vec<Task>, String> {
    let mut tasks = Vec::new();
    for item in &request.items {
        let (source, kind) = lookup(state, &item.id)?;
        let name = source
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let action = match request.mode.as_str() {
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
    let needs_new = request.mode == "encrypt" || protects_pdf;
    if needs_new && chars < 12 {
        return Err("Use a password with at least 12 characters.".into());
    }
    if request.mode == "decrypt" && chars == 0 {
        return Err("Enter the password.".into());
    }
    Ok(())
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
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err("A job is already running".into());
    }
    state.cancel.store(false, Ordering::SeqCst);
    let result = async {
        cancel_preview(&state);
        let _gate = state.preview_gate.lock().await;
        let tasks = build_tasks(&state, &request)?;
        let password = (!request.password.is_empty()).then(|| secret(request.password.clone()));
        let batch = Batch {
            tasks,
            merge: request.mode == "convert" && request.merge,
            layout: request.layout.clone(),
            password,
            protect: request.protect,
            image: request.image.clone(),
        };
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

/// PDF bytes of the real output for the preview panel. A newer preview or a
/// batch cancels the one in flight; the gate runs them one at a time.
#[tauri::command]
async fn preview(
    state: State<'_, AppState>,
    id: String,
    format: String,
    layout: Layout,
) -> Result<tauri::ipc::Response, String> {
    let (source, kind) = lookup(&state, &id)?;
    let target = parse_format(Some(&format))?;
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
        job::preview_pdf(&source, kind, target, &layout, &engines, &flag)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    Ok(tauri::ipc::Response::new(bytes))
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
            preview
        ])
        .run(tauri::generate_context!())
        .expect("Unable to start Doc Converter");
}

#[cfg(test)]
mod tests {
    use super::*;

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
            encryption: "file".into(),
            image: ImageSettings::default(),
        }
    }

    #[test]
    fn password_is_required_only_when_a_pdf_gets_protected() {
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
}
