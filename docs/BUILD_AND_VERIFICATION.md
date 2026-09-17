---
title: "Build And Verification"
description: "Toolchain, LibreOffice for development, build and dev commands, tests, and what each check proves."
type: "guide"
tags:
  - build
  - testing
  - tooling
resource: "docs/BUILD_AND_VERIFICATION.md"
last_updated: "2026-09-17"
source_sync: "manual"
---

# Build And Verification

## Prerequisites

- Windows with the Rust MSVC toolchain and C++ build tools (the `webp` crate compiles libwebp with `cc`)
- Node.js and pnpm
- WebView2 Runtime (present on Windows 11)
- LibreOffice for Office conversions, optional for building and for most tests; see below

No CMake and no Python are needed.

## Layout

| Path | Package | Tool |
| --- | --- | --- |
| `crates/core` | `converter-core` 0.1.0 | Cargo |
| `apps/desktop/src-tauri` | `doc-converter` 0.1.0 | Cargo, Tauri 2 |
| `apps/desktop` | `doc-converter-desktop` 0.1.0 | pnpm, Vite 7, TypeScript 5.8, React 19, pdfjs-dist 6 |

Core dependencies of note: `age`, `image` (png, jpeg, bmp, webp, tiff), `webp`, `lopdf`, `typst`, `typst-pdf`, `typst-layout`, `typst-as-lib` with embedded fonts, `pulldown-cmark`, `htmd`, `zip`, `quick-xml`, `windows-sys` (job objects). Lockfiles for Cargo and pnpm are committed; prefer locked installs in CI.

## LibreOffice for development

Either install LibreOffice normally, or extract it without elevation into the ignored `.tools/` folder:

```powershell
winget download --id TheDocumentFoundation.LibreOffice -e -d $env:TEMP\lo --accept-package-agreements --accept-source-agreements
msiexec /a "$env:TEMP\lo\LibreOffice_26.8.0.3_Machine_X64_msi_en-US.msi" /qn TARGETDIR="D:\projekti\PRIVATE_DOC_CONVERTER\.tools\libreoffice"
```

The app and the tests find `.tools\libreoffice\program\soffice.exe` by walking up from the executable. `DOC_CONVERTER_SOFFICE` overrides the search. Detection details are in [`ENGINES.md`](ENGINES.md).

## Production build

```powershell
cd apps/desktop
pnpm install
pnpm run build
cd ../..
cargo build -p doc-converter --features custom-protocol
```

`pnpm run build` runs `tsc --noEmit` and then `vite build` into `apps/desktop/dist`, including the PDF.js worker as an asset. The `custom-protocol` feature makes Tauri embed that folder. Result: `target/debug/doc-converter.exe`, about 118 MB in debug. The first full build compiles Typst and takes several minutes.

Bundling is off in `tauri.conf.json`, so there is no installer or signing yet.

## Development loop

Terminal 1:

```powershell
cd apps/desktop
pnpm dev
```

Terminal 2, from the repo root:

```powershell
cargo run -p doc-converter
```

Vite serves on `127.0.0.1:1420` with `strictPort`. Rust changes need a rerun; frontend changes hot-reload. Opening the URL in a browser renders the UI but cannot pick or process files.

## Tests and checks

```powershell
cargo test -p converter-core
```

Twenty-nine tests in `crates/core/src/` and one in the desktop shell:

