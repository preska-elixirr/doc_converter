use crate::{check_cancel, commit, message, output, Result};
use quick_xml::{
    events::{BytesStart, Event},
    name::ResolveResult,
    reader::NsReader,
    Writer,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Write},
    path::Path,
    sync::atomic::AtomicBool,
};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

const WORD: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const STRICT_WORD: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
const MAX_PACKAGE: u64 = 256 * 1024 * 1024;

#[derive(Clone)]
enum Node {
    Element(Element),
    Other(Event<'static>),
}

#[derive(Clone)]
struct Element {
    start: BytesStart<'static>,
    ns: String,
    children: Vec<Node>,
}

impl Element {
    fn local(&self) -> String {
        self.start.local_name().as_ref().to_string()
    }
    fn word(&self) -> bool {
        self.ns == WORD || self.ns == STRICT_WORD
    }
    fn is(&self, name: &str) -> bool {
        self.word() && self.local() == name
    }
    fn attr(&self, name: &str) -> Option<String> {
        self.start
            .attributes()
            .flatten()
            .find(|a| a.key.local_name().as_ref() == name)
            .and_then(|a| {
                a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .ok()
                    .map(|v| v.into_owned())
            })
    }
    fn child(&self, name: &str) -> Option<&Element> {
        self.children
            .iter()
            .filter_map(element)
            .find(|e| e.is(name))
    }
    fn contains(&self, name: &str) -> bool {
        self.is(name)
            || self
                .children
                .iter()
                .filter_map(element)
                .any(|e| e.contains(name))
    }
    fn filter_attrs(&mut self, keep: impl Fn(&str) -> bool) -> Result<()> {
        let original = self.start.clone();
        self.start.clear_attributes();
        for attr in original.attributes() {
            let attr = attr.map_err(message)?;
            if keep(attr.key.local_name().as_ref()) {
                self.start.push_attribute(attr);
            }
        }
        Ok(())
    }
}

fn element(node: &Node) -> Option<&Element> {
    if let Node::Element(e) = node {
        Some(e)
    } else {
        None
    }
}

fn parse(bytes: &[u8]) -> Result<Element> {
    let mut reader = NsReader::from_reader(bytes);
    let mut stack: Vec<Element> = Vec::new();
    let mut root = None;
    loop {
        let (ns, event) = reader.read_resolved_event().map_err(message)?;
        let ns = match ns {
            ResolveResult::Bound(ns) => ns.as_ref().to_string(),
            ResolveResult::Unbound => String::new(),
            ResolveResult::Unknown(_) => return Err(message("Unbound XML namespace in DOCX.")),
        };
        let completed = match event {
            Event::Start(start) => {
                if stack.len() >= 128 {
                    return Err(message("DOCX XML nesting exceeds the cleaning limit."));
                }
                // Validate attributes now, including duplicates and escaping.
                for a in start.attributes() {
                    a.map_err(message)?
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(message)?;
                }
                stack.push(Element {
                    start: start.into_owned(),
                    ns,
                    children: Vec::new(),
                });
                None
            }
            Event::Empty(start) => {
                for a in start.attributes() {
                    a.map_err(message)?
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(message)?;
                }
                Some(Element {
                    start: start.into_owned(),
                    ns,
                    children: Vec::new(),
                })
            }
            Event::End(_) => Some(stack.pop().ok_or_else(|| message("Malformed DOCX XML."))?),
            Event::DocType(_) => return Err(message("DOCX XML with a DTD is not supported.")),
            Event::Eof => break,
            // XML comments and processing instructions can carry personal data.
            Event::Comment(_) | Event::PI(_) | Event::Decl(_) => None,
            other => {
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(Node::Other(other.into_owned()));
                }
                None
            }
        };
        if let Some(e) = completed {
            if let Some(parent) = stack.last_mut() {
                parent.children.push(Node::Element(e));
            } else if root.replace(e).is_some() {
                return Err(message("Multiple DOCX XML roots."));
            }
        }
    }
    if !stack.is_empty() {
        return Err(message("Truncated DOCX XML."));
    }
    root.ok_or_else(|| message("Empty DOCX XML."))
}

