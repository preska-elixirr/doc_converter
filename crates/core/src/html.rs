//! Make LibreOffice's generated HTML portable before its work directory is removed.

use crate::{check_cancel, message, write_bytes, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use html5ever::{parse_document, serialize, tendril::TendrilSink};
use markup5ever_rcdom::{NodeData, RcDom, SerializableHandle};
use std::{path::Path, sync::atomic::AtomicBool};

/// Embed local image sidecars from the export directory. Never fetch remote
/// resources or read files outside that directory. Fail before saving if an
/// image cannot be preserved, rather than silently producing a broken export.
pub(crate) fn embed_export_images(
    source: &Path,
    destination: &Path,
    cancel: &AtomicBool,
) -> Result<()> {
    check_cancel(cancel)?;
    let root = source
        .parent()
        .ok_or_else(|| message("Invalid HTML export path"))?
        .canonicalize()?;
    let html = crate::text::read_text_file(source)?;
    let dom = parse_document(RcDom::default(), Default::default()).one(html);
    let mut pending = vec![dom.document.clone()];
    while let Some(node) = pending.pop() {
        check_cancel(cancel)?;
        pending.extend(node.children.borrow().iter().cloned());
        if let NodeData::Element { name, attrs, .. } = &node.data {
            if name.local.as_ref() != "img" {
                continue;
            }
            for attr in attrs
                .borrow_mut()
                .iter_mut()
                .filter(|a| a.name.local.as_ref() == "src")
            {
                let src = attr.value.to_string();
                if src.to_ascii_lowercase().starts_with("data:") {
                    continue;
                }
                let relative = percent_encoding::percent_decode_str(
                    src.split(['?', '#']).next().unwrap_or(""),
                )
                .decode_utf8()
                .map_err(|_| message("Invalid image path in HTML export"))?;
                if relative.is_empty()
                    || relative.contains(':')
                    || relative.starts_with(['/', '\\'])
                {
                    return Err(message(format!(
                        "Cannot embed image outside the HTML export: {src}"
                    )));
                }
                let path = root
                    .join(relative.as_ref())
                    .canonicalize()
                    .map_err(|e| message(format!("Cannot preserve HTML image {src}: {e}")))?;
                if !path.starts_with(&root) {
                    return Err(message(format!(
                        "Cannot embed image outside the HTML export: {src}"
                    )));
                }
                let mime = match path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "png" => "image/png",
                    "jpg" | "jpeg" => "image/jpeg",
                    "gif" => "image/gif",
                    "bmp" => "image/bmp",
                    "webp" => "image/webp",
                    "svg" => "image/svg+xml",
                    other => return Err(message(format!("Cannot embed HTML image type: {other}"))),
                };
                let bytes = std::fs::read(&path)?;
                attr.value = format!("data:{mime};base64,{}", STANDARD.encode(bytes)).into();
            }
        }
    }
    let mut bytes = Vec::new();
    serialize(
        &mut bytes,
        &SerializableHandle::from(dom.document),
        Default::default(),
    )?;
    write_bytes(destination, &bytes, cancel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exported_images_survive_without_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        let exports = dir.path().join("export");
        std::fs::create_dir(&exports).unwrap();
        let picture = exports.join("chart & logo.png");
        image::RgbImage::new(2, 2).save(&picture).unwrap();
        let original = std::fs::read(&picture).unwrap();
        let input = exports.join("sheet.html");
        std::fs::write(&input, r#"<!doctype html><html><body><table><tr><td>Totals &amp; images<IMG SRC="chart%20&amp;%20logo.png"><img src='chart%20&amp;%20logo.png'><img src=data:image/png;base64,AAAA></td></tr></table></body></html>"#).unwrap();
        let output = dir.path().join("saved.html");
        embed_export_images(&input, &output, &AtomicBool::new(false)).unwrap();
        std::fs::remove_dir_all(exports).unwrap();
        let saved = std::fs::read_to_string(&output).unwrap();
        let expected = format!("data:image/png;base64,{}", STANDARD.encode(&original));
        assert_eq!(saved.matches(&expected).count(), 2);
        assert!(saved.contains("Totals &amp; images"));
        assert!(saved.contains("data:image/png;base64,AAAA"));
        assert!(saved.contains("<!DOCTYPE html>"));
    }

    #[test]
    fn missing_external_or_cancelled_images_leave_no_output() {
        let dir = tempfile::tempdir().unwrap();
        let exports = dir.path().join("export");
        std::fs::create_dir(&exports).unwrap();
        std::fs::write(dir.path().join("private.png"), b"private").unwrap();
        let input = exports.join("sheet.html");
        let output = dir.path().join("saved.html");
        for src in [
            "missing.png",
            "../private.png",
            "%2e%2e/private.png",
            "https://example.com/image.png",
        ] {
            std::fs::write(&input, format!("<img src='{src}'>")).unwrap();
            assert!(embed_export_images(&input, &output, &AtomicBool::new(false)).is_err());
            assert!(!output.exists());
        }
        std::fs::write(&input, "<p>Cancelled</p>").unwrap();
        assert!(embed_export_images(&input, &output, &AtomicBool::new(true)).is_err());
        assert!(!output.exists());
    }
}
