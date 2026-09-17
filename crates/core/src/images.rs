//! Static image decode, orientation, resize and encode. Inputs: PNG, JPEG,
//! BMP, WebP, TIFF. Outputs: PNG, JPG, WebP, or a one-page PDF.

use crate::{check_cancel, commit, message, output, pdf, InputKind, Result};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use serde::Serialize;
use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::atomic::AtomicBool,
};

pub const MAX_EDGE: u32 = 16000;
const MAX_ALLOC: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
pub struct ImageInfo {
    pub kind: InputKind,
    pub width: u32,
    pub height: u32,
}

fn limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_EDGE);
    limits.max_image_height = Some(MAX_EDGE);
    limits.max_alloc = Some(MAX_ALLOC);
    limits
}

fn kind_of(format: Option<ImageFormat>) -> Result<(InputKind, ImageFormat)> {
    Ok(match format {
        Some(ImageFormat::Png) => (InputKind::Png, ImageFormat::Png),
        Some(ImageFormat::Jpeg) => (InputKind::Jpg, ImageFormat::Jpeg),
        Some(ImageFormat::Bmp) => (InputKind::Bmp, ImageFormat::Bmp),
        Some(ImageFormat::WebP) => (InputKind::Webp, ImageFormat::WebP),
        Some(ImageFormat::Tiff) => (InputKind::Tiff, ImageFormat::Tiff),
        _ => {
            return Err(message(
                "This build converts static PNG, JPG, BMP, WebP and TIFF inputs.",
            ))
        }
    })
}

/// Rejects animated or multi-page inputs before any frame could be dropped.
fn reject_multi_frame(source: &Path, format: ImageFormat) -> Result<()> {
    match format {
        ImageFormat::Png => {
            let png = image::codecs::png::PngDecoder::with_limits(
                BufReader::new(File::open(source)?),
                limits(),
            )
            .map_err(message)?;
            if png.is_apng().map_err(message)? {
                return Err(message(
                    "Animated PNG is not supported; no frames were discarded.",
                ));
            }
        }
        ImageFormat::WebP => {
            let webp = image::codecs::webp::WebPDecoder::new(BufReader::new(File::open(source)?))
                .map_err(message)?;
            if webp.has_animation() {
                return Err(message(
                    "Animated WebP is not supported; no frames were discarded.",
                ));
            }
        }
        ImageFormat::Tiff if tiff_page_count(source)? > 1 => {
            return Err(message(
                "Multi-page TIFF is not supported; no pages were discarded.",
            ));
        }
        _ => {}
    }
    Ok(())
}

/// Counts the IFD chain of a classic or BigTIFF file without decoding pixels.
fn tiff_page_count(source: &Path) -> Result<u32> {
    let mut file = File::open(source)?;
    let mut head = [0u8; 16];
    file.read_exact(&mut head[..8])?;
    let little = match &head[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Err(message("Unreadable TIFF header.")),
    };
    let u16_at = |b: &[u8]| -> u16 {
        let a = [b[0], b[1]];
        if little {
            u16::from_le_bytes(a)
        } else {
            u16::from_be_bytes(a)
        }
    };
    let u32_at = |b: &[u8]| -> u32 {
        let a = [b[0], b[1], b[2], b[3]];
        if little {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        }
    };
    let u64_at = |b: &[u8]| -> u64 {
        let mut a = [0u8; 8];
        a.copy_from_slice(&b[..8]);
        if little {
            u64::from_le_bytes(a)
        } else {
            u64::from_be_bytes(a)
        }
    };
    let big = match u16_at(&head[2..4]) {
        42 => false,
        43 => true,
        _ => return Err(message("Unreadable TIFF header.")),
    };
    let mut next = if big {
        file.read_exact(&mut head[8..16])?;
        u64_at(&head[8..16])
    } else {
        u64::from(u32_at(&head[4..8]))
    };
    let mut pages = 0u32;
    while next != 0 && pages < 64 {
        pages += 1;
        file.seek(SeekFrom::Start(next))?;
        if big {
            let mut count = [0u8; 8];
            file.read_exact(&mut count)?;
            let entries = u64_at(&count);
            file.seek(SeekFrom::Current((entries as i64).saturating_mul(20)))?;
            file.read_exact(&mut count)?;
            next = u64_at(&count);
        } else {
            let mut count = [0u8; 2];
            file.read_exact(&mut count)?;
            let entries = u16_at(&count);
            file.seek(SeekFrom::Current(i64::from(entries) * 12))?;
            let mut off = [0u8; 4];
            file.read_exact(&mut off)?;
            next = u64::from(u32_at(&off));
        }
    }
    Ok(pages)
}

/// Reads dimensions without decoding the pixels.
pub fn inspect_image(source: &Path) -> Result<ImageInfo> {
    let reader = ImageReader::open(source)?.with_guessed_format()?;
    let (kind, _) = kind_of(reader.format())?;
    let (width, height) = reader.into_dimensions().map_err(message)?;
    Ok(ImageInfo {
        kind,
        width,
        height,
    })
}

/// Decodes a static image, applies EXIF orientation and the decoder limits.
pub fn decode(source: &Path, cancel: &AtomicBool) -> Result<DynamicImage> {
    let mut reader = ImageReader::open(source)?.with_guessed_format()?;
    let (_, format) = kind_of(reader.format())?;
    reject_multi_frame(source, format)?;
    reader.limits(limits());
    let mut decoder = reader.into_decoder().map_err(message)?;
    let orientation = decoder.orientation().map_err(message)?;
    let mut decoded = DynamicImage::from_decoder(decoder).map_err(message)?;
    decoded.apply_orientation(orientation);
    check_cancel(cancel)?;
    Ok(decoded)
}