fn serialize(root: &Element) -> Result<Vec<u8>> {
    fn emit(e: &Element, w: &mut Writer<Vec<u8>>) -> Result<()> {
        w.write_event(Event::Start(e.start.borrow()))?;
        for n in &e.children {
            match n {
                Node::Element(e) => emit(e, w)?,
                Node::Other(ev) => w.write_event(ev.borrow())?,
            }
        }
        w.write_event(Event::End(e.start.to_end()))?;
        Ok(())
    }
    let mut writer = Writer::new(Vec::new());
    emit(root, &mut writer)?;
    Ok(writer.into_inner())
}

fn hidden(e: &Element) -> bool {
    (e.is("vanish") || e.is("webHidden"))
        && !matches!(e.attr("val").as_deref(), Some("0" | "false" | "off"))
        || e.children
            .iter()
            .filter_map(element)
            .filter(|c| !c.local().ends_with("Change"))
            .any(hidden)
}

#[derive(Default)]
struct Styles {
    hidden: BTreeSet<String>,
    defaults: bool,
    paragraph: String,
    character: String,
    table: String,
}

fn styles(root: Option<&Element>) -> Styles {
    let Some(root) = root else {
        return Styles::default();
    };
    let all: Vec<_> = root
        .children
        .iter()
        .filter_map(element)
        .filter(|e| e.is("style"))
        .collect();
    let mut s = Styles {
        defaults: root.child("docDefaults").is_some_and(hidden),
        ..Styles::default()
    };
    for e in &all {
        let id = e.attr("styleId").unwrap_or_default();
        if hidden(e) {
            s.hidden.insert(id.clone());
        }
        if matches!(e.attr("default").as_deref(), Some("1" | "true" | "on")) {
            match e.attr("type").as_deref() {
                Some("paragraph") => s.paragraph = id,
                Some("character") => s.character = id,
                Some("table") => s.table = id,
                _ => {}
            }
        }
    }
    // Conservative: a hidden base style stays hidden even if a descendant
    // toggles it. Privacy takes precedence over retaining ambiguous text.
    loop {
        let before = s.hidden.len();
        for e in &all {
            if e.child("basedOn")
                .and_then(|b| b.attr("val"))
                .is_some_and(|b| s.hidden.contains(&b))
            {
                s.hidden.insert(e.attr("styleId").unwrap_or_default());
            }
        }
        if before == s.hidden.len() {
            break;
        }
    }
    s
}

