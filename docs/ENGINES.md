---
title: "Engines"
description: "How the app finds and runs LibreOffice in isolation, and which work the built-in Rust engines do instead."
type: "guide"
tags:
  - engines
  - libreoffice
  - typst
resource: "docs/ENGINES.md"
last_updated: "2026-09-16"
source_sync: "manual"
---

# Engines

Four engines do the work. Three are compiled into the Rust core; one is an external LibreOffice that the app runs headless in a private profile.

| Engine | Where it runs | Used for |
| --- | --- | --- |
| LibreOffice `soffice.exe` | Separate process, private profile, Windows job object | DOCX, ODT, PPTX, XLSX, HTML and Markdown import; PDF, DOCX, TXT, HTML and Markdown export |
| Typst (`typst`, `typst-pdf`, `typst-as-lib`) | In process, embedded fonts | Paginating Markdown and text into PDF with real page settings and page breaks |
| lopdf | In process | PDF open passwords (AES-256), unlock, merge, text extraction, image pages |
| image, webp, age | In process | Image decode/encode, WebP encoding, `.age` file encryption |

Owning files:

- `crates/core/src/office.rs` - LibreOffice detection, profile, process control
- `crates/core/src/layout.rs` - Typst template and engine
- `crates/core/src/capability.rs` - which engine handles which conversion
- `apps/desktop/src-tauri/src/main.rs` - detection thread at startup, `engine_status`

## LibreOffice detection

`OfficeEngine::detect` looks at these places in order and takes the first `soffice.exe` that exists. It never starts LibreOffice to detect it.

1. The `DOC_CONVERTER_SOFFICE` environment variable, if set.
2. Walking up from the executable's folder (five levels): `engines/libreoffice/program/soffice.exe`, then `.tools/libreoffice/program/soffice.exe`. The first is the planned bundled location; the second is the developer copy in the repo (ignored by git).
3. `%ProgramFiles%`, `%ProgramFiles(x86)%` and `%ProgramW6432%` + `LibreOffice\program\soffice.exe`.
4. Every folder on `PATH`, `soffice.exe` then `soffice`.

The version string comes from `program/bootstrap.ini` (`ProductKey=LibreOffice 26.8`). Markdown support is detected by reading `share/registry/writer.xcd` for a `Markdown` filter node flagged `IMPORT EXPORT`; that filter exists from LibreOffice 25.8.

Detection runs in a background thread at startup. Until it finishes, `engine_status` reports `ready: false` and the Convert tab treats every Office conversion as unavailable. When it finishes the backend emits `engines-ready`, then calls `warm_up`, which runs `soffice --version` once so the profile exists before the first real conversion. A cold first start with profile creation took about 20 seconds on the development machine; later conversions take 3 to 5 seconds each.

## Private profile

Every run passes `-env:UserInstallation=file:///…/lo-profile`. The folder is `%LOCALAPPDATA%\com.privateconverter.desktop\lo-profile` (Tauri's app local data directory). Consequences:

- The user's own LibreOffice, if any, is never touched. Its open windows, recent documents and settings stay separate, and killing our instance cannot kill theirs.
- Two instances can run at the same time: theirs with the default profile, ours with this one.
- The profile gets a `user/registrymodifications.xcu` on first use that sets macro security to *Very high*, disables the online update check, the tip of the day and crash reporting. LibreOffice may rewrite the file later; the values stay.
- A stale `.lock` file from a killed run is deleted before every start, so LibreOffice does not exit silently claiming another instance is running.

The `file:///` URL is percent-encoded, so profile paths with spaces or non-ASCII characters work.

## Process control

The command line is always:

```text
soffice.exe --headless --norestore --nologo --nodefault --nolockcheck --nofirststartwizard
  -env:UserInstallation=file:///… [--infilter=<name>] --convert-to <target> --outdir <dir> <inputs…>
```

- Windows `CREATE_NO_WINDOW` keeps console windows from flashing.
- stdout is discarded; stderr is read on a thread and used for error messages.
- The child is put into a Windows job object with `KILL_ON_JOB_CLOSE`. `soffice.exe` starts `soffice.bin`; terminating the job kills both. Cancellation and timeouts call `TerminateJobObject`, then `kill` as a fallback.
- The wait loop polls every 100 ms and checks the batch cancel flag.
- Timeout: 120 s plus 30 s per input file, capped at 15 minutes. Warm-up allows 180 s.

Passwords never reach this adapter. PDF protection happens afterwards in `pdf::protect`, in memory, so no secret appears on a command line or in a process list.

## Targets and filters

| Output | `--convert-to` | Notes |
| --- | --- | --- |
| PDF | `pdf` | LibreOffice picks the export filter from the document type |
| DOCX | `docx` | |
| TXT | `txt:Text (encoded):UTF8` | Forces UTF-8 |
| HTML | `html:HTML (StarWriter):EmbedImages` | Writer sources; images become data URIs inside the file |
| HTML from XLSX | `html` | Calc export; no embedding option |
| Markdown | `md:Markdown` | Only when the Markdown filter exists |

Input filters: HTML files get `--infilter=HTML (StarWriter)` so they open in Writer rather than Writer/Web, which cannot export every target. Markdown files get `--infilter=Markdown` when the filter exists.

The adapter expects `<outdir>/<stem>.<ext>` for each input. If a file is missing or empty after LibreOffice exits, the item fails with the last stderr line that mentions an error. LibreOffice exits with status 0 even when a conversion fails, so the exit code is never trusted alone.

## Built-in engines

- **Typst** compiles a fixed template with the document passed as JSON through `sys.inputs`. Only the embedded fonts (Libertinus Serif, New Computer Modern, DejaVu Sans Mono) are used, so output is identical on every machine. See [`PAGE_LAYOUT.md`](PAGE_LAYOUT.md).
- **lopdf** handles every PDF-only operation. See [`PDF_TOOLS.md`](PDF_TOOLS.md).
- **image** decodes PNG, JPEG, BMP, WebP and TIFF; the `webp` crate (libwebp) encodes lossy WebP. See [`IMAGE_CONVERSION.md`](IMAGE_CONVERSION.md).

## Developer setup for LibreOffice

No system install is needed. Download the MSI with winget and extract it with an administrative install, which needs no elevation:

```powershell
winget download --id TheDocumentFoundation.LibreOffice -e -d $env:TEMP\lo --accept-package-agreements --accept-source-agreements
msiexec /a "$env:TEMP\lo\LibreOffice_26.8.0.3_Machine_X64_msi_en-US.msi" /qn TARGETDIR="D:\projekti\PRIVATE_DOC_CONVERTER\.tools\libreoffice"
```

`.tools/` is ignored by git. The core test `office::tests::converts_text_to_pdf_when_available` uses this copy and skips with a message when nothing is found.

## Not implemented

- Bundling LibreOffice with the installer and the required licence notices (plan section 12).
- A warm, persistent LibreOffice instance or LibreOfficeKit; every conversion is a new process.
- Memory limits on the job object; only the kill-on-close flag is set.
- Sandboxing beyond the private profile and job object.
