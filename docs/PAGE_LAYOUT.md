---
title: "Page Layout"
description: "Orientation, margins, paragraph spacing and page breaks: how they are applied to DOCX and text sources, and how the preview renders the real output."
type: "guide"
tags:
  - layout
  - page-breaks
  - preview
  - docx
resource: "docs/PAGE_LAYOUT.md"
last_updated: "2026-09-16"
source_sync: "manual"
---

# Page Layout

The page-layout panel appears in the Convert tab when a selected row targets PDF or DOCX, or when combining. It shows the real output of one document with the current settings, lists the document's blocks so a page break can be set before any of them, and lets the user reorder documents for a merge.

Owning files:

- `crates/core/src/layout.rs` - `Layout`, `Orientation`, `Margins`, `Spacing`, Typst rendering
- `crates/core/src/docx.rs` - DOCX rewrite and paragraph outline
- `crates/core/src/job.rs` - `preview_pdf`, `outline`
- `apps/desktop/src/App.tsx` - controls and the outline list
- `apps/desktop/src/Preview.tsx` - PDF.js rendering

## Settings

| Setting | Values | Default |
| --- | --- | --- |
| Orientation | As is, Portrait, Landscape | As is |
| Margins | Narrow 10 mm, Normal 20 mm, Wide 30 mm | Normal |
| Paragraph spacing | Compact, Comfortable, Spacious | Comfortable |
| Page breaks | A set of block indices per document | none |

Orientation, margins and spacing are shared by the whole batch. Page breaks belong to one document and travel with its queue row.

The defaults mean *leave the source alone*: a DOCX keeps its own page size, margins and spacing until a setting is changed. For Markdown and text sources, which have no page settings of their own, the defaults are real values: A4 portrait, 20 mm, comfortable spacing.

## Which sources honour which settings

| Source | Orientation, margins, spacing | Page breaks |
| --- | --- | --- |
| DOCX | Applied by rewriting a copy | Body paragraphs |
| Markdown, TXT | Applied by the built-in engine | Blocks |
| HTML | Only without LibreOffice (built-in fallback) | Only without LibreOffice |
| ODT, PPTX, XLSX, PDF | Not applied | Not available |

The hint under the controls says so, and the outline list is empty for sources without page-break support.

## DOCX rewrite

`docx::rewrite` copies the zip entry by entry and edits two parts as text. The source file is never opened for writing.

`word/document.xml`:

- **Orientation**: every `<w:sectPr>` gets its `<w:pgSz>` width and height swapped when they do not match the request, plus `w:orient`. A section without `pgSz` gets an A4 one.
- **Margins** (when not Normal): `<w:pgMar>` top, right, bottom and left become the chosen size in twips (567, 1134 or 1701). Header, footer and gutter stay. A section without `pgMar` gets one.
- **Page breaks**: `w:pageBreakBefore` is enabled in each chosen body paragraph. Existing false values are changed to true; new properties go after `pStyle`, `keepNext` and `keepLines`. Empty `pPr` elements are expanded, and paragraphs without `pPr` get one. Empty `<w:p/>` paragraphs are expanded too. Tracked-change snapshots are preserved.

`word/styles.xml` (only when spacing is not Comfortable):

- The document defaults (`w:docDefaults/w:pPrDefault`) get `w:spacing w:after` of 0, 160 or 320 twips (0, 8 or 16 pt). Missing elements are created.
- The default paragraph style (`w:type="paragraph" w:default="1"`) gets the same `w:after` when it already has a `w:spacing` element. Other styles and paragraphs with direct spacing keep theirs.

Body paragraphs are the `w:p` elements that are direct children of `w:body`, or nested only inside content controls (`w:sdt`, `w:sdtContent`, `w:customXml`). Paragraphs inside tables and text boxes are not listed and cannot take a break. The index of a paragraph in this list is stable between the outline and the rewrite because both use the same scan.

The outline text is the concatenated `w:t` runs, unescaped, cut to 90 characters. Headings are detected from the style id (`Heading1`, `Title`, and the German and Croatian equivalents LibreOffice may write) and shown as H1 to H6.

## Built-in engine

`layout::render_pdf` serialises the block model plus the settings to JSON and compiles a fixed Typst template. The template reads `sys.inputs.doc`, sets `page(paper: "a4", flipped: …, margin: … mm)`, paragraph spacing in em (0.6, 1.2 or 2.0) and inserts `pagebreak(weak: true)` before every block whose index is in `page_breaks`. Index 0 never breaks. Text, headings, lists, code, quotes, rules and tables map to the matching Typst functions with string arguments, so nothing in the user's document is evaluated as Typst code.

Fonts are the ones embedded by `typst-kit`, so the same file renders the same way on every machine. Page count is available from the compiled document and is part of the `Rendered` result.

## Preview

The panel requests `preview(id, format, layout)` 700 ms after the last change, for the document chosen in *Preview document* (or the highlighted document in the merge order list). The backend runs the same route as a real conversion into a temporary work folder and returns PDF bytes. For a PDF target that is the conversion itself. For a DOCX target from a Markdown, text or HTML source the backend first writes the DOCX exactly as it would be saved, then renders that DOCX to PDF, so the preview shows the real result rather than the built-in engine's idea of it. PDF.js, bundled with the app and loaded from the app's own origin, renders up to 40 pages as canvases at 380 CSS pixels wide, scaled for the display's pixel ratio. The count badge shows `N pages · A4 landscape`, `… portrait` or `… as in document`.

Previews run one at a time behind a gate in the backend. A newer preview request cancels the one in flight (LibreOffice included, through its job object) and takes its place; starting a batch cancels the current preview and holds the gate until the batch ends. A preview requested while a batch runs is refused at once. Office sources take a few seconds per preview; text sources render in milliseconds.

## Tests

- `layout::tests::markdown_renders_with_breaks_and_landscape` - one page by default, two pages with a break, landscape media box, break at index 0 ignored, Typst syntax in text is not executed
- `docx::tests::outline_lists_body_paragraphs_only` - table paragraphs excluded, entities unescaped, empty paragraphs marked
- `docx::tests::rewrite_applies_layout` - landscape swap, narrow margins, three breaks placed correctly including in `keepNext` and empty paragraphs, style spacing, portrait restore, no overwrite

## Not implemented

- Selecting a block by clicking on the rendered page; the outline list is the selection surface.
- Keep-with-next, spacing before, or per-paragraph overrides.
- ODT rewrites (the same edits on `styles.xml` and `content.xml` are feasible).
- Undo history beyond unticking a break or *Reset page breaks*.
