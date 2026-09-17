//! Typed block model for text sources. Markdown and plain text are parsed
//! into blocks; blocks render to HTML, plain text, or (in `layout`) to PDF.
//! User text is never interpreted as markup by the layout engine.

use crate::{message, Result};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub text: String,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub code: bool,
    #[serde(default)]
    pub link: Option<String>,
    /// A hard line break. `text` is empty.
    #[serde(default)]
    pub br: bool,
}

impl Span {
    pub fn text(text: impl Into<String>) -> Self {
        Span {
            text: text.into(),
            ..Default::default()
        }
    }
    pub fn br() -> Self {
        Span {
            br: true,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Block {
    Heading {
        level: u8,
        spans: Vec<Span>,
    },
    Paragraph {
        spans: Vec<Span>,
    },
    List {
        ordered: bool,
        items: Vec<Vec<Span>>,
    },
    Code {
        text: String,
        lang: Option<String>,
    },
    Quote {
        spans: Vec<Span>,
    },
    Rule,
    /// Row-major cells, header row first.
    Table {
        columns: usize,
        cells: Vec<Vec<Span>>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Doc {
    pub blocks: Vec<Block>,
}

/// One row of the block outline shown beside the preview.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OutlineEntry {
    pub index: usize,
    pub kind: String,
    pub text: String,
}

pub fn plain(spans: &[Span]) -> String {
    let mut s = String::new();
    for span in spans {
        if span.br {
            s.push('\n');
        } else {
            s.push_str(&span.text);
        }
    }
    s
}

pub(crate) fn preview_text(text: &str, max: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= max {
        line
    } else {
        let cut: String = line.chars().take(max).collect();
        format!("{}…", cut.trim_end())
    }
}

impl Doc {
    pub fn outline(&self) -> Vec<OutlineEntry> {
        self.blocks
            .iter()
            .enumerate()
            .map(|(index, block)| {
                let (kind, text) = match block {
                    Block::Heading { level, spans } => (format!("heading{level}"), plain(spans)),
                    Block::Paragraph { spans } => ("paragraph".into(), plain(spans)),
                    Block::List { items, .. } => (
                        "list".into(),
                        items
                            .iter()
                            .map(|i| plain(i))
                            .collect::<Vec<_>>()
                            .join(" · "),
                    ),
                    Block::Code { text, .. } => ("code".into(), text.clone()),
                    Block::Quote { spans } => ("quote".into(), plain(spans)),
                    Block::Rule => ("rule".into(), "———".into()),
                    Block::Table { cells, .. } => (
                        "table".into(),
                        cells
                            .iter()
                            .map(|c| plain(c))
                            .collect::<Vec<_>>()
                            .join(" | "),
                    ),
                };
                OutlineEntry {
                    index,
                    kind,
                    text: preview_text(&text, 90),
                }
            })
            .collect()
    }
}

/// Reads a text file as UTF-8, dropping a BOM and replacing invalid bytes.
pub fn read_text_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    Ok(String::from_utf8_lossy(bytes).replace("\r\n", "\n"))
}

/// Plain text: blank lines separate paragraphs, single newlines stay as breaks.
pub fn parse_text(source: &str) -> Doc {
    let mut blocks = Vec::new();
    for chunk in source.replace("\r\n", "\n").split("\n\n") {
        let lines: Vec<&str> = chunk.lines().map(|l| l.trim_end()).collect();
        let lines: Vec<&str> = lines
            .iter()
            .copied()
            .skip_while(|l| l.trim().is_empty())
            .collect();
        if lines.iter().all(|l| l.trim().is_empty()) {
            continue;
        }
        let mut spans = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                spans.push(Span::br());
            }
            spans.push(Span::text(*line));
        }
        blocks.push(Block::Paragraph { spans });
    }
    Doc { blocks }
}

struct ListCtx {
    ordered: bool,
    items: Vec<Vec<Span>>,
    current: Vec<Span>,
}

struct TableCtx {
    columns: usize,
    cells: Vec<Vec<Span>>,
    current: Vec<Span>,
}

#[derive(Default)]
struct Builder {
    blocks: Vec<Block>,
    spans: Vec<Span>,
    bold: u32,
    italic: u32,
    links: Vec<String>,
    lists: Vec<ListCtx>,
    quote: u32,
    heading: Option<u8>,
    code: Option<(String, Option<String>)>,
    table: Option<TableCtx>,
    skip: u32,
}

impl Builder {
    fn target(&mut self) -> &mut Vec<Span> {
        if let Some(table) = self.table.as_mut() {
            &mut table.current
        } else if let Some(list) = self.lists.last_mut() {
            &mut list.current
        } else {
            &mut self.spans
        }
    }