fn clean_element(
    mut e: Element,
    s: &Styles,
    inherited_hidden: bool,
    parent: &str,
) -> Result<Vec<Node>> {
    let name = e.local();
    let mut hidden_context = inherited_hidden;
    if e.ns
        .starts_with("http://schemas.microsoft.com/office/word/")
        && (name.contains("conflict") || name.contains("Conflict"))
    {
        return Err(message(
            "Resolve tracked editing conflicts in Word before cleaning this DOCX.",
        ));
    }
    if e.word() {
        // These revisions need structural editing beyond a safe subtree rewrite.
        // Refuse the file rather than misrepresent the final accepted document.
        if (name == "del" && parent == "rPr")
            || matches!(name.as_str(), "cellMerge" | "numberingChange")
        {
            return Err(message("Accept paragraph-mark, cell-merge or numbering revisions in Word before cleaning this DOCX."));
        }
        if e.is("tr") && e.child("trPr").is_some_and(|p| p.contains("del")) {
            return Ok(Vec::new());
        }
        if e.is("tc") && e.child("tcPr").is_some_and(|p| p.contains("cellDel")) {
            return Ok(Vec::new());
        }
        if e.is("p") {
            let style = e
                .child("pPr")
                .and_then(|p| p.child("pStyle"))
                .and_then(|p| p.attr("val"))
                .unwrap_or_else(|| s.paragraph.clone());
            hidden_context |= s.hidden.contains(&style);
        }
        if e.is("tbl") {
            let style = e
                .child("tblPr")
                .and_then(|p| p.child("tblStyle"))
                .and_then(|p| p.attr("val"))
                .unwrap_or_else(|| s.table.clone());
            hidden_context |= s.hidden.contains(&style);
        }
        if e.is("r") {
            let props = e.child("rPr");
            let style = props
                .and_then(|p| p.child("rStyle"))
                .and_then(|p| p.attr("val"))
                .unwrap_or_else(|| s.character.clone());
            if hidden_context
                || s.defaults
                || props.is_some_and(hidden)
                || s.hidden.contains(&style)
            {
                return Ok(Vec::new());
            }
        }
        if matches!(
            name.as_str(),
            "del"
                | "moveFrom"
                | "delText"
                | "delInstrText"
                | "trackRevisions"
                | "rsids"
                | "dataBinding"
                | "cellDel"
                | "cellIns"
        ) || name.ends_with("Change")
            || name.starts_with("comment")
            || name.starts_with("moveFromRange")
            || name.starts_with("moveToRange")
            || name.starts_with("customXmlInsRange")
            || name.starts_with("customXmlDelRange")
            || name.starts_with("customXmlMove")
        {
            return Ok(Vec::new());
        }
        e.filter_attrs(|a| {
            !matches!(a, "author" | "date" | "dateUtc" | "initials") && !a.starts_with("rsid")
        })?;
    }
    let mut children = Vec::new();
    for child in std::mem::take(&mut e.children) {
        match child {
            Node::Element(child) => {
                children.extend(clean_element(child, s, hidden_context, &name)?)
            }
            other => children.push(other),
        }
    }
    e.children = children;
    if e.word() && matches!(name.as_str(), "ins" | "moveTo") {
        // Preserve namespace declarations scoped to the revision wrapper.
        for child in &mut e.children {
            if let Node::Element(child) = child {
                for attr in e.start.attributes() {
                    let attr = attr.map_err(message)?;
                    if attr.key.as_ref() == "xmlns" || attr.key.as_ref().starts_with("xmlns:") {
                        if !child
                            .start
                            .attributes()
                            .flatten()
                            .any(|a| a.key == attr.key)
                        {
                            child.start.push_attribute(attr);
                        }
                    }
                }
            }
        }
        Ok(e.children)
    } else {
        Ok(vec![Node::Element(e)])
    }
}

fn private_part(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("docprops/")
        || name.starts_with("customxml/")
        || name.starts_with("_xmlsignatures/")
        || name.starts_with("word/comments")
        || name == "word/people.xml"
}

fn private_relation(kind: &str) -> bool {
    let kind = kind.to_ascii_lowercase();
    kind.contains("properties")
        || kind.contains("comments")
        || kind.ends_with("/person")
        || kind.ends_with("/people")
        || kind.ends_with("/customxml")
        || kind.ends_with("/thumbnail")
        || kind.contains("digital-signature")
}

fn resolve_target(rels: &str, target: &str) -> Result<String> {
    let decoded = percent_encoding::percent_decode_str(target)
        .decode_utf8()
        .map_err(message)?;
    let base = rels
        .rsplit_once("/_rels/")
        .map(|(base, _)| base)
        .unwrap_or("");
    let path = if decoded.starts_with('/') {
        decoded.trim_start_matches('/').to_string()
    } else if base.is_empty() {
        decoded.into_owned()
    } else {
        format!("{base}/{decoded}")
    };
    let mut parts = Vec::new();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(message("Invalid DOCX relationship target."));
                }
            }
            _ => parts.push(p),
        }
    }
    Ok(parts.join("/"))
}

