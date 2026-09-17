---
title: "PDF Tools"
description: "Open passwords with AES-256, unlocking, merging, text extraction and image pages, all through lopdf in process."
type: "guide"
tags:
  - pdf
  - encryption
  - merge
resource: "docs/PDF_TOOLS.md"
last_updated: "2026-09-17"
source_sync: "manual"
---

# PDF Tools

The PDF operations on this page use `crates/core/src/pdf.rs` on top of the `lopdf` crate and do not need LibreOffice. [PDF/A export](PDF_A_EXPORT.md) uses the Office adapter separately.

## Open password

`protect` and `protect_bytes` add a document-open password:

- Encryption is AES-256 in CBC mode with the PDF 2.0 standard security handler (`/V 5`, revision 6, crypt filter `AESV3`). Every reader that can open a PDF 2.0 encrypted file can open these.
- The file encryption key is 32 random bytes. The owner password is 24 random bytes as hex and is not shown or stored; there is no "owner" mode in the UI.
- All permissions are granted. Print and copy restrictions are reader-enforced and would be misleading next to a real open password, as the plan says.
- Metadata is encrypted too (`encrypt_metadata: true`).
- A file that already needs a password is refused with `This PDF already has a password. Unlock it first, then protect it again.` A file that is encrypted with an empty password (restrictions only) is opened, its old encryption dropped, and protected with the new password.

The password reaches lopdf as a `&str` inside the process through `SecretString::expose_secret`. It is never written to disk or passed to another process.

## Unlock

`unlock` writes a copy without the password. It uses `Document::load_with_password`; a wrong password, or a damaged file, fails with `Incorrect password or damaged PDF.` and no output. A file with no password fails with `This PDF has no password.` Unused objects, including the old encryption dictionary, are pruned before saving.

lopdf's plain `load` does not parse the objects of a file it cannot decrypt, so `unlock` and every test use the password-aware loaders.

## Merge

`merge_to_bytes` builds a new document from the sources in order, checking the cancel flag before each source:

1. Each source is loaded and its object numbers shifted past the previous maximum.
2. Each page dictionary is copied. `Resources`, `MediaBox`, `CropBox` and `Rotate` inherited from parent `Pages` nodes are resolved and written onto the page, so pages keep their size and rotation.
3. All other objects are copied except `Catalog`, `Pages`, `Page`, `Outlines` and `Outline`.
4. A fresh `Pages` tree and `Catalog` are written, objects are renumbered, and the result is saved.

Lost in a merge: bookmarks, named destinations, form field trees, document-level metadata and the structure tree. Page content, fonts, images and annotations survive. A locked source fails the merge before anything is written.

## Text extraction

`extract_text` writes the text of every page, using lopdf's extractor with a 64 MiB decompression limit per page so a small compressed stream cannot inflate without bound. Pages are separated by a form feed line. Reading order follows the content stream, so multi-column layouts may interleave. Scanned pages produce nothing; OCR is not implemented.

## Image pages

`image_pdf` makes one A4 page per image. The page is landscape when the image is wider than tall. The image is scaled to fit inside a 10 mm margin without exceeding one image pixel per point (72 dpi), then centred.

- Opaque images are stored as JPEG (`DCTDecode`) at the requested quality.
- Images with transparency are stored as Flate-compressed RGB with a soft mask, so transparency survives.

The Images tab uses this for its PDF output; a future combined image PDF can pass several images at once.

## Page count and lock status

`info` uses lopdf's metadata loader to return the page count and whether the file declares encryption. When even that fails, the file is scanned for `/Encrypt` and reported with zero pages, so the queue can still mark it as locked.

## Tests

In `crates/core/src/pdf.rs`:

- `protect_unlock_and_merge` - protect, wrong password refused without output, double protection refused, unlock, merge page count, merge refuses locked input, protected merge opens with the password only
- `text_extraction_writes_pages` - a generated page with Helvetica text is extracted

`images::tests::webp_and_tiff_roundtrip_and_pdf_output` covers image pages; `job::tests::builtin_routes_run_without_office` covers protected conversions and a protected merge end to end.

## Not implemented

- Page ranges, page reordering and rotation in the UI (the core can rotate by setting `Rotate`, but no command exposes it).
- PDF/A conversion of existing PDFs and compression. Archival export of Office/HTML sources is covered in [PDF/A Export](PDF_A_EXPORT.md).
- Owner passwords and permission flags.
- Bookmarks per merged source.

## Watermarks

In Convert, enable **Watermark every page** and keep “Confidential” or enter a
recipient name (1–80 characters, one line). Select ordinary PDF for all selected
files, or combine them into one PDF. Existing unlocked PDFs use the PDF copy
route. DOCX and other document sources are converted first. Images-tab outputs,
non-PDF outputs, PDF/A and PDF/UA export are not eligible; Rust rejects the whole
batch before writing instead of silently leaving some files unmarked.

`watermark.rs` renders literal text with the local Typst engine and embedded
fonts, then uses a shared PDF Form XObject with 22% opacity on every page. It
centres a diagonal label inside the visible MediaBox/CropBox intersection,
resolves inherited resources and rotation, and fits the label to page size.
The original content streams stay in place, wrapped in `q`/`Q`, and the label
is appended after them, so text extraction (including this app's PDF to TXT)
and other readers still see the text. The label's resource names are chosen
to avoid the page's own. MediaBox and CropBox corners are accepted in any order. Undecodable page streams fail
rather than being omitted; decompression is bounded to 64 MiB per page.

Merged documents are marked after merging, and password protection runs after
watermarking. Source bytes are preserved and final writes use the existing
no-overwrite commit. Cancellation is checked before rendering, between pages,
and before returning/committing bytes. Rendering itself is not interruptible.
The label stays in process and local temporary files; it is not persisted as a
UI preference or sent to a network service.

Limitations: one label applies to the whole batch; there is no recipient-list
mail merge, placement/font control, or DOCX watermark output. Font coverage is
limited to the bundled fonts (Croatian names are covered). A watermark is an
editable visual label, not access control or redaction. Editing signed PDFs
invalidates their signatures; preservation of tagged-PDF accessibility and
archival compliance is not guaranteed. Annotations remain above page content
and can obscure it. Existing PDF preview security restrictions still apply;
the conversion preview for non-PDF sources shows the watermark.