    fn push_text(&mut self, text: &str) {
        if self.skip > 0 || text.is_empty() {
            return;
        }
        if let Some((code, _)) = self.code.as_mut() {
            code.push_str(text);
            return;
        }
        let span = Span {
            text: text.to_string(),
            bold: self.bold > 0,
            italic: self.italic > 0,
            code: false,
            link: self.links.last().cloned(),
            br: false,
        };
        self.target().push(span);
    }

    fn push_span(&mut self, span: Span) {
        if self.skip == 0 {
            self.target().push(span);
        }
    }

    fn end_paragraph(&mut self) {
        if self.lists.last().is_some() || self.table.is_some() {
            // Loose list items and table cells keep flowing; separate parts with a break.
            let target = self.target();
            if !target.is_empty() {
                target.push(Span::br());
            }
            return;
        }
        let spans = std::mem::take(&mut self.spans);
        if spans.is_empty() {
            return;
        }
        if self.quote > 0 {
            self.blocks.push(Block::Quote { spans });
        } else {
            self.blocks.push(Block::Paragraph { spans });
        }
    }
}

fn level_of(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn trim_trailing_breaks(spans: &mut Vec<Span>) {
    while spans.last().map(|s| s.br).unwrap_or(false) {
        spans.pop();
    }
}

/// Parses CommonMark plus tables, strikethrough and task lists.
pub fn parse_markdown(source: &str) -> Doc {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut b = Builder::default();
    for event in Parser::new_ext(source, options) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {}
                Tag::Heading { level, .. } => {
                    b.heading = Some(level_of(level));
                    b.spans.clear();
                }
                Tag::BlockQuote(_) => b.quote += 1,
                Tag::CodeBlock(kind) => {
                    let lang = match kind {
                        CodeBlockKind::Fenced(lang) if !lang.trim().is_empty() => {
                            Some(lang.split_whitespace().next().unwrap_or("").to_string())
                        }
                        _ => None,
                    };
                    b.code = Some((String::new(), lang));
                }
                Tag::List(start) => b.lists.push(ListCtx {
                    ordered: start.is_some(),
                    items: Vec::new(),
                    current: Vec::new(),
                }),
                Tag::Item => {
                    if let Some(list) = b.lists.last_mut() {
                        list.current.clear();
                    }
                }
                Tag::Table(alignments) => {
                    b.table = Some(TableCtx {
                        columns: alignments.len().max(1),
                        cells: Vec::new(),
                        current: Vec::new(),
                    })
                }
                Tag::TableCell => {
                    if let Some(table) = b.table.as_mut() {
                        table.current.clear();
                    }
                }
                Tag::Emphasis => b.italic += 1,
                Tag::Strong => b.bold += 1,
                Tag::Link { dest_url, .. } => b.links.push(dest_url.to_string()),
                Tag::MetadataBlock(_) | Tag::HtmlBlock => b.skip += 1,
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => b.end_paragraph(),
                TagEnd::Heading(_) => {
                    let level = b.heading.take().unwrap_or(1);
                    let spans = std::mem::take(&mut b.spans);
                    b.blocks.push(Block::Heading { level, spans });
                }
                TagEnd::BlockQuote(_) => b.quote = b.quote.saturating_sub(1),
                TagEnd::CodeBlock => {
                    if let Some((text, lang)) = b.code.take() {
                        b.blocks.push(Block::Code {
                            text: text.trim_end_matches('\n').to_string(),
                            lang,
                        });
                    }
                }
                TagEnd::Item => {
                    if let Some(list) = b.lists.last_mut() {
                        let mut item = std::mem::take(&mut list.current);
                        trim_trailing_breaks(&mut item);
                        // Empty after a nested list was flattened into the parent.
                        if !item.is_empty() {
                            list.items.push(item);
                        }
                    }
                }
                TagEnd::List(_) => {
                    if let Some(list) = b.lists.pop() {
                        if let Some(parent) = b.lists.last_mut() {
                            // Nested lists are flattened into the parent list.
                            let mut current = std::mem::take(&mut parent.current);
                            trim_trailing_breaks(&mut current);
                            if !current.is_empty() {
                                parent.items.push(current);
                            }
                            for mut item in list.items {
                                item.insert(0, Span::text("– "));
                                parent.items.push(item);
                            }
                        } else {
                            b.blocks.push(Block::List {
                                ordered: list.ordered,
                                items: list.items,
                            });
                        }
                    }
                }
                TagEnd::TableCell => {
                    if let Some(table) = b.table.as_mut() {
                        let mut cell = std::mem::take(&mut table.current);
                        trim_trailing_breaks(&mut cell);
                        table.cells.push(cell);
                    }
                }
                TagEnd::Table => {
                    if let Some(table) = b.table.take() {
                        b.blocks.push(Block::Table {
                            columns: table.columns,
                            cells: table.cells,
                        });
                    }
                }
                TagEnd::Emphasis => b.italic = b.italic.saturating_sub(1),
                TagEnd::Strong => b.bold = b.bold.saturating_sub(1),
                TagEnd::Link => {
                    b.links.pop();
                }
                TagEnd::MetadataBlock(_) | TagEnd::HtmlBlock => b.skip = b.skip.saturating_sub(1),
                _ => {}
            },
            Event::Text(text) => b.push_text(&text),
            Event::Code(code) => b.push_span(Span {
                text: code.to_string(),
                code: true,
                ..Default::default()
            }),
            Event::SoftBreak => b.push_text(" "),
            Event::HardBreak => b.push_span(Span::br()),
            Event::Rule => b.blocks.push(Block::Rule),
            Event::TaskListMarker(checked) => b.push_text(if checked { "[x] " } else { "[ ] " }),
            Event::FootnoteReference(name) => b.push_text(&format!("[{name}]")),
            Event::InlineMath(m) | Event::DisplayMath(m) => b.push_text(&m),
            Event::Html(_) | Event::InlineHtml(_) => {}
        }
    }
    b.end_paragraph();
    Doc { blocks: b.blocks }
}

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn spans_html(spans: &[Span]) -> String {
    let mut out = String::new();
    for span in spans {
        if span.br {
            out.push_str("<br>");
            continue;
        }
        let mut s = escape_html(&span.text);
        if span.code {
            s = format!("<code>{s}</code>");
        }
        if span.bold {
            s = format!("<strong>{s}</strong>");
        }
        if span.italic {
            s = format!("<em>{s}</em>");
        }
        if let Some(link) = &span.link {
            if link.starts_with("http://")
                || link.starts_with("https://")
                || link.starts_with("mailto:")
            {
                s = format!("<a href=\"{}\">{s}</a>", escape_html(link));
            }
        }
        out.push_str(&s);
    }
    out
}

