---
title: "Project Context"
description: "Agent entry context for Doc Converter architecture, invariants, ownership, and pitfalls."
type: "guide"
tags:
  - architecture
  - agent-entry
  - project-context
resource: "docs/PROJECT_CONTEXT.md"
last_updated: "2026-09-17"
source_sync: "manual"
---

# Project Context

This document summarizes the current architecture and working assumptions for Doc Converter. It stays close to the implementation. It is not a product roadmap; the roadmap is in [`APPLICATION_PLAN.md`](APPLICATION_PLAN.md).

## How To Use This Document

Read this file first when starting non-trivial work.

- Use it to find the owning module, command, or UI section and the likely feature doc.
- Do not treat it as a substitute for reading implementation files.
- If this file conflicts with code, trust code and update this file only when the mismatch affects your task.

For exported functions and commands, start at [`methods_report/README.md`](methods_report/README.md). For discovery, use [`index.md`](index.md).

## Overview

Doc Converter is a Windows-first Tauri 2 desktop app. It processes local files in batches and never uploads anything. The second milestone ships everything the accepted mockup shows:

- documents to PDF, DOCX, TXT, HTML or Markdown, with a backend-computed capability matrix
- combining documents into one PDF, in a chosen order
- page orientation, margins, paragraph spacing and real page breaks for Word, Markdown and text sources, with a live preview of the real output
- text watermarks on every page of ordinary PDF conversions and merges
- PDF open passwords (AES-256) on conversion, on existing PDFs, and their removal
- images (PNG, JPEG, BMP, WebP, TIFF) to PNG, JPG, WebP or a PDF page, with resize and quality
- `.age` encryption of any file with a password or for a recipient's public key, key pair creation, and authenticated restore with the password or the secret key
- **Clean before sharing**: DOCX review/hidden-content removal, PDF metadata removal, photo metadata removal into PNG copies
- a multi-select queue with drag and drop, per-row formats, per-row statuses and progress

A License tab exists only as a placeholder. Nothing is simulated.

## Source Of Truth

When documentation conflicts, trust implementation files first.

Highest-signal files:

- `crates/core/src/job.rs` - batch execution, routes to engines, previews, output naming
- `crates/core/src/capability.rs` - which input becomes which output with which engine
- `crates/core/src/office.rs` - LibreOffice detection and isolated execution
- `crates/core/src/watermark.rs` - local text overlays for ordinary PDF outputs
- `crates/core/src/pdf.rs` - passwords, unlock, merge, text, image pages
- `crates/core/src/layout.rs`, `docx.rs`, `text.rs` - page layout for text and Word sources
- `crates/core/src/images.rs`, `crypto.rs`, `inspect.rs` - images, age, input detection
- `crates/core/src/clean.rs`, `clean/word.rs` - privacy cleaning and DOCX package sanitization
- `apps/desktop/src-tauri/src/main.rs` - Tauri commands, app state, dialogs, job gating, engine startup
- `apps/desktop/src/App.tsx` - the whole UI; `api.ts` the contract; `Preview.tsx` PDF.js
- `apps/desktop/src-tauri/tauri.conf.json` and `capabilities/default.json` - CSP, window, permissions

## Module Map

| Module | Doc |
| --- | --- |
| Engines and LibreOffice isolation | [`ENGINES.md`](ENGINES.md) |
| Convert tab, matrix, routes, merge | [`DOCUMENT_CONVERSION.md`](DOCUMENT_CONVERSION.md) |
| Page layout, page breaks, preview | [`PAGE_LAYOUT.md`](PAGE_LAYOUT.md) |
| Archival PDF/A-1b, PDF/A-2b, PDF/A-3b, PDF/A-4, PDF/A-4f and PDF/UA-1 export | [`PDF_A_EXPORT.md`](PDF_A_EXPORT.md) |
| PDF passwords, unlock, merge, text, image pages | [`PDF_TOOLS.md`](PDF_TOOLS.md) |
| Encrypt and Decrypt tabs | [`ENCRYPTION.md`](ENCRYPTION.md) |
| Images tab | [`IMAGE_CONVERSION.md`](IMAGE_CONVERSION.md) |
| Clean before sharing | [`CLEAN_BEFORE_SHARING.md`](CLEAN_BEFORE_SHARING.md) |
| Batches, destinations, cancellation | [`JOB_LIFECYCLE.md`](JOB_LIFECYCLE.md) |
| Commands, events, drag and drop | [`IPC_AND_FILE_ACCESS.md`](IPC_AND_FILE_ACCESS.md) |
| Queue table | [`FILE_QUEUE.md`](FILE_QUEUE.md) |
| UI structure | [`WORKSPACE_UI.md`](WORKSPACE_UI.md) |
| Security posture | [`SECURITY_AND_PRIVACY.md`](SECURITY_AND_PRIVACY.md) |

