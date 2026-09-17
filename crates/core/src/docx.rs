//! Direct edits of a DOCX copy: page orientation, margins, default paragraph
//! spacing, and real `pageBreakBefore` properties on body paragraphs. The
//! source is never modified; the edited copy goes to LibreOffice or to the
//! destination.

use crate::{
    layout::{Layout, Margins, Orientation, Spacing},
    message,
    text::{preview_text, OutlineEntry},
    Result,
};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
    sync::atomic::AtomicBool,
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const TWIPS_PER_MM: f64 = 1440.0 / 25.4;

/// One XML tag with its byte range in the document.
struct Tag<'a> {
    name: &'a str,
    start: usize,
    end: usize, // exclusive, after '>'
    kind: TagKind,
}

#[derive(PartialEq, Clone, Copy)]
enum TagKind {
    Open,
    Close,
    Empty,
    Other, // comments, declarations, processing instructions
}

fn tags(xml: &str) -> Vec<Tag<'_>> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(off) = xml[i..].find('<') {
        let start = i + off;
        if xml[start..].starts_with("<!--") {
            let end = xml[start..]
                .find("-->")
                .map(|e| start + e + 3)
                .unwrap_or(xml.len());
            out.push(Tag {
                name: "",
                start,
                end,
                kind: TagKind::Other,
            });
            i = end;
            continue;
        }
        let Some(close) = xml[start..].find('>') else {
            break;
        };
        let end = start + close + 1;
        let inner = &xml[start + 1..end - 1];
        let kind = if inner.starts_with('?') || inner.starts_with('!') {
            TagKind::Other
        } else if inner.starts_with('/') {
            TagKind::Close
        } else if inner.ends_with('/') {
            TagKind::Empty
        } else {
            TagKind::Open
        };
        let name_src = inner.trim_start_matches('/');
        let name_end = name_src
            .find(|c: char| c.is_whitespace() || c == '/')
            .unwrap_or(name_src.len());
        let name = &name_src[..name_end];
        out.push(Tag {
            name,
            start,
            end,
            kind,
        });
        i = end;
    }
    out
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let entity = &rest[1..end];
        match entity {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => {
                let code = entity
                    .strip_prefix("#x")
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()));
                match code.and_then(char::from_u32) {
                    Some(c) => out.push(c),
                    None => out.push_str(&rest[..=end]),
                }
            }
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let mut search = tag;
    while let Some(pos) = search.find(&key) {
        let before = search[..pos].chars().last();
        if before.map(|c| c.is_whitespace()).unwrap_or(false) {
            let rest = &search[pos + key.len()..];
            let end = rest.find('"')?;
            return Some(&rest[..end]);
        }
        search = &search[pos + key.len()..];
    }
    None
}

/// Rebuilds a self-closing tag with some attributes replaced or added.
fn set_attrs(tag: &str, name: &str, updates: &[(&str, String)]) -> String {
    let inner = tag
        .trim_start_matches('<')
        .trim_end_matches("/>")
        .trim_end_matches('>');
    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut rest = inner[name.len()..].trim_start();
    while !rest.is_empty() {
        let Some(eq) = rest.find('=') else { break };
        let key = rest[..eq].trim().to_string();
        let after = &rest[eq + 1..];
        let quote = after.chars().next().unwrap_or('"');
        let value_start = 1;
        let Some(value_end) = after[value_start..].find(quote) else {
            break;
        };
        let value = after[value_start..value_start + value_end].to_string();
        attrs.push((key, value));
        rest = after[value_start + value_end + 1..].trim_start();
    }
    for (key, value) in updates {
        if let Some(existing) = attrs.iter_mut().find(|(k, _)| k == key) {
            existing.1 = value.clone();
        } else {
            attrs.push(((*key).to_string(), value.clone()));
        }
    }
    let mut out = format!("<{name}");
    for (key, value) in attrs {
        out.push_str(&format!(" {key}=\"{value}\""));
    }
    out.push_str("/>");
    out
}