/// A complete, self-contained HTML document.
pub fn to_html(doc: &Doc, title: &str) -> String {
    let mut body = String::new();
    for block in &doc.blocks {
        match block {
            Block::Heading { level, spans } => {
                let l = (*level).clamp(1, 6);
                body.push_str(&format!("<h{l}>{}</h{l}>\n", spans_html(spans)));
            }
            Block::Paragraph { spans } => body.push_str(&format!("<p>{}</p>\n", spans_html(spans))),
            Block::List { ordered, items } => {
                let tag = if *ordered { "ol" } else { "ul" };
                body.push_str(&format!("<{tag}>\n"));
                for item in items {
                    body.push_str(&format!("<li>{}</li>\n", spans_html(item)));
                }
                body.push_str(&format!("</{tag}>\n"));
            }
            Block::Code { text, lang } => {
                let class = lang
                    .as_ref()
                    .map(|l| format!(" class=\"language-{}\"", escape_html(l)))
                    .unwrap_or_default();
                body.push_str(&format!(
                    "<pre><code{class}>{}</code></pre>\n",
                    escape_html(text)
                ));
            }
            Block::Quote { spans } => body.push_str(&format!(
                "<blockquote><p>{}</p></blockquote>\n",
                spans_html(spans)
            )),
            Block::Rule => body.push_str("<hr>\n"),
            Block::Table { columns, cells } => {
                body.push_str("<table>\n");
                for (row_index, row) in cells.chunks(*columns).enumerate() {
                    let tag = if row_index == 0 { "th" } else { "td" };
                    body.push_str("<tr>");
                    for cell in row {
                        body.push_str(&format!("<{tag}>{}</{tag}>", spans_html(cell)));
                    }
                    body.push_str("</tr>\n");
                }
                body.push_str("</table>\n");
            }
        }
    }
    format!(
        "<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>{}</title>\n<style>body{{font-family:Georgia,serif;max-width:46em;margin:2em auto;padding:0 1em;line-height:1.5}}pre{{background:#f4f4f4;padding:.8em;overflow:auto}}code{{font-family:Consolas,monospace}}table{{border-collapse:collapse}}td,th{{border:1px solid #ccc;padding:.3em .6em}}blockquote{{border-left:3px solid #ccc;margin-left:0;padding-left:1em;color:#555}}</style>\n</head>\n<body>\n{}</body>\n</html>\n",
        escape_html(title),
        body
    )
}

