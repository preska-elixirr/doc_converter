---
title: "Converter Core"
description: "API catalog for the converter-core crate: errors, encryption, image conversion, and safe output helpers."
type: "catalog"
tags:
  - methods-report
  - rust
  - core
resource: "docs/methods_report/01_converter_core.md"
last_updated: "2026-09-16"
doc_version: "1.0.0"
source_sync: "manual"
---

# Converter Core

Crate `converter-core`, file `crates/core/src/lib.rs`. No Tauri dependency. Dependencies: `age 0.11`, `image 0.25` with only `png`, `jpeg`, `bmp` features, `tempfile 3`, `thiserror 2`.

### `crates/core/src/lib.rs`

```rust
pub enum Error {
    Message(String),          // Displays as the message itself
    Io(std::io::Error),       // Displays as "File operation failed: {0}"; From<io::Error>
}
pub type Result<T> = std::result::Result<T, Error>;

pub fn encrypt(
    source: &Path,
    destination: &Path,
    password: SecretString,
    cancel: &AtomicBool,
) -> Result<()>;
// Streams source through age passphrase (scrypt) encryption into a temp file beside
// destination, then commits with persist_noclobber. Fails if destination exists.
// Checks cancel before each 64 KiB chunk and before commit. No length check on password.

pub fn decrypt(
    source: &Path,
    destination: &Path,
    password: SecretString,
    cancel: &AtomicBool,
) -> Result<()>;
// Parses the age header, requires an scrypt recipient, authenticates the header with
// the password before creating the temp file, then streams plaintext with cancel checks.
// Wrong password, truncation, or cancel leaves no file at destination.

pub fn convert_image(
    source: &Path,
    destination: &Path,
    format: &str,      // "png" | "jpg"
    max_edge: u32,     // 0 = keep size; else fit within max_edge square; max 16000
    quality: u8,       // 1..=100, JPEG only
    cancel: &AtomicBool,
) -> Result<()>;
// Sniffs format from content; accepts PNG, JPEG, BMP only; rejects APNG.
// Limits: 16000 px per side, 256 MiB decoder allocation. Applies EXIF orientation,
// checks cancel, resizes with Lanczos3 without upscaling, writes PNG or flattens
// alpha over white and encodes JPEG at quality. Metadata is not copied.

pub fn secret(value: String) -> SecretString;
// Wraps a String in age's SecretString.
```

Private helpers with contracts:

```rust
fn message(e: impl Display) -> Error;
// Error::Message(e.to_string())

fn copy_cancel<R: Read, W: Write>(reader: R, writer: W, cancel: &AtomicBool) -> Result<()>;
// 64 KiB chunks; returns Err("Cancelled. No output was saved.") when the flag is set.

fn output(path: &Path) -> Result<NamedTempFile>;
// Err("Output already exists. Choose another name.") if path exists;
// Err("Invalid destination") if path has no parent; else a temp file in that parent.

fn commit(file: NamedTempFile, path: &Path, cancel: &AtomicBool) -> Result<()>;
// Re-checks cancel, sync_all, then persist_noclobber. Never overwrites.
```

### Error strings

| Origin | Text |
| --- | --- |
| `copy_cancel`, `commit` | `Cancelled. No output was saved.` |
| `convert_image` cancel point | `Cancelled` |
| `output` | `Output already exists. Choose another name.` |
| `output` | `Invalid destination` |
| `decrypt` | `Not a supported age encrypted file.` |
| `decrypt` | `This build supports password-encrypted age files only.` |
| `decrypt` | `Incorrect password or damaged encrypted file.` |
| `convert_image` | `Invalid image settings.` |
| `convert_image` | `This build converts static PNG, JPG and BMP inputs.` |
| `convert_image` | `Animated PNG is not supported; no frames were discarded.` |
| `convert_image` | `Choose PNG or JPG output.` |

### Tests

- `crypto_roundtrip_wrong_password_and_truncation`
- `image_resize_and_cancel_preserve_source`
