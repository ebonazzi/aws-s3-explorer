# Drag-and-Drop Copy/Move Between Panes — Design

**Status:** Approved for planning (implementation not yet started)
**Date:** 2026-07-05

## Problem

The app has two panes — Local (left) and S3 (right) — each backed by a
`TableBuilder` in `ui/local_pane.rs` / `ui/s3_pane.rs`. Today, moving data
between them requires the right-click context menu ("Copy S3", "Download",
"Upload folder →", "Download folder ↓"), one item at a time, and there is no
way to move a file/folder (copy then remove the source) at all — only copy
or delete exist as separate, independent actions.

This spec covers adding in-app drag-and-drop between the two panes:
- Drag from either pane, drop on the other, to copy.
- Hold **Shift** while dropping to move (copy, then delete the source once
  the copy has actually succeeded).
- Works for both individual files and folders (local directories / S3
  prefixes), reusing the existing recursive scan machinery.

## Scope decisions (confirmed during brainstorming)

| Question | Decision |
|---|---|
| Folder drag-and-drop | In scope. Reuses existing recursive scan (`collect_files_recursive`, `list_all_objects_recursive`). |
| Multi-select interaction | If the dragged row is part of the current selection, the whole selection is dragged. If not, only that single row is dragged. |
| Drop granularity | Whole-pane drop zone only (v1). Dropping onto a specific subfolder row to jump into it is out of scope for now. |
| Move confirmation | Reuses the existing `config.confirm_before_delete` setting. When on, a confirmation dialog is shown once per drop, before any transfer starts. |
| Folder move cleanup | Each file is deleted individually once its own copy succeeds. Now-empty local subdirectories are left on disk (no recursive `rmdir`). |
| Folder move confirmation timing | Scan-first-then-confirm: recursive scans complete before the confirmation dialog appears, so the dialog shows an accurate file count and byte total (mirrors the existing Sync Plan dialog). |
| Same-pane drops | Out of scope. Dropping a Local payload back onto the Local pane (or S3-on-S3) is a no-op. Only cross-pane drops trigger a transfer. |
| Drag source | In-app only, via egui's built-in drag-and-drop (`dnd_drag_source` / `dnd_drop_zone`). No OS-level drag into/out of the window. |

## Data model changes

### New module: `src/ui/dnd.rs`

```rust
enum DragPayload {
    Local(Vec<LocalEntry>),  // snapshot of dragged local files/dirs
    S3(Vec<S3Entry>),        // snapshot of dragged S3 objects/prefixes
}
```

The payload is a **self-contained snapshot** taken at drag-start (not a
live reference into pane selection state). This was chosen over having the
drop handler read `app.local_selected`/`app.s3_selected` live, because:
- it keeps drop-handling decoupled from pane-internal selection state, and
- it cleanly covers the "dragged item wasn't part of the selection" case
  without extra bookkeeping.

This module also owns the shared "compute the effective drag set" helper
used by both panes: given the clicked entry and the current selection set,
return either the whole selection (if the entry is in it) or just that one
entry.

### `types.rs` changes

- `AppMsg::FolderScanComplete` gains `is_move: bool`.
- `AppMsg::S3RecursiveListComplete` gains `is_move: bool`.

No new `TransferKind` variant is needed — a move's second leg is just the
existing `DeleteLocal`/`DeleteRemote`, sequenced after the copy succeeds.

### New `S3ExplorerApp` state (`app.rs`)

```rust
/// Copy JobId -> the delete job to fire once that copy succeeds.
move_followups: HashMap<JobId, TransferKind>,

/// Accumulates scan results for an in-flight folder move until every
/// outstanding scan has reported back, so the confirmation dialog can
/// show an accurate total.
pending_move_scan: Option<PendingMoveScan>,

/// Confirmation dialog state, mirroring show_delete_confirm / delete_confirm_items.
show_move_confirm: bool,
move_confirm_items: Vec<String>,
```

`PendingMoveScan` tracks: how many recursive scans are still outstanding,
and the flat list of `(TransferJob, TransferKind /* companion delete */)`
accumulated so far from both plain files (known immediately) and completed
scans (known once their `AppMsg` arrives).

## UI interaction changes