/// Plain text with simple markers for structure.
pub fn to_text(doc: &Doc) -> String {
    let mut out = String::new();
    for block in &doc.blocks {
        match block {
            Block::Heading { spans, .. } => {
                let text = plain(spans);
                out.push_str(&text);
                out.push('\n');
                out.push_str(&"=".repeat(text.chars().count().clamp(3, 72)));
                out.push_str("\n\n");
            }
            Block::Paragraph { spans } => {
                out.push_str(&plain(spans));
                out.push_str("\n\n");
            }
            Block::List { ordered, items } => {
                for (i, item) in items.iter().enumerate() {
                    if *ordered {
                        out.push_str(&format!("{}. {}\n", i + 1, plain(item)));
                    } else {
                        out.push_str(&format!("- {}\n", plain(item)));
                    }
                }
                out.push('\n');
            }
            Block::Code { text, .. } => {
                out.push_str(text);
                out.push_str("\n\n");
            }
            Block::Quote { spans } => {
                for line in plain(spans).lines() {
                    out.push_str("> ");
                    out.push_str(line);
                    out.push('\n');
                }
                out.push('\n');
            }
            Block::Rule => out.push_str("----------\n\n"),
            Block::Table { columns, cells } => {
                for row in cells.chunks(*columns) {
                    out.push_str(&row.iter().map(|c| plain(c)).collect::<Vec<_>>().join("\t"));
                    out.push('\n');
                }
                out.push('\n');
            }
        }
    }
    out.trim_end().to_string() + "\n"
}

/// HTML to Markdown through `htmd`.
pub fn html_to_markdown(html: &str) -> Result<String> {
    let converter = htmd::HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style", "head", "nav", "iframe", "noscript"])
        .build();
    converter
        .convert(html)
        .map(|s| s.trim().to_string() + "\n")
        .map_err(|e| message(format!("Could not read HTML: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Title\n\nFirst *para* with **bold** and `code` and [a link](https://example.com).\n\n- one\n- two\n  - nested\n\n1. first\n2. second\n\n> quoted text\n\n```rust\nfn main() {}\n```\n\n---\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nLine one  \nLine two\n";

    #[test]
    fn markdown_blocks_and_outline() {
        let doc = parse_markdown(SAMPLE);
        assert!(matches!(&doc.blocks[0], Block::Heading { level: 1, .. }));
        match &doc.blocks[1] {
            Block::Paragraph { spans } => {
                assert!(spans.iter().any(|s| s.italic && s.text == "para"));
                assert!(spans.iter().any(|s| s.bold && s.text == "bold"));
                assert!(spans.iter().any(|s| s.code && s.text == "code"));
                assert!(spans
                    .iter()
                    .any(|s| s.link.as_deref() == Some("https://example.com")));
            }
            other => panic!("unexpected {other:?}"),
        }
        match &doc.blocks[2] {
            Block::List { ordered, items } => {
                assert!(!ordered);
                assert_eq!(items.len(), 3);
                assert_eq!(plain(&items[2]), "– nested");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(&doc.blocks[3], Block::List { ordered: true, .. }));
        assert!(matches!(&doc.blocks[4], Block::Quote { .. }));
        match &doc.blocks[5] {
            Block::Code { text, lang } => {
                assert_eq!(text, "fn main() {}");
                assert_eq!(lang.as_deref(), Some("rust"));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(doc.blocks[6], Block::Rule);
        match &doc.blocks[7] {
            Block::Table { columns, cells } => {
                assert_eq!(*columns, 2);
                assert_eq!(cells.len(), 4);
            }
            other => panic!("unexpected {other:?}"),
        }
        match &doc.blocks[8] {
            Block::Paragraph { spans } => assert!(spans.iter().any(|s| s.br)),
            other => panic!("unexpected {other:?}"),
        }
        let outline = doc.outline();
        assert_eq!(outline.len(), doc.blocks.len());
        assert_eq!(outline[0].kind, "heading1");
        assert_eq!(outline[0].text, "Title");

        let html = to_html(&doc, "T");
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<a href=\"https://example.com\">a link</a>"));
        assert!(html.contains("<ol>"));
        let text = to_text(&doc);
        assert!(text.starts_with("Title\n====="));
        assert!(text.contains("- one\n"));
        assert!(text.contains("1. first\n"));
    }

    #[test]
    fn plain_text_paragraphs_and_html_markdown() {
        let doc = parse_text("Line one\nLine two\n\n\nSecond paragraph <b>\n");
        assert_eq!(doc.blocks.len(), 2);
        match &doc.blocks[0] {
            Block::Paragraph { spans } => {
                assert_eq!(spans.len(), 3);
                assert!(spans[1].br);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(to_html(&doc, "x").contains("Second paragraph &lt;b&gt;"));
        let md = html_to_markdown("<html><head><style>x</style></head><body><h1>Hi</h1><p>Some <b>bold</b> text.</p></body></html>").unwrap();
        assert!(md.starts_with("# Hi"));
        assert!(md.contains("**bold**"));
    }
}
