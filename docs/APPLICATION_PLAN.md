---
title: "Application Plan"
description: "Proposed architecture, capability matrix, engines, encryption design, milestones, release gates, and licensing model."
type: "plan"
tags:
  - plan
  - architecture
  - licensing
resource: "docs/APPLICATION_PLAN.md"
last_updated: "2026-09-15"
source_sync: "manual"
---

# Doc Converter — application implementation plan

Date: 15 September 2026. Status: proposed architecture and staged delivery plan.

## 1. Recommendation and assumptions

Build a Windows-first desktop application using **Tauri 2, Rust, React and TypeScript**. Keep the processing core independent of the desktop UI so macOS/Linux ports and a CLI can be added later. Use established document engines behind narrow adapters instead of implementing Office and PDF formats from scratch.

Assumptions: local document processing, no account/login, license-key activation per machine, no cloud conversion, preserve original files, English initial UI with Unicode/Croatian document support. One license can authorize one, two or more machines (section 14). Windows x64 is the initial release target. Cross-platform packaging is a later milestone, not an automatic consequence of selecting Tauri.

The repository currently contains design artifacts and an interactive demonstration. It has no Rust application, processing engine, real pagination or production tests. Reuse its visual direction and interaction requirements; replace simulated progress, fake file records, and sample pagination.

