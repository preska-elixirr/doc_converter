---
title: "Clean Before Sharing"
description: "Local metadata removal for DOCX, PDF and photos, with accepted revisions, copy safety and explicit limits."
type: "guide"
tags:
  - privacy
  - metadata
resource: "docs/CLEAN_BEFORE_SHARING.md"
last_updated: "2026-09-17"
source_sync: "manual"
---

# Clean before sharing

Metadata is hidden information inside a file. The **Clean before sharing** tab
creates new copies without the supported metadata below. It uses only built-in
Rust engines; no upload or LibreOffice is needed. Add files, select the desired
rows, then choose **Save cleaned copies**. One file uses a save dialog; a batch
uses a folder picker. Existing files are never overwritten.

Owning files: `crates/core/src/clean.rs`, `clean/word.rs`, `job.rs`,
`apps/desktop/src-tauri/src/main.rs`, and `apps/desktop/src/{App.tsx,api.ts,i18n.ts}`.

| Input | Output | Cleaning |
| --- | --- | --- |
| DOCX | `name-clean.docx` | Remove core, app and custom properties (including author and dates), comments and people parts, custom XML data, thumbnails and package signatures. Accept supported tracked changes and remove hidden runs. Reset ZIP entry dates, comments and extra fields. |
| PDF without encryption | `name-clean.pdf` | Remove trailer Info and ID, XMP metadata references and streams, PieceInfo and LastModified; prune unreachable objects and write a new PDF without old incremental revisions or object streams. |
| Static PNG, JPEG, BMP, WebP, TIFF | `name-clean.png` | Decode pixels, apply EXIF orientation, encode a fresh lossless PNG without source EXIF, GPS, camera, XMP, IPTC, comments or ICC profiles. No resizing. |

Output names retain the source stem and get numbered if needed. New filesystem
timestamps are normal; this does not erase filesystem history or the originals.

## DOCX behavior

Cleaning accepts insertions and moves to their new location, removes deletions
and old formatting revisions, removes deleted table rows/cells, and turns off
revision tracking. It removes review markers and revision author/date attributes
throughout XML parts, including headers, footers, footnotes and text boxes.
Relationship and content-type entries for removed parts are removed too.

Hidden `vanish` and `webHidden` runs are removed, including styles inherited via
`basedOn`, paragraph/table styles, default character styles and document defaults. Style handling is conservative:
a hidden base style remains hidden even when a derived style toggles it off.
Review the cleaned copy because this can remove more text than Word hides.

Paragraph-mark deletions, cell-merge revisions, numbering revisions and unresolved editing conflicts are refused
with an instruction to accept those changes in Word first. They cannot safely be
accepted by deleting a subtree: for example, a deleted paragraph mark joins two
paragraphs. See Microsoft's [revision documentation](https://learn.microsoft.com/en-us/office/open-xml/word/how-to-accept-all-revisions-in-a-word-processing-document)
and [paragraph-mark semantics](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.deleted?view=openxml-3.0.1).

DOCX limits: 10,000 entries, 256 MiB decompressed package data, 128 levels of XML
nesting. Duplicate or ambiguous paths, malformed XML and DTDs are rejected.

## Limits and safety

- This is metadata cleaning, not content redaction or a guarantee of anonymity.
  Visible names/dates, text in images, PDF annotations/attachments and embedded
  files/images in DOCX are not scrubbed. White text, off-page objects and other
  visually concealed content are not equivalent to Word's hidden-text flag.
- Unlock encrypted PDFs using **Decrypt** before cleaning, then protect the
  cleaned result if needed. Even PDFs with an empty opening password are refused
  when the original trailer is encrypted.
- Fresh PDF serialization invalidates digital signatures. DOCX package
  signatures are removed. Review and re-sign cleaned copies if necessary.
- Photo output can be larger than the input. Removing colour profiles can change
  colour appearance; orientation is applied to pixels before metadata is dropped.
  Animated PNG/WebP and multipage TIFF are rejected rather than flattened.
- Sources remain untouched. Cancellation and failures do not commit partial files.
  Parsing/encoding runs in process and cancellation occurs at stage boundaries.
  Temporary output is deleted on failure, not securely erased from the device.
- Cleaning is a separate operation; ordinary conversion does not automatically
  clean DOCX/PDF metadata. Applications used to edit cleaned files may add metadata
  again, so clean after the final edit.

## Verification

Regression fixtures cover private DOCX properties/comments, nonstandard comment
part locations, headers, tracked insertions/deletions/moves, hidden styles and
defaults, malformed XML, unsupported revisions, source preservation and no-overwrite
behavior. PDF fixtures check Info/ID/XMP/orphan removal, page retention, encrypted
input and cancellation. Photo fixtures carry camera/GPS EXIF and a rotated
orientation, then assert a metadata-free PNG with the correct dimensions.

See [Build and verification](BUILD_AND_VERIFICATION.md) for commands and recorded
results. Native Word compatibility across arbitrary third-party documents remains
a release verification task.
