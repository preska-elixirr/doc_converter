---
title: "Frontend"
description: "Catalog of TypeScript types, invoke contracts, the translation module, component state, derived flags, and the PDF preview component."
type: "catalog"
tags:
  - methods-report
  - typescript
  - react
resource: "docs/methods_report/03_frontend.md"
last_updated: "2026-09-16"
doc_version: "2.1.0"
source_sync: "manual"
---

# Frontend

Package `doc-converter-desktop`, folder `apps/desktop/src/`. Dependencies: `react 19`, `react-dom 19`, `@tauri-apps/api 2`, `pdfjs-dist 6`. Dev: `vite 7`, `typescript 5.8`, `@tauri-apps/cli 2`.

### `api.ts`

```typescript
type InputKind = 'pdf' | 'docx' | 'odt' | 'pptx' | 'xlsx' | 'md' | 'html' | 'txt' | 'png' | 'jpg' | 'bmp' | 'webp' | 'tiff' | 'age' | 'other';
type OutputFormat = 'pdf' | 'docx' | 'txt' | 'html' | 'md';
type ImageFormat = 'png' | 'jpg' | 'webp' | 'pdf';
type Mode = 'convert' | 'images' | 'encrypt' | 'decrypt' | 'license';
type Availability = { format: OutputFormat; available: boolean; engine: string; reason: string | null };
type Asset = { id: string; name: string; bytes: number; kind: InputKind; label: string; outputs: Availability[];
               pdf: { pages: number; encrypted: boolean } | null; image: { kind: InputKind; width: number; height: number } | null };
type Orientation = 'keep' | 'portrait' | 'landscape';
type Margins = 'narrow' | 'normal' | 'wide';
type Spacing = 'compact' | 'comfortable' | 'spacious';
type Layout = { orientation: Orientation; margins: Margins; spacing: Spacing; page_breaks: number[] };
type OutlineEntry = { index: number; kind: string; text: string };
type Status = 'queued' | 'working' | 'done' | 'failed' | 'cancelled';
type ItemReport = { index: number; status: Status; detail: string; output: string | null };
type EngineStatus = { ready: boolean; office: { path: string; version: string; markdown: boolean } | null };
type BatchRequest = { mode: Exclude<Mode, 'license'>; items: { id: string; format?: string; page_breaks?: number[] }[];
                      merge: boolean; merge_name: string; layout: Layout; password: string; protect: boolean;
                      encryption: 'pdf' | 'file'; image: { max_edge: number; quality: number } };
type BatchOutcome = { result: 'saved' | 'cancelled' | 'nothing'; reports: ItemReport[] };
type AssetOutputs = { id: string; outputs: Availability[] };

const desktop: boolean;   // isTauri()
const api = {
  pickFiles(): Promise<Asset[]>,                 // invoke('pick_files')
  addPaths(paths: string[]): Promise<Asset[]>,   // invoke('add_paths', { paths })
  engineStatus(): Promise<EngineStatus>,         // invoke('engine_status')
  runBatch(request: BatchRequest): Promise<BatchOutcome>,   // invoke('run_batch', { request })
  cancel(): Promise<void>,                       // invoke('cancel_job')
  outline(id: string): Promise<OutlineEntry[]>,  // invoke('outline', { id })
  preview(id: string, format: string, layout: Layout): Promise<ArrayBuffer>,  // invoke('preview', { id, format, layout })
  refreshOutputs(ids: string[]): Promise<AssetOutputs[]>,      // invoke('refresh_outputs', { ids })
};
function formatBytes(bytes: number): string;     // "1.2 MB" or "640 KB"
```

### `i18n.ts`

```typescript
type Language = 'en' | 'hr';
const LANGUAGES: { code: Language; label: string }[];      // English, Hrvatski
type Key = keyof typeof en;                                // every UI string; `hr` must cover the same keys
function pluralIndex(language: Language, count: number): number;   // en: 0 for 1, else 1; hr: one, few, many
function translate(language: Language, key: Key, vars?: Record<string, string | number>): string;   // fills {name}
function translatePlural(language: Language, key: Key, count: number, vars?): string;              // picks the `|` form, fills {count}
function translateDetail(language: Language, detail: string): string;   // known backend status texts, else unchanged
function detectLanguage(): Language;                       // navigator.language starting with "hr" => 'hr'
```

### `App.tsx`

