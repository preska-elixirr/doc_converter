---
title: "Security And Privacy"
description: "Content security policy, IPC surface, LibreOffice isolation, secret handling, decoder limits, temporary files, and the gaps that remain before release."
type: "guide"
tags:
  - security
  - privacy
  - csp
resource: "docs/SECURITY_AND_PRIVACY.md"
last_updated: "2026-09-17"
source_sync: "manual"
---

# Security And Privacy

The product promise is local processing with no uploads. This document lists what the build enforces today and what it only claims.

Owning files:

- `apps/desktop/src-tauri/tauri.conf.json` - CSP and window settings
- `apps/desktop/src-tauri/capabilities/default.json` - webview permissions
- `apps/desktop/src-tauri/src/main.rs` - the only IPC surface
- `crates/core/src/office.rs` - LibreOffice isolation
- `crates/core/src/pdf.rs`, `crypto.rs`, `images.rs` - secrets, limits, safe writes

## Network

The app makes no network requests. There is no update check, no telemetry, no licensing call, and no remote font or script. PDF.js is bundled and its worker loads from the app's own origin. The CSP is unchanged:

```text
default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline';
img-src 'self' data:; connect-src ipc: http://ipc.localhost
```

`default-src 'self'` also governs the PDF.js worker. `style-src 'unsafe-inline'` is present for Vite-injected styles; tightening it is a release task.

LibreOffice runs with its online update check disabled in the private profile. It is not otherwise firewalled; a document with remote links could make LibreOffice fetch them during conversion. Blocking that needs a firewall rule or a restricted token and is listed under gaps.

## IPC surface

Seven commands and two events, described in [`IPC_AND_FILE_ACCESS.md`](IPC_AND_FILE_ACCESS.md). The plugin permissions are `core:default`, needed for events, and `core:webview:allow-set-webview-zoom` for the UI scale setting. The webview has no filesystem, shell, HTTP or dialog access of its own. Paths accepted by the backend are the ones it canonicalized after a user-driven picker or a native drop event.

## LibreOffice isolation

- Private user profile under the app's local data folder; the user's own LibreOffice is never involved.
- Macro security *Very high* in that profile; headless conversion never runs document macros.
- A Windows job object with kill-on-close owns every process LibreOffice starts; cancel and timeout terminate the whole tree.
- No passwords, no secrets and no user settings are passed to LibreOffice. It sees the input file and an output folder inside the batch work folder.
- Its stdout is discarded; stderr is kept in memory for error messages only and never logged to disk.

This is process separation, not a sandbox. A malicious document can still exploit LibreOffice with the user's rights. See [`ENGINES.md`](ENGINES.md).

## Secrets

- Passwords go from the input field, through `invoke`, into `SecretString` in Rust. The core never logs them.
- PDF passwords are handed to lopdf in memory through `expose_secret`; they never appear on a command line, in a process list, or in a temporary file.
- age secret keys are pasted like a password, parsed into `age::x25519::Identity` in the command layer before any dialog opens, and dropped with the batch. Public keys are not secret; the recipient list stays in the page.
- A generated key pair goes straight from the core into the file the user chose in the save dialog; the page receives only the public key and the file's path.
- The UI clears the password, confirmation and secret key fields after every job and on every tab change.
- The only persisted data is the UI language and UI scale, in the webview's `localStorage`. There is no settings file and no history; no path, name, or password is ever stored.
- JavaScript strings and the IPC JSON copy cannot be zeroed. Treat that as a known limit.

## File safety

- Sources are opened read-only. DOCX edits and every conversion work on copies in the batch work folder, and the DOCX rewrite itself goes through a temp file and a no-overwrite move like every other output.
- Final output goes to a temp file beside the destination and is moved with `persist_noclobber`. Existing files are never overwritten; folder outputs get numbered names.
- Wrong password, truncation, cancellation, or an engine failure leaves no partial file at the destination.
- The batch work folder is removed when the batch ends; stale folders are removed at the next start, except folders another running instance still holds a lock on.

## Metadata cleaning

The **Clean before sharing** tab removes supported DOCX, PDF and photo metadata
locally. It creates new copies and accepts supported DOCX revisions, with explicit
failure for structural revisions it cannot safely accept. Metadata cleaning is
not redaction: PDF annotations/attachments and DOCX embedded files/photos are
outside its scope. Photo outputs are fresh PNGs; PDF outputs are fully rewritten.
See [Clean before sharing](CLEAN_BEFORE_SHARING.md) for precise coverage and limits.

## Temporary data

Some data touches disk briefly inside `%TEMP%\doc-converter-*`: edited DOCX copies, LibreOffice output, per-document PDFs before a merge, HTML bridges, preview PDFs. Deleting a file on an SSD does not guarantee secure erasure. Decrypted plaintext from the Decrypt tab streams directly to the temp file beside the chosen destination, as before.

## Decoder and parser limits

| Limit | Value |
| --- | --- |
| Image width or height | 16000 px |
| Image decoder allocation | 256 MiB |
| Animated PNG, animated WebP, multi-page TIFF | Rejected before decode |
| PDF text extraction per page | 64 MiB decompressed |
| Dropped paths per event | 500 |

Formats are sniffed from content. Image decoding, PDF parsing, Typst compilation and age run in the application process, so a malicious file can still crash the app.

## Encryption formats

- Files: `.age` with the passphrase (scrypt) recipient or with X25519 recipients (`age1…` public keys), standard and interoperable. The header decides which secret opens a file. Decryption authenticates the whole stream before commit.
- PDFs: AES-256, PDF 2.0 standard security handler, random owner password, all permissions granted. See [`PDF_TOOLS.md`](PDF_TOOLS.md).

Do not reintroduce a custom container; the mockup's `.dcenc` was replaced on purpose.

## Known gaps before release

- No worker process isolation for lopdf, image, Typst or age; only LibreOffice is out of process.
- LibreOffice has no memory limit and no network block beyond the disabled update check.
- Suggested output names reveal the original name, for example `report.docx.age`.
- The secret key file written by *Create a key pair* inherits its folder's permissions; nothing restricts it to the current user.
- No colour-profile handling; CMYK JPEG and ICC profiles are untested.
- No code signing, installer, or WebView2 provisioning.
- Licensing is not wired, so paid-feature gating does not exist.

The [application plan](APPLICATION_PLAN.md) sections 4, 7, 8, and 12 describe the intended fixes.

## PDF standards validator and attachments

Local Java/veraPDF receives a generated PDF or an explicitly selected existing PDF;
no service upload is used. Runtime setup is an explicit developer script; the app
never downloads executables. Validation runs with a 512 MiB Java heap, 120-second
wall timeout, 8 MiB report limit and cancellation/job-object cleanup. Reports are
stored in a temporary file and removed after parsing. No custom validator paths
or profile files are accepted from IPC. See [PDF Standards](PDF_A_EXPORT.md).

Attachments intentionally preserve their bytes, filenames, descriptions and source
modification dates. Rust caps count and bytes, rejects duplicate names and refuses
to replace existing embedded files. Adding XML is not e-invoice certification.
PDF/UA validation reports never imply that human accessibility review is complete.
The preview uses PDF.js 6; the removed `isEvalSupported` option no longer exists in
that version's implementation or types. Existing CSP restrictions remain in place.
