//! PDF operations on top of `lopdf`: open password (AES-256), unlock, merge,
//! text extraction, page count, and image pages.

use crate::{commit, message, output, write_bytes, Result, SecretString};
use age::secrecy::ExposeSecret;
use image::DynamicImage;
use lopdf::{
    dictionary,
    encryption::crypt_filters::{Aes256CryptFilter, CryptFilter},
    Document, EncryptionState, EncryptionVersion, Object, ObjectId, Permissions, Stream,
};
use rand::RngExt;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};

const A4: (f64, f64) = (595.276, 841.89);
const IMAGE_MARGIN: f64 = 28.35; // 10 mm

#[derive(Clone, Debug, Serialize)]
pub struct PdfInfo {
    pub pages: u32,
    pub encrypted: bool,
}

fn load(source: &Path) -> Result<Document> {
    Document::load(source).map_err(|e| message(format!("Unreadable PDF: {e}")))
}

/// Page count and whether an open password is needed.
pub fn info(source: &Path) -> Result<PdfInfo> {
    match Document::load_metadata(source) {
        Ok(meta) => Ok(PdfInfo {
            pages: meta.page_count,
            encrypted: meta.encrypted,
        }),
        Err(_) => {
            let bytes = std::fs::read(source)?;
            if !bytes.starts_with(b"%PDF") {
                return Err(message("Unreadable PDF."));
            }
            Ok(PdfInfo {
                pages: 0,
                encrypted: bytes.windows(8).any(|w| w == b"/Encrypt"),
            })
        }
    }
}

fn random_owner_password() -> String {
    let mut bytes = [0u8; 24];
    rand::rng().fill(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn encrypt_document(doc: &mut Document, password: &SecretString) -> Result<()> {
    if doc.is_encrypted() {
        return Err(message(
            "This PDF already has a password. Unlock it first, then protect it again.",
        ));
    }
    doc.encryption_state = None;
    let mut key = [0u8; 32];
    rand::rng().fill(&mut key);
    let filter: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);
    let owner = random_owner_password();
    let state = EncryptionState::try_from(EncryptionVersion::V5 {
        encrypt_metadata: true,
        crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), filter)]),
        file_encryption_key: &key,
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: &owner,
        user_password: password.expose_secret(),
        permissions: Permissions::all(),
    })
    .map_err(|e| message(format!("Could not prepare encryption: {e}")))?;
    doc.encrypt(&state)
        .map_err(|e| message(format!("Could not encrypt PDF: {e}")))?;
    Ok(())
}

fn to_bytes(doc: &mut Document) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| message(format!("Could not write PDF: {e}")))?;
    Ok(out)
}

/// Adds an AES-256 open password to a PDF held in memory.
pub fn protect_bytes(pdf: &[u8], password: &SecretString) -> Result<Vec<u8>> {
    let mut doc = Document::load_mem(pdf).map_err(|e| message(format!("Unreadable PDF: {e}")))?;
    encrypt_document(&mut doc, password)?;
    to_bytes(&mut doc)
}

/// Writes a copy of `source` that needs `password` to open. AES-256, PDF 2.0 style.
pub fn protect(
    source: &Path,
    destination: &Path,
    password: &SecretString,
    cancel: &AtomicBool,
) -> Result<()> {
    let mut doc = load(source)?;
    encrypt_document(&mut doc, password)?;
    let bytes = to_bytes(&mut doc)?;
    write_bytes(destination, &bytes, cancel)
}

/// Writes a copy of `source` without its open password.
pub fn unlock(
    source: &Path,
    destination: &Path,
    password: &SecretString,
    cancel: &AtomicBool,
) -> Result<()> {
    if !info(source)?.encrypted {
        return Err(message("This PDF has no password."));
    }
    let mut doc = Document::load_with_password(source, password.expose_secret())
        .map_err(|_| message("Incorrect password or damaged PDF."))?;
    if doc.is_encrypted() {
        return Err(message("Incorrect password or damaged PDF."));
    }
    doc.encryption_state = None;
    doc.prune_objects();
    let bytes = to_bytes(&mut doc)?;
    write_bytes(destination, &bytes, cancel)
}

