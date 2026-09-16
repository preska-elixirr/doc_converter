# Doc Converter

Windows-first Tauri 2 desktop app with a Rust processing core and React/TypeScript UI.

## Current milestone

Implemented:

- Native file picker and save dialogs; files stay local.
- Standard password-based `.age` encryption and authenticated decryption.
- Static PNG/JPG/BMP input to PNG/JPG output, aspect-ratio-preserving resizing, JPEG quality, EXIF orientation and white alpha flattening for JPEG.
- One selected file processed at a time, background execution, cooperative cancellation, temporary output and no-overwrite commit.
- Backend-owned input IDs; no arbitrary filesystem API exposed to the frontend.

Not implemented: Office conversion, PDF encryption/merging/preview, editable page layout, WebP/TIFF, batch jobs, worker process isolation, licensing server or signed activation. These are marked as unavailable in the UI. This is a development build, not production-ready software.

Original design: `opendesign/mockups/document-converter/`. Architecture and next milestones: [application plan](docs/APPLICATION_PLAN.md). Feature and API documentation starts at the [documentation index](docs/index.md); read [project context](docs/PROJECT_CONTEXT.md) first.

## Build

Prerequisites: Windows Rust MSVC toolchain and C++ build tools, Node.js, pnpm and WebView2 Runtime.

```powershell
cd apps/desktop
pnpm install
pnpm run build
cd ../..
cargo build -p doc-converter --features custom-protocol
```

Run `target/debug/doc-converter.exe`. The embedded production UI does not need a web server. This debug executable is for local development; an installer and code signing are future work.

For frontend development, run `pnpm dev` in `apps/desktop`, then `cargo run -p doc-converter` at the project root. The browser-only UI cannot process files; native commands are available inside the desktop app.

```powershell
cargo test -p converter-core
cargo fmt --all --check
```

The repository includes Cargo and pnpm lockfiles. Prefer locked installs/builds in CI.

## First-build limits

- Encryption writes a new `.age` file; it does not add a password that a PDF reader understands.
- Images are decoded in the application process with a 256 MiB decoder allocation limit and dimension limits. Further process isolation and colour-profile handling are required before release.
- Cancellation is cooperative: it checks between streaming chunks and image stages; a decoder, encoder or password derivation operation may finish its current stage first.
- Restored plaintext uses a temporary file beside the chosen destination until complete authentication succeeds. Temporary files are removed on normal errors, not guaranteed after force termination or power failure; crash cleanup remains to be implemented.
- Source files are never intentionally modified. Existing destinations are rejected, including collision detection during commit.
- Passwords are not persisted. Rust secret wrappers are used in crypto, but JS/IPC temporary copies cannot be guaranteed erased.
- Licensing is unconfigured and does not gate this local development build. Do not distribute commercially until signed entitlements and release hardening are implemented.

## Next slice

Connect an isolated LibreOffice engine and qpdf helper, generate a real PDF from DOCX, preview it, then add PDF password protection. Continue with page composition and licensing as described in the plan.
