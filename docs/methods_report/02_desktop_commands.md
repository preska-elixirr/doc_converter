---
title: "Desktop Commands"
description: "API catalog for the Tauri shell: application state, asset shape, and the three invoke commands."
type: "catalog"
tags:
  - methods-report
  - tauri
  - commands
resource: "docs/methods_report/02_desktop_commands.md"
last_updated: "2026-09-16"
doc_version: "1.0.0"
source_sync: "manual"
---

# Desktop Commands

Binary `doc-converter`, file `apps/desktop/src-tauri/src/main.rs`. Dependencies: `tauri 2`, `converter-core`, `rfd 0.16`, `serde 1`. Feature `custom-protocol` enables the embedded frontend.

### `apps/desktop/src-tauri/src/main.rs`

```rust
#[derive(Default)]
struct AppState {
    files: Mutex<HashMap<String, PathBuf>>, // opaque id -> canonical path; grows only
    next: AtomicU64,                        // next id, starts at 0
    busy: AtomicBool,                       // one job at a time, includes save dialog
    cancel: AtomicBool,                     // set by cancel_job, reset per job
}

#[derive(Serialize)]
struct Asset {
    id: String,
    name: String, // file name only
    bytes: u64,
}

#[tauri::command]
async fn pick_files(state: State<'_, AppState>) -> Result<Vec<Asset>, String>;
// Err("A job is running") while busy. Native multi-select picker. Canonicalizes,
// skips non-files, mints ids, stores paths. Empty Vec on cancel.

#[tauri::command]
fn cancel_job(state: State<'_, AppState>);
// Sets cancel = true. Non-blocking.

#[tauri::command]
async fn process_file(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    operation: String, // "encrypt" | "decrypt" | "image"
    password: String,
    format: String,    // "png" | "jpg"
    max_edge: u32,     // JS name: maxEdge
    quality: u8,
) -> Result<String, String>;
// Order: operation check -> encrypt password >= 12 chars -> busy swap -> cancel reset
// -> id lookup -> suggested name -> save dialog -> destination exists check
// -> spawn_blocking(core) -> busy reset.
// Ok("Saved {path}") or Ok("Save cancelled. No output created.").
```

### Error strings

| Check | Text |
| --- | --- |
| operation not in the three | `This operation is not implemented yet.` |
| encrypt password under 12 chars | `Use a password with at least 12 characters.` |
| busy on `pick_files` | `A job is running` |
| busy on `process_file` | `A job is already running` |
| mutex poisoned | `File state unavailable` |
| unknown id | `Select the file again` |
| destination exists | `Output already exists. Choose a new filename.` |
| core error | The `converter_core::Error` display text |
| blocking task panicked | The join error text |

### Suggested output names

```text
encrypt: "{name}.age"
decrypt: "restored-{name minus trailing .age}"
image:   "{stem}-converted.{format}"
```

### `apps/desktop/src-tauri/tauri.conf.json`

```json
productName: "Doc Converter"
identifier:  "com.privateconverter.desktop"
build:       frontendDist "../dist", devUrl "http://localhost:1420"
window:      title "Doc Converter — Development", 1220×840, min 800×620
csp:         default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline';
             img-src 'self' data:; connect-src ipc: http://ipc.localhost
bundle:      active false
```

No plugins and no capability files are configured beyond the generated defaults in `gen/schemas/`.