pub(super) fn clean(source: &Path, destination: &Path, cancel: &AtomicBool) -> Result<()> {
    let mut archive = ZipArchive::new(File::open(source)?).map_err(message)?;
    if archive.len() > 10_000 {
        return Err(message("DOCX has too many parts to clean."));
    }
    let mut parts = BTreeMap::new();
    let mut budget = MAX_PACKAGE;
    for i in 0..archive.len() {
        check_cancel(cancel)?;
        let mut entry = archive.by_index(i).map_err(message)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        if name.contains('\\')
            || name.split('/').any(|p| p == "..")
            || name.starts_with('/')
            || parts.contains_key(&name)
        {
            return Err(message("Ambiguous DOCX package paths."));
        }
        if entry.size() > budget {
            return Err(message("DOCX exceeds the 256 MiB cleaning limit."));
        }
        let mut bytes = Vec::new();
        (&mut entry).take(budget + 1).read_to_end(&mut bytes)?;
        budget = budget
            .checked_sub(bytes.len() as u64)
            .ok_or_else(|| message("DOCX exceeds the cleaning limit."))?;
        parts.insert(name, bytes);
    }
    if !parts.contains_key("word/document.xml") || !parts.contains_key("[Content_Types].xml") {
        return Err(message("Incomplete DOCX package."));
    }
    let mut removed: BTreeSet<String> = parts.keys().filter(|p| private_part(p)).cloned().collect();
    let mut xml = BTreeMap::new();
    for (name, bytes) in &parts {
        if !removed.contains(name) && (name.ends_with(".xml") || name.ends_with(".rels")) {
            check_cancel(cancel)?;
            xml.insert(name.clone(), parse(bytes)?);
        }
    }
    // Relationship types also identify nonstandard locations for private parts.
    for (name, root) in &xml {
        if name.ends_with(".rels") {
            for rel in root.children.iter().filter_map(element) {
                if private_relation(&rel.attr("Type").unwrap_or_default())
                    && rel.attr("TargetMode").as_deref() != Some("External")
                {
                    if let Some(target) = rel.attr("Target") {
                        removed.insert(resolve_target(name, &target)?);
                    }
                }
            }
        }
    }
    // Remove dependencies owned only by a removed part (e.g. comment images).
    // Shared resources are preserved, and orphaned .rels files are discarded.
    let mut links = Vec::new();
    for (name, root) in &xml {
        if let Some((base, file)) = name.rsplit_once("/_rels/") {
            let owner = format!("{base}/{}", file.trim_end_matches(".rels"));
            for rel in root.children.iter().filter_map(element) {
                if rel.attr("TargetMode").as_deref() != Some("External") {
                    if let Some(target) = rel.attr("Target") {
                        links.push((owner.clone(), resolve_target(name, &target)?));
                    }
                }
            }
        }
    }
    loop {
        let before = removed.len();
        for (owner, target) in &links {
            if removed.contains(owner)
                && !links
                    .iter()
                    .any(|(o, t)| t == target && !removed.contains(o))
            {
                removed.insert(target.clone());
            }
        }
        if before == removed.len() {
            break;
        }
    }
    for name in parts.keys() {
        if let Some((base, file)) = name.rsplit_once("/_rels/") {
            if removed.contains(&format!("{base}/{}", file.trim_end_matches(".rels"))) {
                removed.insert(name.clone());
            }
        }
    }
    // Styles can be stored at a nonstandard package path.
    let style_parts: Vec<_> = xml.values().filter(|root| root.is("styles")).collect();
    if style_parts.len() > 1 {
        return Err(message(
            "Multiple DOCX style parts are not supported for cleaning.",
        ));
    }
    let styles = styles(style_parts.first().copied());
    let mut temp = output(destination)?;
    {
        let mut out = ZipWriter::new(&mut temp);
        for (name, bytes) in parts {
            check_cancel(cancel)?;
            if removed.contains(&name) {
                continue;
            }
            let bytes = if let Some(mut root) = xml.remove(&name) {
                if name.ends_with(".rels") {
                    let mut children = Vec::new();
                    for child in root.children {
                        let drop = if let Some(rel) = element(&child) {
                            private_relation(&rel.attr("Type").unwrap_or_default())
                                || (rel.attr("TargetMode").as_deref() != Some("External")
                                    && rel
                                        .attr("Target")
                                        .map(|t| {
                                            resolve_target(&name, &t).map(|p| removed.contains(&p))
                                        })
                                        .transpose()?
                                        .unwrap_or(false))
                        } else {
                            false
                        };
                        if !drop {
                            children.push(child);
                        }
                    }
                    root.children = children;
                } else if name == "[Content_Types].xml" {
                    root.children.retain(|n| {
                        !element(n).is_some_and(|e| {
                            e.attr("PartName")
                                .is_some_and(|p| removed.contains(p.trim_start_matches('/')))
                        })
                    });
                }
                let nodes = clean_element(root, &styles, false, "")?;
                let root = nodes
                    .first()
                    .and_then(element)
                    .ok_or_else(|| message("Invalid DOCX part root."))?;
                serialize(root)?
            } else {
                bytes
            };
            // Fresh ZIP entries reset timestamps, comments and extra fields.
            out.start_file(
                name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .map_err(message)?;
            out.write_all(&bytes)?;
        }
        out.finish().map_err(message)?;
    }
    commit(temp, destination, cancel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(path: &Path, body: &str, style: &str) {
        let mut zip = ZipWriter::new(File::create(path).unwrap());
        let document =
            format!(r#"<x:document xmlns:x="{WORD}"><x:body>{body}</x:body></x:document>"#);
        let styles = format!(r#"<x:styles xmlns:x="{WORD}">{style}</x:styles>"#);
        let entries = [
            ("word/document.xml", document.as_str()),
            ("word/header1.xml", document.as_str()),
            ("word/styles.xml", styles.as_str()),
            ("word/settings.xml", "<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:trackRevisions/><w:rsids><w:rsid w:val=\"PRIVATE-RSID\"/></w:rsids></w:settings>"),
            ("[Content_Types].xml", "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Override PartName=\"/docProps/core.xml\" ContentType=\"core\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/odd/review.xml\" ContentType=\"comment\"/></Types>"),
            ("docProps/core.xml", "PRIVATE-AUTHOR-DATE"),
            ("docProps/custom.xml", "PRIVATE-CUSTOM"),
            ("customXml/item1.xml", "PRIVATE-DATA"),
            ("word/comments.xml", "PRIVATE-COMMENT"),
            ("odd/review.xml", "<comments>PRIVATE-NONSTANDARD-COMMENT</comments>"),
            ("word/_rels/document.xml.rels", "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"c\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments\" Target=\"../odd/review.xml\"/><Relationship Id=\"h\" Type=\"header\" Target=\"header1.xml\"/></Relationships>"),
            ("_rels/.rels", "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"doc\" Type=\"officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"p\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>"),
        ];
        zip.set_comment("PRIVATE-ARCHIVE-COMMENT").unwrap();
        for (name, bytes) in entries {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn docx_removes_private_parts_revisions_hidden_styles_and_headers() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("source.docx");
        let dst = dir.path().join("clean.docx");
        let body = r#"<x:p x:rsidR="PRIVATE-RSID"><x:r><x:t>Visible &amp; intact</x:t></x:r>
            <x:ins x:author="PRIVATE-EDITOR" x:date="PRIVATE-DATE"><x:r><x:t>Inserted</x:t></x:r></x:ins>
            <x:del><x:r><x:delText>PRIVATE-DELETED</x:delText></x:r></x:del>
            <x:moveFrom><x:r><x:t>PRIVATE-MOVED</x:t></x:r></x:moveFrom><x:moveTo><x:r><x:t>Moved here</x:t></x:r></x:moveTo>
            <x:r><x:rPr><x:vanish/></x:rPr><x:t>PRIVATE-HIDDEN</x:t></x:r>
            <x:r><x:rPr><x:webHidden/></x:rPr><x:t>PRIVATE-WEB</x:t></x:r>
            <x:r><x:rPr><x:vanish x:val="false"/></x:rPr><x:t>Explicitly visible</x:t></x:r>
            <x:r><x:rPr><x:rStyle x:val="Derived"/></x:rPr><x:t>PRIVATE-STYLE</x:t></x:r>
            <x:commentRangeStart x:id="0"/><x:r><x:commentReference x:id="0"/></x:r><x:commentRangeEnd x:id="0"/>
            <x:pPrChange x:author="PRIVATE-EDITOR"><x:pPr/></x:pPrChange></x:p>
            <x:p><x:pPr><x:pStyle x:val="Hidden"/></x:pPr><x:r><x:t>PRIVATE-PARAGRAPH</x:t></x:r></x:p>
            <x:tbl><x:tr><x:trPr><x:del/></x:trPr><x:tc><x:p><x:r><x:t>PRIVATE-ROW</x:t></x:r></x:p></x:tc></x:tr></x:tbl>"#;
        let style = r#"<x:style x:styleId="Hidden"><x:rPr><x:vanish/></x:rPr></x:style><x:style x:styleId="Derived"><x:basedOn x:val="Hidden"/></x:style>"#;
        fixture(&src, body, style);
        let before = std::fs::read(&src).unwrap();
        let cancel = AtomicBool::new(false);
        clean(&src, &dst, &cancel).unwrap();
        let mut archive = ZipArchive::new(File::open(&dst).unwrap()).unwrap();
        assert!(archive.comment().is_empty());
        for i in 0..archive.len() {
            let mut part = archive.by_index(i).unwrap();
            assert!(!private_part(part.name()));
            let mut text = String::new();
            part.read_to_string(&mut text).unwrap();
            assert!(!text.contains("PRIVATE-"), "{}: {text}", part.name());
            parse(text.as_bytes()).unwrap();
            if part.name() == "word/document.xml" || part.name() == "word/header1.xml" {
                assert!(text.contains("Visible &amp; intact"));
                assert!(text.contains("Inserted"));
                assert!(text.contains("Moved here"));
                assert!(text.contains("Explicitly visible"));
                assert!(!text.contains("x:ins"));
                assert!(!text.contains("commentReference"));
            }
        }
        assert_eq!(before, std::fs::read(&src).unwrap());
        assert!(clean(&src, &dst, &cancel).is_err());
    }

    #[test]
    fn hidden_default_character_table_styles_and_strict_namespaces() {
        for ns in [WORD, STRICT_WORD] {
            let style_xml = format!(
                r#"<s:styles xmlns:s="{ns}"><s:style s:styleId="Hidden" s:type="character" s:default="1"><s:rPr><s:vanish/></s:rPr></s:style></s:styles>"#
            );
            let root = parse(style_xml.as_bytes()).unwrap();
            let s = styles(Some(&root));
            let body = format!(
                r#"<s:document xmlns:s="{ns}"><s:p><s:r><s:t>SECRET</s:t></s:r></s:p></s:document>"#
            );
            let nodes = clean_element(parse(body.as_bytes()).unwrap(), &s, false, "").unwrap();
            assert!(
                !String::from_utf8(serialize(element(&nodes[0]).unwrap()).unwrap())
                    .unwrap()
                    .contains("SECRET")
            );
            let style_xml = style_xml.replace("character", "table");
            let root = parse(style_xml.as_bytes()).unwrap();
            let s = styles(Some(&root));
            let body = format!(
                r#"<s:document xmlns:s="{ns}"><s:tbl><s:tr><s:tc><s:p><s:r><s:t>SECRET</s:t></s:r></s:p></s:tc></s:tr></s:tbl><s:p><s:r><s:t>Visible</s:t></s:r></s:p></s:document>"#
            );
            let nodes = clean_element(parse(body.as_bytes()).unwrap(), &s, false, "").unwrap();
            let text = String::from_utf8(serialize(element(&nodes[0]).unwrap()).unwrap()).unwrap();
            assert!(!text.contains("SECRET"));
            assert!(text.contains("Visible"));
        }
        let conflict = parse(
            br#"<c:conflictDel xmlns:c="http://schemas.microsoft.com/office/word/2010/wordml"/>"#,
        )
        .unwrap();
        assert!(clean_element(conflict, &Styles::default(), false, "").is_err());
    }

    #[test]
    fn docx_defaults_failures_and_cancel_leave_no_output() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("source.docx");
        let dst = dir.path().join("clean.docx");
        let cancel = AtomicBool::new(false);
        fixture(&src, "<x:p><x:r><x:t>PRIVATE-DEFAULT</x:t></x:r></x:p>", "<x:docDefaults><x:rPrDefault><x:rPr><x:vanish/></x:rPr></x:rPrDefault></x:docDefaults>");
        clean(&src, &dst, &cancel).unwrap();
        let mut archive = ZipArchive::new(File::open(&dst).unwrap()).unwrap();
        let mut xml = String::new();
        archive
            .by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(!xml.contains("PRIVATE-DEFAULT"));
        drop(archive);
        std::fs::remove_file(&dst).unwrap();
        for body in [
            "<x:p><x:pPr><x:rPr><x:del/></x:rPr></x:pPr></x:p>",
            "<x:tcPr><x:cellMerge/></x:tcPr>",
            "<x:p><x:r>",
        ] {
            fixture(&src, body, "");
            assert!(clean(&src, &dst, &cancel).is_err());
            assert!(!dst.exists());
        }
        fixture(&src, "<x:p/>", "");
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(clean(&src, &dst, &cancel).is_err());
        assert!(!dst.exists());
    }
}