fn inherited(doc: &Document, page: ObjectId, key: &[u8]) -> Option<Object> {
    let mut current = page;
    for _ in 0..64 {
        let dict = doc.get_dictionary(current).ok()?;
        if let Ok(value) = dict.get(key) {
            return Some(value.clone());
        }
        current = dict.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

/// Concatenates the pages of `sources` in order into one document.
pub fn merge_to_bytes(sources: &[PathBuf], cancel: &AtomicBool) -> Result<Vec<u8>> {
    if sources.is_empty() {
        return Err(message("Nothing to merge."));
    }
    let mut merged = Document::with_version("1.7");
    let pages_id = merged.new_object_id();
    let mut kids: Vec<Object> = Vec::new();
    for source in sources {
        crate::check_cancel(cancel)?;
        let name = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut doc =
            Document::load(source).map_err(|e| message(format!("{name}: unreadable PDF ({e})")))?;
        if doc.is_encrypted() {
            return Err(message(format!(
                "{name} has a password. Unlock it before merging."
            )));
        }
        doc.renumber_objects_with(merged.max_id + 1);
        merged.max_id = doc.max_id;
        let page_ids: Vec<ObjectId> = doc.get_pages().into_values().collect();
        if page_ids.is_empty() {
            return Err(message(format!("{name} has no pages.")));
        }
        for page_id in &page_ids {
            let mut dict = doc
                .get_dictionary(*page_id)
                .map_err(|e| message(format!("{name}: {e}")))?
                .clone();
            for key in [b"Resources".as_slice(), b"MediaBox", b"CropBox", b"Rotate"] {
                if !dict.has(key) {
                    if let Some(value) = inherited(&doc, *page_id, key) {
                        dict.set(key, value);
                    }
                }
            }
            dict.set("Parent", pages_id);
            merged.objects.insert(*page_id, Object::Dictionary(dict));
            kids.push((*page_id).into());
        }
        for (id, object) in doc.objects {
            match object.type_name().unwrap_or(b"") {
                b"Catalog" | b"Pages" | b"Page" | b"Outlines" | b"Outline" => {}
                _ => {
                    merged.objects.insert(id, object);
                }
            }
        }
    }
    let count = kids.len() as u32;
    merged.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => count,
        }),
    );
    let catalog = merged.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    merged.trailer.set("Root", catalog);
    merged.renumber_objects();
    to_bytes(&mut merged)
}

pub fn merge(sources: &[PathBuf], destination: &Path, cancel: &AtomicBool) -> Result<()> {
    let bytes = merge_to_bytes(sources, cancel)?;
    write_bytes(destination, &bytes, cancel)
}

/// Writes the text of every page, pages separated by a form feed line.
pub fn extract_text(source: &Path, destination: &Path, cancel: &AtomicBool) -> Result<()> {
    let doc = load(source)?;
    if doc.is_encrypted() {
        return Err(message("This PDF has a password. Unlock it first."));
    }
    let pages: Vec<u32> = doc.get_pages().into_keys().collect();
    let mut text = String::new();
    for (i, page) in pages.iter().enumerate() {
        if i > 0 {
            text.push_str("\n\u{c}\n");
        }
        let chunk = doc
            .extract_text_with_limit(&[*page], 64 * 1024 * 1024)
            .map_err(|e| message(format!("Could not read page {page}: {e}")))?;
        text.push_str(chunk.trim_end());
        text.push('\n');
    }
    let mut temp = output(destination)?;
    temp.write_all(text.as_bytes())?;
    commit(temp, destination, cancel)
}

