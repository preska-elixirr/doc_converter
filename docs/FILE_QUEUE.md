---
title: "File Queue"
description: "The multi-select queue table: adding by picker or drop, per-row formats, eligibility per mode, statuses, and removal."
type: "guide"
tags:
  - queue
  - file-picker
  - drag-and-drop
  - ui
resource: "docs/FILE_QUEUE.md"
last_updated: "2026-09-16"
source_sync: "manual"
---

# File Queue

The left column lists every file the user has added. Rows carry a checkbox, so one batch can process many files. The queue is shared by all tabs; switching tabs keeps the rows and re-evaluates which ones the tab can use.

Owning files:

- `apps/desktop/src/App.tsx` - `Row`, `blocked`, `statusText`, the table
- `apps/desktop/src-tauri/src/main.rs` - `pick_files`, `add_paths`, `register`

## Adding files

- **Add files** and the drop zone open the native multi-select picker.
- **Drag and drop** from Explorer works anywhere in the window; the drop zone lights up while dragging.
- Files are appended; adding the same file twice makes two rows with different IDs.
- Each new row starts selected, with the output format defaulting to PDF when available.

## Columns

| Column | Content |
| --- | --- |
| Selected | Checkbox. The header checkbox selects all, shows indeterminate for a partial selection. |
| Document | Type badge (from content detection, not extension), name, size, and extra facts: page count and `locked` for PDFs, `width × height` for images. |
| Output | Convert: a per-row select of PDF, DOCX, TXT, HTML, MD; formats the file cannot reach are marked ✕ and disabled, with the reason as tooltip. `Merged PDF` when combining. Images: PNG, JPG, WEBP, PDF. Encrypt: `PDF + key` or `.age`. Decrypt: `Original`. The header reads `RESTORE TO` in Decrypt. |
| Status | See below. |
| Remove | Drops the row. The backend keeps the path until exit. |

`Clear all` empties the queue. `N selected` counts checked rows regardless of eligibility.

## Eligibility per mode

`blocked(row, mode, protection)` returns the reason shown in the status column, or nothing when the row can run. Blocked rows stay in the queue, keep their checkbox, and are skipped; the action note counts them.

| Mode | Eligible | Reason otherwise |
| --- | --- | --- |
| Convert | PDF, DOCX, ODT, PPTX, XLSX, MD, HTML, TXT | `Unsupported file` |
| Images | PNG, JPG, BMP, WebP, TIFF | `Unsupported file` |
| Encrypt, password-protected PDF | PDF without a password | `PDF required`, `Already protected` |
| Encrypt, encrypted file | anything | none |
| Decrypt | locked PDF, `.age` | `No password set`, `Unsupported file` |

Kinds come from the backend's content sniffing, so a `.txt` that is really a PDF is treated as a PDF.

## Statuses

| Status | Colour | Meaning |
| --- | --- | --- |
| Ready | green | Will run when the action is pressed |
| Queued | green | Batch started, waiting |
| Working…, Converting to PDF…, Ready to merge | grey | Backend detail text |
| Saved, Combined | green | Output written; the path is in the tooltip |
| Failed | red | The error message |
| Cancelled | amber | Stopped before or during the item |
| blocked reasons | amber | Not eligible in this mode |

Selecting rows, changing a format, or switching tabs clears finished statuses back to Ready and hides the result bar.

## Differences from the accepted design

Implemented from the mockup: multi-select with select-all, per-row output select, status column, drop zone, `Clear all`, type badges, size and metadata line. Not implemented: `Load example files`, because the app never fabricates files.
