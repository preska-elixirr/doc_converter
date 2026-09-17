//! Read-only PDF preflight. Never renders pages, executes actions or follows links.
use crate::{message, Result};
use lopdf::{Dictionary, Document, LoadOptions, Object};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs::File, io::Read, path::Path};

const MAX_BYTES: u64 = 32 * 1024 * 1024;
const MAX_OBJECTS: usize = 500_000;

#[derive(Clone, Debug, Serialize)]
pub struct SecurityReport {
    /// None means unknown, never an implicit negative finding.
    pub javascript: Option<bool>,
    pub embedded_files: Option<bool>,
    pub internet_links: Option<bool>,
    pub encryption: EncryptionInfo,
    /// complete, locked, incomplete, unreadable, or too_large.
    pub status: String,
    /// Only complete inspections can authorize a preview of these exact bytes.
    pub fingerprint: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EncryptionInfo {
    /// none, rc4, aes128, aes256, mixed, unknown.
    pub algorithm: String,
    pub bits: Option<i64>,
    pub revision: Option<i64>,
}

fn unknown(status: &str) -> SecurityReport {
    SecurityReport {
        javascript: None,
        embedded_files: None,
        internet_links: None,
        encryption: EncryptionInfo {
            algorithm: "unknown".into(),
            bits: None,
            revision: None,
        },
        status: status.into(),
        fingerprint: None,
    }
}

fn read_pdf(source: &Path) -> Result<Vec<u8>> {
    let file = File::open(source)?;
    if !file.metadata()?.is_file() {
        return Err(message("Select a regular PDF file."));
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn fingerprint(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Inspects a bounded snapshot of a local PDF. Content eligibility is enforced here.
pub fn inspect(source: &Path) -> Result<SecurityReport> {
    let bytes = read_pdf(source)?;
    if !bytes.starts_with(b"%PDF-") {
        return Err(message("Select a PDF file."));
    }
    if bytes.len() as u64 > MAX_BYTES {
        return Ok(unknown("too_large"));
    }
    Ok(inspect_bytes(&bytes))
}

/// Returns only the exact bytes previously inspected, without rereading after validation.
pub fn reviewed_bytes(source: &Path, expected: &str) -> Result<Vec<u8>> {
    let bytes = read_pdf(source)?;
    if bytes.len() as u64 > MAX_BYTES
        || !bytes.starts_with(b"%PDF-")
        || fingerprint(&bytes) != expected
    {
        return Err(message(
            "The PDF changed. Inspect it again before opening the preview.",
        ));
    }
    Ok(bytes)
}

fn resolved<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Object> {
    doc.dereference(object).ok().map(|(_, object)| object)
}

fn name<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a [u8]> {
    resolved(doc, dict.get(key).ok()?)?.as_name().ok()
}

fn encryption(doc: &Document) -> EncryptionInfo {
    // Empty-password encryption is automatically decrypted by lopdf; its state
    // retains the original parameters even though /Encrypt has been removed.
    let encoded = doc.encryption_state.as_ref().and_then(|s| s.encode().ok());
    let dict = doc
        .trailer
        .get(b"Encrypt")
        .ok()
        .and_then(|o| resolved(doc, o))
        .and_then(|o| o.as_dict().ok())
        .or(encoded.as_ref());
    let Some(dict) = dict else {
        return EncryptionInfo {
            algorithm: if doc.is_encrypted() || doc.encryption_state.is_some() {
                "unknown"
            } else {
                "none"
            }
            .into(),
            bits: None,
            revision: None,
        };
    };
    encryption_dictionary(doc, dict)
}

fn encryption_dictionary(doc: &Document, dict: &Dictionary) -> EncryptionInfo {
    let number = |key: &[u8]| {
        dict.get(key)
            .ok()
            .and_then(|o| resolved(doc, o))
            .and_then(|o| o.as_i64().ok())
    };
    let revision = number(b"R");
    let mut info = EncryptionInfo {
        algorithm: "unknown".into(),
        bits: None,
        revision,
    };
    if name(doc, dict, b"Filter") != Some(b"Standard") {
        return info;
    }
    match number(b"V") {
        Some(1) if revision == Some(2) => {
            info.algorithm = "rc4".into();
            info.bits = Some(40);
        }
        Some(2) if matches!(revision, Some(2 | 3)) => {
            let bits = number(b"Length").unwrap_or(40);
            if (40..=128).contains(&bits) && bits % 8 == 0 {
                info.algorithm = "rc4".into();
                info.bits = Some(bits);
            }
        }
        Some(4 | 5) => {
            let filters = dict
                .get(b"CF")
                .ok()
                .and_then(|o| resolved(doc, o))
                .and_then(|o| o.as_dict().ok());
            let stream = name(doc, dict, b"StmF").unwrap_or(b"Identity");
            let string = name(doc, dict, b"StrF").unwrap_or(b"Identity");
            let embedded = name(doc, dict, b"EFF").unwrap_or(stream);
            let mut methods = BTreeSet::new();
            for filter in [stream, string, embedded] {
                let method = if filter == b"Identity" {
                    b"None".as_slice()
                } else {
                    filters
                        .and_then(|f| f.get(filter).ok())
                        .and_then(|o| resolved(doc, o))
                        .and_then(|o| o.as_dict().ok())
                        .and_then(|f| name(doc, f, b"CFM"))
                        .unwrap_or(b"Unknown")
                };
                methods.insert(method);
            }
            if methods.len() > 1 {
                info.algorithm = "mixed".into();
            } else {
                match methods.first().copied() {
                    Some(b"AESV2") => {
                        info.algorithm = "aes128".into();
                        info.bits = Some(128);
                    }
                    Some(b"AESV3") => {
                        info.algorithm = "aes256".into();
                        info.bits = Some(256);
                    }
                    Some(b"V2") => {
                        let bits = number(b"Length").unwrap_or(40);
                        if (40..=128).contains(&bits) && bits % 8 == 0 {
                            info.algorithm = "rc4".into();
                            info.bits = Some(bits);
                        }
                    }
                    // Identity with an encryption dictionary is not an unencrypted file.
                    _ => {}
                }
            }
        }
        _ => {}
    }
    info
}

fn inspect_bytes(bytes: &[u8]) -> SecurityReport {
    let options = LoadOptions {
        strict: true,
        max_decompressed_size: Some(16 * 1024 * 1024),
        ..Default::default()
    };
    let Ok(doc) = Document::load_mem_with_options(bytes, options) else {
        return unknown("unreadable");
    };
    let mut report = unknown("incomplete");
    report.encryption = encryption(&doc);
    if doc.is_encrypted() {
        report.status = "locked".into();
        return report;
    }
    let mut complete = doc.catalog().is_ok();
    // lopdf can omit damaged encrypted objects even in strict mode. Check the
    // live xref and references so omissions cannot become negative findings.
    let encrypt_id = doc
        .encryption_state
        .as_ref()
        .and_then(|s| s.encrypt_object_id());
    for (id, entry) in &doc.reference_table.entries {
        let object_id = match entry {
            lopdf::xref::XrefEntry::Normal { generation, .. } => Some((*id, *generation)),
            lopdf::xref::XrefEntry::Compressed { .. } => Some((*id, 0)),
            _ => None,
        };
        if let Some(id) = object_id {
            if Some(id) != encrypt_id && !doc.objects.contains_key(&id) {
                complete = false;
            }
        }
    }
    let mut js = false;
    let mut files = false;
    let mut links = false;
    let mut stack: Vec<&Object> = doc.objects.values().collect();
    let mut visited = 0;
    while let Some(object) = stack.pop() {
        visited += 1;
        if visited > MAX_OBJECTS || stack.len() > MAX_OBJECTS {
            complete = false;
            break;
        }
        let dict = match object {
            Object::Dictionary(dict) => Some(dict),
            Object::Stream(stream) => Some(&stream.dict),
            Object::Array(items) => {
                stack.extend(items);
                None
            }
            Object::Reference(id) => {
                if !doc.objects.contains_key(id) {
                    complete = false;
                }
                None
            }
            _ => None,
        };
        if let Some(dict) = dict {
            js |= dict.has(b"JS")
                || dict.has(b"JavaScript")
                || name(&doc, dict, b"S") == Some(b"JavaScript");
            files |= dict.has(b"EF")
                || dict.has(b"EmbeddedFiles")
                || name(&doc, dict, b"Type") == Some(b"EmbeddedFile")
                || name(&doc, dict, b"Subtype") == Some(b"FileAttachment");
            // URI actions include relative and non-HTTP targets. Flag all of
            // them conservatively; no target is ever fetched or made clickable.
            links |= dict.has(b"URI") || name(&doc, dict, b"FS") == Some(b"URL");
            if matches!(
                name(&doc, dict, b"S"),
                Some(b"SubmitForm" | b"ImportData" | b"GoToR")
            ) {
                if let Some(value) = dict
                    .get(b"F")
                    .ok()
                    .and_then(|o| resolved(&doc, o))
                    .and_then(|o| o.as_str().ok())
                {
                    let value = String::from_utf8_lossy(value).to_ascii_lowercase();
                    links |= value.contains("://") || value.starts_with("//");
                }
            }
            stack.extend(dict.iter().map(|(_, value)| value));
        }
    }
    report.javascript = if js {
        Some(true)
    } else if complete {
        Some(false)
    } else {
        None
    };
    report.embedded_files = if files {
        Some(true)
    } else if complete {
        Some(false)
    } else {
        None
    };
    report.internet_links = if links {
        Some(true)
    } else if complete {
        Some(false)
    } else {
        None
    };
    if complete {
        report.status = "complete".into();
        report.fingerprint = Some(fingerprint(bytes));
    }
    report
}
