---
title: "Frontend"
description: "Catalog of TypeScript types, invoke contracts, component state, and derived flags in the React UI."
type: "catalog"
tags:
  - methods-report
  - typescript
  - react
resource: "docs/methods_report/03_frontend.md"
last_updated: "2026-09-16"
doc_version: "1.0.0"
source_sync: "manual"
---

# Frontend

Package `doc-converter-desktop`, file `apps/desktop/src/main.tsx`. Dependencies: `react 19`, `react-dom 19`, `@tauri-apps/api 2`. Dev: `vite 7`, `typescript 5.8`, `@tauri-apps/cli 2`.

### `apps/desktop/src/main.tsx`

```typescript
type Asset = { id: string; name: string; bytes: number };
type Operation = 'image' | 'encrypt' | 'decrypt' | 'documents' | 'license';

function App(): JSX.Element; // the whole UI; mounted under React.StrictMode into #root
```

Invoke contracts as called from the UI:

```typescript
invoke<Asset[]>('pick_files');
invoke<string>('process_file', {
  id: string,
  operation: Operation,   // backend accepts image | encrypt | decrypt
  password: string,       // '' for image
  format: string,         // 'png' | 'jpg'
  maxEdge: number,        // 0 | 1920 | 1280 | 640
  quality: number,        // 1..100
});
invoke('cancel_job');
```

Component state:

```typescript
files: Asset[]           // queue rows
selected: string         // id of the radio-selected row, '' for none
operation: Operation     // active tab, default 'image'
password: string
confirm: string
show: boolean            // password visibility for both fields
format: string           // 'png' default
edge: number             // 0 default
quality: number          // 85 default
busy: boolean            // true from run() start to finish
message: string          // green notice
error: string            // red alert
```

Derived values:

```typescript
desktop = isTauri();
file = files.find(f => f.id === selected);
supported =
  operation === 'image'   ? /\.(png|jpg|jpeg|bmp)$/i.test(file?.name || '') :
  operation === 'decrypt' ? /\.age$/i.test(file?.name || '') :
  operation === 'encrypt';
valid = !!file && supported && (
  operation === 'image' ||
  (operation === 'decrypt' ? password.length > 0
                           : [...password].length >= 12 && password === confirm));
```

Handlers:

```typescript
changeOperation(next: Operation): void;
// sets operation; clears password, confirm, message, error

add(): Promise<void>;
// pick_files; appends; selects the first new asset; error -> setError

run(): Promise<void>;
// busy=true; message = 'Choose a new output filename in the save dialog.';
// process_file -> message; catch -> error, message=''; finally clears passwords, busy=false
```

Tab list, in order: `image` Images, `encrypt` Encrypt, `decrypt` Decrypt, `documents` Documents, `license` License.

### `apps/desktop/src/style.css`

Single stylesheet, no variables. Selectors of note: `header`, `.brand`, `.eyebrow`, `nav` and `nav .active`, `.workspace` grid, `.queue`, `aside`, `.placeholder`, `.empty`, `.badge`, `.password`, `.action`, `.primary`, `.notice`, `.error`, `footer`, and one `@media (max-width: 850px)` block.

### `apps/desktop/package.json`

```json
"dev":   "vite --host 127.0.0.1 --port 1420 --strictPort"
"build": "tsc --noEmit && vite build"
"tauri": "tauri"
```
