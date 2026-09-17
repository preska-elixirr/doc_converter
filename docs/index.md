---
title: "Documentation Index"
description: "Discovery map for Doc Converter architecture, feature, API catalog, design, and planning documentation."
type: "index"
tags:
  - documentation
  - index
  - okf-lite
resource: "docs/index.md"
last_updated: "2026-09-17"
source_sync: "manual"
---

# Documentation Index

Use this file as a discovery map, not as a required reading list. Start with `PROJECT_CONTEXT.md` for architecture and invariants, then open only the feature or catalog document relevant to the task.

## Start Here

- [Project Context](PROJECT_CONTEXT.md) - agent entry context for architecture, invariants, ownership, and pitfalls.
- [API & Method Catalog](methods_report/README.md) - compact exported API and command catalog.
- [Implementation Status](IMPLEMENTATION_STATUS.md) - what the second milestone delivered and verified.

## Architecture And Runtime

- [Engines](ENGINES.md) - LibreOffice detection, private profile, job object, and the built-in Typst, lopdf and image engines.
- [Job Lifecycle](JOB_LIFECYCLE.md) - batches, destinations, the work folder, progress events, cancellation, and no-overwrite commits.
- [IPC Commands And File Access](IPC_AND_FILE_ACCESS.md) - the seven commands, two events, backend-owned IDs, drag and drop, and the capability file.
- [Security And Privacy](SECURITY_AND_PRIVACY.md) - CSP, isolation, secret handling, limits, temporary data, and known gaps.

## Feature Systems

- [Document Conversion](DOCUMENT_CONVERSION.md) - the capability matrix, routes per format pair, merging, and PDF protection on output.
- [Page Layout](PAGE_LAYOUT.md) - orientation, margins, spacing, page breaks for DOCX and text, and the real-output preview.
- [PDF Tools](PDF_TOOLS.md) - AES-256 passwords, unlock, merge, text extraction, image pages.
- [PDF/A Export](PDF_A_EXPORT.md) - archival PDF/A-1b, PDF/A-2b, PDF/A-3b, PDF/A-4, PDF/A-4f and PDF/UA-1, supported inputs and validation limits.
- [Encryption And Decryption](ENCRYPTION.md) - protected PDFs and `.age` files, and their reversal.
- [Image Conversion](IMAGE_CONVERSION.md) - PNG, JPEG, BMP, WebP, TIFF to PNG, JPG, WebP or PDF.
- [Clean Before Sharing](CLEAN_BEFORE_SHARING.md) - DOCX review/hidden-content removal, PDF metadata removal, photo GPS/camera removal, and limits.
- [File Queue](FILE_QUEUE.md) - the multi-select table, eligibility per mode, statuses.
- [Workspace UI](WORKSPACE_UI.md) - tabs, columns, settings per mode, validation, result bar.

## Design

- [Design Reference](DESIGN_REFERENCE.md) - the accepted `opendesign` mockup, its tokens and flows, and the deliberate differences in the shipped UI.

## Build, Test, And Planning

- [Build And Verification](BUILD_AND_VERIFICATION.md) - toolchain, LibreOffice for development, commands, regression tests, and the smoke check.
- [Application Plan](APPLICATION_PLAN.md) - proposed architecture, milestones, release gates, and licensing model.

## Metadata Convention

Documentation files use an OKF-lite YAML frontmatter shape:

```yaml
title: "Human-readable title"
description: "One-sentence discovery summary."
type: "guide | reference | catalog | plan | audit | design | index"
tags:
  - topic
resource: "repo-relative/path.md"
last_updated: "YYYY-MM-DD"
source_sync: "manual"
```

This metadata is for discovery and machine indexing. It does not replace the repo rule that implementation files in `crates/` and `apps/` remain the highest-confidence source when docs and code conflict.
