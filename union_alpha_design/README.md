# Union Alpha design concept

An alternative UI for Doc Converter, built as a standalone prototype. Open
`index.html` in a browser, or serve the folder and browse to it. Nothing here
touches the app, `design/` or `opendesign/`.

## What changed against the accepted mockup

- Sidebar navigation replaces top tabs. The five operations gain a persistent
  privacy card and an about dialog; the License placeholder stays out.
- The workspace splits into three cards: files, an illustrative preview, and
  output settings. The old design interleaved settings with the queue.
- Output formats are tile buttons with a select fallback, per-row outputs stay,
  and merge protects the batch format instead of hiding it.
- Page layout, margins and watermark moved into a collapsed advanced panel.
  The preview is one sample page with portrait/landscape and margin changes,
  not a pagination demo.
- Encrypt merges the two protection types behind one select; Decrypt drops its
  confirmation field.
- Validation lives next to the action button as a single status line.

## Keeping the identity

The green palette, IBM Plex with Segoe UI/Consolas fallbacks, warm neutrals and
the file-queue-plus-settings structure survive. So do the invariants: local
processing, untouched originals, no network requests, no persisted state.

## Boundaries

Demonstrative only. Added files contribute a name and a size; contents are
never read. No conversion, encryption, cleaning, saving or uploads happen.
Eligibility is illustrative and not the application's Rust capability matrix.
Passwords typed here stay in memory and are cleared when switching tools or
finishing a run. Recipient-key encryption, PDF standards validation, page
breaks and merge reordering are outside this concept.

## Verification

`node verify.mjs` (requires a Chromium; the script uses the installed Chrome
via Playwright) checks selection, per-row and batch formats, merge lock and
restore, watermark text, password mismatch, all five tools, eligibility
skips, simulated completion and cancellation, an HTML-tagged filename staying
text, the dialog, and seven viewport widths without horizontal overflow. Last
run passed with no external requests and no browser errors on 2026-09-17.
