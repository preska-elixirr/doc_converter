---
title: "Documentation Index"
description: "Discovery map for Doc Converter architecture, feature, API catalog, design, and planning documentation."
type: "index"
tags:
  - documentation
  - index
  - okf-lite
resource: "docs/index.md"
last_updated: "2026-09-16"
source_sync: "manual"
---

# Documentation Index

Use this file as a discovery map, not as a required reading list. Start with `PROJECT_CONTEXT.md` for architecture and invariants, then open only the feature or catalog document relevant to the task.

## Start Here

- [Project Context](PROJECT_CONTEXT.md) - agent entry context for architecture, invariants, ownership, and pitfalls.
- [API & Method Catalog](methods_report/README.md) - compact exported API and command catalog.
- [Implementation Status](IMPLEMENTATION_STATUS.md) - what the first native milestone delivered and verified.

## Architecture And Runtime

- [Job Lifecycle](JOB_LIFECYCLE.md) - one job at a time, cancellation, temporary output, and no-overwrite commit.
- [IPC Commands And File Access](IPC_AND_FILE_ACCESS.md) - backend-owned input IDs, the three Tauri commands, and dialog flow.
- [Security And Privacy](SECURITY_AND_PRIVACY.md) - content security policy, secret handling, decoder limits, and known gaps.

## Feature Systems

- [File Queue](FILE_QUEUE.md) - adding, selecting, and removing files in the workspace.
- [Encryption And Decryption](ENCRYPTION.md) - password-based `.age` files and authenticated restore.
- [Image Conversion](IMAGE_CONVERSION.md) - PNG/JPG/BMP input, PNG/JPG output, resize, quality, orientation, alpha flattening.
- [Workspace UI](WORKSPACE_UI.md) - operation tabs, output settings panel, validation, messages, and placeholder tabs.

## Design

- [Design Reference](DESIGN_REFERENCE.md) - the accepted `opendesign` mockup, its tokens and flows, and how the shipped UI maps to it.

## Build, Test, And Planning

- [Build And Verification](BUILD_AND_VERIFICATION.md) - toolchain, commands, tests, and the dev loop.
- [Application Plan](APPLICATION_PLAN.md) - proposed architecture, capability matrix, milestones, and licensing model.

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
