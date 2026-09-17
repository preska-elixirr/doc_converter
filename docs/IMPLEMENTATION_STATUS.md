---
title: "Implementation Status"
description: "What the second milestone (everything the mockup promised) delivered, what was verified, and what remains outstanding."
type: "audit"
tags:
  - status
  - milestone
resource: "docs/IMPLEMENTATION_STATUS.md"
last_updated: "2026-09-17"
source_sync: "manual"
---

# Implementation status, 17 September 2026

## Clean before sharing

Added a dedicated English/Croatian tab for DOCX properties, comments, supported
tracked changes and hidden text; PDF document/XMP metadata; and photo GPS/camera
metadata into lossless PNG copies. Uses the existing batch, cancellation and
no-overwrite flow without LibreOffice. Scope and deliberate refusals are recorded
in [Clean before sharing](CLEAN_BEFORE_SHARING.md).

Verification: all 29 core tests and the desktop-shell test passed; the frontend
and embedded desktop builds succeeded. Formatting and diff checks passed. The
cleaning tab was checked in a browser in English and Croatian; native dialog
automation and arbitrary Word-document compatibility remain unverified.

## Second milestone delivered: the mockup's promises

- **Convert tab**: DOCX, ODT, PPTX, XLSX, HTML, Markdown and text to PDF, DOCX, TXT, HTML or Markdown through LibreOffice or the built-in engines; PDF text extraction; a per-file capability matrix with reasons; batch and per-row formats.
- **Combine into one document**: ordered merge into one PDF with the order list, optional password.
- **Password-protect PDFs**: AES-256 open passwords on conversion output and on merged output.
- **Page layout**: orientation (As is, Portrait, Landscape), margins, paragraph spacing, page breaks on Word body paragraphs and on Markdown/text blocks, applied by rewriting a DOCX copy or by the Typst engine; a live preview of the real output rendered with bundled PDF.js.
- **Images tab**: WebP and TIFF input, WebP and one-page PDF output, per-row formats, batch processing.
- **Encrypt tab**: password-protected PDF or `.age` file. **Decrypt tab**: unlock a PDF or restore an `.age` file.
- **Queue**: multi-select table, select-all, drag and drop anywhere in the window, per-row status with live progress, `Clear all`, type badges, size and metadata line.
- **Engines**: LibreOffice found without a system install, run in a private profile inside a job object, warmed up at startup; engine state shown in the queue footer.
- **Batches**: several files to a chosen folder with numbered names, one file or a merge through a save dialog, per-item reports, cancellation that also stops LibreOffice.

## Verified on the development machine

- `cargo test -p converter-core`: 16 tests pass, including a real LibreOffice text-to-PDF conversion and a cancelled one (LibreOffice 26.8 extracted to `.tools/libreoffice`).
- `cargo fmt --all --check`: clean.
- `pnpm run build`: TypeScript check and Vite build pass; the PDF.js worker is emitted as an asset.
- `cargo build -p doc-converter --features custom-protocol`: builds `target/debug/doc-converter.exe`.
- Startup smoke check: the window opens with the title `Doc Converter — Development`; within about 30 seconds the LibreOffice profile is created under `%LOCALAPPDATA%\com.privateconverter.desktop\lo-profile`, which proves detection, the private profile and the warm-up run inside the app.
- Batch behaviour end to end is covered by `job::tests::builtin_routes_run_without_office`: nine mixed items to a folder, a protected merge, numbered names on rerun, preview bytes, outline, and cancellation.

Not verified by an automated test: clicking through the native window (the UI was checked by type-checking and the build only), conversions of real-world Office documents beyond LibreOffice's own output, and Word compatibility of rewritten DOCX files beyond LibreOffice rendering them.

## Review fixes, 16 September 2026

Two independent reviews of the uncommitted milestone found issues that are now fixed and covered by tests:

- A self-closing `<w:pPrDefault/>` in `styles.xml` produced malformed XML when paragraph spacing was changed. The rewrite now handles every shape of the defaults block and leaves character spacing alone.
- A preview could block a conversion or a newer preview. Previews now run behind their own gate; a newer preview or a batch cancels the one in flight.
- The preview for a DOCX target from Markdown, text or HTML showed the built-in engine's layout instead of LibreOffice's. It now renders the real DOCX.
- LibreOffice HTML export wrote pictures as loose files that were left behind. Writer HTML embeds them as data URIs; Calc's generated image sidecars are embedded before saving as well.
- Startup cleanup could delete a second instance's active work folder. Work folders now hold a lock file that cleanup respects.
- The Convert tab demanded a hidden password when protection was on but no PDF was produced. The rule now matches the outputs.
- Also: `docx::rewrite` writes through a temp file and honours cancel, merging checks cancel per source, files added before engine detection get their Office formats refreshed, and the deprecated lopdf call in a test is gone.

Follow-up fixes on 17 September also replace paired spacing elements including
their closing tags, expand empty paragraph properties without duplicating them,
and enable page breaks that were explicitly false. Regression tests cover these
XML cases and a real spreadsheet HTML export with an embedded image.

## Scope still outstanding

- Installer, code signing, and shipping LibreOffice with its notices, or a guided prerequisite install.
- Worker process isolation for the in-process engines; memory limits and network blocking for LibreOffice.
- Licensing and activation (plan section 14).
- PDF page ranges, rotation and per-source bookmarks in merges; PDF/A conversion of existing PDFs, watermarks, compression.
- ODT page-layout rewrites; spreadsheet print options; font substitution warnings.
- OCR, HEIC/AVIF, colour management, metadata preservation options.

See the root README for commands and current limits.

## PDF/A export — 17 September 2026

Convert now offers PDF/A-1b, PDF/A-2b, PDF/A-3b, PDF/A-4, PDF/A-4f and PDF/UA-1 for Office and HTML sources through LibreOffice 25.8+. Local veraPDF validates final outputs before commit. PDF/A-3b and PDF/A-4f support associated files; existing PDFs can be checked without modification. PDF/UA reports explicitly require human review. Backend safeguards refuse unsupported combinations. The validator/runtime are installed for development but still need production packaging. See [PDF/A Export](PDF_A_EXPORT.md) for scope and [Build And Verification](BUILD_AND_VERIFICATION.md) for checks run.