## Invariants

- **Sources are never modified.** Every edit happens on a copy in the batch work folder; every output is a new file, committed with `persist_noclobber`. Folder outputs get numbered names.
- **The backend decides.** Input kinds come from content sniffing; the capability matrix, password rules and eligibility are enforced in Rust even though the UI mirrors them.
- **No paths in the page.** The page holds opaque IDs; the only paths it sends are the ones the native drop event gave it, and the backend re-validates them.
- **Secrets stay in process.** Passwords are `SecretString`; PDF passwords go to lopdf in memory; LibreOffice never sees a secret or a command-line password. age secret keys are parsed into `x25519::Identity` in the command layer; a generated secret key goes straight to the user's file and only the public key returns to the page.
- **LibreOffice is isolated.** Private profile, macro security very high, update check off, job object with kill-on-close, timeouts. The user's own LibreOffice is untouched.
- **User text is data.** The Typst template receives blocks as JSON and never evaluates document text as markup.
- **One job at a time.** `busy` covers batches and dialogs; previews run one at a time behind their own gate and are cancelled by a newer preview or a batch. Cancel flags are checked at every stage boundary.
- **Standard formats only.** `.age` for files, PDF 2.0 AES-256 for PDFs. No custom container.

## Pitfalls

- lopdf's plain `Document::load` returns an empty object table for a PDF whose password it does not know. Use `load_with_password` / `load_mem_with_password` for locked files; `unlock` and the tests do.
- LibreOffice exits with status 0 even when a conversion fails. The adapter checks that the expected output exists and is non-empty.
- A Tauri page can listen to events only with a capability. `capabilities/default.json` grants `core:default`; deleting it breaks progress, engine status and drag and drop silently.
- `Asset.outputs` is computed when the file is added; the page refreshes it through `refresh_outputs` when `engines-ready` fires. Keep that call if the startup sequence changes.
- `Layout` defaults mean *keep the document as is* for DOCX. Only non-default orientation, margins or spacing trigger a rewrite.
- Page breaks are per document (`Task.page_breaks`); the batch-wide `Layout.page_breaks` is ignored by `run`. Do not put breaks in the batch layout.
- Body paragraph indices for DOCX are stable only because `outline` and `rewrite` use the same scan (`body_paragraphs`). Do not change one without the other.
- The `.tools/libreoffice` copy is a developer convenience found by walking up from the executable. A packaged build must ship LibreOffice under `engines/libreoffice` next to the executable or document the prerequisite.
- The first LibreOffice start creates the profile and can take 20 seconds; `warm_up` at startup hides that from the user.
- Two LibreOffice runs must not overlap on the same profile: the second one hands its work to the first. The preview gate exists for that reason; do not run previews and batches concurrently.
- `apply_spacing` must cope with self-closing `<w:pPrDefault/>` and `<w:docDefaults/>` tags and must never touch `w:rPr/w:spacing` (character spacing). The regression test covers every shape.
- `cargo test` runs the LibreOffice test only when a copy is found; a passing suite on a machine without LibreOffice has not exercised Office conversion.

## Ownership

| Area | Files |
| --- | --- |
| Core processing | `crates/core/src/*.rs` |
| Desktop shell | `apps/desktop/src-tauri/src/main.rs`, `tauri.conf.json`, `capabilities/` |
| UI | `apps/desktop/src/*.ts`, `*.tsx`, `style.css` |
| Design source | `opendesign/`, `design/` |
| Docs | `docs/` (this convention), `README.md` |
