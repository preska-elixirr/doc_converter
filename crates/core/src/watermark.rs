//! Local, embedded-font text overlays for ordinary PDF outputs.
use crate::{check_cancel, message, Result};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use std::sync::{atomic::AtomicBool, OnceLock};
use typst::foundations::{Dict, Str, Value};
use typst_as_lib::{typst_kit_options::TypstKitFontOptions, TypstEngine, TypstTemplateMainFile};
use typst_layout::PagedDocument;

/// Reject empty, multiline, control-containing or excessively long labels.
pub fn validate(text: &str) -> Result<()> {
    if text.trim().is_empty()
        || text.chars().count() > 80
        || text
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
    {
        return Err(message(
            "Watermark text must contain 1–80 characters on one line.",
        ));
    }
    Ok(())
}

fn label_pdf(text: &str) -> Result<Vec<u8>> {
    static ENGINE: OnceLock<TypstEngine<TypstTemplateMainFile>> = OnceLock::new();
    let engine = ENGINE.get_or_init(|| {
        TypstEngine::builder()
            .main_file(
                r##"
#set page(width: 600pt, height: 200pt, margin: 0pt, fill: none)
#let label = sys.inputs.label
#set text(size: calc.min(48, 480 / label.len()) * 1pt, fill: rgb("#707070"))
#place(center + horizon, text(label))
"##,
            )
            .search_fonts_with(
                TypstKitFontOptions::default()
                    .include_system_fonts(false)
                    .include_embedded_fonts(true),
            )
            .build()
    });
    let mut inputs = Dict::new();
    inputs.insert(Str::from("label"), Value::Str(Str::from(text.trim())));
    let compiled: PagedDocument = engine.compile_with_input(inputs).output.map_err(message)?;
    typst_pdf::pdf(&compiled, &typst_pdf::PdfOptions::default())
        .map_err(|_| message("Could not render watermark text."))
}

fn inherited(doc: &Document, page: ObjectId, key: &[u8]) -> Result<Option<Object>> {
    let mut id = page;
    for _ in 0..64 {
        let dict = doc.get_dictionary(id).map_err(message)?;
        if let Ok(value) = dict.get(key) {
            return Ok(Some(doc.dereference(value).map_err(message)?.1.clone()));
        }
        match dict.get(b"Parent") {
            Ok(parent) => id = parent.as_reference().map_err(message)?,
            Err(_) => return Ok(None),
        }
    }
    Err(message("Invalid PDF page tree."))
}