Tauri places the UI in an OS webview and the core application in Rust. This suits a responsive document workspace with native file access. [Tauri process model](https://v2.tauri.app/concept/process-model/)

### Alternatives

| Stack | When I would choose it | Assessment for this project |
|---|---|---|
| Tauri + Rust + React/TypeScript | Cross-platform direction, web-style UI, Rust processing core | Recommended; preserve the design and isolate engine complexity |
| C#/.NET + WPF | Windows-only product and a team strongest in .NET | Strong alternative; requires rebuilding the UI in XAML and still needs conversion engines |
| Electron + TypeScript | Team prioritizes JavaScript and a bundled browser runtime | Viable; Rust can be a worker, but offers less benefit here than directly using Tauri |
| All-Rust native GUI | Avoiding a web UI is a firm requirement | Possible, but would require more work to reproduce this interface and integrate document preview |

WPF is a Windows desktop framework. Electron bundles Chromium and Node.js. These facts inform the tradeoffs; relative development effort depends on the team. [WPF overview](https://learn.microsoft.com/en-us/dotnet/desktop/wpf/overview/), [Electron introduction](https://www.electronjs.org/docs/latest)

## 2. Product scope and capability matrix

Do not advertise conversion between every pair of formats. Return available operations from the backend after file inspection and engine detection; disable unsupported choices with a reason.

| Input | Initial supported output/operation | Engine direction | Boundary |
|---|---|---|---|
| DOCX, ODT | PDF; DOCX/ODT interchange after fixtures pass | LibreOffice | Font/layout differences must be shown and tested |
| PPTX | PDF | LibreOffice Impress | Preserve slide orientation; no paragraph-reflow controls |
| XLSX | PDF | LibreOffice Calc | Print area, sheet selection and scaling need spreadsheet-specific settings |
| TXT, Markdown | PDF with editable page layout | Structured content + Typst | Known block model; controlled templates |
| PDF | Merge, select/reorder/rotate pages, protect/decrypt | qpdf | Fixed-layout document; no arbitrary paragraph reflow |
| JPG, PNG, BMP, TIFF, WebP | PNG, JPG, WebP | Rust image codecs; libwebp for lossy WebP | Begin with supported static variants; explicitly handle multi-page/animated inputs |
| Images | Individual PDFs or one combined PDF | Image placement + PDF generation | Fit/contain, page size, orientation, margins |
| Any regular file | Password-encrypted .age file; restore original bytes | Rust age library | General file protection, separate from PDF passwords |

Later: sanitized HTML import/export, PDF page raster export, OCR, reconstructed PDF-to-DOCX, HEIC/AVIF, and broader Office conversions. Do not expose them as working until their adapters and fixtures exist. PDF-to-DOCX needs a separate fidelity evaluation; text extraction alone is not a faithful converter.

### First complete release

Include the requested conversion, images, both kinds of encryption, PDF merging, portrait/landscape, actual PDF preview and manual page breaks for supported reflowable sources. For arbitrary DOCX paragraph editing, use the early feasibility gate below before committing to full support. If that gate fails, release the explicitly limited capability and schedule a dedicated editing engine evaluation.

## 3. Proposed components

| Layer | Technology | Responsibility |
|---|---|---|
| Desktop shell | Tauri 2 | Windows, dialogs, scoped IPC, installer integration |
| UI | React + TypeScript + Vite, existing CSS tokens | Queue, settings, page thumbnails, errors, progress, accessible controls |
| Preview | Locally bundled PDF.js | Render actual generated PDFs, zoom, navigation and selection overlays |
| Application core | Rust, serde, structured error types, Tokio | Job validation, orchestration, cancellation and engine adapters |
| Image worker | Rust image crate + libwebp where needed | Decode, orientation normalization, resize, encode |
| Office worker | LibreOffice headless; UNO bridge for layout changes | Import/export Office formats in a dedicated process/profile |
| PDF worker | qpdf through an isolated helper | Assemble, validate, encrypt and decrypt PDFs |
| Generated layout | Typst in a constrained worker | Paginate controlled text/Markdown/image documents |
| General encryption | age Rust crate | Stream passphrase encryption/decryption in standard .age format |
| Settings | Small versioned JSON file | Non-sensitive preferences; no database required for initial release |

PDF.js is a viewer, not the pagination engine. Typst supports page setup and page breaks. The Rust image crate's WebP encoder currently supports lossless output; lossy WebP quality needs libwebp integration. [PDF.js examples](https://mozilla.github.io/pdf.js/examples/), [Typst page setup](https://typst.app/docs/guides/page-setup/), [Typst page breaks](https://typst.app/docs/reference/layout/pagebreak/), [WebP encoder limitations](https://docs.rs/image/latest/image/codecs/webp/struct.WebPEncoder.html)

## 4. Runtime architecture

```text
React UI
  -> typed, narrowly scoped Tauri commands
Rust application core
  -> inspect inputs and resolve capabilities
  -> build immutable job recipe
  -> schedule isolated workers
       Office | image | layout | PDF | encryption
  -> validate outputs
  -> commit to destination safely
  -> emit structured progress and result events
```

File contents and arbitrary shell commands must not travel through normal UI state. The backend issues opaque input IDs after approved file selection. Commands accept IDs and typed options. Backend validation remains authoritative, including destination checks and output collisions.

Native decoders and Office converters run out of process where practical. A process boundary improves crash recovery but is not a security sandbox by itself. Add restricted Windows worker permissions, time/memory limits, no network access, and job-object ownership. Kill only the worker processes owned by this application, never the user's open LibreOffice session.

Bundle UI fonts, scripts, PDF.js assets and templates locally. No CDN, analytics or remote document assets. License activation and background validation are the only required network operations (section 14); user-invoked update checks are a separate capability. Document workers remain offline. Use Tauri capabilities and a restrictive content security policy. [Tauri capabilities](https://v2.tauri.app/security/capabilities/)

### Pipeline examples

1. DOCX to protected PDF: inspect -> import -> apply permitted layout edits -> render PDF -> preview -> encrypt final PDF -> validate -> save.
2. Merge: inspect -> convert non-PDF inputs to PDF -> choose page ranges -> arrange in queue order -> merge -> preview -> optional encryption -> save.
3. Image: inspect -> decode -> apply EXIF orientation -> resize/colour policy -> encode -> verify dimensions -> save.
4. General encryption: open source stream -> age encrypt -> finish stream -> commit encrypted output. Decryption verifies the complete stream before exposing a final restored file.

Preview/export share a recipe ID and revision. Changing settings invalidates stale results. Cancel obsolete preview requests and never show an old render as the current export. Reuse the reviewed intermediate PDF when safe instead of rerendering with different settings.

## 5. Landscape, whitespace and page breaks

### Distinguish three operations

- **Page orientation:** change page width/height and repaginate supported source content.
- **PDF rotation:** rotate an existing page; paragraph layout stays fixed.
- **Fit to a landscape sheet:** place a fixed page/image on a wider sheet with scaling and margins.

These must have distinct labels. A rotated PDF is not a reflowed landscape document.

### Reflowable sources

Maintain stable blocks: heading, paragraph, list, table, image. A block has a stable ID and source anchor. Store edits separately from the original file: pageBreakBefore, keepWithNext, spacingBefore/After, page settings and margin overrides. Undo/redo uses an edit-command history.

User interaction:

1. Select a supported content block from the source outline or its preview overlay.
2. Choose “Start on next page.”
3. Set a real page-break property; do not insert blank paragraphs.
4. Rerender through the export engine.
5. Show the new page and preserve the selected block.
6. Undo or remove the break without modifying the source file.

For text/Markdown/image composition, generate safe Typst content from the typed model. Do not interpret untrusted text as executable Typst markup. Restrict template filesystem access and package lookup. For long content, handle oversized tables/images and keep-with-next rules without silently clipping content.

### DOCX/ODT preservation path

Evaluate LibreOffice UNO on a copied document: enumerate paragraphs, anchor source blocks, update page styles, insert paragraph breaks and export. UNO exposes paragraph break and page properties, but robust mapping from rendered PDF coordinates back to source paragraphs is a separate engineering problem. [Paragraph properties](https://api.libreoffice.org/docs/idl/ref/servicecom_1_1sun_1_1star_1_1style_1_1ParagraphProperties.html), [Page properties](https://api.libreoffice.org/docs/idl/ref/servicecom_1_1sun_1_1star_1_1style_1_1PageProperties.html)

Start with a selectable source outline beside the real preview. Add click-on-page selection only when block-to-page mapping is reliable. An isolated small Python/UNO helper may be more practical than Rust UNO bindings; permit this implementation detail while keeping the app core in Rust. Package/runtime compatibility must be proven on Windows before choosing the bridge.

Test floating images, section breaks, headers/footers, lists and tables during the first spike. Restrict unsupported edits instead of rebuilding an arbitrary Word file from extracted text and silently losing its layout.

### Existing PDF and scans

Offer page operations and preserved-layout merge. Do not offer paragraph whitespace editing by default. An optional later “reconstruct for editing” operation needs extraction/OCR, reading-order recovery and explicit review; it will not preserve every layout.

## 6. Combining documents

First merge target: **one PDF**. Support documents and images through conversion to PDF, with explicit page order and ranges. Preserve differing page sizes/orientations by default; offer normalization as an intentional setting.

Add output name, drag reorder and keyboard move controls, per-document page ranges, estimated page count, duplicate handling, and optional bookmark per source. Specify bookmark, annotation, form and attachment preservation in tests; avoid promising that qpdf page assembly preserves every document-level feature. Detect signed PDFs and explain that a transformed output cannot retain the original signature's validity.

For a combined output, stop on a failed included input. Let the user explicitly exclude it and rerun. Separate-output jobs may complete successful files and report failures individually. Never silently omit a source from a combined document.

## 7. Encryption design

### PDF passwords

Use qpdf AES-256 encryption with a non-empty document-open password and a separate generated strong owner password. Do not offer legacy weak encryption for new output. Copy/print permission flags are reader-enforced controls and must not be presented as equivalent to protecting the document-open key. [qpdf encryption](https://qpdf.readthedocs.io/en/stable/encryption.html), [qpdf weak cryptography](https://qpdf.readthedocs.io/en/stable/weak-crypto.html)

Use a narrow helper/library interface with secrets passed through a private IPC channel. Never include passwords in process arguments, debug logs or ordinary job JSON files. Verify the exact helper API during the engine spike.

### General files

Replace the prototype's invented `.dcenc` format with **standard `.age` files**, using the Rust age library's passphrase mode. This avoids designing cryptographic framing/KDF rules and allows interoperability with age-compatible tools. [Rust age Encryptor](https://docs.rs/age/latest/age/struct.Encryptor.html), [rage project](https://github.com/str4d/rage)

Encrypt each file independently initially; no custom archive wrapper. Default output could be `report.docx.age`, which reveals the outer filename. Offer a generic filename when privacy of the name matters. Restoring original bytes is guaranteed after full authenticated decryption; original filenames/metadata are not guaranteed by an invented embedded manifest. Folder encryption can later use a well-defined archive inside age with safe extraction rules.

Passwords are session-only, never saved in settings/history or telemetry. Clear UI fields promptly; keep backend secrets short-lived and use secret wrappers/zeroization where available, without promising that every runtime copy can be erased. Stream large files and require stream completion. On wrong password, corruption, truncation or cancellation, remove partial output and never publish it as a valid restored file.

Private per-job temporary directories are unavoidable for some preview/conversion engines. Restrict access, clean them on completion and next startup, and explain that deletion does not guarantee secure erasure on SSDs. Do not claim decrypted content never touches disk.

## 8. Images

Implement dimensions/format inspection before decoding. Enforce decoded-pixel, frame-count, memory and file-size limits. Apply EXIF orientation before resizing; preserve aspect ratio and never upscale unless selected.

JPG: quality and explicit background colour for alpha flattening. PNG: lossless output; compression is distinct from visual quality. WebP: explicit lossy/lossless choice with the correct encoder. Preserve alpha when supported. Convert colour to a documented sRGB policy; establish tests for CMYK JPEG and ICC profiles before advertising colour fidelity. Strip location/other metadata by default after orientation/colour conversion, with a reviewed metadata-preservation option later.

TIFF multi-page and animated WebP must be detected. Until implemented, reject them with a clear explanation or require an explicit first-frame selection; never silently drop frames. Image-to-PDF offers A4/Letter, landscape/portrait, fit/contain, margins and source ordering.

## 9. Jobs, persistence and errors

Core types:

```text
InputAsset: id, backend path, detected format, size, fingerprint, capabilities
JobRecipe: id, revision, ordered inputs, operations, engine requirements,
           layout settings, image settings, destination policy, ephemeral secret handle
LayoutEdit: source ID, block anchor, edit type, value
Artifact: job/revision, private path, media type, page count, verification state
JobEvent: job/revision, stage, completed units, total units if known, safe message
```

State machine: queued -> inspecting -> awaiting input (if needed) -> processing -> validating -> saving -> complete. Failed/cancelled are terminal alternatives. Engine stages without measurable progress show an indeterminate bar and file/stage count, not invented percentages. Cancellation has a defined safe boundary and cleans partial files.

Use bounded workers; start with one Office conversion and a small memory-budgeted image pool. Cache previews per content fingerprint + recipe + engine version + fonts, in session scope by default. Source changes invalidate caches.

Settings JSON contains theme/window preferences and non-sensitive presets. History and project saving are deferred or explicit opt-ins because filenames and layout edits can expose document content. Never resume encryption automatically after a crash using a saved secret.

Errors should identify source, stage and recovery: missing engine, unsupported format, password required/incorrect, malformed file, missing fonts, destination denied, disk full, source changed, timeout, unsupported colour/frame type. Output saving uses exclusive creation or safe temporary-write-and-commit semantics; preserve originals and handle collisions without overwriting by default.

## 10. Repository structure

```text
apps/desktop/                 React UI, generated TS API types, UI tests
apps/desktop/src-tauri/       Tauri bootstrap, capabilities and native commands
crates/core/                 Jobs, capabilities, recipes and domain errors
crates/engine-office/        LibreOffice adapter and worker protocol
crates/engine-pdf/           qpdf adapter
crates/engine-image/         Codecs and resize pipeline
crates/engine-layout/        Source blocks, edits, Typst rendering
crates/engine-crypto/        age and secret lifecycle
crates/worker/               Worker executable and resource controls
tools/office-bridge/         UNO helper if the spike validates it
resources/                  Fonts, templates and engine notices
tests/fixtures/              Synthetic documents, corrupt files and expected results
docs/                       Architecture decisions, format matrix, release procedures
design/ and opendesign/     Existing references retained
```

Begin with a small number of crates and split only at real process/module boundaries; the tree describes ownership rather than a requirement to scaffold everything immediately.

## 11. Implementation milestones

Estimates are planning ranges for one experienced full-time developer with periodic design/QA support. They are not delivery commitments. Re-estimate after the first spike; unfamiliar Office automation and advanced editing can materially increase effort.

| Phase | Work | Exit criterion | Estimate |
|---|---|---|---|
| 0. Feasibility | Windows Tauri shell; real DOCX/XLSX/PPTX PDF export; qpdf password IPC; age round trip; UNO paragraph-break experiment; font and packaging review | A recorded capability matrix and representative output corpus; decide supported DOCX editing scope | 1–2 weeks |
| 1. Foundation | Port UI, typed IPC, actual file queue, destination handling, workers, cancellation, safe output writes | Real files move through a job lifecycle; cancellation leaves sources intact | 1–2 weeks |
| 2. Document/PDF engine | Office adapters, PDF preview, merge/reorder/ranges, orientation semantics | Combined PDF matches ordered inputs; preview shows actual output | 2–3 weeks |
| 3. Image engine | Format conversion, resize, alpha, orientation, quality, image-to-PDF | Image corpus passes dimensional/visual checks and frame limits | 1–2 weeks |
| 4. Encryption | age streaming, PDF protect/decrypt, secret IPC, failure cleanup | Cross-tool round trips; corruption and wrong-password cases fail safely | 1–2 weeks |
| 5. Layout editor | Text/Markdown blocks, margins/spacing, break/undo, actual repagination; bounded DOCX route if viable | Preview/export agree and no supported content is clipped or lost | 3–5 weeks |
| 6. Release hardening | Installer, engine distribution, offline test, accessibility, security/resource tests, crash recovery | Reproducible signed Windows build passes clean-machine acceptance | 2–3 weeks |

Total planning range: **11–19 developer-weeks**, plus contingency and external review. Full arbitrary-PDF editing, high-fidelity PDF-to-Word, OCR and additional OS releases are separate projects. The basic conversion/image/encryption build can be useful before the full layout editor ships.

## 12. Verification and release gates

- Unit tests: capability selection, source order, layout edit application, collision handling, safe destination validation and error mapping.
- Engine integration: compare output types, page counts, extracted text, visible layout and source immutability. Do not rely on exit code alone.
- Crypto: byte-identical restore, independent age-tool compatibility, qpdf/reader interoperability, Unicode passwords, empty and large files, wrong passwords, truncated/tampered streams, cancellation and disk-full cleanup.
- Layout: short and long paragraphs, tables crossing pages, images, headings with following text, Croatian characters, mixed orientation, missing fonts, manual breaks, undo and export/preview revision consistency.
- Images: EXIF orientation, transparency, ICC/CMYK cases, resize bounds, large dimensions, malformed files, multi-frame detection, JPG/WebP quality behaviour.
- Worker robustness: malformed documents, timeout, crash, application close, stale temporary folders, symbolic-link/reparse-point paths, locked files and resource limits.
- UI: keyboard navigation, high-DPI scaling, 200% text/zoom, accessible controls, narrow windows, large queues and genuine progress states.
- Privacy: no document uploads or network requests outside the explicit licensing/update policy, no secrets in process arguments/logs/settings, imported HTML/PDF cannot execute privileged UI actions.

Release target: clean Windows machine without developer tools; offline startup/conversion within the signed license validity window after activation and installation of all required engine components; explicit detection and repair guidance for missing engines.

Tauri supports Windows installer packaging and WebView2 installation modes. Choose an offline-capable distribution route and test WebView2 provisioning as part of release. [Tauri Windows installer](https://v2.tauri.app/distribute/windows-installer/)

Office engine packaging is likely to dominate installation size and maintenance. For the first spike detect an installed LibreOffice. Before beta choose and document either a managed engine distribution with required notices, or a clearly explained prerequisite installer. Pin and validate engine versions; do not download executables silently. Review the exact redistributable binaries, licences and fonts before shipping, and maintain dependency notices and vulnerability tracking.

## 13. First executable slice

Start with **DOCX -> actual PDF preview -> AES-256 protected PDF -> save**, alongside a minimal **file -> .age -> original bytes** round trip. Add a JPEG resize conversion to validate the Rust image path. At the same time, prove that a real Word paragraph can be moved to a new page and previewed without damaging the document.

This establishes the engines and the hardest layout constraint before implementing every control in the prototype. Proceed to queue/merge/image workflows only after these gates demonstrate reliable output on Windows.

## 14. Licensing and one-time activation without login

### 14.1 Recommended commercial model

Use a **license key with a maximum number of activated machines**. The user enters a key once on each machine; there is no username, password, account creation or routine login. The application validates its saved authorization locally and periodically refreshes it online in the background.

| License | Machine allowance | Example |
|---|---|---|
| Personal | 1 activated machine | One desktop |
| Duo | 2 activated machines | Desktop and laptop |
| Team | Configurable N machines | 5, 10, 25 or a purchased quantity |

These are installed-device seats, not concurrent-use seats. Closing the app does not free a seat. Multiple windows on the same machine do not consume additional seats. Initial implementation should explicitly define Windows-user scope: either provision one shared machine activation securely during installation or clearly limit support to the activating Windows user. Product intention is one seat per machine, not per Windows login.

Recommended initial business policy: perpetual use of the purchased major version, with a separate upgrade entitlement. Seat count, supported product/version and optional support/upgrade expiry are independent fields. A perpetual purchase can still require periodic validation; this requirement must be explained before purchase. Subscription expiry can be supported in the schema without making subscriptions the initial product model.

### 14.2 What “activate once” means

**Default proposal: one manual activation, then automatic checks.** This matches the requested absence of online login while allowing seat transfers and revocation.

Alternative for customers requiring no internet after activation: issue a non-expiring, machine-bound signed certificate. The app can then run offline permanently, but cannot learn about later revocation or a seat moved elsewhere. It is impossible to guarantee immediate seat enforcement on computers that never contact the server. Offer this only as an explicit offline licence policy, not an undisclosed fallback from the normal policy.

All timings and commercial limits below are proposed configurable defaults, not final sales terms.

### 14.3 User activation flow

1. Purchase produces a high-entropy license key supplied by checkout/email. Delivery does not require an app account.
2. On first use, show “Activate this computer,” a license-key field and optional friendly device name. Explain that licensing contacts the server but documents stay local.
3. Rust generates a device keypair and derives a stable, product-scoped machine binding.
4. Send the license key, device public key/binding, client version and an idempotency token over HTTPS to the activation endpoint. No documents, file paths, document passwords or document metadata are included.
5. The service validates the license, entitlement and machine quota. In one database transaction, reuse an existing matching activation or allocate one seat.
6. Return a signed machine authorization and an activation-specific credential. Display “Activated — 1 of 2 computers in use,” for example.
7. Store authorization and protected device credentials locally. Discard the full license key unless a documented protected-storage requirement exists; display only a masked suffix.

If the network response is lost, retry with the same idempotency token. Activating the same installation repeatedly must not consume additional seats. Quota enforcement must be atomic so two computers racing for the final seat cannot both succeed.

### 14.4 Local authorization and online refresh

Proposed defaults:

| Setting | Default behaviour |
|---|---|
| Startup | Verify the cached signed authorization immediately; do not block launch on a network request while valid |
| Background check | Refresh if last success is older than 24 hours, including while the app stays open |
| Local validity | 30 days from the last successful server authorization |
| Reminder | Non-blocking reconnect reminder during the final 7 days |
| Offline/server failure | Keep working until the existing signed validity ends; failed requests do not extend it |
| Retry | Exponential backoff with jitter and a manual “Check now”; short bounded network timeouts |
| Expired authorization | Require a successful refresh before starting new paid conversion/encryption jobs |
| Existing job | Complete an already-authorized job and save its output if expiry occurs during processing |

The 30-day validity window is the offline grace allowance; do not accidentally implement an extra 30 days after it expires. Online refresh is silent when successful and never asks the user to log in or re-enter the original key.

Signed authorization fields: schema version, issuer, product ID, license ID, activation ID, device-key binding, machine-binding policy/version, seat limit snapshot, enabled features, allowed app major versions, issued-at, next-check-at, valid-until and signing-key ID. Never embed document-encryption secrets. Validate signature, issuer/product, binding and validity in Rust before permitting paid jobs. Do not trust an editable `activated=true` setting or UI-only checks.

Use the licensing provider's supported signature verifier, or a fixed standard signature scheme such as Ed25519 with a reviewed implementation if building the service. Signing private keys exist only on the service in protected key storage; the app carries public verification keys. Plan overlapping key rotation. Reject unknown algorithms, malformed payloads, mismatched devices and invalid signatures. Bind renewals to the activation credential and device proof, using a challenge/nonce to prevent replay.

Store last trusted server time and detect suspicious clock rollback. Use monotonic time within a running session. Clock-change handling can request a refresh rather than silently expanding the validity window. Fully preventing clock rollback or VM snapshot replay on an offline user-controlled computer is not achievable with ordinary software checks.

### 14.5 Revocation, expiry and safe access to documents

Differentiate an explicit authenticated server decision from a connection failure:

- Timeout, DNS error, TLS failure, rate limit or server 5xx: temporary inability to check; continue under the cached validity window.
- Verified revoked/refunded/deactivated response: invalidate paid authorization and explain the reason with support guidance.
- Version not entitled: offer the last entitled app version or purchase upgrade; do not mislabel as a broken license.
- Local expiry: request reconnection, retaining the user's settings and files.

**Keep decryption and document recovery available even when the license expires or is revoked.** A customer must not lose access to their own encrypted documents because the licensing server is unavailable. Allow decrypt-to-new-copy with the correct document password, viewing already generated local results and saving work from already-authorized jobs. Do not watermark or alter files as punishment for a licensing error. The license key is completely separate from document encryption keys/passwords.

An offline revoked machine can continue using its cached paid authorization until expiry, at most 30 days under this proposal. A shorter lease improves revocation speed but reduces offline tolerance. Transfers may therefore temporarily overlap; document this limit honestly.

### 14.6 Machine identity and license transfers

Use a stable product-scoped machine fingerprint plus a generated device keypair; do not bind to IP address, hostname or a single network adapter. Hashing identifiers makes them pseudonymous, not anonymous. Avoid sending raw hardware serial numbers. Store device private keys/credentials with OS protection (Windows DPAPI or an equivalent protected key store); TPM-backed keys can be evaluated later. Do not assume a random file on disk alone is sufficient machine binding.

Machine binding should tolerate normal OS updates and reasonable hardware changes. Reinstall or motherboard replacement may require controlled reactivation; do not silently consume a new seat every time. Restored/cloned installations and VMs require explicit handling, with the limitation that software-only anti-cloning is imperfect.

In Settings → License, show masked key, licensed version, seat count, this device name, last successful check, offline-valid-until and buttons for “Check now” and “Deactivate this computer.”

Transfer flow: deactivate online on the old computer -> server releases seat -> local credentials are removed -> activate on the new computer. Local file deletion alone does not release a server-side seat. Uninstall should offer deactivation but must not imply that it happened if offline.

For lost/broken computers, use a purchase-email one-time management link or support-assisted reset. This is an exceptional seat-management action, not a login requirement to use the app. Possession of a shared team license key alone should not permit silently removing other team devices. Apply audited, configurable reset rate limits and provide a support override for legitimate failures. Requests should be authenticated and should not expose a device list to someone guessing keys.

### 14.7 Service/provider architecture

Prefer evaluating an established licensing service before implementing a custom one. Keygen supports machine activation and signed offline machine/license files; Cryptlex offers node-locked activation and an SDK that handles periodic sync and offline grace. Check current Rust/FFI integration, commercial terms, export/migration options and offline policies during a short spike. These are candidates, not purchased services. [Keygen machine activation](https://keygen.sh/docs/activating-machines/), [Keygen offline licensing](https://keygen.sh/docs/choosing-a-licensing-model/offline-licenses/), [Cryptlex node-locked licensing](https://cryptlex.com/docs/licensing-models/node-locked-licenses), [Cryptlex SDK overview](https://cryptlex.com/docs/sdks-and-apis/overview)

Expose licensing through a `LicenseService` interface in Rust so the rest of the app does not depend on a provider-specific API. Never embed vendor administrator credentials in the desktop application.

If a custom service is justified, use a small Rust/Axum service with PostgreSQL and protected signing keys. Proposed API operations:

```text
POST /v1/activations             Redeem key and allocate/reuse a machine seat
POST /v1/activations/refresh     Authenticate device and issue a fresh signed authorization
POST /v1/activations/deactivate  Revoke this activation and release its seat
POST /v1/license-management     Request purchase-email management link
```

Tables: licenses (key digest, product, status, seat limit, version entitlement), activations (license, device binding/public key, status, last check), idempotency records, purchase events and administrative audit events. Store a keyed digest of high-entropy license keys for lookup; avoid plaintext keys in logs/backups. Keep administrative endpoints separate with strong operator authentication. No-account users do not mean an unauthenticated admin system.

Purchase integration uses verified, idempotent payment webhooks: successful payment issues entitlement; extra-seat purchase updates the same license; refund/revocation follows documented policy. Do not trust a checkout success page to grant a license. Maintain backups, service monitoring, signing-key rotation and an incident recovery runbook.

Licensing communicates from the Rust core to narrowly allowed HTTPS endpoints. Document workers remain network-isolated. Record only necessary licensing events and minimize IP/log retention. Online licensing must be disclosed separately from “documents processed locally.”

For sustained provider outage, plan how the business will distribute signed emergency extensions without shipping a universal bypass key. For product sunset, define a final offline authorization/recovery policy so legitimate customers are not permanently dependent on an abandoned service.

### 14.8 Implementation and verification additions

Add `crates/licensing/` for cached verification, activation, refresh, state transitions and secret storage; add a License screen and startup status indicator to the UI. Keep entitlement enforcement at the Rust job-submission boundary, with an explicit recovery/decryption exception. Do not mix licensing with content encryption code.

Tests must include: one/two/N-seat limits; simultaneous final-seat activation; duplicate/retried activation; token tampering and wrong-device copies; expired or wrong-version entitlements; routine hardware/OS changes; offline startup; DNS/TLS/5xx/rate-limit failures; signed revocation; clock rollback; key rotation; interrupted deactivation; seat upgrades; lost-machine recovery; and decryption without a paid authorization.

Add a licensing feasibility spike to phase 0 and complete integration before release hardening. Planning allowance: **2–4 additional developer-weeks with a hosted provider**, giving **13–23 weeks** for the expanded app scope. A custom licensing/purchase/admin service may require **4–8 additional weeks instead**, giving **15–27 weeks**, with ongoing operational responsibilities. These ranges need revision after provider selection; hosting/licensing fees and payment processing are separate costs.

