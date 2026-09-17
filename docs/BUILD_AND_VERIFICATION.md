---
title: "Build And Verification"
description: "Toolchain, LibreOffice for development, build and dev commands, tests, and what each check proves."
type: "guide"
tags:
  - build
  - testing
  - tooling
resource: "docs/BUILD_AND_VERIFICATION.md"
last_updated: "2026-09-16"
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

Twenty-three tests in `crates/core/src/` and one in the desktop shell:

| Module | Test | Proves |
| --- | --- | --- |
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

## Smoke check of the executable

Start `target/debug/doc-converter.exe`. The window title is `Doc Converter — Development`. Within about 30 seconds the queue footer changes from `Checking engines…` to the LibreOffice version, and `%LOCALAPPDATA%\com.privateconverter.desktop\lo-profile\user\registrymodifications.xcu` exists. This was verified on 16 September 2026.

## What is not automated

- No end-to-end test drives the Tauri window.
- No CI configuration exists in the repo.
- No lint for TypeScript beyond `tsc --strict`.
- No fixtures corpus of real-world Office documents; the DOCX tests use a generated minimal file.

The [application plan](APPLICATION_PLAN.md) section 12 lists the release gates that should become automated checks.
