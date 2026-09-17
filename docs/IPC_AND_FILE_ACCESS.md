---
title: "IPC Commands And File Access"
description: "The eight Tauri commands, two events, backend-owned input IDs, native dialogs, drag-and-drop, and the capability file."
type: "reference"
tags:
  - ipc
  - tauri
  - file-access
resource: "docs/IPC_AND_FILE_ACCESS.md"
last_updated: "2026-09-16"
source_sync: "manual"
---

# IPC Commands And File Access

The frontend talks to Rust through eight commands and listens to two events. It never receives a filesystem path for reading, and the only paths it sends are the ones the native drag-and-drop event handed it. The backend mints an opaque ID per file and accepts only those IDs.

Owning files:

- `apps/desktop/src-tauri/src/main.rs` - commands, events, `AppState`
- `apps/desktop/src-tauri/capabilities/default.json` - webview permissions
- `apps/desktop/src/api.ts` - the typed `invoke` wrappers

## Why IDs instead of paths

- No Tauri filesystem, shell, HTTP or dialog plugin is enabled. The page cannot open, read, list or write files.
- The path map lives in Rust. A compromised page can only name IDs the user already added.
- Save locations and folders come from native dialogs run by Rust.

## Capability

`capabilities/default.json` grants `core:default` and `core:webview:allow-set-webview-zoom` to the `main` window. The first includes `core:event:default`, which the page needs to listen for events and for the native drag-and-drop event; the second lets the UI scale setting zoom the webview. Application commands are allowed for local content by Tauri's default policy because the app defines no ACL manifest of its own.

## Commands

### `pick_files() -> Asset[]`

Refused while busy. Opens the native multi-select picker, canonicalizes each path, skips non-files, inspects the kind, and returns assets. Empty list on cancel.

### `add_paths(paths: string[]) -> Asset[]`

Same registration for paths dropped on the window. The paths come from Tauri's drag-and-drop event, which the native layer produces; they are still canonicalized and checked to be regular files. At most 500 per call. Refused while busy.

### `engine_status() -> { ready, office }`

`ready` is false until detection finished. `office` is `{ path, version, markdown }` or `null`. See [`ENGINES.md`](ENGINES.md).

### `run_batch(request) -> { result, reports }`

Runs a batch; see [`JOB_LIFECYCLE.md`](JOB_LIFECYCLE.md). `result` is `saved`, `nothing` or `cancelled`.

```typescript
type BatchRequest = {
  mode: 'convert' | 'images' | 'encrypt' | 'decrypt';
  items: { id: string; format?: string; page_breaks?: number[] }[];
  merge: boolean;              // convert only
  merge_name: string;
  layout: { orientation: 'keep' | 'portrait' | 'landscape';
            margins: 'narrow' | 'normal' | 'wide';
            spacing: 'compact' | 'comfortable' | 'spacious';
            page_breaks: number[] };   // batch-wide part; per-item breaks win
  password: string;
  protect: boolean;            // convert: protect PDF outputs
  encryption: 'pdf' | 'file';  // encrypt tab
  image: { max_edge: number; quality: number };
};
```

Errors reject with a string: `Select at least one file.`, the password messages, `A job is already running`, `Select the file again`, `Unknown output format …`, `Output already exists. Choose a new filename.`

### `cancel_job()`

Sets the cancel flag. Returns immediately.

### `outline(id) -> OutlineEntry[]`

Blocks (Markdown, text, HTML) or body paragraphs (DOCX) as `{ index, kind, text }`. Empty for other kinds.

### `preview(id, format, layout) -> ArrayBuffer`

The PDF the current settings would produce for the row's target `format` (`pdf` or `docx`), as raw bytes through Tauri's binary response. A newer call cancels the previous preview; a running batch refuses it. Images preview as a one-page PDF.

### `refresh_outputs(ids) -> { id, outputs }[]`

Recomputes the Convert options for queued files with the engines known now. The page calls it when `engines-ready` fires, so files added before detection finished get their Office formats.

## Events

| Event | Payload | When |
| --- | --- | --- |
| `engines-ready` | none | engine detection finished at startup |
| `batch-progress` | `ItemReport { index, status, detail, output }` | every item state change during `run_batch` |

## Asset shape

```typescript
type Asset = {
  id: string; name: string; bytes: number;
  kind: InputKind; label: string;          // e.g. 'docx', 'DOCX'
  outputs: Availability[];                 // Convert options for documents, else []
  pdf: { pages: number; encrypted: boolean } | null;
  image: { kind: InputKind; width: number; height: number } | null;
};
```

`outputs` is computed with the engines known at the time the file was added and refreshed through `refresh_outputs` once detection finishes.

## Drag and drop

`getCurrentWebview().onDragDropEvent` fires `enter`, `over`, `leave` and `drop`. The page highlights the drop zone on enter/over and calls `add_paths` on drop with the event's paths. Dropping works anywhere in the window.

## Browser preview

When `isTauri()` is false, the page shows a notice and disables Add and the primary action. Everything else renders, so layout work can happen with `pnpm dev` alone.

## Not exposed

- reading file contents into the page (previews are rendered PDFs, not the source)
- listing directories
- deleting or overwriting files
- opening the output after save
