//! Explicit, local privacy cleaning. Always creates a new copy.

mod word;

use crate::{check_cancel, commit, images, message, output, InputKind, Result};
use lopdf::{Document, Object};
use std::{path::Path, sync::atomic::AtomicBool};

pub fn supported(kind: InputKind) -> bool {
    matches!(kind, InputKind::Docx | InputKind::Pdf) || kind.is_image()
}

/// Photos become lossless PNGs so cleaning never introduces JPEG artifacts.
pub fn extension(kind: InputKind) -> &'static str {
    match kind {
        InputKind::Docx => "docx",
        InputKind::Pdf => "pdf",
        _ => "png",
    }
}

/// Removes supported metadata and DOCX review/hidden content. No Office required.
pub fn clean(source: &Path, destination: &Path, cancel: &AtomicBool) -> Result<()> {
    check_cancel(cancel)?;
    match crate::inspect(source) {
        InputKind::Docx => word::clean(source, destination, cancel),
        InputKind::Pdf => clean_pdf(source, destination, cancel),
        kind if kind.is_image() => {
            let pixels = images::decode(source, cancel)?;
            crate::write_bytes(destination, &images::encode(&pixels, "png", 100)?, cancel)
        }
        _ => Err(message(
            "Cleaning supports DOCX, PDF, PNG, JPG, WebP, TIFF and BMP.",
        )),
    }
}

fn scrub_pdf_object(object: &mut Object) {
    match object {
        Object::Dictionary(dict) => scrub_pdf_dict(dict),
        Object::Stream(stream) => scrub_pdf_dict(&mut stream.dict),
        Object::Array(items) => items.iter_mut().for_each(scrub_pdf_object),
        _ => {}
    }
}

fn scrub_pdf_dict(dict: &mut lopdf::Dictionary) {
    // XMP may be attached to the catalog, pages, images or other objects.
    dict.remove(b"Metadata");
    dict.remove(b"PieceInfo");
    dict.remove(b"LastModified");
    for (_, value) in dict.iter_mut() {
        scrub_pdf_object(value);
    }
}

