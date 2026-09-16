---
title: "Design Reference"
description: "The accepted opendesign mockup: modes, queue, settings, page layout preview, tokens, and the gap between the design and the shipped UI."
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

The accepted design is the interactive mockup in `opendesign/mockups/document-converter/`. Open `index.html` directly, or open `opendesign/index.html` for the viewer with a sidebar. The mockup is a prototype: it simulates progress, never reads file contents, and generates no output.

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

Fonts: `"IBM Plex Sans", "Segoe UI", sans-serif` for text, `"IBM Plex Mono", Consolas, monospace` for eyebrows and labels, Georgia for the `d.` brand mark. No web font is loaded; IBM Plex is used only when installed. Focus outline is `3px solid #73a391` with `3px` offset. Header is 76 px, main content is capped at 1440 px with 38 px padding.

## Modes

Four tabs: Convert, Images, Encrypt, Decrypt. Switching a mode:

- clears passwords and resets Show/Hide
- shows the matching settings block and hides the others
- rewrites the queue description, drop-zone title and accepted types, output column heading, settings title and subtitle
- saves the mode to `localStorage` under `doc-converter-mode`

Eligibility per mode, checked by extension:

| Mode | Eligible types |
| --- | --- |
| Convert | PDF, DOCX, PPTX, XLSX, MD, HTML, TXT, ODT |
| Images | JPG, JPEG, PNG, WEBP, TIFF, TIF, BMP |
| Encrypt, PDF protection | PDF only |
| Encrypt, encrypted file | any |
| Decrypt | PDF, DCENC |

Ineligible selected files show `Unsupported file` or `PDF required` in the status column and are skipped, and the action note counts them.

## Queue

A table with columns: select checkbox, document (type badge, name, size, `Example file` marker), output, status, remove. Above it: select-all with indeterminate state, selection count, `Clear all`. Below it: a drop zone that also opens the file picker, `Original files are kept`, and `Load example files`, which loads mode-specific samples.

Output column per mode:

- Convert: a per-row format select (PDF, DOCX, TXT, HTML, MD), or `Merged PDF` when combining
- Images: a per-row select (PNG, JPG, WEBP, PDF)
- Encrypt: `PDF + key` or `.dcenc`
- Decrypt: `Original`

## Settings aside

Step label `02`, a title and subtitle per mode, then mode blocks:

- **Convert**: batch format select that applies to selected rows, `Password-protect PDFs` switch, note that it applies only to PDF output.
- **Images**: format select, quality slider 10 to 100 with live percent, resize select (original, 1920, 1280, 640), notes about transparency and lossy quality.
- **Document layout** (Convert only): `Combine into one document` switch with output filename, orientation toggle Portrait or Landscape.
- **Encrypt**: radio choice between `Password-protected PDF` and `Encrypted file`.
- **Decrypt**: info card `Unlock your documents`.
- **Password** (Encrypt, Decrypt, or Convert with protection on): password with Show, confirm field except in Decrypt, hint `Use at least 12 characters`, mismatch error, warning that it cannot be recovered.
- Destination card `Save a new copy · Choose a location when saving`.
- Action area: summary `N documents ready` plus output type, primary button `Preview conversion/merge/encryption/decryption →`, and a note `Preview only · no files are processed`.

Validation mirrors the app: 12 or more characters and a match for new passwords, non-empty for Decrypt, a non-empty merge name when merging, and at least one eligible selected file.

## Page layout preview

Shown in Convert mode for rows whose output is PDF or DOCX. It renders sample text, not the user's documents.

- Preview document select, or the ordered document list with up and down arrows when merging.
- Margins: Normal 20 mm, Narrow 10 mm, Wide 30 mm. Paragraph spacing: Compact, Comfortable, Spacious.
- Each document contributes four sample blocks: a heading and three paragraphs. Blocks pack onto A4 pages using a capacity of 6 for portrait or 4 for landscape, adjusted by margins, with spacing setting the cost per block.
- Clicking a block selects it. `Move to next page` sets a page break before it. `Reset page breaks` clears all breaks.
- Page count reads `N pages · A4 portrait`.

This models the plan's intent: page breaks are real properties on a block, not inserted blank lines.

## Simulated run

`Preview …` marks selected rows `Queued`, shows a result bar with a progress element, and advances every 350 ms through `Previewing…` to `Demo complete`. Cancel resets statuses and says `Your original files are unchanged`. Completion clears the passwords.

## What the shipped app takes from the design

Implemented in `apps/desktop/src/main.tsx`:

- header with brand mark, tagline, and build label
- intro eyebrow, title `A new format. The same privacy.`, subtitle
- tab bar with `aria-pressed`, green underline on the active tab
- two-column workspace with white 12 px radius cards and the 340 px settings column
- queue heading with count badge, dashed empty-state card, footnote
- output settings eyebrow and heading, form control styling, hints, password Show/Hide, mismatch error
- primary green full-width action with arrow, Cancel while busy
- green notice and red error boxes, footer `Private by design`
- all colours listed above

Not yet implemented from the design:

- Convert mode, per-row formats, batch format, PDF protection switch
- multi-select queue, statuses, drop zone, Clear all, example files
- page layout preview, orientation, margins, spacing, page breaks, merge order
- WebP and TIFF images, image to PDF
- IBM Plex font stack and CSS variables
- progress bar

The plan replaced the design's `.dcenc` format with standard `.age`. Any future queue copy that mentions `.dcenc` should say `.age`.
