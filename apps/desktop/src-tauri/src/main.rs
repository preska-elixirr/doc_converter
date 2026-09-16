#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use serde::Serialize;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex,
    },
};
use tauri::State;

#[derive(Default)]
struct AppState {
    files: Mutex<HashMap<String, PathBuf>>,
    next: AtomicU64,
    busy: AtomicBool,
    cancel: AtomicBool,
}
#[derive(Serialize)]
struct Asset {
    id: String,
    name: String,
    bytes: u64,
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
    let mut files = state.files.lock().map_err(|_| "File state unavailable")?;
    for file in picked {
        let path = file.path().canonicalize().map_err(|e| e.to_string())?;
        let metadata = path.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() {
            continue;
        }
        let id = state.next.fetch_add(1, Ordering::Relaxed).to_string();
        result.push(Asset {
            id: id.clone(),
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            bytes: metadata.len(),
        });
        files.insert(id, path);
    }
    Ok(result)
}

#[tauri::command]
fn cancel_job(state: State<'_, AppState>) {
    state.cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
async fn process_file(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    operation: String,
    password: String,
    format: String,
    max_edge: u32,
    quality: u8,
) -> Result<String, String> {
    if !["encrypt", "decrypt", "image"].contains(&operation.as_str()) {
        return Err("This operation is not implemented yet.".into());
    }
    if operation == "encrypt" && password.chars().count() < 12 {
        return Err("Use a password with at least 12 characters.".into());
    }
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err("A job is already running".into());
    }
    state.cancel.store(false, Ordering::SeqCst);
    let result = async {
        let source = state
            .files
            .lock()
            .map_err(|_| "File state unavailable")?
            .get(&id)
            .cloned()
            .ok_or("Select the file again")?;
        let name = source.file_name().unwrap_or_default().to_string_lossy();
        let suggested = match operation.as_str() {
            "encrypt" => format!("{name}.age"),
            "decrypt" => format!("restored-{}", name.strip_suffix(".age").unwrap_or(&name)),
            _ => format!(
                "{}-converted.{format}",
                source.file_stem().unwrap_or_default().to_string_lossy()
            ),
        };
        let Some(destination) = rfd::AsyncFileDialog::new()
            .set_file_name(&suggested)
            .save_file()
            .await
        else {
            return Ok("Save cancelled. No output created.".into());
        };
        let destination = destination.path().to_owned();
        if destination.exists() {
            return Err("Output already exists. Choose a new filename.".into());
        }
        let label = destination.to_string_lossy().to_string();
        tauri::async_runtime::spawn_blocking(move || {
            use tauri::Manager;
            let state = app.state::<AppState>();
            match operation.as_str() {
                "encrypt" => converter_core::encrypt(
                    &source,
                    &destination,
                    converter_core::secret(password),
                    &state.cancel,
                ),
                "decrypt" => converter_core::decrypt(
                    &source,
                    &destination,
                    converter_core::secret(password),
                    &state.cancel,
                ),
                _ => converter_core::convert_image(
                    &source,
                    &destination,
                    &format,
                    max_edge,
                    quality,
                    &state.cancel,
                ),
            }
            .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??;
        Ok(format!("Saved {label}"))
    }
    .await;
    state.busy.store(false, Ordering::SeqCst);
    result
}

fn main() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            pick_files,
            process_file,
            cancel_job
        ])
        .run(tauri::generate_context!())
        .expect("Unable to start Doc Converter");
}