| Module | Test | Proves |
| --- | --- | --- |
| clean | `pdf_removes_info_nested_xmp_and_orphans_preserves_pages` | Info/ID/XMP and compressed object metadata removal, aliased Info, pages, source preservation, no overwrite, locked input and cancellation |
| clean | `photo_discards_exif_gps_camera_and_applies_orientation` | EXIF camera/GPS removal, applied orientation, metadata-free PNG, source unchanged |
| clean/word | `docx_removes_private_parts_revisions_hidden_styles_and_headers` | properties/comments, nonstandard part paths, headers, revisions, hidden styles, source preserved |
| clean/word | `hidden_default_character_table_styles_and_strict_namespaces` | hidden default character/table styles, strict OOXML namespaces, editing-conflict refusal |
| clean/word | `docx_defaults_failures_and_cancel_leave_no_output` | hidden defaults, unsupported structural revisions, malformed XML, cancellation |
| job | `cleaning_batch_reports_failures_and_numbers_copies` | mixed privacy batch, per-file failure, PNG pixels, numbered copies, cancellation |
| crypto | `crypto_roundtrip_wrong_password_and_truncation` | age round trip, wrong password, no overwrite, truncation |
| images | `image_resize_and_cancel_preserve_source` | resize, alpha flattening, source untouched, cancel |
| images | `webp_and_tiff_roundtrip_and_pdf_output` | WebP lossless and lossy, TIFF, image PDF, no overwrite |
| pdf | `protect_unlock_and_merge` | AES-256 protect, unlock, merge, locked input refused |
| pdf | `text_extraction_writes_pages` | text extraction |
| text | `markdown_blocks_and_outline` | block model, HTML and text output |
| text | `plain_text_paragraphs_and_html_markdown` | text parsing, HTML to Markdown |
| layout | `markdown_renders_with_breaks_and_landscape` | Typst page count, landscape, breaks, no code execution |
| docx | `outline_lists_body_paragraphs_only` | paragraph outline |
| docx | `rewrite_applies_layout` | orientation, margins, breaks, spacing, restore, well-formed XML, cancel leaves no file |
| docx | `spacing_handles_every_default_shape` | self-closing defaults, missing defaults, empty `pPr`, character spacing untouched |
| docx | `paired_spacing_tags_remain_well_formed` | paired spacing tags in defaults and Normal style, attributes preserved |
| docx | `page_breaks_update_existing_paragraph_properties` | empty properties, explicit false breaks, paired tags, tracked changes, repeated edits |
| html | `exported_images_survive_without_sidecars` | embedded image bytes, HTML entities and quoted/unquoted attributes |
| html | `missing_external_or_cancelled_images_leave_no_output` | incomplete exports refused without saving a partial file |
| inspect | `sniffs_by_content_and_extension` | kind detection |
| capability | `matrix_without_office` | routes without LibreOffice |
| job | `builtin_routes_run_without_office` | whole batches, merge, protection, numbering, cancel |
| job | `output_names_follow_the_action` | output names |
| job | `cleanup_skips_folders_that_are_still_in_use` | locked work folders survive cleanup, stale ones go |
| job | `spreadsheet_html_keeps_images_when_office_is_available` | actual XLSX-to-HTML batch output retains a decodable image after work cleanup |
| desktop | `password_is_required_only_when_a_pdf_gets_protected` | the password rule per mode and output (`cargo test -p doc-converter`) |
| office | `file_url_escapes_spaces` | profile URL |
| office | `converts_text_to_pdf_when_available` | a real LibreOffice conversion and cancel; skips with a message when LibreOffice is absent |

The office test creates a fresh profile in a temp folder, so it takes about 20 seconds.

```powershell
cargo fmt --all --check
```

Formatting must be clean.

```powershell
cd apps/desktop
pnpm run build
```

This is the TypeScript check; there are no frontend unit tests.

## Clean-before-sharing verification, 17 September 2026

- `cargo test -p converter-core --locked`: all 29 tests passed, including the
  privacy regressions and both real LibreOffice integration tests.
- `cargo test -p doc-converter --locked`: desktop password-rule test passed,
  including clean mode with stale protection/merge options and no password.
- `cargo build -p doc-converter --features custom-protocol --locked`: desktop
  executable built with the new embedded interface.
- Local `tsc --noEmit` and `vite build`: passed. The existing bundle-size warning
  remains. These were invoked directly from installed `node_modules` because the
  pnpm launcher attempted an install that the sandbox could not write.
- `cargo test -p doc-converter`: 2 tests passed, including rejection of archival requests before merge normalization.
- `cargo fmt --all --check` and `git diff --check`: passed.
- Browser smoke check: the cleaning tab, explanations, disabled empty-queue
  action, fixed output description and English/Croatian translations were checked;
  the rendered desktop-width layout was inspected. Native save dialogs and Word
  compatibility across arbitrary documents were not exercised by that check.

## Smoke check of the executable

Start `target/debug/doc-converter.exe`. The window title is `Doc Converter — Development`. Within about 30 seconds the queue footer changes from `Checking engines…` to the LibreOffice version, and `%LOCALAPPDATA%\com.privateconverter.desktop\lo-profile\user\registrymodifications.xcu` exists. This was verified on 16 September 2026.

