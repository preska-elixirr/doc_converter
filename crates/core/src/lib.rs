use age::secrecy::SecretString;
use image::{ImageFormat, ImageReader};
use std::{
    fs::File,
    io::{self, BufReader, Read, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
use tempfile::NamedTempFile;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),
    #[error("File operation failed: {0}")]
    Io(#[from] io::Error),
}
pub type Result<T> = std::result::Result<T, Error>;
fn message(e: impl std::fmt::Display) -> Error {
    Error::Message(e.to_string())
}

fn copy_cancel<R: Read, W: Write>(mut reader: R, mut writer: W, cancel: &AtomicBool) -> Result<()> {
    let mut buf = vec![0; 64 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(message("Cancelled. No output was saved."));
        }
        let n = reader.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        writer.write_all(&buf[..n])?;
    }
}

// Temporary output lives beside its destination; persist_noclobber never overwrites.
fn output(path: &Path) -> Result<NamedTempFile> {
    if path.exists() {
        return Err(message("Output already exists. Choose another name."));
    }
    Ok(NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| message("Invalid destination"))?,
    )?)
}
fn commit(file: NamedTempFile, path: &Path, cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(message("Cancelled. No output was saved."));
    }
    file.as_file().sync_all()?;
    file.persist_noclobber(path).map_err(|e| message(e.error))?;
    Ok(())
}

pub fn encrypt(
    source: &Path,
    destination: &Path,
    password: SecretString,
    cancel: &AtomicBool,
) -> Result<()> {
    let input = File::open(source)?;
    let mut temp = output(destination)?;
    let encryptor = age::Encryptor::with_user_passphrase(password);
    let mut writer = encryptor.wrap_output(&mut temp).map_err(message)?;
    copy_cancel(input, &mut writer, cancel)?;
    writer.finish().map_err(message)?;
    commit(temp, destination, cancel)
}

pub fn decrypt(
    source: &Path,
    destination: &Path,
    password: SecretString,
    cancel: &AtomicBool,
) -> Result<()> {
    let decryptor = age::Decryptor::new(BufReader::new(File::open(source)?))
        .map_err(|_| message("Not a supported age encrypted file."))?;
    if !decryptor.is_scrypt() {
        return Err(message(
            "This build supports password-encrypted age files only.",
        ));
    }
    let identity = age::scrypt::Identity::new(password);
    let reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|_| message("Incorrect password or damaged encrypted file."))?;
    let mut temp = output(destination)?;
    copy_cancel(reader, &mut temp, cancel)?;
    commit(temp, destination, cancel)
}

pub fn convert_image(
    source: &Path,
    destination: &Path,
    format: &str,
    max_edge: u32,
    quality: u8,
    cancel: &AtomicBool,
) -> Result<()> {
    if !(1..=100).contains(&quality) || max_edge > 16000 {
        return Err(message("Invalid image settings."));
    }
    let mut reader = ImageReader::open(source)?.with_guessed_format()?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Bmp)
    ) {
        return Err(message(
            "This build converts static PNG, JPG and BMP inputs.",
        ));
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16000);
    limits.max_image_height = Some(16000);
    limits.max_alloc = Some(256 * 1024 * 1024);
    if reader.format() == Some(ImageFormat::Png) {
        let png = image::codecs::png::PngDecoder::with_limits(
            BufReader::new(File::open(source)?),
            limits.clone(),
        )
        .map_err(message)?;
        if png.is_apng().map_err(message)? {
            return Err(message(
                "Animated PNG is not supported; no frames were discarded.",
            ));
        }
    }
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(message)?;
    use image::ImageDecoder;
    let orientation = decoder.orientation().map_err(message)?;
    let mut decoded = image::DynamicImage::from_decoder(decoder).map_err(message)?;
    decoded.apply_orientation(orientation);
    if cancel.load(Ordering::Relaxed) {
        return Err(message("Cancelled"));
    }
    if max_edge > 0 && (decoded.width() > max_edge || decoded.height() > max_edge) {
        decoded = decoded.resize(max_edge, max_edge, image::imageops::FilterType::Lanczos3);
    }
    let mut temp = output(destination)?;
    match format {
        "png" => decoded
            .write_to(&mut temp, ImageFormat::Png)
            .map_err(message)?,
        "jpg" => {
            let mut rgb = image::RgbImage::new(decoded.width(), decoded.height());
            for (x, y, p) in decoded.to_rgba8().enumerate_pixels() {
                let a = u32::from(p[3]);
                rgb.put_pixel(
                    x,
                    y,
                    image::Rgb(
                        [0, 1, 2]
                            .map(|i| ((u32::from(p[i]) * a + 255 * (255 - a) + 127) / 255) as u8),
                    ),
                );
            }
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut temp, quality)
                .encode_image(&rgb)
                .map_err(message)?;
        }
        _ => return Err(message("Choose PNG or JPG output.")),
    }
    commit(temp, destination, cancel)
}

pub fn secret(value: String) -> SecretString {
    SecretString::from(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crypto_roundtrip_wrong_password_and_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("source");
        let enc = dir.path().join("secret.age");
        let out = dir.path().join("restored");
        let bytes = b"Private document \0 with binary bytes \xff";
        std::fs::write(&src, bytes).unwrap();
        let cancel = AtomicBool::new(false);
        encrypt(&src, &enc, secret("a long test passphrase".into()), &cancel).unwrap();
        assert!(decrypt(&enc, &out, secret("wrong".into()), &cancel).is_err());
        assert!(!out.exists());
        decrypt(&enc, &out, secret("a long test passphrase".into()), &cancel).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), bytes);
        assert!(encrypt(&src, &enc, secret("another password".into()), &cancel).is_err());
        let mut broken = std::fs::read(&enc).unwrap();
        broken.truncate(broken.len() - 8);
        std::fs::write(&enc, broken).unwrap();
        let partial = dir.path().join("partial");
        assert!(decrypt(
            &enc,
            &partial,
            secret("a long test passphrase".into()),
            &cancel
        )
        .is_err());
        assert!(!partial.exists());
    }
    #[test]
    fn image_resize_and_cancel_preserve_source() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("source.png");
        let out = dir.path().join("small.jpg");
        image::RgbaImage::from_pixel(80, 40, image::Rgba([255, 0, 0, 0]))
            .save(&src)
            .unwrap();
        let original = std::fs::read(&src).unwrap();
        convert_image(&src, &out, "jpg", 20, 90, &AtomicBool::new(false)).unwrap();
        let decoded = image::open(&out).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (20, 10));
        assert!(decoded
            .to_rgb8()
            .get_pixel(0, 0)
            .0
            .iter()
            .all(|channel| *channel > 245));
        assert_eq!(original, std::fs::read(&src).unwrap());
        let cancelled = dir.path().join("cancelled.png");
        assert!(convert_image(&src, &cancelled, "png", 20, 90, &AtomicBool::new(true)).is_err());
        assert!(!cancelled.exists());
    }
}
