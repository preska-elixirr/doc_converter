---
title: "Converter Core"
description: "API catalog for the converter-core crate: errors and helpers, input detection, capability routing, crypto, images, PDF, LibreOffice, text model, layout, DOCX edits, and batch jobs."
type: "catalog"
tags:
  - methods-report
  - rust
  - core
resource: "docs/methods_report/01_converter_core.md"
last_updated: "2026-09-16"
doc_version: "2.0.0"
source_sync: "manual"
---

# Converter Core

Crate `converter-core`, folder `crates/core/src/`. No Tauri dependency. Every public item below is listed under its module. `cancel: &AtomicBool` is the batch cancel flag; every operation that takes it fails with `Cancelled. No output was saved.` and leaves no output when it is set.

### `lib.rs`

```rust
pub enum Error { Message(String), Io(std::io::Error) }   // Display: the message, or "File operation failed: {0}"
pub type Result<T> = std::result::Result<T, Error>;
pub const CANCELLED: &str = "Cancelled. No output was saved.";
pub use age::secrecy::SecretString;
pub use capability::{capabilities, Availability, Engines, OutputFormat};
pub use crypto::{decrypt, encrypt};
pub use images::convert_image;
pub use inspect::{inspect, InputKind};
pub use layout::{Layout, Margins, Orientation, Spacing};

pub fn copy_file(source: &Path, destination: &Path, cancel: &AtomicBool) -> Result<()>;
// Streams through a temp file beside destination; never overwrites.

pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf;
// "{stem}.{ext}", then "{stem} (2).{ext}", "{stem} (3).{ext}", ... whichever does not exist.

pub fn secret(value: String) -> SecretString;
pub fn stem(path: &Path) -> String;   // file stem, "document" when empty
```

Crate-private helpers with contracts: `message(e) -> Error`, `check_cancel(cancel)`, `copy_cancel(reader, writer, cancel)` (64 KiB chunks), `output(path) -> NamedTempFile` (fails if path exists), `commit(temp, path, cancel)` (sync, `persist_noclobber`), `write_bytes(destination, bytes, cancel)`.

### `inspect.rs`

```rust
#[serde(rename_all = "lowercase")]
pub enum InputKind { Pdf, Docx, Odt, Pptx, Xlsx, Md, Html, Txt, Png, Jpg, Bmp, Webp, Tiff, Age, Other }
impl InputKind {
    pub fn is_image(self) -> bool;     // Png Jpg Bmp Webp Tiff
    pub fn is_document(self) -> bool;  // Pdf Docx Odt Pptx Xlsx Md Html Txt
    pub fn is_text(self) -> bool;      // Md Txt
    pub fn label(self) -> &'static str; // "PDF", "DOCX", ... "FILE"
}
pub fn inspect(path: &Path) -> InputKind;
// Content signatures for PDF, age, zip-based Office/ODT, images; extension for md/html/txt; Other otherwise.
```

### `capability.rs`

```rust
#[serde(rename_all = "lowercase")]
pub enum OutputFormat { Pdf, Docx, Txt, Html, Md }
impl OutputFormat { pub const ALL: [OutputFormat; 5]; pub fn ext(self) -> &'static str; pub fn label(self) -> &'static str; }

#[derive(Default)] pub struct Engines { pub office: Option<OfficeEngine> }

pub enum Route { Unavailable(String), Copy, PdfText, Office, TextToPdf, TextToHtml, TextToTxt,
                 HtmlToMd, HtmlToPdfBuiltin, HtmlToTxtBuiltin, OfficeToMdViaHtml, TextToOfficeViaHtml }
pub const NO_OFFICE: &str;   // "LibreOffice was not found. Install it or copy it to the engines folder, then restart."
pub fn route(kind: InputKind, format: OutputFormat, engines: &Engines) -> Route;

pub struct Availability { pub format: OutputFormat, pub available: bool, pub engine: String, pub reason: Option<String> }
pub fn capabilities(kind: InputKind, engines: &Engines) -> Vec<Availability>;  // one row per OutputFormat::ALL
```