/// Add a translucent diagonal label to every page. Reject locked or empty PDFs.
/// The caller commits the resulting bytes using the no-overwrite output helpers.
pub fn apply(bytes: &[u8], text: &str, cancel: &AtomicBool) -> Result<Vec<u8>> {
    validate(text)?;
    check_cancel(cancel)?;
    let mut doc = Document::load_mem(bytes).map_err(message)?;
    if doc.is_encrypted() {
        return Err(message(
            "This PDF has a password. Unlock it before watermarking.",
        ));
    }
    if matches!(doc.version.as_str(), "1.0" | "1.1" | "1.2" | "1.3") {
        doc.version = "1.4".into(); // transparency requires PDF 1.4 or later
    }
    let pages = doc.get_pages();
    if pages.is_empty() {
        return Err(message("This PDF has no pages."));
    }
    let mut label = Document::load_mem(&label_pdf(text)?).map_err(message)?;
    check_cancel(cancel)?;
    label.renumber_objects_with(doc.max_id + 1);
    let label_page = *label
        .get_pages()
        .values()
        .next()
        .ok_or_else(|| message("Missing watermark page."))?;
    let resources =
        inherited(&label, label_page, b"Resources")?.unwrap_or(Object::Dictionary(dictionary! {}));
    let content = page_content(&label, label_page)?;
    doc.max_id = label.max_id;
    doc.objects.extend(label.objects);
    let mut form = Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "FormType" => 1,
            "BBox" => vec![0.into(), 0.into(), 600.into(), 200.into()],
            "Resources" => resources,
        },
        content,
    );
    form.compress().map_err(message)?;
    let form = doc.add_object(form);
    let opacity = doc.add_object(dictionary! { "Type" => "ExtGState", "ca" => 0.22, "CA" => 0.22 });
    for page in pages.into_values() {
        check_cancel(cancel)?;
        let media =
            inherited(&doc, page, b"MediaBox")?.ok_or_else(|| message("PDF page has no size."))?;
        let crop = inherited(&doc, page, b"CropBox")?.unwrap_or(media.clone());
        let rect = |obj: &Object| -> Result<Vec<f32>> {
            let values = obj
                .as_array()
                .map_err(message)?
                .iter()
                .map(|v| v.as_float().map_err(message))
                .collect::<Result<Vec<_>>>()?;
            if values.len() != 4 || values.iter().any(|v| !v.is_finite()) {
                return Err(message("Invalid PDF page size."));
            }
            // Corners may come in any order; normalise to lower-left, upper-right.
            Ok(vec![
                values[0].min(values[2]),
                values[1].min(values[3]),
                values[0].max(values[2]),
                values[1].max(values[3]),
            ])
        };
        let m = rect(&media)?;
        let c = rect(&crop)?;
        let (x0, y0, x1, y1) = (
            m[0].max(c[0]),
            m[1].max(c[1]),
            m[2].min(c[2]),
            m[3].min(c[3]),
        );
        let (w, h) = (x1 - x0, y1 - y0);
        if w <= 0.0 || h <= 0.0 {
            return Err(message("Invalid PDF page size."));
        }
        let rotation = inherited(&doc, page, b"Rotate")?
            .map(|o| o.as_i64().map_err(message))
            .transpose()?
            .unwrap_or(0);
        if rotation % 90 != 0 {
            return Err(message("Invalid PDF page rotation."));
        }
        let angle = (35.0 + rotation.rem_euclid(360) as f32).to_radians();
        let scale = w.min(h) * 0.9 / 600.0;
        let (a, b) = (scale * angle.cos(), scale * angle.sin());
        let (x, y) = (
            (x0 + x1) / 2.0 - 300.0 * a + 100.0 * b,
            (y0 + y1) / 2.0 - 300.0 * b - 100.0 * a,
        );
        // The original streams stay in place, wrapped in q/Q, so text extraction
        // and other readers still see them; the label is appended after them.
        page_content(&doc, page)?; // every original stream must decode
        let parts = existing_contents(&mut doc, page)?;
        let mut resources = match inherited(&doc, page, b"Resources")? {
            Some(Object::Dictionary(dict)) => dict,
            Some(_) => return Err(message("Invalid PDF page resources.")),
            None => dictionary! {},
        };
        let mut xobjects = sub_dict(&doc, &resources, b"XObject")?;
        let mut states = sub_dict(&doc, &resources, b"ExtGState")?;
        let form_name = unique_name(&xobjects, "DocConverterWatermark");
        let state_name = unique_name(&states, "DocConverterOpacity");
        xobjects.set(form_name.clone(), form);
        states.set(state_name.clone(), opacity);
        resources.set("XObject", xobjects);
        resources.set("ExtGState", states);
        let prefix = doc.add_object(Stream::new(dictionary! {}, b"q\n".to_vec()));
        let suffix = doc.add_object(Stream::new(
            dictionary! {},
            format!(
                "\nQ\nq /{state_name} gs {a} {b} {} {a} {x} {y} cm /{form_name} Do Q\n",
                -b
            )
            .into_bytes(),
        ));
        let mut contents = vec![Object::Reference(prefix)];
        contents.extend(parts);
        contents.push(Object::Reference(suffix));
        let page_dict = doc.get_dictionary_mut(page).map_err(message)?;
        page_dict.set("Contents", contents);
        page_dict.set("Resources", resources);
    }
    doc.prune_objects();
    check_cancel(cancel)?;
    let mut output = Vec::new();
    doc.save_to(&mut output).map_err(message)?;
    check_cancel(cancel)?;
    Ok(output)
}

/// The page's content streams as references, in order, without touching them.
fn existing_contents(doc: &mut Document, page: ObjectId) -> Result<Vec<Object>> {
    let contents = doc
        .get_dictionary(page)
        .map_err(message)?
        .get(b"Contents")
        .ok()
        .cloned();
    Ok(match contents {
        None => Vec::new(),
        Some(Object::Array(parts)) => parts,
        Some(Object::Reference(id)) => match doc.get_object(id).map_err(message)? {
            Object::Array(parts) => parts.clone(),
            _ => vec![Object::Reference(id)],
        },
        Some(direct) => vec![Object::Reference(doc.add_object(direct))],
    })
}

/// A resource sub-dictionary as an owned copy, empty when absent.
fn sub_dict(doc: &Document, resources: &Dictionary, key: &[u8]) -> Result<Dictionary> {
    match resources.get(key) {
        Ok(value) => Ok(doc
            .dereference(value)
            .map_err(message)?
            .1
            .as_dict()
            .map_err(message)?
            .clone()),
        Err(_) => Ok(dictionary! {}),
    }
}

/// `base`, or `base2`, `base3`, … when the page already uses that name.
fn unique_name(dict: &Dictionary, base: &str) -> String {
    let mut name = base.to_string();
    let mut n = 1;
    while dict.has(name.as_bytes()) {
        n += 1;
        name = format!("{base}{n}");
    }
    name
}

