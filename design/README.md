# Doc Converter design

The refined interactive design is in [../opendesign/mockups/document-converter/index.html](../opendesign/mockups/document-converter/index.html). Open it directly or use http://localhost:8289/opendesign/mockups/document-converter/ while the preview server is running.

The original canvas export and three artboards are preserved as source references. The new design keeps their green palette, IBM Plex font preference, warm neutrals, file queue, and adjacent output settings. Fonts fall back to Segoe UI and Consolas when IBM Plex is not installed; there are no external asset requests.

## Review paths

- Convert: select files, change individual or batch formats, optionally protect PDF outputs, then preview progress.
- Encrypt → Password-protected PDF: PDF inputs only; other formats are visibly skipped. Set and confirm a password.
- Encrypt → Encrypted file: any file type; create a separate encrypted copy with a proposed `.dcenc` extension.
- Decrypt: use “Load example files” for protected PDF and `.dcenc` examples. Enter a password to preview restoring copies.
- Also try clearing the queue, adding or dropping files, mismatched passwords, showing/hiding passwords, cancelling progress, and a narrow window.
- Page layout: choose portrait or landscape, margins and paragraph spacing. Select a sample content block and move it to the next page; reset page breaks to undo. Available for PDF and Word output.
- Combine into one document: selected documents become one PDF; use the order arrows in Page layout to arrange them. Individual targets are preserved when merging is switched off.
- Images: use “Load example files” or add JPG, PNG, WebP, TIFF or BMP files. Choose PNG, JPG, WebP or PDF output, quality for lossy formats, and resize bounds. Original aspect ratios are retained in the proposed workflow.

## Implementation boundary

Page preview uses illustrative text for every file, not the actual document contents. Pagination, merge, image conversion, resize, and quality settings demonstrate intended behavior only. They do not generate output. Layout controls apply to the batch; explicit page breaks are tracked per sample content block during the session.

This is a design prototype, not a document processing app. Progress and completion are explicitly simulated. File selection uses names and sizes only; file contents are never read. No output files are generated. Passwords remain in memory and are cleared when switching operations or completing a preview. Only the current operation is saved in localStorage.

Actual conversion, PDF password protection, and general file encryption/decryption still require implementation. `.dcenc` is a proposed product format, not an implemented cryptographic format. Before shipping, define a versioned authenticated file format, use established cryptographic libraries, implement compatible PDF encryption, and test decryption, wrong-password errors, corruption, and conversion fidelity. The current format choices illustrate the interface and do not promise every conversion pair is supported. Desktop folder selection and saving also require app integration.
