//! Page layout settings and the built-in pagination engine (Typst) for text
//! sources. The Typst template only ever receives data, never user markup.

use crate::{message, text::Doc, Result};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use typst::foundations::{Dict, Str, Value};
use typst_as_lib::{typst_kit_options::TypstKitFontOptions, TypstEngine, TypstTemplateMainFile};
use typst_layout::PagedDocument;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    /// Keep what the source document says.
    #[default]
    Keep,
    Portrait,
    Landscape,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Margins {
    Narrow,
    #[default]
    Normal,
    Wide,
}

impl Margins {
    pub fn mm(self) -> u32 {
        match self {
            Margins::Narrow => 10,
            Margins::Normal => 20,
            Margins::Wide => 30,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Spacing {
    Compact,
    #[default]
    Comfortable,
    Spacious,
}

impl Spacing {
    /// Space between paragraphs in em for the built-in engine.
    pub fn em(self) -> f64 {
        match self {
            Spacing::Compact => 0.6,
            Spacing::Comfortable => 1.2,
            Spacing::Spacious => 2.0,
        }
    }
    /// Space after each paragraph in twips (1/20 pt) for DOCX rewrites.
    pub fn twips(self) -> u32 {
        match self {
            Spacing::Compact => 0,
            Spacing::Comfortable => 160,
            Spacing::Spacious => 320,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub orientation: Orientation,
    pub margins: Margins,
    pub spacing: Spacing,
    /// Indices of blocks (text sources) or body paragraphs (DOCX) that start a new page.
    pub page_breaks: Vec<usize>,
}

impl Layout {
    /// True when nothing would change in a source document.
    pub fn is_default(&self) -> bool {
        *self == Layout::default()
    }
}

pub struct Rendered {
    pub pdf: Vec<u8>,
    pub pages: usize,
}

const TEMPLATE: &str = r##"
#let d = json(bytes(sys.inputs.doc))
#let pg = d.page
#set page(paper: "a4", flipped: pg.landscape, margin: pg.margin_mm * 1mm)
#set text(size: 11pt, hyphenate: false)
#set par(justify: false, spacing: pg.spacing_em * 1em, leading: 0.65em)
#set block(spacing: pg.spacing_em * 1em)
#show heading: it => block(above: 1.2em, below: 0.7em, it)
#show link: set text(fill: rgb("#2f6f5e"))

#let inline(spans) = {
  if spans.len() == 0 { return [] }
  spans.map(s => {
    if s.br { return linebreak() }
    let t = if s.code { raw(s.text) } else { s.text }
    if s.bold { t = strong(t) }
    if s.italic { t = emph(t) }
    if s.link != none { t = link(s.link, t) }
    t
  }).join()
}

#for b in d.blocks {
  if b.break_before { pagebreak(weak: true) }
  if b.kind == "heading" {
    heading(level: b.level, inline(b.spans))
  } else if b.kind == "paragraph" {
    par(inline(b.spans))
  } else if b.kind == "list" {
    if b.ordered {
      enum(..b.items.map(i => inline(i)))
    } else {
      list(..b.items.map(i => inline(i)))
    }
  } else if b.kind == "code" {
    block(width: 100%, fill: luma(245), inset: 8pt, radius: 3pt, raw(b.text, block: true, lang: b.lang))
  } else if b.kind == "quote" {
    quote(block: true, inline(b.spans))
  } else if b.kind == "rule" {
    line(length: 100%, stroke: 0.5pt + luma(180))
  } else if b.kind == "table" {
    table(columns: b.columns, ..b.cells.map(c => inline(c)))
  }
}
"##;

fn engine() -> &'static TypstEngine<TypstTemplateMainFile> {
    static ENGINE: OnceLock<TypstEngine<TypstTemplateMainFile>> = OnceLock::new();
    ENGINE.get_or_init(|| {
        TypstEngine::builder()
            .main_file(TEMPLATE)
            .search_fonts_with(
                TypstKitFontOptions::default()
                    .include_system_fonts(false)
                    .include_embedded_fonts(true),
            )
            .build()
    })
}

fn document_json(doc: &Doc, layout: &Layout) -> String {
    let blocks: Vec<serde_json::Value> = doc
        .blocks
        .iter()
        .enumerate()
        .map(|(index, block)| {
            let mut value = serde_json::to_value(block).unwrap_or(serde_json::Value::Null);
            if let serde_json::Value::Object(map) = &mut value {
                map.insert(
                    "break_before".into(),
                    serde_json::Value::Bool(index > 0 && layout.page_breaks.contains(&index)),
                );
            }
            value
        })
        .collect();
    serde_json::json!({
        "page": {
            "landscape": layout.orientation == Orientation::Landscape,
            "margin_mm": layout.margins.mm(),
            "spacing_em": layout.spacing.em(),
        },
        "blocks": blocks,
    })
    .to_string()
}

/// Paginates a block document into a PDF using the embedded fonts.
pub fn render_pdf(doc: &Doc, layout: &Layout) -> Result<Rendered> {
    let mut inputs = Dict::new();
    inputs.insert(
        Str::from("doc"),
        Value::Str(Str::from(document_json(doc, layout))),
    );
    let compiled: PagedDocument = engine()
        .compile_with_input(inputs)
        .output
        .map_err(|e| message(format!("Layout engine error: {e}")))?;
    let pdf = typst_pdf::pdf(&compiled, &typst_pdf::PdfOptions::default()).map_err(|errors| {
        let text = errors
            .iter()
            .map(|e| e.message.to_string())
            .collect::<Vec<_>>()
            .join("; ");
        message(format!("Could not write PDF: {text}"))
    })?;
    Ok(Rendered {
        pdf,
        pages: compiled.pages().len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text;

    #[test]
    fn markdown_renders_with_breaks_and_landscape() {
        let md = "# Report\n\nParagraph one with **bold** and `code`.\n\n- a\n- b\n\n## Part two\n\nMore text.\n\n| x | y |\n|---|---|\n| 1 | 2 |\n";
        let doc = text::parse_markdown(md);
        let plain = render_pdf(&doc, &Layout::default()).unwrap();
        assert_eq!(plain.pages, 1);
        assert!(plain.pdf.starts_with(b"%PDF"));
        let mut layout = Layout {
            orientation: Orientation::Landscape,
            margins: Margins::Wide,
            spacing: Spacing::Spacious,
            page_breaks: vec![3],
        };
        let broken = render_pdf(&doc, &layout).unwrap();
        assert_eq!(broken.pages, 2);
        let pdf = lopdf::Document::load_mem(&broken.pdf).unwrap();
        let page = *pdf.get_pages().values().next().unwrap();
        let media = pdf
            .get_dictionary(page)
            .unwrap()
            .get(b"MediaBox")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o.as_float().unwrap())
            .collect::<Vec<_>>();
        assert!(media[2] > media[3], "landscape page expected: {media:?}");
        layout.page_breaks = vec![0];
        assert_eq!(render_pdf(&doc, &layout).unwrap().pages, 1);
        let hostile = text::parse_text("#pagebreak() #set page(fill: red) *not markup*\n\nSecond");
        let rendered = render_pdf(&hostile, &Layout::default()).unwrap();
        assert_eq!(rendered.pages, 1);
    }
}
