# Doc Converter

Windows-first Tauri 2 desktop app with a Rust processing core and React/TypeScript UI. Everything runs on the user's computer; nothing is uploaded.

## Current milestone

Everything the accepted mockup shows is implemented with real processing:

- **Convert**: DOCX, ODT, PPTX, XLSX, HTML, Markdown and text to PDF, DOCX, TXT, HTML or Markdown. LibreOffice does Office formats in a private, isolated profile; Markdown and text are paginated by a built-in engine. PDFs can be extracted to text. Each file shows which formats it can reach and why not.
- **Combine** selected documents into one PDF in a chosen order. **Password-protect** PDF output with AES-256.
- **Watermark every page**: add “Confidential” or a recipient name to ordinary PDF conversions and combined PDFs, before optional password protection. [Behavior and limits](docs/PDF_TOOLS.md#watermarks).
- **Page layout**: orientation, margins, paragraph spacing and page breaks on Word paragraphs or text blocks, with a preview of the real output.
- **Images**: PNG, JPEG, BMP, WebP and TIFF to PNG, JPG, WebP or an A4 PDF page, with resize and quality.
- **Clean before sharing**: remove DOCX properties, comments, supported tracked changes and hidden text; strip PDF metadata; remove photo GPS/camera metadata into lossless PNG copies. [Behavior and limits](docs/CLEAN_BEFORE_SHARING.md).
- **Encrypt** a PDF with an open password, or any file as a standard `.age` copy that a password or the recipient's secret key opens, encrypted for their public key; create your own key pair. **Decrypt** all of it with the password or your secret key.
- **Queue**: many files per batch, drag and drop, per-file formats and statuses, live progress, cancellation.
- **Languages and scale**: English and Croatian, switched from the `Aa` button in the header, plus a UI scale from 90 % to 150 %.

Placeholders only: the License tab. Not implemented: installer and code signing, bundled LibreOffice, licensing, worker isolation for in-process engines, PDF page tools beyond merge, OCR. This is a development build, not production-ready software.

Original design: `opendesign/mockups/document-converter/`. Architecture and roadmap: [application plan](docs/APPLICATION_PLAN.md). Feature and API documentation starts at the [documentation index](docs/index.md); read [project context](docs/PROJECT_CONTEXT.md) first.

## Build

Prerequisites: Windows Rust MSVC toolchain and C++ build tools, Node.js, pnpm, WebView2 Runtime. LibreOffice is optional for building and needed for Office conversions; see [engines](docs/ENGINES.md) for a no-install setup into `.tools/`.

```powershell
cd apps/desktop
pnpm install
pnpm run build
cd ../..
cargo build -p doc-converter --features custom-protocol
```

Run `target/debug/doc-converter.exe`. The embedded UI needs no web server. The first full build compiles the Typst engine and takes several minutes.

For frontend development, run `pnpm dev` in `apps/desktop`, then `cargo run -p doc-converter` at the project root. The browser-only UI cannot process files.

```powershell
cargo test -p converter-core
cargo fmt --all --check
```

The core and desktop suites include privacy-cleaning regression tests; Office integration tests skip with a message when no LibreOffice is found. See [verification](docs/BUILD_AND_VERIFICATION.md).

## Limits of this build

- LibreOffice must be present on the machine (installed, on `PATH`, in `.tools/libreoffice`, or named by `DOC_CONVERTER_SOFFICE`). Without it, Office conversions are marked unavailable; text, Markdown, HTML, PDF and image features still work.
- Page layout settings apply to Word, Markdown and text sources. Presentations, spreadsheets and ODT keep their own page setup.
- PDF to DOCX, HTML or Markdown is not offered; only text extraction.
- Merging drops bookmarks and form field trees; page content survives.
- Decoders, PDF handling and Typst run in the application process; only LibreOffice runs out of process.
- Temporary copies live briefly in `%TEMP%\doc-converter-*` and are removed after each batch and at startup.
- Existing files are never overwritten; folder outputs get numbered names.
- Passwords are not persisted. `.age` output names reveal the original file name. A saved secret key file inherits its folder's permissions.
- Licensing is unconfigured and does not gate this build. Do not distribute commercially until signed entitlements and release hardening are implemented.

## Next slice

Package LibreOffice with the installer and notices, add code signing, and move the in-process engines into a worker. Then PDF page tools (ranges, rotation, bookmarks), and licensing as described in the plan.
