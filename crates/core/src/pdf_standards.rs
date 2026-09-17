//! Associated-file embedding for PDF/A-3b and PDF/A-4f. Validation follows editing.
use crate::{check_cancel, message, OutputFormat, Result};
use lopdf::{dictionary, Object, Stream};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

pub const MAX_ATTACHMENTS: usize = 20;
pub const MAX_ATTACHMENT_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub enum Relationship {
    Source,
    Data,
    #[default]
    Supplement,
    Alternative,
    Unspecified,
}
impl Relationship {
    fn name(self) -> &'static str {
        match self {
            Self::Source => "Source",
            Self::Data => "Data",
            Self::Supplement => "Supplement",
            Self::Alternative => "Alternative",
            Self::Unspecified => "Unspecified",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Attachment {
    pub source: PathBuf,
    pub relationship: Relationship,
    pub description: String,
}

fn unicode(value: &str) -> Object {
    let mut bytes = vec![0xfe, 0xff];
    bytes.extend(value.encode_utf16().flat_map(u16::to_be_bytes));
    Object::String(bytes, lopdf::StringFormat::Hexadecimal)
}

/// Writes a fresh work file, never the source. Only the validated result is committed.
pub fn embed(
    source: &Path,
    output: &Path,
    format: OutputFormat,
    attachments: &[Attachment],
    cancel: &AtomicBool,
) -> Result<()> {
    check_cancel(cancel)?;
    if !format.supports_attachments()
        || attachments.is_empty()
        || attachments.len() > MAX_ATTACHMENTS
    {
        return Err(message(
            "Attachments require PDF/A-3b or PDF/A-4f and between 1 and 20 files.",
        ));
    }
    let mut doc = lopdf::Document::load(source).map_err(message)?;
    if doc.is_encrypted() {
        return Err(message("Cannot attach files to an encrypted PDF."));
    }
    let mut files = Vec::new();
    let mut names = HashSet::new();
    let mut total = 0;
    for attachment in attachments {
        check_cancel(cancel)?;
        let mut file = std::fs::File::open(&attachment.source)?;
        let meta = file.metadata()?;
        if !meta.is_file() || meta.len() > MAX_ATTACHMENT_BYTES {
            return Err(message(
                "Each attachment must be a regular file no larger than 32 MiB.",
            ));
        }
        let name = attachment
            .source
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| message("Invalid attachment filename."))?
            .to_string();
        if name.chars().any(char::is_control)
            || attachment.description.len() > 2000
            || !names.insert(name.to_lowercase())
        {
            return Err(message(
                "Attachment names must be unique and descriptions at most 2000 bytes.",
            ));
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_ATTACHMENT_BYTES + 1)
            .read_to_end(&mut bytes)?;
        total += bytes.len();
        if bytes.len() as u64 > MAX_ATTACHMENT_BYTES || total > MAX_TOTAL_BYTES {
            return Err(message(
                "Attachments exceed the 32 MiB per-file or 128 MiB total limit.",
            ));
        }
        let modified: chrono::DateTime<chrono::Utc> = meta.modified()?.into();
        files.push((
            name,
            bytes,
            attachment,
            modified.format("D:%Y%m%d%H%M%SZ").to_string(),
        ));
    }
    // Build the PDF name tree in byte-string order; preserve unrelated catalog names.
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut name_entries = Vec::new();
    let mut associated = Vec::new();
    for (index, (name, bytes, attachment, modified)) in files.into_iter().enumerate() {
        check_cancel(cancel)?;
        use md5::{Digest, Md5};
        let checksum = Md5::digest(&bytes).to_vec();
        let mime = match attachment
            .source
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "xml" => "application/xml",
            "pdf" => "application/pdf",
            "txt" => "text/plain",
            "csv" => "text/csv",
            "json" => "application/json",
            _ => "application/octet-stream",
        };
        let size = bytes.len() as i64;
        let embedded = doc.add_object(Stream::new(dictionary! {
            "Type" => "EmbeddedFile", "Subtype" => Object::Name(mime.as_bytes().to_vec()),
            "Params" => dictionary! {"Size" => size, "CheckSum" => Object::String(checksum, lopdf::StringFormat::Hexadecimal), "ModDate" => Object::string_literal(modified)}
        }, bytes));
        let ascii_name: String = name
            .chars()
            .map(|c| if c.is_ascii() { c } else { '_' })
            .collect();
        let specification = doc.add_object(dictionary! {
            "Type" => "Filespec", "F" => Object::string_literal(ascii_name), "UF" => unicode(&name),
            "Desc" => unicode(if attachment.description.trim().is_empty() { &name } else { &attachment.description }),
            "AFRelationship" => Object::Name(attachment.relationship.name().as_bytes().to_vec()),
            "EF" => dictionary! {"F" => embedded, "UF" => embedded}
        });
        name_entries.push(Object::string_literal(format!("attachment-{index:04}")));
        name_entries.push(specification.into());
        associated.push(specification.into());
    }
    let names_object = doc.catalog().map_err(message)?.get(b"Names").ok().cloned();
    let mut catalog_names = match names_object {
        Some(Object::Reference(id)) => doc.get_dictionary(id).map_err(message)?.clone(),
        Some(Object::Dictionary(d)) => d,
        None => lopdf::Dictionary::new(),
        _ => return Err(message("Invalid PDF name dictionary.")),
    };
    if catalog_names.has(b"EmbeddedFiles") || doc.catalog().map_err(message)?.has(b"AF") {
        return Err(message(
            "The exported PDF already contains attachments; refusing to replace them.",
        ));
    }
    let tree = doc.add_object(dictionary! {"Names" => Object::Array(name_entries)});
    catalog_names.set("EmbeddedFiles", tree);
    let names_id = doc.add_object(catalog_names);
    let catalog = doc.catalog_mut().map_err(message)?;
    catalog.set("Names", names_id);
    catalog.set("AF", Object::Array(associated));
    if format == OutputFormat::Pdfa4f {
        let metadata = doc
            .catalog()
            .map_err(message)?
            .get(b"Metadata")
            .and_then(Object::as_reference)
            .map_err(message)?;
        let stream = doc
            .get_object(metadata)
            .and_then(Object::as_stream)
            .map_err(message)?;
        let bytes = if stream.dict.has(b"Filter") {
            stream.decompressed_content().map_err(message)?
        } else {
            stream.content.clone()
        };
        let xmp = add_4f_conformance(&bytes)?;
        let stream = doc
            .get_object_mut(metadata)
            .and_then(Object::as_stream_mut)
            .map_err(message)?;
        stream.dict.remove(b"Filter");
        stream.dict.remove(b"DecodeParms");
        stream.set_content(xmp);
    }
    check_cancel(cancel)?;
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).map_err(message)?;
    crate::write_bytes(output, &bytes, cancel)
}