```typescript
type RowState = 'ready' | Status;
type Row = Asset & { selected: boolean; target: OutputFormat; imageTarget: ImageFormat; state: RowState; detail: string; output?: string };
type Protection = 'pdf' | 'file';
type ResultBar = { title: string; detail: string; done: number; total: number; finished: boolean };
// title is a code: 'choose' | '' (progress) | 'save_cancelled' | 'stopping' | 'could_not_start' | 'done' | 'stopped' | 'nothing';
// detail carries the numbers, so a language change re-renders the bar.

const SCALES = [0.9, 1, 1.1, 1.25, 1.5];
const LANGUAGE_PREF = 'doc-converter-language';   // localStorage keys, read and written in try/catch
const SCALE_PREF = 'doc-converter-scale';

function defaultTarget(asset: Asset): OutputFormat;
function toRow(asset: Asset, imageTarget: ImageFormat): Row;
function blocked(row: Row, mode: Mode, protection: Protection): Key | null;   // reason key or null
function statusText(row: Row, reason: Key | null, t: T, language: Language): { text: string; className: string };
function ShieldIcon(): JSX.Element;
export function App(): JSX.Element;
```

Component state, in addition to the batch state listed below: `language: Language` (saved preference, else browser locale), `scale: number` (saved preference, else 1), `prefsOpen: boolean`.

```typescript
mode: Mode; rows: Row[]; protection: Protection; password, confirm: string; show: boolean
batchFormat: OutputFormat; protect: boolean; merge: boolean; mergeName: string
orientation: Orientation; margins: Margins; spacing: Spacing; breaks: Record<string, number[]>
imageFormat: ImageFormat; imageQuality: number (85); imageSize: number (0)
previewId: string; outline: OutlineEntry[]; previewData: ArrayBuffer | null
previewState: 'idle' | 'loading' | 'ready' | 'error'; previewError: string; pageCount: number
busy: boolean; result: ResultBar | null; error: string; engine: EngineStatus | null; dragging: boolean
batchIds: useRef<string[]>; prefsRef: useRef<HTMLDivElement>
```

Derived values:

```typescript
t = (key, vars) => translate(language, key, vars);  tn = (key, count, vars) => translatePlural(language, key, count, vars)
merging = mode === 'convert' && merge
outputFor(row) = merging ? 'pdf' : mode === 'images' ? row.imageTarget : row.target
selected = rows.filter(r => r.selected && !blocked(r, mode, protection))
needsPassword = encrypt || decrypt || (convert && protect && (merging || some selected row targets pdf))
passwordValid = !needsPassword || (decrypt ? password.length > 0 : [...password].length >= 12 && password === confirm)
targetsValid = mode !== 'convert' || merging || every selected row can reach its target
valid = desktop && selected.length > 0 && passwordValid && targetsValid && (!merging || mergeName.trim())
layoutVisible = convert && (merging || some selected row targets pdf or docx)
previewCandidates = selected rows with pdf/docx target (all when merging)
layoutFor(id) = { orientation, margins, spacing, page_breaks: breaks[id] ?? [] }
```

Effects:

- language: sets `document.documentElement.lang`, saves the preference
- scale: saves the preference, then `getCurrentWebview().setZoom(scale)` in the app or CSS `zoom` in a browser
- prefs panel: closes on an outside `mousedown` or Escape
- on mount (desktop only): `engineStatus` (which also calls `refreshOutputs` for queued rows once engines are ready), listeners for `engines-ready` and `batch-progress`, `onDragDropEvent`
- keep `previewId` inside `previewCandidates`; load the outline; request a preview for the row's target format 700 ms after a layout or format change unless busy

Handlers: `switchMode(next)`, `add()`, `update(id, patch)`, `toggleBreak(id, index)`, `move(id, delta)`, `run()`, `resultText(bar)`.

### `Preview.tsx`

```typescript
type PreviewLabels = { empty: string; error: string; page: (number: number) => string; more: (count: number) => string };
export function PdfPreview(props: { data: ArrayBuffer | null; labels: PreviewLabels; onPages?: (pages: number) => void }): JSX.Element;
// PDF.js with the bundled worker (pdf.worker.min.mjs?url). Renders up to 40 pages as canvases 380 CSS px wide,
// scaled by devicePixelRatio; reports numPages; labels are read through a ref so a language change does not re-render the PDF.
```

### `main.tsx`

Mounts `<App/>` under `React.StrictMode` into `#root` and imports `style.css`.

### `apps/desktop/package.json`

```json
"dev":   "vite --host 127.0.0.1 --port 1420 --strictPort"
"build": "tsc --noEmit && vite build"
"tauri": "tauri"
```

`vite-env.d.ts` references `vite/client` for the `?url` asset import.
