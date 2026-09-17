---
title: "Document Conversion"
description: "The Convert tab: input detection, the capability matrix, routes per format pair, merging, and PDF protection on output."
type: "guide"
tags:
  - conversion
  - documents
  - capability-matrix
resource: "docs/DOCUMENT_CONVERSION.md"
last_updated: "2026-09-17"
source_sync: "manual"
---

# Document Conversion

The Convert tab turns documents into PDF, DOCX, TXT, HTML or Markdown, optionally combines them into one PDF, and optionally adds an open password to PDF output. The backend decides what each file can become; the UI only shows that decision.

Owning files:

- `crates/core/src/inspect.rs` - input kind detection
- `crates/core/src/capability.rs` - `route`, `capabilities`, `OutputFormat`
- `crates/core/src/job.rs` - `convert_document`, batch execution, merge
- `crates/core/src/text.rs` - Markdown, text and HTML block model
- `apps/desktop/src/App.tsx` - per-row and batch format controls

## Input detection

`inspect` reads the first bytes of the file. `%PDF` is a PDF, `age-encryption.org/v1` is an `.age` file, a zip is opened and classified by its parts (`word/document.xml`, `ppt/presentation.xml`, `xl/workbook.xml`, or an ODF `mimetype` of `application/vnd.oasis.opendocument.text`), and image signatures go through the `image` crate. Only `.md`, `.markdown`, `.html`, `.htm`, `.xhtml`, `.txt` and `.text` are classified by extension. Everything else is `Other` and is skipped by every tab except file encryption.

A renamed file is therefore handled by what it is, not what it is called.

## Capability matrix

`capabilities(kind, engines)` returns one row per output format with `available`, the `engine` name and a `reason` when unavailable. The UI marks unavailable formats with ✕ in the per-row select and shows the reason as a tooltip. With LibreOffice present:

| Input | PDF | DOCX | TXT | HTML | MD |
| --- | --- | --- | --- | --- | --- |
| PDF | copy | no | text extraction | no | no |
| DOCX | Office | copy | Office | Office | Office, or Office HTML then built-in |
| ODT | Office | Office | Office | Office | Office, or Office HTML then built-in |
| PPTX | Office | no | no | no | no |
| XLSX | Office | no | no | Office | no |
| MD | built-in (Typst) | Office | built-in | built-in | copy |
| TXT | built-in (Typst) | Office | copy | built-in | copy |
| HTML | Office | Office | Office | copy | built-in |

PDF/A-1b, PDF/A-2b, PDF/A-3b, PDF/A-4, PDF/A-4f and PDF/UA-1 are additional targets for DOCX, ODT, PPTX, XLSX and
HTML only, requiring a detected LibreOffice version of 25.8 or later. Both
use Office without a built-in fallback; see [PDF/A Export](PDF_A_EXPORT.md).

Without LibreOffice, every `Office` cell becomes unavailable with the reason `LibreOffice was not found…`, except HTML to PDF and HTML to TXT, which fall back to the built-in path (HTML to Markdown, then Typst or plain text). The reasons behind each *no*:

- PDF: `A PDF keeps its layout. Text can be extracted to TXT; editable formats are not offered.`
- PPTX: `Presentations convert to PDF only.`
- XLSX: `Spreadsheets convert to PDF or HTML.`

## Routes

`route(kind, format, engines)` picks one of these; `convert_document` runs it and writes the result into the batch work folder.

