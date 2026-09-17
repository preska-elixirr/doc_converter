---
title: "Codebase API & Method Catalog"
description: "Index and usage guide for the compact API, command, and frontend catalog."
type: "catalog"
tags:
  - methods-report
  - api-catalog
  - agent-entry
resource: "docs/methods_report/README.md"
last_updated: "2026-09-17"
doc_version: "2.0.0"
source_sync: "manual"
---

# Codebase API & Method Catalog

This catalog tracks every public Rust item in `crates/core`, every Tauri command and event in `apps/desktop/src-tauri`, and the exported types and invoke contracts in the frontend. Private helpers are listed when they carry a behavioural contract.

**Mandatory update rule**: when a public function, command, argument, return type, or side effect changes, update the matching section file in the same task.

## Sections

- [01 Converter Core](01_converter_core.md) - `converter-core` crate: `lib`, `inspect`, `capability`, `crypto`, `images`, `pdf`, `office`, `text`, `layout`, `docx`, `clean`, `job`
- [02 Desktop Commands](02_desktop_commands.md) - `AppState`, request and asset shapes, the seven commands, two events, startup
- [03 Frontend](03_frontend.md) - TypeScript types, invoke wrappers, `App` state and derived flags, `PdfPreview`

## Coverage Policy

- Every `pub` item in `crates/core/src/*.rs` is listed under its module heading.
- Every `#[tauri::command]` is listed with its JavaScript-side argument names.
- Frontend entries cover exported and module-level types plus the `invoke` contracts. JSX is described, not declared.
- Tests are outside the scan; they are listed in `BUILD_AND_VERIFICATION.md`.

There is no automated drift checker yet. Compare by reading the source; the core is about 3,000 lines of Rust, the shell 430, the frontend 600 lines of TypeScript.

## Format

Rust entries use Rust signatures in fenced blocks with a trailing comment for the contract. Frontend entries use TypeScript.

PDF standards APIs now include `pdf_standards::{Attachment, Relationship, embed}`,
`validation::{Validator, ValidationReport}`, the `validate_pdf` command and the
`PdfStandards`/`ValidationDetails` components. See the corresponding catalog entries.
