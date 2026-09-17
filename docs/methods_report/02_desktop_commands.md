---
title: "Desktop Commands"
description: "API catalog for the Tauri shell: application state, asset and request shapes, the eight invoke commands, and the two events."
type: "catalog"
tags:
  - methods-report
  - tauri
  - commands
resource: "docs/methods_report/02_desktop_commands.md"
last_updated: "2026-09-16"
doc_version: "2.0.0"
source_sync: "manual"
---

# Desktop Commands

Binary `doc-converter`, file `apps/desktop/src-tauri/src/main.rs`. Dependencies: `tauri 2`, `converter-core`, `rfd 0.16`, `serde 1`. Feature `custom-protocol` enables the embedded frontend.

### State and shapes

```rust
struct Entry { path: PathBuf, kind: InputKind }
struct AppState {
    files: Mutex<HashMap<String, Entry>>,   // opaque id -> canonical path and kind; grows only
    next: AtomicU64,                        // next id, starts at 0
    busy: AtomicBool,                       // one job or preview at a time, includes dialogs
    cancel: AtomicBool,                     // set by cancel_job, reset per job
    engines: Mutex<Option<Engines>>,        // None until detection finished
    preview_gate: tauri::async_runtime::Mutex<()>,   // previews one at a time, batches hold it
    preview_cancel: Mutex<Option<Arc<AtomicBool>>>,  // flag of the running or waiting preview
}

#[derive(Serialize)]
struct Asset { id: String, name: String, bytes: u64, kind: InputKind, label: String,
               outputs: Vec<Availability>, pdf: Option<PdfInfo>, image: Option<ImageInfo> }
#[derive(Serialize)] struct EngineStatus { ready: bool, office: Option<OfficeEngine> }
#[derive(Serialize)] struct AssetOutputs { id: String, outputs: Vec<Availability> }
#[derive(Deserialize)] struct RequestItem { id: String, format: Option<String>, page_breaks: Vec<usize> }
#[derive(Deserialize)]
struct BatchRequest { mode: String, items: Vec<RequestItem>, merge: bool, merge_name: String, layout: Layout,
                      password: String, protect: bool, encryption: String, image: ImageSettings }
#[derive(Serialize)] struct BatchOutcome { result: String, reports: Vec<ItemReport> }  // "saved" | "cancelled" | "nothing"
```

All request fields except `mode` and `items` have serde defaults.

### Commands

```rust
#[tauri::command] async fn pick_files(state) -> Result<Vec<Asset>, String>;
// Err("A job is running") while busy. Native multi-select picker; registers regular files; empty Vec on cancel.

#[tauri::command] fn add_paths(state, paths: Vec<String>) -> Result<Vec<Asset>, String>;
// Same registration for dropped paths; first 500; canonicalized; non-files skipped.

#[tauri::command] fn engine_status(state) -> EngineStatus;

#[tauri::command] async fn run_batch(app, state, request: BatchRequest) -> Result<BatchOutcome, String>;
// Order: items non-empty -> check_password -> busy swap -> cancel reset -> cancel preview and take the gate -> build_tasks -> destination dialog
// (merge: save dialog with merge_name; one item: save dialog with output_name; else folder picker)
// -> File must not exist -> spawn_blocking(job::run) emitting "batch-progress" -> busy reset.

#[tauri::command] fn cancel_job(state);                      // sets cancel = true

#[tauri::command] fn refresh_outputs(state, ids: Vec<String>) -> Result<Vec<AssetOutputs>, String>;
// Convert options recomputed with the engines known now; unknown ids are skipped.

#[tauri::command] async fn outline(state, id: String) -> Result<Vec<OutlineEntry>, String>;

#[tauri::command] async fn preview(state, id: String, format: String, layout: Layout) -> Result<tauri::ipc::Response, String>;
// Refused while busy. Cancels the previous preview, installs its own cancel flag, waits for the gate,
// then job::preview_pdf on a blocking thread; returns the PDF bytes as a binary response.
```

Helpers: `register(state, path) -> Result<Option<Asset>>` (inspects kind, computes `outputs` for documents, `pdf::info` for PDFs, `inspect_image` for images), `lookup(state, id)`, `parse_format`, `build_tasks`, `check_password` (a new password is required for encrypt, and for convert only when a PDF output or a merge will be protected), `cancel_preview`, `engines_or_default`.

### Startup

`setup` stores `AppState`, then a thread removes stale work folders, detects LibreOffice with the profile at `app_local_data_dir()/lo-profile`, stores `Engines`, emits `engines-ready`, and runs `warm_up`.

### Events

| Event | Payload |
| --- | --- |
| `engines-ready` | `()` |
| `batch-progress` | `ItemReport { index, status, detail, output }` |

### Error strings

| Check | Text |
| --- | --- |
| no items | `Select at least one file.` |
| new password under 12 chars | `Use a password with at least 12 characters.` |
| decrypt without password | `Enter the password.` |
| busy on pick or add | `A job is running` |
| busy on run or preview | `A job is already running` |
| mutex poisoned | `File state unavailable` |
| unknown id | `Select the file again` |
| bad format | `Unknown output format {x}` |
| bad mode | `Unknown mode {x}` |
| destination exists | `Output already exists. Choose a new filename.` |
| core error | The `converter_core::Error` display text |

### `apps/desktop/src-tauri/tauri.conf.json` and capabilities

```json
productName: "Doc Converter"
identifier:  "com.privateconverter.desktop"
build:       frontendDist "../dist", devUrl "http://localhost:1420"
window:      title "Doc Converter — Development", 1220×840, min 800×620
csp:         default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline';
             img-src 'self' data:; connect-src ipc: http://ipc.localhost
bundle:      active false
```

`capabilities/default.json`: window `main`, permissions `["core:default", "core:webview:allow-set-webview-zoom"]`.
