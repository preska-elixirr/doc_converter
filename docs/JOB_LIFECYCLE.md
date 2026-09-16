---
title: "Job Lifecycle"
description: "How one processing job runs: busy gating, save dialog, blocking thread, cooperative cancellation, temporary output, and no-overwrite commit."
type: "guide"
tags:
  - jobs
  - cancellation
  - file-safety
resource: "docs/JOB_LIFECYCLE.md"
last_updated: "2026-09-16"
source_sync: "manual"
---

# Job Lifecycle

A job is one call to the `process_file` command for one input file. The app runs at most one job at a time. Every job ends in exactly one of three ways: a saved output, an error with no output, or a cancelled save dialog with no output.

Owning files:

- `apps/desktop/src-tauri/src/main.rs` - `process_file`, `cancel_job`, `AppState`
- `crates/core/src/lib.rs` - `copy_cancel`, `output`, `commit`, and the three operations

## Flow

```mermaid
flowchart TD
  A[UI calls process_file] --> B{operation is encrypt, decrypt, or image?}
  B -- no --> E1[Err: not implemented yet]
  B -- yes --> C{encrypt and password shorter than 12 chars?}
  C -- yes --> E2[Err: use a longer password]
  C -- no --> D{busy already true?}
  D -- yes --> E3[Err: a job is already running]
  D -- no --> F[busy = true, cancel = false]
  F --> G{ID in file map?}
  G -- no --> E4[Err: select the file again]
  G -- yes --> H[Build suggested output name]
  H --> I[Native save dialog]
  I -- cancelled --> R1[Ok: save cancelled, no output]
  I -- chosen --> J{destination exists?}
  J -- yes --> E5[Err: output already exists]
  J -- no --> K[spawn_blocking: core operation]
  K -- Ok --> R2[Ok: Saved path]
  K -- Err --> E6[Err: core message]
  R1 --> Z[busy = false]
  R2 --> Z
  E4 --> Z
  E5 --> Z
  E6 --> Z
```

The early checks for operation name, password length, and busy state happen before `busy` flips, so a rejected call never blocks a later one.

## Suggested output names

| Operation | Suggested name |
| --- | --- |
| encrypt | `<source name>.age` |
| decrypt | `restored-<source name without .age>` |
| image | `<source stem>-converted.<png or jpg>` |

The user can change the name in the dialog. The dialog does not restrict the folder.

## Temporary output and commit

Every core operation writes to a `NamedTempFile` created in the destination's parent directory, then moves it into place.

```mermaid
flowchart TD
  A[output: destination exists?] -- yes --> X[Err: choose another name]
  A -- no --> B[Create temp file in same directory]
  B --> C[Write all output]
  C --> D[commit: cancel flag set?]
  D -- yes --> Y[Err: cancelled, temp dropped]
  D -- no --> E[sync_all]
  E --> F[persist_noclobber to destination]
  F -- destination appeared meanwhile --> Z[Err, temp dropped]
  F -- ok --> G[Done]
```

Why the same directory: a rename inside one directory is atomic on the same volume, so the destination either exists complete or not at all.

Why `persist_noclobber`: the existence check in `output()` and in `process_file` runs before the work. If another process creates the destination during the job, the final move still refuses to overwrite.

Dropping a `NamedTempFile` deletes it. That covers normal errors and cancellation. It does not cover a force-killed process or power loss; cleanup of orphaned temp files on next start is not implemented.

## Cancellation

`cancel_job` sets `AppState.cancel` to `true` and returns. It does not wait. The core checks the flag at these points:

- `copy_cancel`: before every 64 KiB chunk during encrypt and decrypt
- `convert_image`: once after decode and orientation, before resize
- `commit`: once before the final move

A stage that is already running finishes first. Examples: scrypt key derivation for age, a single large PNG decode, or the JPEG encode. The UI keeps the Cancel button visible until the command returns.

The flag is reset to `false` when the next job starts. Calling `cancel_job` while idle has no effect on that next job.

## Busy state

`busy` is `true` from the moment the checks pass until the command returns, which includes the time the save dialog is open. While busy:

- `pick_files` returns an error instead of opening the picker
- a second `process_file` returns an error
- the UI disables every control except Cancel

## What the UI shows

| Moment | Message area |
| --- | --- |
| Job started | `Choose a new output filename in the save dialog.` |
| Save dialog cancelled | `Save cancelled. No output created.` as a notice |
| Success | `Saved <full destination path>` as a notice |
| Any error | The error string in a red alert; the notice is cleared |

After every job, success or not, the password and confirmation fields are cleared. The file stays selected so the user can retry.

## Known limits

- One file per job. Batch queues are planned, not implemented.
- No progress percentage. The UI shows a `Working…` label only.
- Cancellation is cooperative, as described above.
- Temp files from a crash are not cleaned up.