fn clean_pdf(source: &Path, destination: &Path, cancel: &AtomicBool) -> Result<()> {
    // Check the original trailer before lopdf can auto-decrypt an empty password.
    if crate::pdf::info(source)?.encrypted {
        return Err(message("Unlock this PDF in Decrypt before cleaning it."));
    }
    let mut doc = Document::load(source).map_err(message)?;
    if doc.is_encrypted() || doc.get_pages().is_empty() {
        return Err(message(
            "Unreadable or encrypted PDF. No cleaned copy was saved.",
        ));
    }
    if let Ok(Object::Reference(id)) = doc.trailer.get(b"Info") {
        // An aliased Info dictionary must not survive via another reference.
        doc.objects.insert(*id, Object::Null);
    }
    doc.trailer.remove(b"Info");
    doc.trailer.remove(b"ID");
    doc.trailer.remove(b"Prev");
    // Drop old object streams too: their compressed bytes can contain stale Info
    // objects even after the decoded objects have been pruned.
    doc.objects.retain(|_, obj| {
        let dict = match obj {
            Object::Stream(s) => Some(&s.dict),
            Object::Dictionary(d) => Some(&*d),
            _ => None,
        };
        !dict.is_some_and(|d| {
            matches!(
                d.get(b"Type").and_then(Object::as_name),
                Ok(b"Metadata" | b"ObjStm" | b"XRef")
            )
        })
    });
    for obj in doc.objects.values_mut() {
        check_cancel(cancel)?;
        scrub_pdf_object(obj);
    }
    doc.prune_objects();
    let mut temp = output(destination)?;
    doc.save_to(&mut temp).map_err(message)?;
    commit(temp, destination, cancel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Stream};
    use std::{fs, sync::atomic::Ordering};

    #[test]
    fn pdf_removes_info_nested_xmp_and_orphans_preserves_pages() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("private.pdf");
        let out = dir.path().join("clean.pdf");
        let pixels = image::DynamicImage::new_rgb8(4, 3);
        let mut doc = Document::load_mem(&crate::pdf::image_pdf(&[pixels], 90).unwrap()).unwrap();
        let info = doc.add_object(dictionary! {"Author" => Object::string_literal("PRIVATE-AUTHOR"), "CreationDate" => Object::string_literal("PRIVATE-DATE")});
        doc.trailer.set("Info", info);
        doc.catalog_mut().unwrap().set("InfoAlias", info);
        doc.trailer
            .set("ID", vec![Object::string_literal("PRIVATE-ID")]);
        let meta = doc.add_object(Stream::new(
            dictionary! {"Type" => "Metadata", "Subtype" => "XML"},
            b"PRIVATE-XMP".to_vec(),
        ));
        doc.catalog_mut().unwrap().set("Metadata", meta);
        let page = *doc.get_pages().values().next().unwrap();
        doc.get_object_mut(page)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set(
                "Nested",
                vec![Object::Dictionary(dictionary! {"Metadata" => meta})],
            );
        doc.add_object(Object::string_literal("PRIVATE-ORPHAN"));
        // Pack the Info dictionary into a real compressed object stream.
        doc.save_modern(&mut fs::File::create(&source).unwrap())
            .unwrap();
        let before = fs::read(&source).unwrap();
        assert!(String::from_utf8_lossy(&before).contains("ObjStm"));
        let cancel = AtomicBool::new(false);
        clean(&source, &out, &cancel).unwrap();
        let bytes = fs::read(&out).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("PRIVATE-"));
        let cleaned = Document::load(&out).unwrap();
        assert_eq!(cleaned.get_pages().len(), 1);
        assert!(cleaned.trailer.get(b"Info").is_err());
        assert!(cleaned.trailer.get(b"ID").is_err());
        assert!(cleaned.catalog().unwrap().get(b"Metadata").is_err());
        assert_eq!(before, fs::read(&source).unwrap());
        assert!(clean(&source, &out, &cancel).is_err());
        assert_eq!(bytes, fs::read(&out).unwrap());
        cancel.store(true, Ordering::Relaxed);
        let cancelled = dir.path().join("cancelled.pdf");
        assert!(clean(&source, &cancelled, &cancel).is_err());
        assert!(!cancelled.exists());
        cancel.store(false, Ordering::Relaxed);
        let empty_password = dir.path().join("empty-password.pdf");
        crate::pdf::protect(
            &source,
            &empty_password,
            &crate::secret(String::new()),
            &cancel,
        )
        .unwrap();
        assert!(clean(&empty_password, &cancelled, &cancel)
            .unwrap_err()
            .to_string()
            .contains("Unlock"));
        assert!(!cancelled.exists());
        let locked = dir.path().join("locked.pdf");
        cancel.store(false, Ordering::Relaxed);
        crate::pdf::protect(
            &source,
            &locked,
            &crate::secret("long-test-password".into()),
            &cancel,
        )
        .unwrap();
        assert!(clean(&locked, &cancelled, &cancel)
            .unwrap_err()
            .to_string()
            .contains("Unlock"));
        assert!(!cancelled.exists());
    }

    #[test]
    fn photo_discards_exif_gps_camera_and_applies_orientation() {
        use image::{ImageDecoder, ImageEncoder};
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("photo.jpg");
        // Little-endian EXIF: orientation=6, Make, and a GPS latitude IFD.
        let mut exif = b"II\x2a\0\x08\0\0\0\x03\0".to_vec();
        exif.extend_from_slice(&[0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0]);
        exif.extend_from_slice(&[0x0f, 0x01, 2, 0, 15, 0, 0, 0, 50, 0, 0, 0]);
        exif.extend_from_slice(&[0x25, 0x88, 4, 0, 1, 0, 0, 0, 65, 0, 0, 0]);
        exif.extend_from_slice(&[0, 0, 0, 0]);
        exif.extend_from_slice(b"PRIVATE-CAMERA\0");
        exif.extend_from_slice(&[1, 0]); // One GPS entry at offset 65.
        exif.extend_from_slice(&[2, 0, 5, 0, 3, 0, 0, 0, 83, 0, 0, 0]);
        exif.extend_from_slice(&[0, 0, 0, 0]);
        for value in [45u32, 1, 48, 1, 30, 1] {
            exif.extend_from_slice(&value.to_le_bytes());
        }
        let pixels = image::DynamicImage::new_rgb8(8, 4);
        let mut data = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut data);
        encoder.set_exif_metadata(exif).unwrap();
        encoder.encode_image(&pixels).unwrap();
        fs::write(&source, &data).unwrap();
        let out = dir.path().join("photo-clean.png");
        clean(&source, &out, &AtomicBool::new(false)).unwrap();
        let decoded = image::open(&out).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (4, 8));
        let mut decoder = image::ImageReader::open(&out)
            .unwrap()
            .into_decoder()
            .unwrap();
        assert!(decoder.exif_metadata().unwrap().is_none());
        assert!(decoder.icc_profile().unwrap().is_none());
        assert!(!String::from_utf8_lossy(&fs::read(&out).unwrap()).contains("PRIVATE"));
        assert_eq!(fs::read(&source).unwrap(), data);
    }
}