fn add_4f_conformance(xml: &[u8]) -> Result<Vec<u8>> {
    use quick_xml::{
        events::{BytesEnd, BytesStart, BytesText, Event},
        name::ResolveResult,
    };
    let text = std::str::from_utf8(xml).map_err(message)?;
    let mut reader = quick_xml::NsReader::from_str(text);
    let mut writer = quick_xml::Writer::new(Vec::new());
    let mut inserted = false;
    loop {
        let (ns, event) = reader.read_resolved_event().map_err(message)?;
        match event {
            Event::Start(ref e)
                if matches!(ns, ResolveResult::Bound(n) if n.as_ref() == "http://www.aiim.org/pdfa/ns/id/")
                    && e.local_name().as_ref() == "conformance" =>
            {
                reader.read_to_end(e.name()).map_err(message)?;
            }
            Event::Start(ref e)
                if !inserted
                    && matches!(ns, ResolveResult::Bound(n) if n.as_ref() == "http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                    && e.local_name().as_ref() == "Description" =>
            {
                writer.write_event(event.borrow()).map_err(message)?;
                let mut start = BytesStart::new("pdfaid:conformance");
                start.push_attribute(("xmlns:pdfaid", "http://www.aiim.org/pdfa/ns/id/"));
                writer.write_event(Event::Start(start)).map_err(message)?;
                writer
                    .write_event(Event::Text(BytesText::new("F")))
                    .map_err(message)?;
                writer
                    .write_event(Event::End(BytesEnd::new("pdfaid:conformance")))
                    .map_err(message)?;
                inserted = true;
            }
            Event::Eof => break,
            _ => writer.write_event(event).map_err(message)?,
        }
    }
    if !inserted {
        return Err(message("Missing RDF metadata for PDF/A-4f."));
    }
    Ok(writer.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attachments_are_bounded_preserve_sources_and_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.pdf");
        let pdf = crate::pdf::image_pdf(&[image::DynamicImage::new_rgb8(2, 2)], 85).unwrap();
        std::fs::write(&source, &pdf).unwrap();
        let data = dir.path().join("data.xml");
        std::fs::write(&data, b"<data/>").unwrap();
        let a = Attachment {
            source: data.clone(),
            relationship: Relationship::Data,
            description: "Data".into(),
        };
        let out = dir.path().join("out.pdf");
        let cancel = AtomicBool::new(false);
        for (format, files) in [
            (OutputFormat::Pdfa2b, vec![a.clone()]),
            (OutputFormat::Pdfa3b, vec![]),
            (OutputFormat::Pdfa3b, vec![a.clone(), a.clone()]),
            (OutputFormat::Pdfa3b, vec![a.clone(); 21]),
        ] {
            assert!(embed(&source, &out, format, &files, &cancel).is_err());
            assert!(!out.exists());
        }
        let huge = dir.path().join("huge.bin");
        std::fs::File::create(&huge)
            .unwrap()
            .set_len(MAX_ATTACHMENT_BYTES + 1)
            .unwrap();
        assert!(embed(
            &source,
            &out,
            OutputFormat::Pdfa3b,
            &[Attachment {
                source: huge,
                ..a.clone()
            }],
            &cancel
        )
        .is_err());
        assert!(!out.exists());
        assert!(embed(
            &source,
            &out,
            OutputFormat::Pdfa3b,
            &[a.clone()],
            &AtomicBool::new(true)
        )
        .is_err());
        embed(&source, &out, OutputFormat::Pdfa3b, &[a.clone()], &cancel).unwrap();
        let saved = std::fs::read(&out).unwrap();
        assert!(embed(&source, &out, OutputFormat::Pdfa3b, &[a.clone()], &cancel).is_err());
        assert!(embed(
            &out,
            &dir.path().join("again.pdf"),
            OutputFormat::Pdfa3b,
            &[a],
            &cancel
        )
        .is_err());
        assert_eq!(std::fs::read(&source).unwrap(), pdf);
        assert_eq!(std::fs::read(&out).unwrap(), saved);
        assert_eq!(std::fs::read(&data).unwrap(), b"<data/>");
    }

    #[test]
    fn pdfa4f_metadata_uses_correct_namespace_and_keeps_other_metadata() {
        let xml = br#"<x xmlns:r='http://www.w3.org/1999/02/22-rdf-syntax-ns#' xmlns:a='http://www.aiim.org/pdfa/ns/id/'><r:Description><a:part>4</a:part><a:conformance>E</a:conformance><other>Keep me</other></r:Description></x>"#;
        let result = String::from_utf8(add_4f_conformance(xml).unwrap()).unwrap();
        assert!(result.contains("<a:part>4</a:part>"));
        assert!(result.contains("Keep me"));
        assert!(result.contains(">F</pdfaid:conformance>"));
        assert!(!result.contains(">E<"));
        assert!(add_4f_conformance(b"<x/>").is_err());
    }
}
