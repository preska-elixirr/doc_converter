---
title: "Implementation Status"
description: "What the first native milestone delivered, what was verified, and what remains outstanding."
type: "audit"
tags:
  - status
  - milestone
resource: "docs/IMPLEMENTATION_STATUS.md"
last_updated: "2026-09-15"
source_sync: "manual"
---

# Implementation status — 15 September 2026

## First native milestone delivered

- Cargo workspace with a reusable Rust processing crate and Tauri desktop shell.
- React/TypeScript interface using the design's green/neutral palette.
- Native file selection and safe save-as flow with opaque backend file IDs.
- Standard passphrase-based age encryption and authenticated decryption.
- Static PNG/JPG/BMP inputs, PNG/JPG output, resize without upscaling, EXIF orientation, JPEG quality, white alpha flattening; APNG rejected explicitly.
- Background processing with cooperative cancellation and no-overwrite temporary-file commit.
- No document uploads and no licensing-network calls.

## Verified

- TypeScript check and Vite production build passed.
- Windows executable compiled with embedded frontend assets.
- Rust tests passed: byte-identical encryption round trip; wrong-password and truncated-file rejection without publishing plaintext; destination collision; resize/aspect ratio; transparency flattening; original-source preservation; cancellation without output.
- Rust formatting check passed.
- Native executable started and exposed a window titled “Doc Converter — Development”. This is a startup smoke check, not a complete end-to-end native UI interaction test.

Executable: `target/debug/doc-converter.exe` (development build, not signed installer).

## Scope still outstanding

LibreOffice and qpdf were not found in the checked environment. Their adapters, real PDF preview, PDF passwords, merging, pagination/landscape editing, WebP/TIFF, multi-file scheduling, process isolation, crash cleanup, license activation/service and release packaging remain unimplemented. The app labels document and licensing features as unavailable instead of simulating them.

Next implementation milestone: install/configure the document engines and connect DOCX → PDF → preview → password-protected PDF. Choose the licensing service independently before integrating activation.

See the root README for commands and current safety/format limitations.