- **Row widgets** in both panes wrap the existing `selectable_label` in
  `ui.dnd_drag_source(id, payload, ...)`. Click-to-select is unaffected —
  egui only starts a drag once the pointer moves past its internal drag
  threshold, so no manual click/drag disambiguation is needed. The drag
  `Id` is derived from the path (local) or key (S3) for frame-to-frame
  stability.
- **Each pane body** becomes a `dnd_drop_zone::<DragPayload, _>`. A
  `Local` payload dropped on the S3 pane triggers upload(s); an `S3`
  payload dropped on the Local pane triggers download(s). A payload
  dropped back onto its originating pane is ignored.
- **Modifier state is read at drop time**, not drag-start: `ui.input(|i|
  i.modifiers.shift)` is sampled in the same frame the drop zone reports a
  completed drop.
- **Visual feedback:** while a compatible payload is hovering a pane, that
  pane gets a highlighted border, and a small floating label near the
  cursor reads "Copy N item(s)" or "Move N item(s)" depending on the live
  shift state. This is cosmetic and can be trimmed later without affecting
  the core mechanism.

## Move/copy execution flow

**Copy (Shift not held):** unchanged from today's per-item logic, just
generalized from one item to the effective drag set. Plain files go
straight to `enqueue_transfer`. Directories go through
`start_folder_upload` / `start_folder_download` per dragged directory,
with `is_move: false` — behaviorally identical to today's "Upload/Download
folder" context-menu actions.

**Move (Shift held):**
1. Split the dropped set into plain files (size/kind already known from
   the payload) and directories (need scanning).
2. Seed `pending_move_scan` with the plain files immediately, and kick off
   one recursive scan per dragged directory with `is_move: true`.
3. As each `FolderScanComplete` / `S3RecursiveListComplete` arrives with
   `is_move: true`, its files are appended to `pending_move_scan` instead
   of being enqueued immediately (this differs from the `is_move: false`
   path, which enqueues as today).
4. Once every outstanding scan has reported back, build the confirmation
   dialog from the accumulated list (count + total bytes), gated by
   `config.confirm_before_delete`. If that setting is off, skip straight
   to step 5.
5. On confirm: enqueue every copy job normally (`job_tx` + push to
   `transfer_jobs`, same as today), and register each job's `JobId` in
   `move_followups` with its paired delete `TransferKind`.
6. In `apply_message`'s `AppMsg::TransferDone(id)` handler, after marking
   the job done, check `move_followups.remove(&id)`. If present, enqueue
   that delete job directly — bypassing `request_delete_local` /
   `request_delete_remote`'s own confirmation path, since the user already
   confirmed once for the whole batch in step 4.

## Error handling / edge cases

- A file whose copy **fails**: `TransferFailed` never consults
  `move_followups`, so its companion delete is simply never fired and the
  source file is untouched. No special-case code required.
- A file whose copy **succeeds but whose companion delete then fails**
  (e.g. a permissions error) surfaces as its own ordinary `Failed` row in
  the transfer panel — this is already handled generically by the existing
  transfer panel rendering.
- Folder moves deliberately leave empty directory trees behind on the
  local side (per the confirmed scope decision). A status-bar message
  should note this so it isn't surprising.
- A payload dropped on its own originating pane is a no-op.
- Cancelling the move-confirmation dialog discards `pending_move_scan`
  entirely; nothing is enqueued and no scans' results are applied.

## Testing

- Unit tests for the new pure logic:
  - Building the confirmation summary from a mixed file+folder drop.
  - `move_followups` bookkeeping: companion fires only on `TransferDone`,
    never on `TransferFailed`.
  - The "effective drag set" helper (selected vs. not-selected dragged
    row).
- Manual UAT in the running app:
  - Single-file copy and move, each direction.
  - Multi-select drag (copy and move).
  - Folder copy and move, each direction.
  - A move with a deliberately-failed file (e.g. read-only target) to
    confirm the source survives.
  - Cancelling the move-confirmation dialog.

## Out of scope (for this spec)

- Dropping onto a specific subfolder row (only whole-pane drop zones).
- OS-level drag into/out of the application window.
- Same-pane drag-and-drop (Local→Local, S3→S3).
- Recursive removal of now-empty local directory trees after a folder
  move.
