---
title: "Design Reference"
description: "The accepted opendesign mockup: modes, queue, settings, page layout preview, tokens, and how the shipped UI now maps to it."
type: "design"
tags:
  - design
  - mockup
  - tokens
resource: "docs/DESIGN_REFERENCE.md"
last_updated: "2026-09-16"
source_sync: "manual"
---

# Design Reference

The accepted design is the interactive mockup in `opendesign/mockups/document-converter/`. Open `index.html` directly, or `opendesign/index.html` for the viewer with a sidebar. The mockup is a prototype: it simulates progress, never reads file contents, and generates no output.

The `design/` folder holds the earlier source material and is kept for reference only:

- `canvas.json` - two pages. Page `App` has the `Main window` and `While converting` artboards and the note `Chosen direction. One window: file list on the left, output settings on the right, one big Convert button. Each file can have its own target format.` Page `Not chosen` has the `AltWizard` three-step sketch.
- `Main.dc.html`, `Converting.dc.html`, `AltWizard.dc.html` - the artboards.
- `doc-converter-app.html` - a 2.5 MB canvas export.
- `README.md` - review paths for the mockup and its implementation boundary.

## Files

| File | Role |
| --- | --- |
| `index.html` | Markup: header, intro, mode tabs, queue, page layout preview, settings aside, result bar, footer |
| `app.js` | Queue model, mode switching, eligibility, validation, simulated run |
| `layout.js` | Orientation, margins, spacing, sample pagination, page breaks, merge order, image quality notes |
| `styles.css` | Tokens and all component styles |

## Tokens

```css
--bg: #f4f3f0;        /* page */
--paper: #fff;        /* cards, header */
--ink: #262923;       /* text */
--muted: #686c63;     /* secondary text */
--line: #e2e4dc;      /* borders */
--green: #2f6f5e;     /* accent */
--green-dark: #245548;/* accent hover */
--tint: #edf3ee;      /* accent background */
--amber: #8c5b24;     /* warnings */
--radius: 12px;
```

Fonts: `"IBM Plex Sans", "Segoe UI", sans-serif` for text, `"IBM Plex Mono", Consolas, monospace` for eyebrows and labels, Georgia for the `d.` brand mark. No web font is loaded. Focus outline is `3px solid #73a391` with `3px` offset. Header is 76 px, main content is capped at 1440 px with 38 px padding.

## Modes

Four tabs: Convert, Images, Encrypt, Decrypt. Switching a mode clears passwords, shows the matching settings block, rewrites the queue description, drop-zone texts, output column heading, settings title and subtitle, and saves the mode to `localStorage`.

Eligibility per mode, checked by extension in the mockup:

| Mode | Eligible types |
| --- | --- |
| Convert | PDF, DOCX, PPTX, XLSX, MD, HTML, TXT, ODT |
| Images | JPG, JPEG, PNG, WEBP, TIFF, TIF, BMP |
| Encrypt, PDF protection | PDF only |
| Encrypt, encrypted file | any |
| Decrypt | PDF, DCENC |

## Queue, settings, page layout, simulated run

A table with select, document, output, status and remove columns; select-all with indeterminate state; `Clear all`; a drop zone; `Load example files`. Settings per mode as described in [`WORKSPACE_UI.md`](WORKSPACE_UI.md). A page-layout section with a preview document select or the merge order list, margins, spacing, sample blocks that can be moved to the next page, and a page count. `Preview …` runs a simulated progress bar.

## How the shipped app maps to the design

Implemented, with real behaviour behind it:

- the four modes, plus a License placeholder tab the design does not show
- the queue table with checkboxes, select-all, per-row format selects, status column, type badges, size line, `Clear all`, drop zone and window-wide drag and drop
- batch format, `Password-protect PDFs`, `Combine into one document` with output filename and order arrows, page orientation
- the page-layout section: margins, paragraph spacing, page breaks, page count; the sample pages are replaced by the real rendered output and the click-to-select blocks by an outline list with checkboxes
- Images: PNG, JPG, WebP and PDF output, quality slider, resize, transparency notes, WebP and TIFF input
- Encrypt: protected PDF or encrypted file; Decrypt: locked PDF or `.age`
- password fields, Show/Hide, confirmation, mismatch error, warning text
- destination card, summary, primary action, action note, result bar with progress and Cancel, footer
- all colours as CSS variables, the IBM Plex font stack with fallbacks

Deliberate differences:

- Orientation has a third choice, *As is*, so a document keeps its own orientation unless changed. The mockup's default *Portrait* would have rotated landscape sources.
- Margins *Normal* and spacing *Comfortable* leave Word documents untouched; they are real values only for text sources.
- Page breaks are set by ticking an outline block, not by clicking a sample block, because the outline is the real block list.
- `.dcenc` is `.age`, a standard format.
- `Load example files` is not offered; the app never fabricates files.
- Eligibility comes from content detection, not the extension.
- The primary button says what it does (`Convert 3 files`), not `Preview conversion`, because files are really processed.
- The `Processed on this computer` badge moved from the intro row to the top right of the header, with a shield icon in front of it, and replaced the `Interactive design preview` chip. Next to it sits an `Aa` button for language and UI scale, which the mockup does not have.
- Every string exists in English and Croatian; the mockup is English only.