| Route | What happens |
| --- | --- |
| `Copy` | Bytes are copied. A DOCX whose page layout was changed is rewritten instead (see [`PAGE_LAYOUT.md`](PAGE_LAYOUT.md)). |
| `PdfText` | `pdf::extract_text` writes one text block per page, separated by a form feed. |
| `Office` | LibreOffice converts the file. A DOCX with layout changes is first rewritten into the work folder and that copy is converted. HTML output from Writer sources uses the `EmbedImages` filter option, so pictures travel inside the file as data URIs instead of loose files next to it; Calc HTML has no such option. |
| `TextToPdf` | Markdown or text is parsed into blocks and paginated by Typst with the layout settings. |
| `TextToHtml`, `TextToTxt` | The block model is written as a self-contained HTML page or as plain text. |
| `HtmlToMd` | `htmd` converts the HTML in process; `script`, `style`, `head`, `nav`, `iframe` and `noscript` are dropped. |
| `HtmlToPdfBuiltin`, `HtmlToTxtBuiltin` | HTML to Markdown, then the text pipeline. Used only when LibreOffice is missing. |
| `OfficeToMdViaHtml` | LibreOffice writes HTML, then `htmd` writes Markdown. Used when LibreOffice has no Markdown filter. |
| `TextToOfficeViaHtml` | The built-in engine writes HTML, LibreOffice reads it. Used for Markdown to DOCX when LibreOffice has no Markdown filter. |

## The text block model

Markdown is parsed with `pulldown-cmark` (CommonMark plus tables, strikethrough and task lists) into `Block` values: heading, paragraph, list, code, quote, rule, table. Inline runs are `Span` values with bold, italic, code, link and hard-break flags. Plain text becomes paragraphs split on blank lines, with single newlines kept as hard breaks.

The model matters for two reasons. The Typst template receives it as data and never interprets user text as markup, so a `.txt` file containing `#pagebreak()` renders those characters. And the outline shown in the page-layout panel is the list of these blocks, which is what a page break attaches to.

Nested lists are flattened into their parent list with an en dash prefix. Images in Markdown render as their alt text. Raw HTML inside Markdown is dropped.

## Batch format and per-row format

Each row has its own output format, defaulting to PDF when available, else the first available format. The batch select in the settings column applies its format to every selected row that can reach it; rows that cannot keep their previous choice. The primary action is disabled while any selected row targets an unavailable format, and the note under it says so.

## Combine into one document

With the switch on, every selected document is converted to PDF in the work folder, in queue order, then merged with `pdf::merge_to_bytes`. The order list in the page-layout panel moves rows up and down; the list is the queue order. Any failing input fails the whole merge and nothing is saved. Locked PDFs are refused with `… has a password. Unlock it before merging.`

The output name comes from the *Output filename* field; `.pdf` is added when missing, and the save dialog lets the user change it.

## Password-protect PDFs

The switch in the settings column asks for a new password (12 characters or more, confirmed). Every item whose output is PDF is protected after conversion with `pdf::protect`, AES-256, readable by any PDF viewer. Non-PDF outputs in the same batch are not affected. For a combined document, protection is applied to the merged file. See [`PDF_TOOLS.md`](PDF_TOOLS.md).

## Output names

| Case | Name |
| --- | --- |
| Single file | Save dialog, suggested `<stem>.<ext>` |
| Several files | Folder picker; `<stem>.<ext>`, then `<stem> (2).<ext>` when taken |
| Combined document | Save dialog with the chosen name |

`<stem>` is the file name without its last extension, so `report.final.docx` becomes `report.final.pdf`.

## Not implemented

- PDF to DOCX, HTML or Markdown; only text extraction is offered.
- Spreadsheet options such as sheet selection, print areas and scaling.
- Passing several files to one LibreOffice process; each Office item starts its own.
- Font substitution warnings when a DOCX uses fonts LibreOffice does not have.

## Archival PDF export

PDF/A-1b, PDF/A-2b, PDF/A-3b, PDF/A-4, PDF/A-4f and PDF/UA-1 are additional Convert targets for DOCX, ODT, PPTX, XLSX and HTML with detected LibreOffice 25.8+. See [PDF/A Export](PDF_A_EXPORT.md) for eligibility, preview behavior, merge/password restrictions and validation limits.

## Watermark every page

Convert can add “Confidential” or a recipient name to every page of ordinary PDF outputs, including existing PDFs and combined documents. All selected outputs must be ordinary PDF; Rust rejects incompatible actions and formats. Marking happens before password protection. See [PDF Tools](PDF_TOOLS.md#watermarks) for behavior and limitations.