## What is not automated

- No end-to-end test drives the Tauri window.
- No CI configuration exists in the repo.
- No lint for TypeScript beyond `tsc --strict`.
- No fixtures corpus of real-world Office documents; the DOCX tests use a generated minimal file.

The [application plan](APPLICATION_PLAN.md) section 12 lists the release gates that should become automated checks.

## PDF/A export verification — 17 September 2026

- `cargo test -p converter-core -- --nocapture`: 33 tests passed. The PDF/A
  integration ran with detected LibreOffice 26.8, exporting HTML through Writer
  and XLSX through Calc to both PDF/A-2b and PDF/A-4. Source preservation,
  no-overwrite behavior, numbered outputs, wrong-part rejection and cancellation
  passed. Eligibility, filter mapping, XMP namespace/conformance checks and
  merge/protection refusal have regression coverage.
- `cargo test -p doc-converter`: 2 tests passed, including rejection of archival requests before merge normalization.
- `cargo fmt --all --check` and `git diff --check`: passed.
- `vite build --outDir dist/pdfa-verification`: passed (existing large-chunk
  warning). The build required sandbox escalation for output writes.
- `tsc --noEmit`: blocked by pre-existing work in `Preview.tsx`
  (`isEvalSupported` is not part of `DocumentInitParameters`) and missing
  `security.*` translation keys referenced by `SecurityInspector.tsx`.
  No reported type errors concern the PDF/A changes. The normal combined
  frontend build cannot be described as passing while those errors remain.
- No independent ISO/PDF-A validator was run. Live native UI interactions,
  PDF/A-specific DOCX/ODT/PPTX exports, and LibreOffice 25.8 itself were not
  exercised. The integration verifies declaration and preservation, not full
  conformance or acceptance by a receiving office.

## Expanded PDF standards verification — 17 September 2026

This entry supersedes the earlier PDF/A export limitations above.

- `cargo check -p doc-converter`: passed.
- `cargo test -p doc-converter`: both desktop command tests passed after the final validator fix.
- `tsc --noEmit` and `vite build`: passed. The existing large-chunk warning remains.
  Missing security-inspector translations were completed and the obsolete PDF.js
  `isEvalSupported` option was removed to restore type checking.
- Core regression suite: 36 tests passed in the workspace run; its one failing
  integration test exposed veraPDF's exit-code-1 behavior for completed,
  non-compliant results. After fixing that handling,
  `cargo test -p converter-core all_pdf_standards_and_attachments_validate_locally -- --nocapture`
  passed. Thus all 37 core tests have passing coverage across the run and rerun.
- Real integration used LibreOffice 26.8, veraPDF Greenfield 1.30.2 and local
  Temurin JRE 21.0.12.1. All six profiles passed final-file validation: PDF/A-1b,
  PDF/A-2b, PDF/A-3b, PDF/A-4, PDF/A-4f and PDF/UA-1. PDF/A-3b/4f included an XML
  attachment with a Unicode filename; payload bytes and Data relationship were
  checked, source bytes were preserved and folder outputs were numbered.
- Negative integration: a PDF/UA source with an image lacking alternative text
  failed validation, returned rule descriptions and left no output. An ordinary
  PDF failed PDF/A validation. Validator cancellation was exercised before start.
- New unit coverage checks attachment limits, duplicate names, unsupported targets,
  no overwrite, no replacement of existing embedded files, metadata namespaces,
  preservation, missing/wrong/incomplete validator reports and all-profile routing.
- `cargo fmt --all --check` and `git diff --check`: passed.
- The local validator/runtime were installed and exercised. The setup script's
  already-installed path was run; its initial-download path mirrors the manual
  installation but was not rerun against a fresh directory.
- Not tested: native file-dialog interactions, exhaustive real-world Office files,
  PDF/A-specific DOCX/ODT/PPTX fixtures, forced validator timeout/report-overflow,
  independent human accessibility review, or a packaged installer. PDF/UA means
  machine checks plus a remaining human review; e-invoice business validation is
  not implemented. No claim of universal receiving-system acceptance is made.