// Unlike lopdf's convenience reader, never fall back to undecoded bytes or
// silently omit broken streams: a watermark must not destroy source content.
fn page_content(doc: &Document, page: ObjectId) -> Result<Vec<u8>> {
    let dict = doc.get_dictionary(page).map_err(message)?;
    let Ok(contents) = dict.get(b"Contents") else {
        return Ok(Vec::new());
    };
    let obj = doc.dereference(contents).map_err(message)?.1;
    let parts = match obj {
        Object::Array(parts) => parts.as_slice(),
        _ => std::slice::from_ref(obj),
    };
    let mut bytes = Vec::new();
    for part in parts {
        let stream = doc
            .dereference(part)
            .map_err(message)?
            .1
            .as_stream()
            .map_err(message)?;
        let limit = (64 * 1024 * 1024usize).saturating_sub(bytes.len());
        bytes.extend(
            stream
                .decompressed_content_with_limit(limit)
                .map_err(message)?,
        );
        bytes.push(b'\n');
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> Vec<u8> {
        let images = vec![
            image::DynamicImage::new_rgb8(30, 50),
            image::DynamicImage::new_rgb8(50, 30),
        ];
        crate::pdf::image_pdf(&images, 80).unwrap()
    }

    #[test]
    fn watermark_every_page_preserves_content_and_handles_inherited_geometry() {
        let mut doc = Document::load_mem(&source()).unwrap();
        let pages = doc.get_pages();
        let first = pages[&1];
        let parent = doc
            .get_dictionary(first)
            .unwrap()
            .get(b"Parent")
            .unwrap()
            .as_reference()
            .unwrap();
        let resources = inherited(&doc, first, b"Resources").unwrap().unwrap();
        doc.get_dictionary_mut(parent)
            .unwrap()
            .set("Resources", resources);
        doc.get_dictionary_mut(first).unwrap().remove(b"Resources");
        doc.get_dictionary_mut(parent).unwrap().set(
            "CropBox",
            vec![20.into(), 40.into(), 450.into(), 550.into()],
        );
        doc.get_dictionary_mut(parent).unwrap().set("Rotate", 90);
        let before: Vec<_> = pages
            .values()
            .map(|p| page_content(&doc, *p).unwrap())
            .collect();
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let marked = apply(&bytes, "Željko Čović #pagebreak()", &AtomicBool::new(false)).unwrap();
        let result = Document::load_mem(&marked).unwrap();
        assert_eq!(result.get_pages().len(), 2);
        for (i, page) in result.get_pages().into_values().enumerate() {
            let resources = result
                .get_dictionary(page)
                .unwrap()
                .get(b"Resources")
                .unwrap()
                .as_dict()
                .unwrap();
            let objects = resources.get(b"XObject").unwrap().as_dict().unwrap();
            let label = result
                .get_object(
                    objects
                        .get(b"DocConverterWatermark")
                        .unwrap()
                        .as_reference()
                        .unwrap(),
                )
                .unwrap()
                .as_stream()
                .unwrap();
            assert!(!label.content.is_empty());
            assert!(label.dict.has(b"Resources"));
            let contents = page_content(&result, page).unwrap();
            assert!(
                contents.windows(before[i].len()).any(|w| w == before[i]),
                "original streams stay in place"
            );
            let text = String::from_utf8_lossy(&contents);
            assert!(text.starts_with("q\n"));
            assert!(text.contains("/DocConverterOpacity gs"));
            assert!(text.trim_end().ends_with("/DocConverterWatermark Do Q"));
        }
        assert_ne!(bytes, marked);
    }

    /// Text stays extractable, and a reversed MediaBox is accepted.
    #[test]
    fn watermarked_text_is_still_extractable() {
        let mut doc = Document::with_version("1.5");
        let font = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let content = doc.add_object(Stream::new(
            dictionary! {},
            b"BT /F1 24 Tf 72 700 Td (Hello watermark) Tj ET".to_vec(),
        ));
        let pages_id = doc.new_object_id();
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content,
            "MediaBox" => vec![612.into(), 792.into(), 0.into(), 0.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let before = Document::load_mem(&bytes)
            .unwrap()
            .extract_text(&[1])
            .unwrap();
        assert!(before.contains("Hello watermark"), "{before}");
        let marked = apply(&bytes, "Confidential", &AtomicBool::new(false)).unwrap();
        let after = Document::load_mem(&marked)
            .unwrap()
            .extract_text(&[1])
            .unwrap();
        assert!(after.contains("Hello watermark"), "{after}");
    }

    #[test]
    fn watermark_rejects_invalid_locked_cancelled_and_broken_content() {
        for text in [
            "",
            "   ",
            "a\nb",
            "a\0b",
            "a\u{2028}b",
            "a\u{2029}b",
            &"x".repeat(81),
        ] {
            assert!(apply(&source(), text, &AtomicBool::new(false)).is_err());
        }
        assert!(validate("Confidential").is_ok());
        assert!(apply(&source(), "Recipient", &AtomicBool::new(true)).is_err());
        let locked =
            crate::pdf::protect_bytes(&source(), &crate::secret("password".into())).unwrap();
        assert!(apply(&locked, "Recipient", &AtomicBool::new(false)).is_err());
        let mut doc = Document::load_mem(&source()).unwrap();
        let page = doc.get_pages()[&1];
        let id = doc.add_object(Stream::new(
            dictionary! {"Filter" => "UnknownFilter"},
            b"garbage".to_vec(),
        ));
        doc.get_dictionary_mut(page).unwrap().set("Contents", id);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        assert!(apply(&bytes, "Recipient", &AtomicBool::new(false)).is_err());
    }
}