/// Fits within `max_edge` on both sides and never upscales. `0` keeps the size.
pub fn resize(image: DynamicImage, max_edge: u32) -> DynamicImage {
    if max_edge > 0 && (image.width() > max_edge || image.height() > max_edge) {
        image.resize(max_edge, max_edge, image::imageops::FilterType::Lanczos3)
    } else {
        image
    }
}

/// Blends every pixel over white and drops the alpha channel.
pub fn flatten_white(image: &DynamicImage) -> image::RgbImage {
    let mut rgb = image::RgbImage::new(image.width(), image.height());
    for (x, y, p) in image.to_rgba8().enumerate_pixels() {
        let a = u32::from(p[3]);
        rgb.put_pixel(
            x,
            y,
            image::Rgb(
                [0, 1, 2].map(|i| ((u32::from(p[i]) * a + 255 * (255 - a) + 127) / 255) as u8),
            ),
        );
    }
    rgb
}

pub fn has_alpha(image: &DynamicImage) -> bool {
    image.color().has_alpha() && image.to_rgba8().pixels().any(|p| p[3] != 255)
}

pub fn encode_jpeg(image: &DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let rgb = flatten_white(image);
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(&rgb)
        .map_err(message)?;
    Ok(out)
}

fn validate(max_edge: u32, quality: u8) -> Result<()> {
    if !(1..=100).contains(&quality) || max_edge > MAX_EDGE {
        return Err(message("Invalid image settings."));
    }
    Ok(())
}

/// Encodes a decoded image as `png`, `jpg`, `webp` or a one-page `pdf`.
pub fn encode(image: &DynamicImage, format: &str, quality: u8) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    match format {
        "png" => image
            .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
            .map_err(message)?,
        "jpg" => out = encode_jpeg(image, quality)?,
        "webp" => {
            let rgba = image.to_rgba8();
            let encoder = webp::Encoder::from_rgba(&rgba, rgba.width(), rgba.height());
            let memory = if quality >= 100 {
                encoder.encode_lossless()
            } else {
                encoder.encode(f32::from(quality))
            };
            out.extend_from_slice(&memory);
        }
        "pdf" => out = pdf::image_pdf(std::slice::from_ref(image), quality)?,
        _ => return Err(message("Choose PNG, JPG, WebP or PDF output.")),
    }
    Ok(out)
}

/// Converts one image file. `format` is `png`, `jpg`, `webp` or `pdf`.
pub fn convert_image(
    source: &Path,
    destination: &Path,
    format: &str,
    max_edge: u32,
    quality: u8,
    cancel: &AtomicBool,
) -> Result<()> {
    validate(max_edge, quality)?;
    if !matches!(format, "png" | "jpg" | "webp" | "pdf") {
        return Err(message("Choose PNG, JPG, WebP or PDF output."));
    }
    let decoded = resize(decode(source, cancel)?, max_edge);
    check_cancel(cancel)?;
    let bytes = encode(&decoded, format, quality)?;
    let mut temp = output(destination)?;
    temp.write_all(&bytes)?;
    commit(temp, destination, cancel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cancel() -> AtomicBool {
        AtomicBool::new(false)
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
        convert_image(&src, &out, "jpg", 20, 90, &cancel()).unwrap();
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

    #[test]
    fn webp_and_tiff_roundtrip_and_pdf_output() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("source.png");
        image::RgbaImage::from_fn(30, 20, |x, _| {
            image::Rgba([x as u8 * 8, 100, 50, if x < 15 { 255 } else { 0 }])
        })
        .save(&src)
        .unwrap();
        let webp_out = dir.path().join("out.webp");
        convert_image(&src, &webp_out, "webp", 0, 100, &cancel()).unwrap();
        assert_eq!(inspect_image(&webp_out).unwrap().kind, InputKind::Webp);
        let back = decode(&webp_out, &cancel()).unwrap();
        assert_eq!((back.width(), back.height()), (30, 20));
        assert!(has_alpha(&back));
        let lossy = dir.path().join("lossy.webp");
        convert_image(&src, &lossy, "webp", 0, 60, &cancel()).unwrap();
        assert_eq!(inspect_image(&lossy).unwrap().width, 30);
        let tiff_src = dir.path().join("source.tiff");
        image::open(&src)
            .unwrap()
            .to_rgb8()
            .save_with_format(&tiff_src, ImageFormat::Tiff)
            .unwrap();
        assert_eq!(tiff_page_count(&tiff_src).unwrap(), 1);
        let png_out = dir.path().join("from-tiff.png");
        convert_image(&tiff_src, &png_out, "png", 0, 85, &cancel()).unwrap();
        assert_eq!(inspect_image(&png_out).unwrap().width, 30);
        let pdf_out = dir.path().join("out.pdf");
        convert_image(&src, &pdf_out, "pdf", 0, 85, &cancel()).unwrap();
        assert_eq!(pdf::info(&pdf_out).unwrap().pages, 1);
        assert!(convert_image(&src, &pdf_out, "pdf", 0, 85, &cancel()).is_err());
    }
}
