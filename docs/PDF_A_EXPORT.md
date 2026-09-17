---
title: "PDF Standards, Attachments And Validation"
description: "Local PDF/A and PDF/UA export, associated files, veraPDF validation and accessibility review."
type: "guide"
tags:
  - pdf
  - archival
  - accessibility
resource: "docs/PDF_A_EXPORT.md"
last_updated: "2026-09-17"
source_sync: "manual"
---

# PDF Standards, Attachments And Validation

Convert offers PDF/A-1b, PDF/A-2b, PDF/A-3b, PDF/A-4, PDF/A-4f and PDF/UA-1.
All use `.pdf` filenames. DOCX, ODT, PPTX, XLSX and HTML sources require a
detected LibreOffice 25.8+ and local veraPDF/Java. Existing PDFs can be validated
without modification, but cannot be converted to another standard by this app.
Markdown, plain text and images remain unsupported as sources for standards export.
PDF/UA-2 and combinations of PDF/A plus PDF/UA are not offered.

## Export and validation

1. Select a profile in the batch or per-file format control.
2. Optionally add attachments for PDF/A-3b; PDF/A-4f requires at least one.
3. LibreOffice exports through the appropriate Writer, Calc or Impress filter.
4. Rust embeds selected files, then checks the XMP declaration and runs local
   veraPDF against the explicit requested profile.
5. Only a passing result is committed, using the existing no-overwrite flow.

The per-file report distinguishes PDF/A validation passed, PDF/UA machine checks
passed with human review required, and validation failure. Failed validation
includes up to 20 rule descriptions and saves no output. Missing validator,
malformed/incomplete reports, wrong reported profile, process failure, timeout
or cancellation never count as conformance. Sources are preserved.

In Convert settings, select exactly one existing PDF and choose **Validate locally**
to check it against any offered profile without converting it. This command shares
the job/preview gate and cancellation mechanism. Nothing is uploaded.

Previews show the page content from the selected export route. They do not include
attachments or imply that final validation has passed. PDF/A-4f previews use the
base PDF/A-4 export. DOCX layout editing retains the existing temporary-copy path.
Other Office source layout limitations are unchanged.

Merging and password protection are refused for all standards exports in both the
command layer and Rust core. Other PDF tools (cleaning, protecting, merging) can
invalidate conformance if later applied to an exported file.

## Attachments

The native file picker registers attachments as opaque IDs, separately from the
conversion queue. Each selection includes a relationship (Source, Data, Supplement,
Alternative or Unspecified) and optional description. The same attachment list is
added to every selected output, so all selected outputs must be PDF/A-3b or PDF/A-4f.

Rust enforces regular files, at most 20 attachments, 32 MiB per file, 128 MiB total,
unique case-insensitive filenames and descriptions at most 2000 UTF-8 bytes.
Payloads are embedded byte-for-byte with Unicode filenames, MIME type, size, MD5
checksum, source modification date, associated-file relationship and catalog/name-tree
entries. Existing embedded files are never silently replaced. PDF/A-4f receives
its F conformance declaration after embedding; veraPDF checks the final document.
Descriptions, filenames, timestamps and attachment contents remain part of the
exported document: adding attachments is not a privacy-cleaning operation.

Adding XML does not produce or certify a Factur-X/ZUGFeRD invoice. Business-data
creation, EN 16931 validation and country-specific submission rules are separate
features and are not implemented here.

## PDF/UA accessibility

PDF/UA-1 uses PDF 1.7, tagging and LibreOffice's accessibility export option.
veraPDF performs machine checks. A passing result always retains the human-review
status: review reading order, heading/table semantics, document language, link
meaning and image descriptions. The application cannot invent accurate alternative
text or guarantee accessibility from an arbitrary source document. Fix failed
checks in the source and export again.

## Local validator setup

Run `powershell -File scripts/setup-pdf-validation.ps1` explicitly for developer
setup. It downloads pinned veraPDF Greenfield 1.30.2 and Temurin JRE 21.0.12.1
Windows x64 archives into ignored `.tools/`, checks SHA-256 hashes and uses an
unattended installer. Existing installations are not overwritten. The application
itself never downloads software. This workspace has those local tools installed.

Detection looks for `engines/verapdf` or `.tools/verapdf` above the executable,
or `DOC_CONVERTER_VERAPDF` pointing to a veraPDF installation directory with
`bin/cli-*.jar`. Java comes from `DOC_CONVERTER_JAVA` (full executable path),
`<validator>/jre/bin/java.exe`, the developer `.tools/pdf-standards/java` tree,
`JAVA_HOME`, or PATH. Restart the application after installing tools.

Java is invoked directly with argument boundaries, not through a batch shell.
Validation uses a 512 MiB Java heap, a 120-second timeout, an 8 MiB report limit,
a hidden Windows process and a kill-on-close job object where available. Reports
are temporary and removed on completion. These controls are not an OS sandbox.
No raw input paths or validator executable paths are accepted from the webview.

A distributable installer still needs to bundle the validator/runtime or document
these prerequisites. Preserve veraPDF and Java licence/notice files when packaging;
the developer setup is not a finished redistribution package.

## Sources and verification

- [LibreOffice export parameters](https://help.libreoffice.org/latest/en-US/text/shared/guide/pdf_params.html)
- [veraPDF profile selection and report schema](https://docs.verapdf.org/cli/validation/)
- [veraPDF automated installation](https://docs.verapdf.org/install/)
- [Associated files in PDF](https://pdfa.org/files-inside-pdf/)

See [Build And Verification](BUILD_AND_VERIFICATION.md) for actual test results,
including limitations of the fixture coverage. Engine success is not a guarantee
that a particular receiving authority accepts the selected profile.