### `crypto.rs`

```rust
pub fn encrypt(source: &Path, destination: &Path, password: SecretString, cancel: &AtomicBool) -> Result<()>;
// age passphrase (scrypt) stream; no password length check here.
pub fn decrypt(source: &Path, destination: &Path, password: SecretString, cancel: &AtomicBool) -> Result<()>;
// Requires an scrypt recipient; authenticates the header before creating the temp file.
```

### `images.rs`

```rust
pub const MAX_EDGE: u32 = 16000;
pub struct ImageInfo { pub kind: InputKind, pub width: u32, pub height: u32 }
pub fn inspect_image(source: &Path) -> Result<ImageInfo>;        // dimensions without decoding pixels
pub fn decode(source: &Path, cancel: &AtomicBool) -> Result<DynamicImage>;
// Sniffs PNG/JPEG/BMP/WebP/TIFF, rejects APNG, animated WebP, multi-page TIFF; limits 16000 px, 256 MiB; EXIF orientation applied.
pub fn resize(image: DynamicImage, max_edge: u32) -> DynamicImage;  // fit within square, never upscale, 0 = keep
pub fn flatten_white(image: &DynamicImage) -> RgbImage;
pub fn has_alpha(image: &DynamicImage) -> bool;
pub fn encode_jpeg(image: &DynamicImage, quality: u8) -> Result<Vec<u8>>;
pub fn encode(image: &DynamicImage, format: &str, quality: u8) -> Result<Vec<u8>>;  // "png" | "jpg" | "webp" | "pdf"
pub fn convert_image(source: &Path, destination: &Path, format: &str, max_edge: u32, quality: u8, cancel: &AtomicBool) -> Result<()>;
// quality 1..=100 (WebP: 100 = lossless), max_edge <= 16000; commits without overwrite.
```

### `pdf.rs`

```rust
pub struct PdfInfo { pub pages: u32, pub encrypted: bool }
pub fn info(source: &Path) -> Result<PdfInfo>;   // metadata loader; falls back to an /Encrypt scan with 0 pages
pub fn protect_bytes(pdf: &[u8], password: &SecretString) -> Result<Vec<u8>>;
pub fn protect(source: &Path, destination: &Path, password: &SecretString, cancel: &AtomicBool) -> Result<()>;
// AES-256 (V5/R6, AESV3), random 32-byte key, random owner password, all permissions, metadata encrypted.
// Refuses a file that already needs a password.
pub fn unlock(source: &Path, destination: &Path, password: &SecretString, cancel: &AtomicBool) -> Result<()>;
// load_with_password; wrong password => "Incorrect password or damaged PDF."; unencrypted => "This PDF has no password."
pub fn merge_to_bytes(sources: &[PathBuf], cancel: &AtomicBool) -> Result<Vec<u8>>;   // pages in order; cancel checked per source; refuses locked sources
pub fn merge(sources: &[PathBuf], destination: &Path, cancel: &AtomicBool) -> Result<()>;
pub fn extract_text(source: &Path, destination: &Path, cancel: &AtomicBool) -> Result<()>;  // form feed between pages, 64 MiB/page limit
pub fn image_pdf(images: &[DynamicImage], quality: u8) -> Result<Vec<u8>>;
// One A4 page per image, landscape when wider than tall, 10 mm margin, scale <= 1 px/pt; JPEG for opaque, Flate + SMask for alpha.
```

### `office.rs`

```rust
#[derive(Serialize)] pub struct OfficeEngine { pub path: PathBuf, pub version: String, pub markdown: bool, #[serde(skip)] pub profile: PathBuf }
pub struct Target { pub ext: &'static str, pub convert_to: &'static str }
impl Target { pub const PDF; pub const DOCX; pub const TXT; pub const HTML /* Writer, EmbedImages */; pub const HTML_CALC; pub const MD; }
impl OfficeEngine {
    pub fn candidates() -> Vec<PathBuf>;                       // env var, exe-relative engines/ and .tools/, Program Files, PATH
    pub fn detect(profile: &Path) -> Option<OfficeEngine>;     // first existing candidate; version from bootstrap.ini; no process started
    pub fn warm_up(&self) -> Result<()>;                       // prepares the profile, runs --version (180 s limit)
    pub fn convert(&self, inputs: &[PathBuf], target: Target, infilter: Option<&str>, outdir: &Path, cancel: &AtomicBool) -> Result<Vec<PathBuf>>;
    // Headless run in a job object; timeout 120 s + 30 s per input (max 900 s); returns <outdir>/<stem>.<ext> per input or an error naming the missing ones.
}
```

