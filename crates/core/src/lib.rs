//! Processing core for Doc Converter. No UI, no network.
//!
//! Every operation reads a source file, writes a temporary file beside the
//! destination and moves it into place without overwriting. Cancellation is a
//! shared flag that operations check between stages.

pub mod capability;
pub mod crypto;
pub mod docx;
mod html;
pub mod images;
pub mod inspect;
pub mod job;
pub mod layout;
pub mod office;
pub mod pdf;
pub mod text;

use std::{
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use tempfile::NamedTempFile;

pub use age::secrecy::SecretString;
pub use capability::{capabilities, Availability, Engines, OutputFormat};
pub use crypto::{decrypt, encrypt};
pub use images::convert_image;
pub use inspect::{inspect, InputKind};
pub use layout::{Layout, Margins, Orientation, Spacing};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),
    #[error("File operation failed: {0}")]
    Io(#[from] io::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn message(e: impl std::fmt::Display) -> Error {
    Error::Message(e.to_string())
}

pub const CANCELLED: &str = "Cancelled. No output was saved.";

pub(crate) fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(message(CANCELLED))
    } else {
        Ok(())
    }
}

pub(crate) fn copy_cancel<R: Read, W: Write>(
    mut reader: R,
    mut writer: W,
    cancel: &AtomicBool,
) -> Result<()> {
    let mut buf = vec![0; 64 * 1024];
    loop {
        check_cancel(cancel)?;
        let n = reader.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        writer.write_all(&buf[..n])?;
    }
}

/// Temporary output beside its destination. `persist_noclobber` never overwrites.
pub(crate) fn output(path: &Path) -> Result<NamedTempFile> {
    if path.exists() {
        return Err(message("Output already exists. Choose another name."));
    }
    Ok(NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| message("Invalid destination"))?,
    )?)
}

pub(crate) fn commit(file: NamedTempFile, path: &Path, cancel: &AtomicBool) -> Result<()> {
    check_cancel(cancel)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(path).map_err(|e| message(e.error))?;
    Ok(())
}

/// Writes `bytes` to `destination` through a temp file, never overwriting.
pub(crate) fn write_bytes(destination: &Path, bytes: &[u8], cancel: &AtomicBool) -> Result<()> {
    let mut temp = output(destination)?;
    temp.write_all(bytes)?;
    commit(temp, destination, cancel)
}

/// Copies `source` to `destination` through a temp file, never overwriting.
pub fn copy_file(source: &Path, destination: &Path, cancel: &AtomicBool) -> Result<()> {
    let input = File::open(source)?;
    let mut temp = output(destination)?;
    copy_cancel(input, &mut temp, cancel)?;
    commit(temp, destination, cancel)
}

/// Picks a name in `dir` that does not exist yet: `name.ext`, then `name (2).ext`, ...
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}).{ext}")))
        .find(|p| !p.exists())
        .expect("unbounded range")
}

pub fn secret(value: String) -> SecretString {
    SecretString::from(value)
}

/// File stem without any extension, safe for building output names.
pub fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "document".to_string())
}