struct Paragraph {
    /// Byte range of the opening tag (or the whole empty tag).
    open_start: usize,
    open_end: usize,
    empty: bool,
    text: String,
    style: Option<String>,
}

/// Paragraphs that are direct children of the body (content controls allowed).
fn body_paragraphs(xml: &str) -> Vec<Paragraph> {
    let mut stack: Vec<&str> = Vec::new();
    let mut in_body = false;
    let mut paragraphs = Vec::new();
    let mut current: Option<Paragraph> = None;
    let mut depth_at_p = 0usize;
    let all = tags(xml);
    let mut index = 0;
    while index < all.len() {
        let tag = &all[index];
        match tag.kind {
            TagKind::Open | TagKind::Empty => {
                if tag.name == "w:body" && tag.kind == TagKind::Open {
                    in_body = true;
                }
                let at_body_level = in_body
                    && stack
                        .iter()
                        .rev()
                        .take_while(|n| **n != "w:body")
                        .all(|n| matches!(*n, "w:sdt" | "w:sdtContent" | "w:customXml"));
                if tag.name == "w:p" && current.is_none() && at_body_level && in_body {
                    let paragraph = Paragraph {
                        open_start: tag.start,
                        open_end: tag.end,
                        empty: tag.kind == TagKind::Empty,
                        text: String::new(),
                        style: None,
                    };
                    if tag.kind == TagKind::Empty {
                        paragraphs.push(paragraph);
                    } else {
                        current = Some(paragraph);
                        depth_at_p = stack.len();
                    }
                } else if let Some(p) = current.as_mut() {
                    match tag.name {
                        "w:pStyle" if stack.len() == depth_at_p + 2 => {
                            p.style = attr(&xml[tag.start..tag.end], "w:val").map(str::to_string);
                        }
                        "w:t" if tag.kind == TagKind::Open => {
                            if let Some(next) = all.get(index + 1) {
                                p.text.push_str(&unescape(&xml[tag.end..next.start]));
                            }
                        }
                        "w:tab" => p.text.push(' '),
                        "w:br" | "w:cr" => p.text.push(' '),
                        _ => {}
                    }
                }
                if tag.kind == TagKind::Open {
                    stack.push(tag.name);
                }
            }
            TagKind::Close => {
                if let Some(pos) = stack.iter().rposition(|n| *n == tag.name) {
                    stack.truncate(pos);
                }
                if tag.name == "w:p" && stack.len() == depth_at_p {
                    if let Some(p) = current.take() {
                        paragraphs.push(p);
                    }
                }
                if tag.name == "w:body" {
                    in_body = false;
                }
            }
            TagKind::Other => {}
        }
        index += 1;
    }
    paragraphs
}