### `text.rs`

```rust
pub struct Span { pub text: String, pub bold: bool, pub italic: bool, pub code: bool, pub link: Option<String>, pub br: bool }
impl Span { pub fn text(text: impl Into<String>) -> Self; pub fn br() -> Self; }
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Block { Heading { level: u8, spans: Vec<Span> }, Paragraph { spans }, List { ordered: bool, items: Vec<Vec<Span>> },
                 Code { text: String, lang: Option<String> }, Quote { spans }, Rule, Table { columns: usize, cells: Vec<Vec<Span>> } }
pub struct Doc { pub blocks: Vec<Block> }
impl Doc { pub fn outline(&self) -> Vec<OutlineEntry>; }
pub struct OutlineEntry { pub index: usize, pub kind: String, pub text: String }   // kind: heading1..6, paragraph, list, code, quote, rule, table, empty
pub fn plain(spans: &[Span]) -> String;
pub fn read_text_file(path: &Path) -> Result<String>;     // UTF-8 lossy, BOM dropped, CRLF normalized
pub fn parse_text(source: &str) -> Doc;                   // blank-line paragraphs, newlines as hard breaks
pub fn parse_markdown(source: &str) -> Doc;               // CommonMark + tables, strikethrough, task lists; nested lists flattened
pub fn to_html(doc: &Doc, title: &str) -> String;         // self-contained page; only http(s)/mailto links kept
pub fn to_text(doc: &Doc) -> String;
pub fn html_to_markdown(html: &str) -> Result<String>;    // htmd; drops script, style, head, nav, iframe, noscript
```

### `layout.rs`

```rust
#[serde(rename_all = "lowercase")] pub enum Orientation { #[default] Keep, Portrait, Landscape }
#[serde(rename_all = "lowercase")] pub enum Margins { Narrow, #[default] Normal, Wide }   // mm(): 10, 20, 30
#[serde(rename_all = "lowercase")] pub enum Spacing { Compact, #[default] Comfortable, Spacious } // em(): 0.6, 1.2, 2.0; twips(): 0, 160, 320
#[serde(default)] pub struct Layout { pub orientation, pub margins, pub spacing, pub page_breaks: Vec<usize> }
impl Layout { pub fn is_default(&self) -> bool; }
pub struct Rendered { pub pdf: Vec<u8>, pub pages: usize }
pub fn render_pdf(doc: &Doc, layout: &Layout) -> Result<Rendered>;
// Typst template with embedded fonts; A4; flipped for Landscape; pagebreak(weak) before indices in page_breaks (never 0).
```

### `docx.rs`

```rust
pub fn outline(source: &Path) -> Result<Vec<OutlineEntry>>;
// Body-level paragraphs (direct children of w:body or inside sdt/sdtContent/customXml), heading kinds from style ids.
pub fn rewrite(source: &Path, destination: &Path, layout: &Layout, cancel: &AtomicBool) -> Result<()>;
// Copies the zip through a temp file beside destination; edits sectPr pgSz/pgMar, inserts w:pageBreakBefore,
// sets default spacing in styles.xml (self-closing defaults handled). Never overwrites; cancel checked per entry.
```

### `job.rs`

