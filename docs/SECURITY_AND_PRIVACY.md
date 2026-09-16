---
title: "Security And Privacy"
description: "Content security policy, IPC surface, secret handling, decoder limits, and the gaps that remain before release."
type: "guide"
tags:
  - security
  - privacy
  - csp
resource: "docs/SECURITY_AND_PRIVACY.md"
last_updated: "2026-09-16"
source_sync: "manual"
---

# Security And Privacy

The product promise is local processing with no uploads. This document lists what the build enforces today and what it only claims.

Owning files:

- `apps/desktop/src-tauri/tauri.conf.json` - CSP and window settings
- `apps/desktop/src-tauri/src/main.rs` - the only IPC surface
- `crates/core/src/lib.rs` - secrets, limits, and safe writes

## Network

The app makes no network requests. There is no update check, no telemetry, no licensing call, and no remote font or script. The CSP allows `connect-src` only to the Tauri IPC origins:

```text
default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline';
img-src 'self' data:; connect-src ipc: http://ipc.localhost
```

`style-src 'unsafe-inline'` is present for Vite-injected styles. Tightening it is a release task.

## IPC surface

Three commands, described in [`IPC_AND_FILE_ACCESS.md`](IPC_AND_FILE_ACCESS.md). No Tauri plugins are enabled, so the webview has no filesystem, shell, HTTP, or dialog access of its own. The only paths the backend accepts are the ones it canonicalized after a user-driven picker.

## Secrets

- Passwords go from the input field, through `invoke`, into `SecretString` in Rust. The core never logs them.
- The UI clears both password fields after every job and on every operation switch.
- Nothing is persisted. There is no settings file, history, or `localStorage` use in the app. The mockup stores the active mode in `localStorage`; the app does not.
- JavaScript strings and the IPC JSON copy cannot be zeroed. Treat that as a known limit, not a bug to fix in the frontend.

## File safety

- Sources are opened read-only. Tests confirm the source bytes are unchanged after image conversion.
- Output goes to a temp file beside the destination and is moved with `persist_noclobber`. See [`JOB_LIFECYCLE.md`](JOB_LIFECYCLE.md).
- An existing destination is rejected twice: once in the command, once in the core.
- Wrong password, truncated ciphertext, or cancellation leaves no partial file. The test `crypto_roundtrip_wrong_password_and_truncation` checks this.

## Decoder limits

Image decoding runs in the application process, so a malicious image could crash the app. Limits reduce the blast radius:

| Limit | Value |
| --- | --- |
| Max width or height | 16000 px |
| Max decoder allocation | 256 MiB |
| Max `max_edge` argument | 16000 |
| Animated PNG | Rejected before decode |

Formats are sniffed from content, not from the file extension. Only PNG, JPEG, and BMP decoders are compiled in.

## Encryption format

`.age` with the passphrase (scrypt) recipient. Standard format, readable by `age` and `rage`. Decryption authenticates the whole stream before the temp file is committed. Decrypting a file that was encrypted to a public-key recipient is refused with a clear message.

The plan replaced the mockup's invented `.dcenc` format with `.age` on purpose. Do not reintroduce a custom container.

## Known gaps before release

- No worker process isolation. Decoders and age run in-process.
- No crash cleanup of orphaned temp files.
- Suggested output names reveal the original name, for example `report.docx.age`.
- No colour-profile handling; CMYK JPEG and ICC profiles are untested.
- No code signing, installer, or WebView2 provisioning.
- Licensing is not wired, so paid-feature gating does not exist.

The [application plan](APPLICATION_PLAN.md) sections 4, 7, 8, and 12 describe the intended fixes.