fn read_entry<R: Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
) -> Result<Option<String>> {
    match zip.by_name(name) {
        Ok(mut entry) => {
            let mut s = String::new();
            entry
                .read_to_string(&mut s)
                .map_err(|_| message("DOCX part is not UTF-8."))?;
            Ok(Some(s))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(e) => Err(message(format!("Unreadable DOCX: {e}"))),
    }
}

fn open(source: &Path) -> Result<ZipArchive<File>> {
    ZipArchive::new(File::open(source)?).map_err(|e| message(format!("Unreadable DOCX: {e}")))
}

fn heading_level(style: &Option<String>) -> Option<u8> {
    let s = style.as_deref()?.to_ascii_lowercase();
    if s == "title" {
        return Some(1);
    }
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if s.starts_with("heading") || s.starts_with("berschrift") || s.starts_with("naslov") {
        return Some(digits.parse().unwrap_or(1));
    }
    None
}

/// Body paragraphs with a short text preview, for the page-break outline.
pub fn outline(source: &Path) -> Result<Vec<OutlineEntry>> {
    let mut zip = open(source)?;
    let xml = read_entry(&mut zip, "word/document.xml")?
        .ok_or_else(|| message("Not a Word document."))?;
    Ok(body_paragraphs(&xml)
        .iter()
        .enumerate()
        .map(|(index, p)| OutlineEntry {
            index,
            kind: match heading_level(&p.style) {
                Some(level) => format!("heading{level}"),
                None if p.text.trim().is_empty() => "empty".to_string(),
                None => "paragraph".to_string(),
            },
            text: if p.text.trim().is_empty() {
                "(empty paragraph)".to_string()
            } else {
                preview_text(&p.text, 90)
            },
        })
        .collect())
}

fn insert_page_breaks(xml: &str, indices: &[usize]) -> String {
    let paragraphs = body_paragraphs(xml);
    let mut wanted: Vec<usize> = indices
        .iter()
        .copied()
        .filter(|i| *i > 0 && *i < paragraphs.len())
        .collect();
    wanted.sort_unstable();
    wanted.dedup();
    let mut out = xml.to_string();
    for index in wanted.into_iter().rev() {
        let p = &paragraphs[index];
        if p.empty {
            let open = &xml[p.open_start..p.open_end];
            let reopened = format!(
                "{}><w:pPr><w:pageBreakBefore/></w:pPr></w:p>",
                open.trim_end_matches("/>").trim_end()
            );
            out.replace_range(p.open_start..p.open_end, &reopened);
            continue;
        }
        let after = &xml[p.open_end..];
        // Paragraph properties are the first child, allowing whitespace/comments.
        let properties = element(after, "w:pPr", 0).filter(|el| {
            tags(&after[..el.open_start])
                .iter()
                .all(|tag| tag.kind == TagKind::Other)
        });
        if let Some(properties) = properties {
            let start = p.open_end + properties.open_start;
            let ppr_open_end = p.open_end + properties.open_end;
            if properties.empty {
                let open = &xml[start..ppr_open_end];
                out.replace_range(
                    start..ppr_open_end,
                    &format!(
                        "{}><w:pageBreakBefore/></w:pPr>",
                        open.trim_end_matches("/>").trim_end()
                    ),
                );
                continue;
            }
            let content = &after[properties.open_end..properties.close_start];
            if let Some(existing) = child_element(content, "w:pageBreakBefore") {
                // An explicit false value must become true; do not add a second property.
                let replacement = set_attrs(
                    &content[existing.open_start..existing.open_end],
                    "w:pageBreakBefore",
                    &[("w:val", "1".to_string())],
                );
                out.replace_range(
                    ppr_open_end + existing.open_start..ppr_open_end + existing.close_end,
                    &replacement,
                );
                continue;
            }
            // Skip properties that must come before pageBreakBefore.
            let mut pos = ppr_open_end;
            for leading in ["<w:pStyle", "<w:keepNext", "<w:keepLines"] {
                if let Some(el) = child_element(content, &leading[1..]) {
                    pos = pos.max(ppr_open_end + el.close_end);
                }
            }
            out.insert_str(pos, "<w:pageBreakBefore/>");
        } else {
            out.insert_str(p.open_end, "<w:pPr><w:pageBreakBefore/></w:pPr>");
        }
    }
    out
}

fn twips(mm: u32) -> String {
    ((f64::from(mm) * TWIPS_PER_MM).round() as u32).to_string()
}

fn apply_section_settings(xml: &str, layout: &Layout) -> String {
    let orientation = layout.orientation;
    let margins = (layout.margins != Margins::Normal).then_some(layout.margins);
    if orientation == Orientation::Keep && margins.is_none() {
        return xml.to_string();
    }
    let mut out = String::with_capacity(xml.len() + 256);
    let mut rest = xml;
    while let Some(pos) = rest.find("<w:sectPr") {
        let sect_start = pos;
        let Some(close_rel) = rest[sect_start..].find("</w:sectPr>") else {
            break;
        };
        let sect_end = sect_start + close_rel + "</w:sectPr>".len();
        let open_end = sect_start + rest[sect_start..].find('>').unwrap_or(0) + 1;
        out.push_str(&rest[..open_end]);
        let mut body = rest[open_end..sect_end - "</w:sectPr>".len()].to_string();
        if orientation != Orientation::Keep {
            let landscape = orientation == Orientation::Landscape;
            if let Some(start) = body.find("<w:pgSz") {
                let end = start + body[start..].find('>').unwrap_or(0) + 1;
                let tag = body[start..end].to_string();
                let w: u32 = attr(&tag, "w:w")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(11906);
                let h: u32 = attr(&tag, "w:h")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(16838);
                let (nw, nh) = if landscape == (w > h) { (w, h) } else { (h, w) };
                let replacement = set_attrs(
                    &tag,
                    "w:pgSz",
                    &[
                        ("w:w", nw.to_string()),
                        ("w:h", nh.to_string()),
                        (
                            "w:orient",
                            if landscape { "landscape" } else { "portrait" }.to_string(),
                        ),
                    ],
                );
                body.replace_range(start..end, &replacement);
            } else {
                let (w, h) = if landscape {
                    (16838, 11906)
                } else {
                    (11906, 16838)
                };
                body.insert_str(
                    0,
                    &format!(
                        "<w:pgSz w:w=\"{w}\" w:h=\"{h}\" w:orient=\"{}\"/>",
                        if landscape { "landscape" } else { "portrait" }
                    ),
                );
            }
        }
        if let Some(margins) = margins {
            let value = twips(margins.mm());
            let updates = [
                ("w:top", value.clone()),
                ("w:right", value.clone()),
                ("w:bottom", value.clone()),
                ("w:left", value.clone()),
            ];
            if let Some(start) = body.find("<w:pgMar") {
                let end = start + body[start..].find('>').unwrap_or(0) + 1;
                let tag = body[start..end].to_string();
                let replacement = set_attrs(&tag, "w:pgMar", &updates);
                body.replace_range(start..end, &replacement);
            } else {
                let insert_at = body
                    .find("<w:pgSz")
                    .and_then(|s| body[s..].find('>').map(|e| s + e + 1))
                    .unwrap_or(0);
                body.insert_str(
                    insert_at,
                    &format!(
                        "<w:pgMar w:top=\"{value}\" w:right=\"{value}\" w:bottom=\"{value}\" w:left=\"{value}\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/>"
                    ),
                );
            }
        }
        out.push_str(&body);
        out.push_str("</w:sectPr>");
        rest = &rest[sect_end..];
    }
    out.push_str(rest);
    out
}

/// Byte ranges of the first `<name …>` element at or after `from`. A self-closing
/// tag has `empty == true` and `close_start == close_end == open_end`. Returns
/// `None` when the element is missing or has no closing tag.
struct Element {
    open_start: usize,
    open_end: usize,
    close_start: usize,
    close_end: usize,
    empty: bool,
}

fn element(xml: &str, name: &str, from: usize) -> Option<Element> {
    let pattern = format!("<{name}");
    let mut search = from;
    loop {
        let open_start = search + xml[search..].find(&pattern)?;
        let next = xml[open_start + pattern.len()..].chars().next();
        if !matches!(next, Some(c) if c.is_whitespace() || c == '>' || c == '/') {
            search = open_start + 1;
            continue;
        }
        let open_end = open_start + xml[open_start..].find('>')? + 1;
        if xml[..open_end].ends_with("/>") {
            return Some(Element {
                open_start,
                open_end,
                close_start: open_end,
                close_end: open_end,
                empty: true,
            });
        }
        let mut depth = 1;
        let close_tag = tags(&xml[open_end..]).into_iter().find(|tag| {
            if tag.name == name {
                match tag.kind {
                    TagKind::Open => depth += 1,
                    TagKind::Close => depth -= 1,
                    _ => {}
                }
            }
            depth == 0
        })?;
        let close_start = open_end + close_tag.start;
        return Some(Element {
            open_start,
            open_end,
            close_start,
            close_end: open_end + close_tag.end,
            empty: false,
        });
    }
}

/// Find a direct child, excluding properties inside tracked-change snapshots.
fn child_element(xml: &str, name: &str) -> Option<Element> {
    let mut depth = 0;
    for tag in tags(xml) {
        if depth == 0 && tag.name == name && matches!(tag.kind, TagKind::Open | TagKind::Empty) {
            return element(xml, name, tag.start);
        }
        match tag.kind {
            TagKind::Open => depth += 1,
            TagKind::Close => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Content of a `w:pPrDefault` with `w:after` set on its paragraph spacing,
/// creating the `w:pPr` and `w:spacing` elements when they are missing.
fn spaced_default(inner: &str, spacing_tag: &str, after: &str) -> String {
    let mut out = inner.to_string();
    match element(inner, "w:pPr", 0) {
        Some(ppr) if ppr.empty => out.replace_range(
            ppr.open_start..ppr.open_end,
            &format!("<w:pPr>{spacing_tag}</w:pPr>"),
        ),
        Some(ppr) => {
            let content = &inner[ppr.open_end..ppr.close_start];
            match element(content, "w:spacing", 0) {
                Some(sp) => {
                    let tag = &content[sp.open_start..sp.open_end];
                    let replacement =
                        set_attrs(tag, "w:spacing", &[("w:after", after.to_string())]);
                    out.replace_range(
                        ppr.open_end + sp.open_start..ppr.open_end + sp.close_end,
                        &replacement,
                    );
                }
                None => out.insert_str(ppr.open_end, spacing_tag),
            }
        }
        None => out.push_str(&format!("<w:pPr>{spacing_tag}</w:pPr>")),
    }
    out
}

fn apply_spacing(styles: &str, spacing: Spacing) -> String {
    let after = spacing.twips().to_string();
    let spacing_tag = format!("<w:spacing w:after=\"{after}\"/>");
    let full_default = format!("<w:pPrDefault><w:pPr>{spacing_tag}</w:pPr></w:pPrDefault>");
    let mut out = styles.to_string();
    // Document defaults. Every shape is handled: missing, self-closing, or with content.
    match element(&out, "w:docDefaults", 0) {
        Some(dd) if dd.empty => out.replace_range(
            dd.open_start..dd.open_end,
            &format!("<w:docDefaults>{full_default}</w:docDefaults>"),
        ),
        Some(dd) => match element(&out[..dd.close_start], "w:pPrDefault", dd.open_end) {
            None => out.insert_str(dd.close_start, &full_default),
            Some(pp) if pp.empty => out.replace_range(pp.open_start..pp.open_end, &full_default),
            Some(pp) => {
                let inner = spaced_default(&out[pp.open_end..pp.close_start], &spacing_tag, &after);
                out.replace_range(pp.open_end..pp.close_start, &inner);
            }
        },
        None => {
            if let Some(root) = element(&out, "w:styles", 0) {
                out.insert_str(
                    root.open_end,
                    &format!("<w:docDefaults>{full_default}</w:docDefaults>"),
                );
            }
        }
    }
    // The default paragraph style often carries its own paragraph spacing; align it.
    // Character spacing (w:rPr/w:spacing) is a different element and is left alone.
    let mut search = 0;
    while let Some(style) = element(&out, "w:style", search) {
        let open_tag = out[style.open_start..style.open_end].to_string();
        if !style.empty
            && attr(&open_tag, "w:type") == Some("paragraph")
            && attr(&open_tag, "w:default") == Some("1")
        {
            let content = out[style.open_end..style.close_start].to_string();
            if let Some(ppr) = element(&content, "w:pPr", 0).filter(|p| !p.empty) {
                let ppr_content = &content[ppr.open_end..ppr.close_start];
                if let Some(sp) = element(ppr_content, "w:spacing", 0) {
                    let tag = &ppr_content[sp.open_start..sp.open_end];
                    let replacement = set_attrs(tag, "w:spacing", &[("w:after", after.clone())]);
                    let abs = style.open_end + ppr.open_end + sp.open_start;
                    out.replace_range(abs..abs + sp.close_end - sp.open_start, &replacement);
                }
            }
            break;
        }
        search = style.close_end.max(style.open_end);
    }
    out
}

/// Writes an edited copy of `source` to `destination` through a temp file
/// beside it; never overwrites and leaves nothing behind on error or cancel.
pub fn rewrite(
    source: &Path,
    destination: &Path,
    layout: &Layout,
    cancel: &AtomicBool,
) -> Result<()> {
    let mut zip = open(source)?;
    let document = read_entry(&mut zip, "word/document.xml")?
        .ok_or_else(|| message("Not a Word document."))?;
    let mut document = apply_section_settings(&document, layout);
    if !layout.page_breaks.is_empty() {
        document = insert_page_breaks(&document, &layout.page_breaks);
    }
    let styles = if layout.spacing != Spacing::Comfortable {
        read_entry(&mut zip, "word/styles.xml")?.map(|s| apply_spacing(&s, layout.spacing))
    } else {
        None
    };
    let temp = crate::output(destination)?;
    let mut writer = ZipWriter::new(temp);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for index in 0..zip.len() {
        crate::check_cancel(cancel)?;
        let entry = zip
            .by_index(index)
            .map_err(|e| message(format!("Unreadable DOCX: {e}")))?;
        let name = entry.name().to_string();
        let replacement = match name.as_str() {
            "word/document.xml" => Some(document.as_str()),
            "word/styles.xml" => styles.as_deref(),
            _ => None,
        };
        match replacement {
            Some(content) => {
                writer.start_file(name, options).map_err(message)?;
                writer.write_all(content.as_bytes())?;
            }
            None => writer.raw_copy_file(entry).map_err(message)?,
        }
    }
    let temp = writer.finish().map_err(message)?;
    crate::commit(temp, destination, cancel)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const DOCUMENT: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Chapter &amp; One</w:t></w:r></w:p>
<w:p><w:r><w:t xml:space="preserve">First </w:t></w:r><w:r><w:t>paragraph.</w:t></w:r></w:p>
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>In a table</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
<w:p/>
<w:p><w:pPr><w:keepNext/></w:pPr><w:r><w:t>Last one</w:t></w:r></w:p>
<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1417" w:bottom="1134" w:left="1417" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr>
</w:body></w:document>"#;

    pub(crate) const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="200" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:pPr><w:spacing w:after="120"/></w:pPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:pPr><w:keepNext/><w:spacing w:before="240" w:after="60"/></w:pPr></w:style>
</w:styles>"#;

    pub(crate) fn write_docx(path: &Path) {
        let mut writer = ZipWriter::new(File::create(path).unwrap());
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        let content_types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#;
        let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
        let doc_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;
        for (name, content) in [
            ("[Content_Types].xml", content_types),
            ("_rels/.rels", rels),
            ("word/_rels/document.xml.rels", doc_rels),
            ("word/document.xml", DOCUMENT),
            ("word/styles.xml", STYLES),
        ] {
            writer.start_file(name, options).unwrap();
            writer.write_all(content.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn outline_lists_body_paragraphs_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.docx");
        write_docx(&path);
        let entries = outline(&path).unwrap();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].kind, "heading1");
        assert_eq!(entries[0].text, "Chapter & One");
        assert_eq!(entries[1].text, "First paragraph.");
        assert_eq!(entries[2].kind, "empty");
        assert_eq!(entries[3].text, "Last one");
    }

    #[test]
    fn rewrite_applies_layout() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.docx");
        write_docx(&path);
        let out = dir.path().join("edited.docx");
        let layout = Layout {
            orientation: Orientation::Landscape,
            margins: Margins::Narrow,
            spacing: Spacing::Spacious,
            page_breaks: vec![0, 1, 2, 3, 9],
        };
        rewrite(&path, &out, &layout, &AtomicBool::new(false)).unwrap();
        let mut zip = ZipArchive::new(File::open(&out).unwrap()).unwrap();
        let document = read_entry(&mut zip, "word/document.xml").unwrap().unwrap();
        assert!(document.contains(r#"<w:pgSz w:w="16838" w:h="11906" w:orient="landscape"/>"#));
        assert!(document.contains(r#"w:top="567" w:right="567" w:bottom="567" w:left="567""#));
        assert_eq!(document.matches("<w:pageBreakBefore/>").count(), 3);
        assert!(document.contains(
            r#"<w:p><w:pPr><w:pageBreakBefore/></w:pPr><w:r><w:t xml:space="preserve">First"#
        ));
        assert!(document.contains("<w:p><w:pPr><w:pageBreakBefore/></w:pPr></w:p>"));
        assert!(document.contains("<w:pPr><w:keepNext/><w:pageBreakBefore/></w:pPr>"));
        assert!(!document.contains("<w:tc><w:p><w:pPr><w:pageBreakBefore/>"));
        well_formed(&document);
        let styles = read_entry(&mut zip, "word/styles.xml").unwrap().unwrap();
        well_formed(&styles);
        assert!(styles.contains(r#"<w:spacing w:after="320" w:line="276" w:lineRule="auto"/>"#));
        assert!(styles.contains(r#"<w:name w:val="Normal"/><w:pPr><w:spacing w:after="320"/>"#));
        assert!(styles.contains(r#"w:before="240" w:after="60""#));
        assert!(read_entry(&mut zip, "[Content_Types].xml")
            .unwrap()
            .is_some());
        assert!(rewrite(&path, &out, &layout, &AtomicBool::new(false)).is_err());
        let cancelled = dir.path().join("cancelled.docx");
        assert!(rewrite(&path, &cancelled, &layout, &AtomicBool::new(true)).is_err());
        assert!(!cancelled.exists());
        let portrait = dir.path().join("portrait.docx");
        rewrite(
            &path,
            &portrait,
            &Layout {
                orientation: Orientation::Portrait,
                ..Default::default()
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        let mut zip = ZipArchive::new(File::open(&portrait).unwrap()).unwrap();
        let document = read_entry(&mut zip, "word/document.xml").unwrap().unwrap();
        assert!(document.contains(r#"<w:pgSz w:w="11906" w:h="16838" w:orient="portrait"/>"#));
        assert!(document.contains(r#"w:top="1417""#));
    }

    pub(crate) fn well_formed(xml: &str) {
        let mut reader = quick_xml::Reader::from_str(xml);
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Eof) => break,
                Ok(_) => {}
                Err(e) => panic!("malformed XML: {e}\n{xml}"),
            }
        }
    }

    #[test]
    fn paired_spacing_tags_remain_well_formed() {
        let styles = r#"<w:styles xmlns:w="w"><w:docDefaults><w:pPrDefault><w:pPr><w:spacing w:before="40" w:after="100"></w:spacing></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1"><w:pPr><w:spacing w:line="240" w:after="80"></w:spacing></w:pPr><w:rPr><w:spacing w:val="12"></w:spacing></w:rPr></w:style></w:styles>"#;
        let out = apply_spacing(styles, Spacing::Spacious);
        well_formed(&out);
        assert_eq!(out.matches(r#"w:after="320""#).count(), 2);
        assert!(out.contains(r#"w:before="40""#));
        assert!(out.contains(r#"w:line="240""#));
        assert!(out.contains(r#"<w:rPr><w:spacing w:val="12"></w:spacing></w:rPr>"#));
        assert_eq!(apply_spacing(&out, Spacing::Spacious), out);
    }

    #[test]
    fn page_breaks_update_existing_paragraph_properties() {
        for properties in [
            "<w:pPr/>",
            "\n<!-- properties --> <w:pPr />",
            r#"<w:pPr><w:pageBreakBefore w:val="0"/></w:pPr>"#,
            r#"<w:pPr><w:pStyle w:val="Normal"></w:pStyle><w:keepNext></w:keepNext><w:keepLines/></w:pPr>"#,
            r#"<w:pPr><w:keepNext/><w:pageBreakBefore w:val="false"></w:pageBreakBefore></w:pPr>"#,
        ] {
            let xml = format!(
                r#"<w:document xmlns:w="w"><w:body><w:p><w:r><w:t>First</w:t></w:r></w:p><w:p>{properties}<w:r><w:t>Second</w:t></w:r></w:p></w:body></w:document>"#
            );
            let out = insert_page_breaks(&xml, &[1]);
            well_formed(&out);
            assert_eq!(out.matches("<w:pPr>").count(), 1, "{out}");
            assert_eq!(out.matches("<w:pageBreakBefore").count(), 1, "{out}");
            assert!(!out.contains(r#"w:val="0""#));
            assert!(!out.contains(r#"w:val="false""#));
            if out.contains("</w:keepNext>") {
                assert!(
                    out.find("<w:pageBreakBefore").unwrap() > out.find("<w:keepLines").unwrap()
                );
            }
            let repeated = insert_page_breaks(&out, &[1]);
            well_formed(&repeated);
            assert_eq!(repeated.matches("<w:pPr>").count(), 1);
            assert_eq!(repeated.matches("<w:pageBreakBefore").count(), 1);
        }
        // A historical property inside pPrChange is not the current value.
        let xml = r#"<w:document xmlns:w="w"><w:body><w:p/><w:p><w:pPr><w:pPrChange w:id="1"><w:pPr><w:pageBreakBefore w:val="0"/></w:pPr></w:pPrChange></w:pPr><w:r><w:t>Second</w:t></w:r></w:p></w:body></w:document>"#;
        let out = insert_page_breaks(xml, &[1]);
        well_formed(&out);
        assert!(out.contains("<w:pPr><w:pageBreakBefore/><w:pPrChange"));
        assert!(out.contains(r#"<w:pPr><w:pageBreakBefore w:val="0"/></w:pPr></w:pPrChange>"#));
    }

    #[test]
    fn spacing_handles_every_default_shape() {
        let self_closing = r#"<?xml version="1.0"?><w:styles xmlns:w="w"><w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/></w:rPr></w:rPrDefault><w:pPrDefault/></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:rPr><w:spacing w:val="20"/></w:rPr></w:style></w:styles>"#;
        let out = apply_spacing(self_closing, Spacing::Spacious);
        well_formed(&out);
        assert!(out.contains(r#"<w:pPrDefault><w:pPr><w:spacing w:after="320"/></w:pPr></w:pPrDefault></w:docDefaults>"#));
        assert!(out.contains(r#"<w:rPr><w:spacing w:val="20"/></w:rPr>"#));
        assert_eq!(out.matches("w:after").count(), 1);

        let empty_defaults = r#"<w:styles xmlns:w="w"><w:docDefaults/><w:style w:type="paragraph" w:styleId="x"><w:pPr><w:spacing w:after="10"/></w:pPr></w:style></w:styles>"#;
        let out = apply_spacing(empty_defaults, Spacing::Compact);
        well_formed(&out);
        assert!(out.contains(r#"<w:docDefaults><w:pPrDefault><w:pPr><w:spacing w:after="0"/></w:pPr></w:pPrDefault></w:docDefaults>"#));
        assert!(out.contains(r#"w:after="10""#));

        let empty_ppr = r#"<w:styles xmlns:w="w"><w:docDefaults><w:pPrDefault><w:pPr/></w:pPrDefault></w:docDefaults></w:styles>"#;
        let out = apply_spacing(empty_ppr, Spacing::Spacious);
        well_formed(&out);
        assert!(out
            .contains(r#"<w:pPrDefault><w:pPr><w:spacing w:after="320"/></w:pPr></w:pPrDefault>"#));

        let no_defaults = r#"<w:styles xmlns:w="w"><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:pPr><w:spacing w:before="5" w:after="100"/></w:pPr></w:style></w:styles>"#;
        let out = apply_spacing(no_defaults, Spacing::Compact);
        well_formed(&out);
        assert!(out.starts_with(r#"<w:styles xmlns:w="w"><w:docDefaults><w:pPrDefault>"#));
        assert!(out.contains(r#"<w:spacing w:before="5" w:after="0"/>"#));

        let no_ppr = r#"<w:styles xmlns:w="w"><w:docDefaults><w:pPrDefault><w:rPr/></w:pPrDefault></w:docDefaults></w:styles>"#;
        let out = apply_spacing(no_ppr, Spacing::Spacious);
        well_formed(&out);
        assert!(out.contains(
            r#"<w:pPrDefault><w:rPr/><w:pPr><w:spacing w:after="320"/></w:pPr></w:pPrDefault>"#
        ));
    }
}