```rust
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Action { Convert { format: OutputFormat }, Image { format: String }, Protect, Unlock, Encrypt, Decrypt }
pub struct Task { pub source: PathBuf, pub name: String, pub kind: InputKind, pub action: Action, pub page_breaks: Vec<usize> }
impl Task { pub fn layout(&self, batch: &Batch) -> Layout; }   // batch layout with this task's breaks
#[serde(default)] pub struct ImageSettings { pub max_edge: u32, pub quality: u8 }
pub struct Batch { pub tasks: Vec<Task>, pub merge: bool, pub layout: Layout, pub password: Option<SecretString>, pub protect: bool, pub image: ImageSettings }
pub enum Destination { Folder(PathBuf), File(PathBuf) }
#[serde(rename_all = "lowercase")] pub enum Status { Queued, Working, Done, Failed, Cancelled }
pub struct ItemReport { pub index: usize, pub status: Status, pub detail: String, pub output: Option<String> }

pub fn output_name(task: &Task, batch: &Batch) -> String;
// Convert "{stem}.{ext}", Image "{stem}.{format}", Protect "{stem}-protected.pdf", Unlock "{stem}-unlocked.pdf",
// Encrypt "{name}.age", Decrypt "restored-{name minus .age}".
pub fn convert_document(source: &Path, kind: InputKind, format: OutputFormat, layout: &Layout, engines: &Engines, work: &Path, tag: &str, cancel: &AtomicBool) -> Result<PathBuf>;
// Runs the route into work/{tag}.{ext} (or LibreOffice's own name) and returns it.
pub fn preview_pdf(source: &Path, kind: InputKind, target: OutputFormat, layout: &Layout, engines: &Engines, cancel: &AtomicBool) -> Result<Vec<u8>>;
// target Pdf: the conversion itself. target Docx from a non-DOCX source: real DOCX first, then that DOCX rendered to PDF.
pub fn outline(source: &Path, kind: InputKind) -> Result<Vec<OutlineEntry>>;   // DOCX paragraphs or text blocks; empty otherwise
pub struct WorkDir;                                   // %TEMP%/doc-converter-*, removed on drop, holds an exclusive `.in-use` lock file
impl WorkDir { pub fn path(&self) -> &Path; }
pub fn work_dir() -> Result<WorkDir>;
pub fn clean_stale_work_dirs() -> usize;             // skips folders whose lock another instance still holds
pub fn run(batch: &Batch, destination: &Destination, engines: &Engines, cancel: &AtomicBool, progress: impl FnMut(ItemReport)) -> Vec<ItemReport>;
// Sequential items or one merge; each change is reported through progress and returned at the end.
```

### Error strings

| Origin | Text |
| --- | --- |
| any cancel point | `Cancelled. No output was saved.` |
| `output` | `Output already exists. Choose another name.` |
| `output` | `Invalid destination` |
| `decrypt` | `Not a supported age encrypted file.` / `This build supports password-encrypted age files only.` / `Incorrect password or damaged encrypted file.` |
| `images` | `Invalid image settings.` / `This build converts static PNG, JPG, BMP, WebP and TIFF inputs.` / `Animated PNG is not supported; no frames were discarded.` / `Animated WebP is not supported; no frames were discarded.` / `Multi-page TIFF is not supported; no pages were discarded.` / `Choose PNG, JPG, WebP or PDF output.` |
| `pdf` | `Unreadable PDF: …` / `This PDF already has a password. Unlock it first, then protect it again.` / `Incorrect password or damaged PDF.` / `This PDF has no password.` / `{name} has a password. Unlock it before merging.` / `{name} has no pages.` / `Nothing to merge.` |
| `office` | `Could not start LibreOffice: …` / `LibreOffice could not convert {names} (…)` / `LibreOffice exited with {status}. …` / `LibreOffice took too long and was stopped.` |
| `capability` | `LibreOffice was not found. Install it or copy it to the engines folder, then restart.` and the per-kind reasons |
| `docx` | `Not a Word document.` / `Unreadable DOCX: …` / `DOCX part is not UTF-8.` |
| `layout` | `Layout engine error: …` / `Could not write PDF: …` |
| `job` | `A combined document needs a file name.` / `Enter a password.` / `Enter the password.` / `Could not create a temporary folder: …` |

### Tests

Sixteen; listed in [`../BUILD_AND_VERIFICATION.md`](../BUILD_AND_VERIFICATION.md).