/// One A4 page per image, landscape when the image is wider than tall, fitted
/// inside a 10 mm margin and centred. Opaque images are stored as JPEG at
/// `quality`; images with transparency keep it through a soft mask.
pub fn image_pdf(images: &[DynamicImage], quality: u8) -> Result<Vec<u8>> {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let mut kids: Vec<Object> = Vec::new();
    for (index, image) in images.iter().enumerate() {
        let (w, h) = (f64::from(image.width()), f64::from(image.height()));
        let (pw, ph) = if w > h { (A4.1, A4.0) } else { A4 };
        let scale = ((pw - 2.0 * IMAGE_MARGIN) / w)
            .min((ph - 2.0 * IMAGE_MARGIN) / h)
            .min(1.0);
        let (dw, dh) = (w * scale, h * scale);
        let (x, y) = ((pw - dw) / 2.0, (ph - dh) / 2.0);
        let xobject = if crate::images::has_alpha(image) {
            let rgba = image.to_rgba8();
            let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
            let mut alpha = Vec::with_capacity(rgba.len() / 4);
            for p in rgba.pixels() {
                rgb.extend_from_slice(&p.0[..3]);
                alpha.push(p.0[3]);
            }
            let mut mask = Stream::new(
                dictionary! {
                    "Type" => "XObject", "Subtype" => "Image",
                    "Width" => image.width(), "Height" => image.height(),
                    "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
                },
                alpha,
            );
            mask.compress().map_err(message)?;
            let mask_id = doc.add_object(mask);
            let mut stream = Stream::new(
                dictionary! {
                    "Type" => "XObject", "Subtype" => "Image",
                    "Width" => image.width(), "Height" => image.height(),
                    "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
                    "SMask" => mask_id,
                },
                rgb,
            );
            stream.compress().map_err(message)?;
            stream
        } else {
            let jpeg = crate::images::encode_jpeg(image, quality)?;
            Stream::new(
                dictionary! {
                    "Type" => "XObject", "Subtype" => "Image",
                    "Width" => image.width(), "Height" => image.height(),
                    "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
                    "Filter" => "DCTDecode",
                },
                jpeg,
            )
            .with_compression(false)
        };
        let image_id = doc.add_object(xobject);
        let name = format!("Im{index}");
        let content = format!("q {dw:.3} 0 0 {dh:.3} {x:.3} {y:.3} cm /{name} Do Q\n");
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        let resources_id = doc.add_object(dictionary! {
            "XObject" => dictionary! { name.as_str() => image_id },
        });
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), pw.into(), ph.into()],
            "Contents" => content_id,
            "Resources" => resources_id,
        });
        kids.push(page_id.into());
    }
    let count = kids.len() as u32;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => count,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    to_bytes(&mut doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret;

    fn sample_pdf(pages: usize) -> Vec<u8> {
        let images: Vec<DynamicImage> = (0..pages)
            .map(|i| {
                DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                    40 + i as u32,
                    30,
                    image::Rgb([200, 30, 30]),
                ))
            })
            .collect();
        image_pdf(&images, 80).unwrap()
    }

    #[test]
    fn protect_unlock_and_merge() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let a = dir.path().join("a.pdf");
        let b = dir.path().join("b.pdf");
        std::fs::write(&a, sample_pdf(2)).unwrap();
        std::fs::write(&b, sample_pdf(3)).unwrap();
        assert_eq!(info(&a).unwrap().pages, 2);
        assert!(!info(&a).unwrap().encrypted);

        let locked = dir.path().join("locked.pdf");
        protect(&a, &locked, &secret("open sesame 123".into()), &cancel).unwrap();
        assert!(info(&locked).unwrap().encrypted);
        assert!(Document::load_with_password(&locked, "wrong").is_err());
        assert!(protect(
            &locked,
            &dir.path().join("twice.pdf"),
            &secret("x".into()),
            &cancel
        )
        .is_err());

        let wrong = dir.path().join("wrong.pdf");
        assert!(unlock(&locked, &wrong, &secret("wrong".into()), &cancel).is_err());
        assert!(!wrong.exists());
        let open = dir.path().join("open.pdf");
        unlock(&locked, &open, &secret("open sesame 123".into()), &cancel).unwrap();
        let reopened = info(&open).unwrap();
        assert_eq!(reopened.pages, 2);
        assert!(!reopened.encrypted);
        assert!(unlock(
            &a,
            &dir.path().join("noop.pdf"),
            &secret("x".into()),
            &cancel
        )
        .is_err());

        let merged = dir.path().join("merged.pdf");
        merge(&[a.clone(), b.clone()], &merged, &cancel).unwrap();
        assert_eq!(info(&merged).unwrap().pages, 5);
        assert!(merge(
            &[a.clone(), locked.clone()],
            &dir.path().join("m2.pdf"),
            &cancel
        )
        .is_err());

        assert!(merge_to_bytes(&[a.clone(), b.clone()], &AtomicBool::new(true)).is_err());
        let protected_merge = protect_bytes(
            &merge_to_bytes(&[a, b], &cancel).unwrap(),
            &secret("pw".into()),
        )
        .unwrap();
        assert!(Document::load_mem_with_options(
            &protected_merge,
            lopdf::LoadOptions::with_password("nope")
        )
        .is_err());
        let doc = Document::load_mem_with_options(
            &protected_merge,
            lopdf::LoadOptions::with_password("pw"),
        )
        .unwrap();
        assert!(!doc.is_encrypted());
        assert_eq!(doc.get_pages().len(), 5);
    }

    #[test]
    fn text_extraction_writes_pages() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let resources_id =
            doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font_id } });
        let content = lopdf::content::Content {
            operations: vec![
                lopdf::content::Operation::new("BT", vec![]),
                lopdf::content::Operation::new("Tf", vec!["F1".into(), 12.into()]),
                lopdf::content::Operation::new("Td", vec![50.into(), 700.into()]),
                lopdf::content::Operation::new(
                    "Tj",
                    vec![Object::string_literal("Hello Doc Converter")],
                ),
                lopdf::content::Operation::new("ET", vec![]),
            ],
        };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let src = dir.path().join("text.pdf");
        doc.save(&src).unwrap();
        let out = dir.path().join("text.txt");
        extract_text(&src, &out, &cancel).unwrap();
        assert!(std::fs::read_to_string(&out)
            .unwrap()
            .contains("Hello Doc Converter"));
    }
}
