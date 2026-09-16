---
title: "Codebase API & Method Catalog"
description: "Index and usage guide for the compact API, command, and frontend catalog."
type: "catalog"
tags:
  - methods-report
  - api-catalog
  - agent-entry
resource: "docs/methods_report/README.md"
last_updated: "2026-09-16"
doc_version: "1.0.0"
source_sync: "manual"
---

# Codebase API & Method Catalog

This catalog tracks every public Rust item in `crates/core`, every Tauri command in `apps/desktop/src-tauri`, and the exported types and invoke contracts in the frontend. Private helpers are listed when they carry a behavioural contract.

**Mandatory update rule**: when a public function, command, argument, return type, or side effect changes, update the matching section file in the same task.

## Sections

- [01 Converter Core](01_converter_core.md) - `converter-core` crate: errors, encrypt, decrypt, convert_image, secret, and the temp-file helpers
- [02 Desktop Commands](02_desktop_commands.md) - `AppState`, `Asset`, and the `pick_files`, `process_file`, `cancel_job` commands
- [03 Frontend](03_frontend.md) - TypeScript types, invoke signatures, and component state

## Coverage Policy

- Every `pub` item in `crates/core/src/lib.rs` is listed under its module heading.
- Every `#[tauri::command]` is listed with its JavaScript-side argument names.
- Frontend entries cover exported and module-level types plus the `invoke` contracts. Internal JSX helpers are described, not declared.
- Tests are outside the scan.

There is no automated drift checker yet. Compare by reading the source; the three files together are under 450 lines.

## Format

Rust entries use Rust signatures in fenced blocks with a trailing comment for the contract. Frontend entries use TypeScript.
