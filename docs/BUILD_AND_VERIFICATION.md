---
title: "Build And Verification"
description: "Toolchain, build and dev commands, tests, and what each check proves."
type: "guide"
tags:
  - build
  - testing
  - tooling
resource: "docs/BUILD_AND_VERIFICATION.md"
last_updated: "2026-09-16"
source_sync: "manual"
---

# Build And Verification

## Prerequisites

- Windows with the Rust MSVC toolchain and C++ build tools
- Node.js and pnpm
- WebView2 Runtime (present on Windows 11)

## Layout

| Path | Package | Tool |
| --- | --- | --- |
| `crates/core` | `converter-core` 0.1.0 | Cargo |
| `apps/desktop/src-tauri` | `doc-converter` 0.1.0 | Cargo, Tauri 2 |
| `apps/desktop` | `doc-converter-desktop` 0.1.0 | pnpm, Vite 7, TypeScript 5.8, React 19 |

The workspace `Cargo.toml` at the root lists both crates with resolver 2. Lockfiles for Cargo and pnpm are committed; prefer locked installs in CI.

## Production build

```powershell
cd apps/desktop
pnpm install
pnpm run build
cd ../..
cargo build -p doc-converter --features custom-protocol
```

`pnpm run build` runs `tsc --noEmit` and then `vite build` into `apps/desktop/dist`. The `custom-protocol` feature makes Tauri embed that folder and serve it from `tauri://localhost`, so the executable needs no web server. Result: `target/debug/doc-converter.exe`.

Bundling is off in `tauri.conf.json` (`"bundle": { "active": false }`), so there is no installer or signing yet.

## Development loop

Terminal 1:

```powershell
cd apps/desktop
pnpm dev
```

Terminal 2, from the repo root:

```powershell
cargo run -p doc-converter
```

Vite serves on `127.0.0.1:1420` with `strictPort`. The Tauri window loads that `devUrl`. Rust changes need a rerun; frontend changes hot-reload.

Opening `http://localhost:1420` in a browser renders the UI but cannot pick or process files.

## Tests and checks

```powershell
cargo test -p converter-core
```

Two tests in `crates/core/src/lib.rs`:

- `crypto_roundtrip_wrong_password_and_truncation` - round trip, wrong password, existing destination, truncated ciphertext
- `image_resize_and_cancel_preserve_source` - resize to 20×10, alpha flattened to white, source unchanged, cancel leaves no file

```powershell
cargo fmt --all --check
```

Formatting must be clean.

```powershell
cd apps/desktop
pnpm run build
```

This is the TypeScript check; there are no frontend unit tests.

## What is not automated

- No end-to-end test drives the Tauri window. The status doc records a manual startup smoke check only.
- No CI configuration exists in the repo.
- No lint for TypeScript beyond `tsc --strict`.
- No fixtures corpus for images or documents.

The [application plan](APPLICATION_PLAN.md) section 12 lists the release gates that should become automated checks.
