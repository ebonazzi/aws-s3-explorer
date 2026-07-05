# Drag-and-Drop Copy/Move Between Panes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user drag files/folders between the Local pane and the S3 pane to copy them, or hold Shift to move them (copy, then delete the source once the copy succeeds).

**Architecture:** Use egui 0.35's built-in in-app drag-and-drop (`Ui::dnd_drag_source` / `Ui::dnd_drop_zone`, confirmed present in the vendored `egui-0.35.0` source at `~/.cargo/registry/src/.../egui-0.35.0/src/{ui.rs,response.rs,drag_and_drop.rs}`). A new `DragPayload` enum snapshots the dragged entries at drag-start. Copy enqueues jobs immediately (same as today's context-menu actions, just generalized to N items). Move defers execution behind one confirmation dialog: recursive folder scans (if any) are collected into `S3ExplorerApp::move_scan` until all have reported back, then every copy job is enqueued with its companion delete job registered in `S3ExplorerApp::move_followups`, keyed by the copy job's `JobId`; the companion fires from the existing `AppMsg::TransferDone` handler.

**Tech Stack:** Rust (edition 2024, nightly toolchain), egui/eframe 0.35.0, egui_extras 0.35.0, tokio, flume, aws-sdk-s3. No new crate dependencies — `dnd_drag_source`/`dnd_drop_zone` are already part of the `egui` crate already in `Cargo.toml`.

**Spec:** `docs/superpowers/specs/2026-07-05-drag-drop-copy-move-design.md`

## Global Constraints

- Rust edition 2024, nightly toolchain (`Cargo.toml`: `edition = "2024"`, `rust-version = "1.96"`).
- No `.unwrap()`/`.expect()` in `src/` production code; `.expect()` only inside `#[cfg(test)]` blocks.
- Match the existing file's brace/formatting style exactly (standard rustfmt/K&R — this project has no `rustfmt.toml` override, so the personal Allman-style preference in the user's global `CLAUDE.md` does not apply here; run `cargo fmt` after each task to normalize).
- Every task ends with `cargo build` succeeding and `cargo clippy --all-targets -- -D warnings` clean, in addition to any task-specific verification.
- Conventional commit messages (`feat:`, `fix:`, `docs:`, `refactor:`), one commit per task.
- No new entries in `Cargo.toml`.

## Deferred from spec (cosmetic only)

Two cosmetic items from the design spec are intentionally not implemented by this plan, consistent with the spec's own framing of them as trimmable polish rather than core mechanism:

- The floating "Copy N item(s)" / "Move N item(s)" label that follows the cursor while dragging. Visual feedback during a drag is still present without it: `Ui::dnd_drop_zone` (used in Tasks 4-5) automatically highlights the target pane's background/border while a compatible payload hovers over it — this is egui's built-in behavior, not something this plan adds.
- The one-line status-bar note calling out that a folder move leaves now-empty local directories behind. The underlying behavior (files deleted individually, directories left in place) is implemented and verified in Task 6, Step 5; only the extra explanatory status-bar text is omitted.

If either is wanted later, it's a small addition on top of the mechanism this plan builds — not a redesign.

---

## Note on line numbers

Line numbers below are accurate as of the start of this plan (before Task 1). Each task's edits shift line numbers in the files it touches for all *subsequent* tasks that touch the same file — where that applies, the step calls it out and gives the surrounding code so you can locate the spot by content instead of by number if it has drifted.

---

### Task 1: `DragPayload` type and selection-aware drag-set helpers

**Files:**
- Create: `src/ui/dnd.rs`
- Modify: `src/ui/mod.rs:1-6`

**Interfaces:**
- Produces: `pub enum DragPayload { Local(Vec<LocalEntry>), S3(Vec<S3Entry>) }`, `pub fn effective_local_drag_set(clicked: &LocalEntry, selected: &HashSet<PathBuf>, all: &[LocalEntry]) -> Vec<LocalEntry>`, `pub fn effective_s3_drag_set(clicked: &S3Entry, selected: &HashSet<String>, all: &[S3Entry]) -> Vec<S3Entry>` — all consumed by Tasks 4 and 5.

- [ ] **Step 1: Write the failing tests**

Create `src/ui/dnd.rs` with only the production signatures stubbed to `unimplemented!()` plus the full test module, so the tests fail to compile/pass first:

```rust
//! Drag-and-drop payload and selection-aware drag-set helpers shared by
//! both panes.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::types::{LocalEntry, S3Entry};

/// What is being dragged between the two panes.
#[derive(Debug, Clone)]
pub enum DragPayload {
    /// Local files/directories dragged out of the Local pane.
    Local(Vec<LocalEntry>),
    /// S3 objects/prefixes dragged out of the S3 pane.
    S3(Vec<S3Entry>),
}

/// Compute the set of local entries that should be dragged when the user
/// starts dragging `clicked`: the whole current selection if `clicked` is
/// part of it, otherwise just `clicked` alone.
#[must_use]
pub fn effective_local_drag_set(
    clicked: &LocalEntry,
    selected: &HashSet<PathBuf>,
    all: &[LocalEntry],
) -> Vec<LocalEntry> {
    unimplemented!()
}

/// Compute the set of S3 entries that should be dragged when the user starts
/// dragging `clicked`: the whole current selection if `clicked` is part of
/// it, otherwise just `clicked` alone.
#[must_use]
pub fn effective_s3_drag_set(
    clicked: &S3Entry,
    selected: &HashSet<String>,
    all: &[S3Entry],
) -> Vec<S3Entry> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EntryKind;
    use chrono::Utc;

    fn local(name: &str) -> LocalEntry {
        LocalEntry {
            name: name.to_owned(),
            path: PathBuf::from(name),
            size_bytes: 0,
            modified: Utc::now(),
            kind: EntryKind::File,
        }
    }

    fn s3(key: &str) -> S3Entry {
        S3Entry {
            key: key.to_owned(),
            name: key.rsplit('/').next().unwrap_or(key).to_owned(),
            size_bytes: 0,
            last_modified: None,
            e_tag: None,
            kind: EntryKind::File,
        }
    }

    #[test]
    fn local_drag_set_is_single_item_when_not_selected() {
        let all = vec![local("a.txt"), local("b.txt")];
        let selected: HashSet<PathBuf> = HashSet::new();
        let set = effective_local_drag_set(&all[0], &selected, &all);
        assert_eq!(set.len(), 1);
        assert_eq!(set[0].name, "a.txt");
    }

    #[test]
    fn local_drag_set_is_whole_selection_when_clicked_item_is_selected() {
        let all = vec![local("a.txt"), local("b.txt"), local("c.txt")];
        let selected: HashSet<PathBuf> = [PathBuf::from("a.txt"), PathBuf::from("c.txt")].into();
        let set = effective_local_drag_set(&all[0], &selected, &all);
        let mut names: Vec<_> = set.iter().map(|e| e.name.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["a.txt", "c.txt"]);
    }

    #[test]
    fn s3_drag_set_is_single_item_when_not_selected() {
        let all = vec![s3("a"), s3("b")];
        let selected: HashSet<String> = HashSet::new();
        let set = effective_s3_drag_set(&all[0], &selected, &all);
        assert_eq!(set.len(), 1);
        assert_eq!(set[0].key, "a");
    }

    #[test]
    fn s3_drag_set_is_whole_selection_when_clicked_item_is_selected() {
        let all = vec![s3("a"), s3("b"), s3("c")];
        let selected: HashSet<String> = ["a".to_owned(), "c".to_owned()].into();
        let set = effective_s3_drag_set(&all[0], &selected, &all);
        let mut keys: Vec<_> = set.iter().map(|e| e.key.clone()).collect();
        keys.sort();
        assert_eq!(keys, vec!["a", "c"]);
    }
}
```

- [ ] **Step 2: Register the module**

Modify `src/ui/mod.rs`, current lines 1-6:

```rust
//! UI module: re-exports and shared helpers.

pub mod local_pane;
pub mod s3_pane;
pub mod toolbar;
pub mod transfer_panel;
```

Add `pub mod dnd;` so the new module is reachable:

```rust
//! UI module: re-exports and shared helpers.

pub mod dnd;
pub mod local_pane;
pub mod s3_pane;
pub mod toolbar;
pub mod transfer_panel;
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib ui::dnd`
Expected: compiles, then panics with `not implemented` on the first test (`unimplemented!()`).

- [ ] **Step 4: Implement the two helpers**

In `src/ui/dnd.rs`, replace the two `unimplemented!()` bodies:

```rust
#[must_use]
pub fn effective_local_drag_set(
    clicked: &LocalEntry,
    selected: &HashSet<PathBuf>,
    all: &[LocalEntry],
) -> Vec<LocalEntry> {
    if selected.contains(&clicked.path) {
        all.iter()
            .filter(|e| selected.contains(&e.path))
            .cloned()
            .collect()
    } else {
        vec![clicked.clone()]
    }
}
```

```rust
#[must_use]
pub fn effective_s3_drag_set(
    clicked: &S3Entry,
    selected: &HashSet<String>,
    all: &[S3Entry],
) -> Vec<S3Entry> {
    if selected.contains(&clicked.key) {
        all.iter()
            .filter(|e| selected.contains(&e.key))
            .cloned()
            .collect()
    } else {
        vec![clicked.clone()]
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib ui::dnd`
Expected: `test result: ok. 4 passed; 0 failed`

- [ ] **Step 6: Format, lint, build**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo build`
Expected: all three succeed with no warnings/errors.

- [ ] **Step 7: Commit**

```bash
git add src/ui/dnd.rs src/ui/mod.rs
git commit -m "feat: add drag payload type and selection-aware drag-set helpers"
```

---

### Task 2: Move bookkeeping in `S3ExplorerApp`

This is the core state machine: threading `is_move` through the existing recursive-scan messages, tracking in-flight folder scans for a move, and firing each source deletion once its paired copy succeeds. It has no isolated unit-testable surface (like the rest of `app.rs`, which has no existing tests — the project's convention is to verify this class of orchestration code by running the app), so verification here is `cargo build` + `cargo clippy` + a quick manual smoke check that the pre-existing "Upload folder" / "Download folder" context-menu actions still work unchanged. Full move behavior is verified end-to-end in Task 6, once Tasks 4-5 wire up the UI that can actually trigger it.

**Files:**
- Modify: `src/types.rs:340-362` (the `FolderScanComplete` / `S3RecursiveListComplete` variants of `AppMsg`)
- Modify: `src/app.rs` (struct fields, constructor, `start_folder_upload`, `start_folder_download`, new methods, `apply_message`)
- Modify: `src/ui/local_pane.rs:163` (`start_folder_upload` call site)
- Modify: `src/ui/s3_pane.rs:228` (`start_folder_download` call site)

**Interfaces:**
- Consumes: `TransferJob::description(&self) -> String` (`src/types.rs`), `S3ExplorerApp::alloc_job_id`, `enqueue_transfer`, `job_tx`, `transfer_jobs` (all pre-existing in `src/app.rs`).
- Produces: `S3ExplorerApp::start_folder_upload(&mut self, local_root: &Path, is_move: bool)`, `S3ExplorerApp::start_folder_download(&mut self, s3_folder_prefix: &str, is_move: bool)` (signature changes — both gain the trailing `is_move: bool`), `S3ExplorerApp::handle_local_payload_dropped_on_s3(&mut self, entries: Vec<LocalEntry>, is_move: bool)`, `S3ExplorerApp::handle_s3_payload_dropped_on_local(&mut self, entries: Vec<S3Entry>, is_move: bool)`, `S3ExplorerApp::confirm_move(&mut self)`, `S3ExplorerApp::cancel_move(&mut self)`, public fields `S3ExplorerApp::show_move_confirm: bool` and `S3ExplorerApp::move_confirm_items: Vec<String>` — all consumed by Task 3 (dialog) and Tasks 4-5 (pane wiring).

- [ ] **Step 1: Add `is_move` to the two `AppMsg` variants**

Modify `src/types.rs`, current lines 340-362:

```rust
    // ── Recursive operations ──────────────────────────────────────────────────
    /// Local directory walk finished: ready to enqueue upload jobs.
    FolderScanComplete {
        /// All files found under the walked root.
        files: Vec<(std::path::PathBuf, u64)>,
        /// Absolute path of the folder that was scanned.
        local_root: std::path::PathBuf,
        /// S3 prefix that will be prepended to relative paths.
        s3_prefix: String,
        /// Target S3 bucket.
        bucket: String,
    },
    /// S3 recursive listing finished: ready to enqueue download jobs.
    S3RecursiveListComplete {
        /// All objects found: (key, `size_bytes`).
        objects: Vec<(String, u64)>,
        /// Local directory into which files are written.
        local_root: std::path::PathBuf,
        /// S3 prefix that is stripped to compute relative local paths.
        s3_prefix: String,
        /// Source S3 bucket.
        bucket: String,
    },
```

Replace with:

```rust
    // ── Recursive operations ──────────────────────────────────────────────────
    /// Local directory walk finished: ready to enqueue upload jobs.
    FolderScanComplete {
        /// All files found under the walked root.
        files: Vec<(std::path::PathBuf, u64)>,
        /// Absolute path of the folder that was scanned.
        local_root: std::path::PathBuf,
        /// S3 prefix that will be prepended to relative paths.
        s3_prefix: String,
        /// Target S3 bucket.
        bucket: String,
        /// `true` if this scan is part of a drag-and-drop move: each
        /// resulting upload should delete its local source file once it
        /// succeeds, rather than leaving the source in place.
        is_move: bool,
    },
    /// S3 recursive listing finished: ready to enqueue download jobs.
    S3RecursiveListComplete {
        /// All objects found: (key, `size_bytes`).
        objects: Vec<(String, u64)>,
        /// Local directory into which files are written.
        local_root: std::path::PathBuf,
        /// S3 prefix that is stripped to compute relative local paths.
        s3_prefix: String,
        /// Source S3 bucket.
        bucket: String,
        /// `true` if this listing is part of a drag-and-drop move: each
        /// resulting download should delete its source S3 object once it
        /// succeeds, rather than leaving the source in place.
        is_move: bool,
    },
```

- [ ] **Step 2: Add imports and new state fields to `S3ExplorerApp`**

Modify `src/app.rs`, current lines 7-17:

```rust
use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::Result;
use tracing::{error, info};

use crate::config::AppConfig;
use crate::types::{
    AppMsg, AppSettings, JobId, LocalEntry, S3Entry, S3Location, SyncDirection, SyncOptions,
    SyncPlan, TransferJob, TransferKind, TransferStatus,
};
use crate::ui;
```

Replace with:

```rust
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use anyhow::Result;
use tracing::{error, info};

use crate::config::AppConfig;
use crate::types::{
    AppMsg, AppSettings, EntryKind, JobId, LocalEntry, S3Entry, S3Location, SyncDirection,
    SyncOptions, SyncPlan, TransferJob, TransferKind, TransferStatus,
};
use crate::ui;

/// Accumulates the results of one or more in-flight recursive folder scans
/// triggered by a drag-and-drop move, until every scan has reported back.
#[derive(Debug)]
struct PendingMoveScan {
    /// How many recursive folder scans are still running for this drop.
    scans_outstanding: usize,
    /// Copy job paired with the delete job to run once that copy succeeds.
    items: Vec<(TransferJob, TransferKind)>,
}
```

- [ ] **Step 3: Add new fields to the `S3ExplorerApp` struct**

Modify `src/app.rs`, current lines 63-73:

```rust
    // ── UI state ─────────────────────────────────────────────────────────────
    pub status_message: String,
    pub show_sync_dialog: bool,
    pub show_delete_confirm: bool,
    /// Items about to be deleted — displayed in the confirmation dialog.
    pub delete_confirm_items: Vec<String>,
    /// Jobs that will execute on confirm (populated alongside `delete_confirm_items`).
    pub pending_delete_jobs: Vec<TransferJob>,
    /// Fatal startup error to display in a modal before anything else.
    pub fatal_error: Option<String>,
}
```

Replace with:

```rust
    // ── UI state ─────────────────────────────────────────────────────────────
    pub status_message: String,
    pub show_sync_dialog: bool,
    pub show_delete_confirm: bool,
    /// Items about to be deleted — displayed in the confirmation dialog.
    pub delete_confirm_items: Vec<String>,
    /// Jobs that will execute on confirm (populated alongside `delete_confirm_items`).
    pub pending_delete_jobs: Vec<TransferJob>,
    /// Fatal startup error to display in a modal before anything else.
    pub fatal_error: Option<String>,

    // ── Move (drag-and-drop) state ──────────────────────────────────────────
    /// Copy `JobId` -> the delete job to fire once that copy succeeds.
    move_followups: HashMap<JobId, TransferKind>,
    /// Set while one or more recursive folder scans are outstanding for an
    /// in-progress drag-and-drop move.
    move_scan: Option<PendingMoveScan>,
    /// Finalized move items, ready for the confirmation dialog or immediate execution.
    pending_move_items: Vec<(TransferJob, TransferKind)>,
    pub show_move_confirm: bool,
    /// Descriptions of `pending_move_items`, displayed in the confirmation dialog.
    pub move_confirm_items: Vec<String>,
}
```

- [ ] **Step 4: Initialize the new fields in the constructor**

Modify `src/app.rs`, current lines 154-159 (inside the `Self { ... }` literal in `S3ExplorerApp::new`):

```rust
            status_message: String::from("Ready"),
            show_sync_dialog: false,
            show_delete_confirm: false,
            delete_confirm_items: Vec::new(),
            pending_delete_jobs: Vec::new(),
            fatal_error: None,
        };
```

Replace with:

```rust
            status_message: String::from("Ready"),
            show_sync_dialog: false,
            show_delete_confirm: false,
            delete_confirm_items: Vec::new(),
            pending_delete_jobs: Vec::new(),
            fatal_error: None,
            move_followups: HashMap::new(),
            move_scan: None,
            pending_move_items: Vec::new(),
            show_move_confirm: false,
            move_confirm_items: Vec::new(),
        };
```

- [ ] **Step 5: Thread `is_move` through `start_folder_upload` and `start_folder_download`**

Modify `src/app.rs`, current lines 295-326 (`start_folder_upload`):

```rust
    /// Walk `local_root` recursively and enqueue an `Upload` job for every file
    /// found, using the current S3 bucket and prefix as the destination.
    pub fn start_folder_upload(&mut self, local_root: &std::path::Path) {
        if self.s3_location.bucket.is_empty() {
            "Select an S3 bucket before uploading a folder.".clone_into(&mut self.status_message);
            return;
        }
        let bucket = self.s3_location.bucket.clone();
        let s3_prefix = self.s3_location.prefix.clone();
        let root = local_root.to_path_buf();
        let tx = self.msg_tx.clone();
        let ctx = self.egui_ctx.clone();

        self.status_message = format!("Scanning {}…", root.display());

        self.tokio_handle.spawn(async move {
            match crate::fs::local::collect_files_recursive(&root).await {
                Ok(files) => {
                    tx.send(AppMsg::FolderScanComplete {
                        files,
                        local_root: root,
                        s3_prefix,
                        bucket,
                    })
                    .ok();
                }
                Err(e) => {
                    tx.send(AppMsg::BackgroundError(format!("Folder scan failed: {e}")))
                        .ok();
                }
            }
            ctx.request_repaint();
        });
    }
```

Replace with:

```rust
    /// Walk `local_root` recursively and enqueue an `Upload` job for every file
    /// found, using the current S3 bucket and prefix as the destination.
    ///
    /// `is_move` is threaded through to `AppMsg::FolderScanComplete` so the
    /// message handler knows whether to delete each source file once its
    /// upload succeeds.
    pub fn start_folder_upload(&mut self, local_root: &std::path::Path, is_move: bool) {
        if self.s3_location.bucket.is_empty() {
            "Select an S3 bucket before uploading a folder.".clone_into(&mut self.status_message);
            return;
        }
        let bucket = self.s3_location.bucket.clone();
        let s3_prefix = self.s3_location.prefix.clone();
        let root = local_root.to_path_buf();
        let tx = self.msg_tx.clone();
        let ctx = self.egui_ctx.clone();

        self.status_message = format!("Scanning {}…", root.display());

        self.tokio_handle.spawn(async move {
            match crate::fs::local::collect_files_recursive(&root).await {
                Ok(files) => {
                    tx.send(AppMsg::FolderScanComplete {
                        files,
                        local_root: root,
                        s3_prefix,
                        bucket,
                        is_move,
                    })
                    .ok();
                }
                Err(e) => {
                    tx.send(AppMsg::BackgroundError(format!("Folder scan failed: {e}")))
                        .ok();
                }
            }
            ctx.request_repaint();
        });
    }
```

Modify `src/app.rs`, current lines 328-367 (`start_folder_download`):

```rust
    /// List all S3 objects under `s3_folder_prefix` recursively and enqueue a
    /// `Download` job for each, recreating the sub-folder structure under
    /// `self.local_path`.
    ///
    /// `s3_folder_prefix` is the full S3 prefix of the chosen sub-folder
    /// (e.g. `"photos/2023/Italy/"`). The current browsing prefix
    /// (`self.s3_location.prefix`) is stripped from each key to produce the
    /// relative local path.
    pub fn start_folder_download(&mut self, s3_folder_prefix: &str) {
        let bucket = self.s3_location.bucket.clone();
        let s3_prefix = self.s3_location.prefix.clone();
        let folder_prefix = s3_folder_prefix.to_owned();
        let local_root = self.local_path.clone();
        let tx = self.msg_tx.clone();
        let ctx = self.egui_ctx.clone();
        let s3 = self.s3_client.clone();

        self.status_message = format!("Listing S3 objects under {folder_prefix}…");

        self.tokio_handle.spawn(async move {
            match crate::s3::client::list_all_objects_recursive(&s3, &bucket, &folder_prefix).await
            {
                Ok(entries) => {
                    let objects = entries.into_iter().map(|e| (e.key, e.size_bytes)).collect();
                    tx.send(AppMsg::S3RecursiveListComplete {
                        objects,
                        local_root,
                        s3_prefix,
                        bucket,
                    })
                    .ok();
                }
                Err(e) => {
                    tx.send(AppMsg::BackgroundError(format!("S3 listing failed: {e}")))
                        .ok();
                }
            }
            ctx.request_repaint();
        });
    }
```

Replace with:

```rust
    /// List all S3 objects under `s3_folder_prefix` recursively and enqueue a
    /// `Download` job for each, recreating the sub-folder structure under
    /// `self.local_path`.
    ///
    /// `s3_folder_prefix` is the full S3 prefix of the chosen sub-folder
    /// (e.g. `"photos/2023/Italy/"`). The current browsing prefix
    /// (`self.s3_location.prefix`) is stripped from each key to produce the
    /// relative local path.
    ///
    /// `is_move` is threaded through to `AppMsg::S3RecursiveListComplete` so
    /// the message handler knows whether to delete each source object once
    /// its download succeeds.
    pub fn start_folder_download(&mut self, s3_folder_prefix: &str, is_move: bool) {
        let bucket = self.s3_location.bucket.clone();
        let s3_prefix = self.s3_location.prefix.clone();
        let folder_prefix = s3_folder_prefix.to_owned();
        let local_root = self.local_path.clone();
        let tx = self.msg_tx.clone();
        let ctx = self.egui_ctx.clone();
        let s3 = self.s3_client.clone();

        self.status_message = format!("Listing S3 objects under {folder_prefix}…");

        self.tokio_handle.spawn(async move {
            match crate::s3::client::list_all_objects_recursive(&s3, &bucket, &folder_prefix).await
            {
                Ok(entries) => {
                    let objects = entries.into_iter().map(|e| (e.key, e.size_bytes)).collect();
                    tx.send(AppMsg::S3RecursiveListComplete {
                        objects,
                        local_root,
                        s3_prefix,
                        bucket,
                        is_move,
                    })
                    .ok();
                }
                Err(e) => {
                    tx.send(AppMsg::BackgroundError(format!("S3 listing failed: {e}")))
                        .ok();
                }
            }
            ctx.request_repaint();
        });
    }
```

- [ ] **Step 6: Update the two existing call sites for the new parameter**

Modify `src/ui/local_pane.rs`, current line 163:

```rust
    if let Some(folder) = upload_folder {
        app.start_folder_upload(&folder);
    }
```

Replace with:

```rust
    if let Some(folder) = upload_folder {
        app.start_folder_upload(&folder, false);
    }
```

Modify `src/ui/s3_pane.rs`, current line 228:

```rust
    if let Some(folder_prefix) = download_folder {
        app.start_folder_download(&folder_prefix);
    }
```

Replace with:

```rust
    if let Some(folder_prefix) = download_folder {
        app.start_folder_download(&folder_prefix, false);
    }
```

- [ ] **Step 7: Add the move state machine methods**

Modify `src/app.rs`: insert the following methods immediately before the closing `}` of the `impl S3ExplorerApp` block that contains `confirm_delete` (current lines 430-438 hold `confirm_delete`; insert right after its closing brace, still inside the same `impl` block):

```rust
    /// Begin a drag-and-drop move: seed with plain files whose size is
    /// already known, and wait for `dir_count` recursive folder scans (if
    /// any) before finalizing.
    fn begin_move(&mut self, plain: Vec<(TransferJob, TransferKind)>, dir_count: usize) {
        self.move_scan = Some(PendingMoveScan {
            scans_outstanding: dir_count,
            items: plain,
        });
        if dir_count == 0 {
            self.finalize_move_scan();
        }
    }

    /// Record one recursive folder scan's results against the in-flight
    /// move, finalizing once every outstanding scan has reported back.
    fn record_move_scan_result(&mut self, items: Vec<(TransferJob, TransferKind)>) {
        if let Some(scan) = self.move_scan.as_mut() {
            scan.items.extend(items);
            scan.scans_outstanding = scan.scans_outstanding.saturating_sub(1);
            if scan.scans_outstanding == 0 {
                self.finalize_move_scan();
            }
        }
    }

    /// All scans for the current move are in: show the confirmation dialog,
    /// or execute immediately if `confirm_before_delete` is disabled.
    fn finalize_move_scan(&mut self) {
        let Some(scan) = self.move_scan.take() else {
            return;
        };
        self.pending_move_items = scan.items;
        self.move_confirm_items = self
            .pending_move_items
            .iter()
            .map(|(job, _)| job.description())
            .collect();

        if self.config.confirm_before_delete {
            self.show_move_confirm = true;
        } else {
            self.confirm_move();
        }
    }

    /// Confirm pending move items (from the dialog, or immediately when
    /// `confirm_before_delete` is disabled): enqueue every copy job and
    /// register its companion delete to fire once that copy succeeds.
    pub fn confirm_move(&mut self) {
        let count = self.pending_move_items.len();
        for (job, companion) in self.pending_move_items.drain(..) {
            self.job_tx.send(job.clone()).ok();
            self.move_followups.insert(job.id, companion);
            self.transfer_jobs.push(job);
        }
        self.move_confirm_items.clear();
        self.show_move_confirm = false;
        self.status_message = format!("Moving {count} item(s)…");
    }

    /// Cancel a pending move: discard everything, nothing is enqueued.
    pub fn cancel_move(&mut self) {
        self.pending_move_items.clear();
        self.move_confirm_items.clear();
        self.show_move_confirm = false;
    }

    /// Entry point for a `Local` payload dropped on the S3 pane.
    ///
    /// Files are enqueued (or staged for move-confirmation) directly.
    /// Directories go through `start_folder_upload`, tagged with `is_move`
    /// so its `AppMsg::FolderScanComplete` handler knows whether to stage
    /// its results for move-confirmation too.
    pub fn handle_local_payload_dropped_on_s3(&mut self, entries: Vec<LocalEntry>, is_move: bool) {
        if self.s3_location.bucket.is_empty() {
            "Select an S3 bucket before dropping files here.".clone_into(&mut self.status_message);
            return;
        }
        let bucket = self.s3_location.bucket.clone();
        let s3_prefix = self.s3_location.prefix.clone();

        let mut plain: Vec<(TransferJob, TransferKind)> = Vec::new();
        let mut dir_count = 0usize;

        for entry in entries {
            match entry.kind {
                EntryKind::File => {
                    let key = format!("{s3_prefix}{}", entry.name);
                    let kind = TransferKind::Upload {
                        local: entry.path.clone(),
                        bucket: bucket.clone(),
                        key,
                    };
                    if is_move {
                        let job = TransferJob {
                            id: self.alloc_job_id(),
                            kind,
                            size_bytes: entry.size_bytes,
                            status: TransferStatus::Queued,
                        };
                        plain.push((job, TransferKind::DeleteLocal { path: entry.path }));
                    } else {
                        self.enqueue_transfer(kind, entry.size_bytes);
                    }
                }
                EntryKind::Directory => {
                    dir_count += 1;
                    self.start_folder_upload(&entry.path, is_move);
                }
            }
        }

        if is_move {
            self.begin_move(plain, dir_count);
        }
    }

    /// Entry point for an `S3` payload dropped on the Local pane. Mirrors
    /// `handle_local_payload_dropped_on_s3` for the opposite direction.
    pub fn handle_s3_payload_dropped_on_local(&mut self, entries: Vec<S3Entry>, is_move: bool) {
        let bucket = self.s3_location.bucket.clone();
        let local_root = self.local_path.clone();

        let mut plain: Vec<(TransferJob, TransferKind)> = Vec::new();
        let mut dir_count = 0usize;

        for entry in entries {
            match entry.kind {
                EntryKind::File => {
                    let local = local_root.join(&entry.name);
                    let kind = TransferKind::Download {
                        bucket: bucket.clone(),
                        key: entry.key.clone(),
                        local,
                    };
                    if is_move {
                        let job = TransferJob {
                            id: self.alloc_job_id(),
                            kind,
                            size_bytes: entry.size_bytes,
                            status: TransferStatus::Queued,
                        };
                        plain.push((
                            job,
                            TransferKind::DeleteRemote {
                                bucket: bucket.clone(),
                                key: entry.key,
                            },
                        ));
                    } else {
                        self.enqueue_transfer(kind, entry.size_bytes);
                    }
                }
                EntryKind::Directory => {
                    dir_count += 1;
                    self.start_folder_download(&entry.key, is_move);
                }
            }
        }

        if is_move {
            self.begin_move(plain, dir_count);
        }
    }
```

- [ ] **Step 8: Branch on `is_move` in `apply_message`, and fire move companions on `TransferDone`**

Modify `src/app.rs`, the `AppMsg::TransferDone(id)` arm (current lines 494-499):

```rust
            AppMsg::TransferDone(id) => {
                if let Some(job) = self.transfer_jobs.iter_mut().find(|j| j.id == id) {
                    job.status = TransferStatus::Done;
                }
                self.prune_completed();
            }
```

Replace with:

```rust
            AppMsg::TransferDone(id) => {
                if let Some(job) = self.transfer_jobs.iter_mut().find(|j| j.id == id) {
                    job.status = TransferStatus::Done;
                }
                if let Some(companion) = self.move_followups.remove(&id) {
                    self.enqueue_transfer(companion, 0);
                }
                self.prune_completed();
            }
```

Modify `src/app.rs`, the `AppMsg::FolderScanComplete { .. }` arm (current lines 506-546):

```rust
            AppMsg::FolderScanComplete {
                files,
                local_root,
                s3_prefix,
                bucket,
            } => {
                let count = files.len();
                // Include the uploaded folder's own name in the S3 prefix so that
                //   local:  C:\Photos\Italy\Venice\IMG_001.jpg
                //   uploads to:  {s3_prefix}Italy/Venice/IMG_001.jpg
                // rather than dropping the folder name entirely.
                let folder_name = local_root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let dest_prefix = if folder_name.is_empty() {
                    s3_prefix.clone()
                } else {
                    format!("{s3_prefix}{folder_name}/")
                };
                for (path, size) in files {
                    // Relative path inside the scanned folder, with forward slashes.
                    let rel = path
                        .strip_prefix(&local_root)
                        .map(|r| r.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_default();
                    let key = format!("{dest_prefix}{rel}");
                    self.enqueue_transfer(
                        TransferKind::Upload {
                            local: path,
                            bucket: bucket.clone(),
                            key,
                        },
                        size,
                    );
                }
                self.status_message = format!(
                    "Queued {count} file(s) → S3: {dest_prefix} \
                     (navigate to that prefix in S3 pane to see them)"
                );
            }
```

Replace with:

```rust
            AppMsg::FolderScanComplete {
                files,
                local_root,
                s3_prefix,
                bucket,
                is_move,
            } => {
                let count = files.len();
                // Include the uploaded folder's own name in the S3 prefix so that
                //   local:  C:\Photos\Italy\Venice\IMG_001.jpg
                //   uploads to:  {s3_prefix}Italy/Venice/IMG_001.jpg
                // rather than dropping the folder name entirely.
                let folder_name = local_root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let dest_prefix = if folder_name.is_empty() {
                    s3_prefix.clone()
                } else {
                    format!("{s3_prefix}{folder_name}/")
                };
                let mut move_items: Vec<(TransferJob, TransferKind)> = Vec::new();
                for (path, size) in files {
                    // Relative path inside the scanned folder, with forward slashes.
                    let rel = path
                        .strip_prefix(&local_root)
                        .map(|r| r.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_default();
                    let key = format!("{dest_prefix}{rel}");
                    let kind = TransferKind::Upload {
                        local: path.clone(),
                        bucket: bucket.clone(),
                        key,
                    };
                    if is_move {
                        let job = TransferJob {
                            id: self.alloc_job_id(),
                            kind,
                            size_bytes: size,
                            status: TransferStatus::Queued,
                        };
                        move_items.push((job, TransferKind::DeleteLocal { path }));
                    } else {
                        self.enqueue_transfer(kind, size);
                    }
                }
                if is_move {
                    self.record_move_scan_result(move_items);
                } else {
                    self.status_message = format!(
                        "Queued {count} file(s) → S3: {dest_prefix} \
                         (navigate to that prefix in S3 pane to see them)"
                    );
                }
            }
```

Modify `src/app.rs`, the `AppMsg::S3RecursiveListComplete { .. }` arm (current lines 547-575):

```rust
            AppMsg::S3RecursiveListComplete {
                objects,
                local_root,
                s3_prefix,
                bucket,
            } => {
                let count = objects.len();
                for (key, size) in objects {
                    // Strip the browsing prefix to get the relative path, then
                    // build a native OS path by splitting on '/'.
                    let rel = key.strip_prefix(&s3_prefix).unwrap_or(&key);
                    let local_path = rel.split('/').filter(|c| !c.is_empty()).fold(
                        local_root.clone(),
                        |mut p, component| {
                            p.push(component);
                            p
                        },
                    );
                    self.enqueue_transfer(
                        TransferKind::Download {
                            bucket: bucket.clone(),
                            key,
                            local: local_path,
                        },
                        size,
                    );
                }
                self.status_message = format!("Queued {count} object(s) for download");
            }
```

Replace with:

```rust
            AppMsg::S3RecursiveListComplete {
                objects,
                local_root,
                s3_prefix,
                bucket,
                is_move,
            } => {
                let count = objects.len();
                let mut move_items: Vec<(TransferJob, TransferKind)> = Vec::new();
                for (key, size) in objects {
                    // Strip the browsing prefix to get the relative path, then
                    // build a native OS path by splitting on '/'.
                    let rel = key.strip_prefix(&s3_prefix).unwrap_or(&key);
                    let local_path = rel.split('/').filter(|c| !c.is_empty()).fold(
                        local_root.clone(),
                        |mut p, component| {
                            p.push(component);
                            p
                        },
                    );
                    let kind = TransferKind::Download {
                        bucket: bucket.clone(),
                        key: key.clone(),
                        local: local_path,
                    };
                    if is_move {
                        let job = TransferJob {
                            id: self.alloc_job_id(),
                            kind,
                            size_bytes: size,
                            status: TransferStatus::Queued,
                        };
                        move_items.push((
                            job,
                            TransferKind::DeleteRemote {
                                bucket: bucket.clone(),
                                key,
                            },
                        ));
                    } else {
                        self.enqueue_transfer(kind, size);
                    }
                }
                if is_move {
                    self.record_move_scan_result(move_items);
                } else {
                    self.status_message = format!("Queued {count} object(s) for download");
                }
            }
```

- [ ] **Step 9: Format, lint, build**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo build`
Expected: all three succeed with no warnings/errors. If clippy flags `too_many_arguments` or similar on the two new `handle_*_dropped_on_*` methods, that is unexpected at 2 params (self + entries + is_move) — investigate rather than suppressing.

- [ ] **Step 10: Manual smoke check of the unchanged copy path**

Run: `cargo run`. Right-click a local folder → "Upload folder to S3 →" with an S3 bucket selected. Confirm the status bar still reports "Queued N file(s) → S3: ..." exactly as before, and the files appear in the transfer panel. This confirms the `is_move: false` path is behaviorally unchanged.

- [ ] **Step 11: Commit**

```bash
git add src/types.rs src/app.rs src/ui/local_pane.rs src/ui/s3_pane.rs
git commit -m "feat: add move bookkeeping (companion deletes, folder-scan aggregation)"
```

---

### Task 3: Move confirmation dialog

**Files:**
- Modify: `src/app.rs` (new `draw_move_confirm_dialog` method, wired into `eframe::App::ui`)

**Interfaces:**
- Consumes: `S3ExplorerApp::show_move_confirm`, `move_confirm_items`, `confirm_move()`, `cancel_move()` (all from Task 2).
- Produces: nothing new consumed by later tasks — this is a leaf UI component.

- [ ] **Step 1: Add the dialog method**

Modify `src/app.rs`: insert the following method immediately after `draw_delete_confirm_dialog` and before `draw_fatal_error`. Task 2 added roughly 150 lines earlier in this file, so the line numbers from before Task 2 (687-718) no longer apply — locate `draw_delete_confirm_dialog` by name/content instead:

```rust
    fn draw_move_confirm_dialog(&mut self, ui: &mut egui::Ui) {
        let mut open = self.show_move_confirm;
        egui::Window::new("Confirm Move")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(ui.ctx(), |ui| {
                ui.label("Copy the following items, then delete them from the source?");
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        for item in &self.move_confirm_items {
                            ui.label(item);
                        }
                    });
                ui.horizontal(|ui| {
                    if ui.button("Move").clicked() {
                        self.confirm_move();
                    }
                    if ui.button("Cancel").clicked() {
                        self.cancel_move();
                    }
                });
            });
        if !open {
            self.cancel_move();
        }
    }
```

- [ ] **Step 2: Wire it into the main `ui()` loop**

Modify `src/app.rs`, inside `impl eframe::App for S3ExplorerApp { fn ui(...) }` (line numbers have shifted from Task 2's earlier additions — locate by the anchor code below):

```rust
        // 4. Modal dialogs drawn on top.
        if self.show_sync_dialog {
            self.draw_sync_dialog(ui);
        }
        if self.show_delete_confirm {
            self.draw_delete_confirm_dialog(ui);
        }
    }
```

Replace with:

```rust
        // 4. Modal dialogs drawn on top.
        if self.show_sync_dialog {
            self.draw_sync_dialog(ui);
        }
        if self.show_delete_confirm {
            self.draw_delete_confirm_dialog(ui);
        }
        if self.show_move_confirm {
            self.draw_move_confirm_dialog(ui);
        }
    }
```

- [ ] **Step 3: Format, lint, build**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo build`
Expected: all three succeed with no warnings/errors. The dialog is not yet reachable from the UI (nothing sets `show_move_confirm = true` from a live code path until Tasks 4-5 land), so no manual check is possible yet — that happens in Task 6.

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat: add move confirmation dialog"
```

---

### Task 4: Wire drag-and-drop into the Local pane

**Files:**
- Modify: `src/ui/local_pane.rs` (full function body of `draw`)

**Interfaces:**
- Consumes: `ui::dnd::DragPayload`, `ui::dnd::effective_local_drag_set` (Task 1), `S3ExplorerApp::handle_s3_payload_dropped_on_local`, `start_folder_upload(&mut self, &Path, bool)` (Task 2).

- [ ] **Step 1: Replace `src/ui/local_pane.rs` in full**

The whole file (169 lines) becomes:

```rust
//! Left pane: local filesystem browser.

use std::path::PathBuf;

use egui_extras::{Column, TableBuilder};

use crate::app::S3ExplorerApp;
use crate::types::{EntryKind, TransferKind};
use crate::ui;

/// Draw the local pane into the given `ui`.
#[allow(clippy::too_many_lines)]
pub fn draw(app: &mut S3ExplorerApp, ui: &mut egui::Ui) {
    ui.heading("Local");

    // Address + navigation buttons.
    let mut refresh_requested = false;
    ui.horizontal(|ui| {
        if ui.button("[..]").clicked()
            && let Some(parent) = app.local_path.parent().map(std::path::Path::to_path_buf)
        {
            app.load_local_directory(&parent);
        }
        if ui
            .button("⟳")
            .on_hover_text("Refresh current folder")
            .clicked()
        {
            refresh_requested = true;
        }
        ui.label(app.local_path.display().to_string());
    });
    if refresh_requested {
        let path = app.local_path.clone();
        app.load_local_directory(&path);
    }

    if app.local_loading {
        ui.spinner();
        return;
    }

    // Snapshot the state we need inside the closures.
    let entries = app.local_entries.clone();
    let selected = app.local_selected.clone();

    // Accumulate at most one action per frame.
    let mut nav_path: Option<PathBuf> = None;
    let mut toggle_path: Option<PathBuf> = None;
    let mut delete_paths: Vec<PathBuf> = Vec::new();
    let mut copy_to_s3: Vec<(PathBuf, String, u64)> = Vec::new();
    let mut upload_folder: Option<PathBuf> = None;

    let prefix = app.s3_location.prefix.clone();
    let bucket = app.s3_location.bucket.clone();
    let has_bucket = !bucket.is_empty();

    // Dropping a `Local` payload here (an S3 payload dropped back onto the
    // Local pane) is intentionally ignored below — only cross-pane drops act.
    let (_, dropped) = ui.dnd_drop_zone::<ui::dnd::DragPayload, _>(egui::Frame::NONE, |ui| {
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .auto_shrink(false) // fill the panel's full height so the resize handle works
            .column(Column::auto()) // icon
            .column(Column::remainder()) // name
            .column(Column::initial(80.0)) // size
            .column(Column::initial(140.0)) // modified
            .header(20.0, |mut header| {
                header.col(|_ui| {});
                header.col(|ui| {
                    ui.strong("Name");
                });
                header.col(|ui| {
                    ui.strong("Size");
                });
                header.col(|ui| {
                    ui.strong("Modified");
                });
            })
            .body(|mut body| {
                for entry in &entries {
                    let is_selected = selected.contains(&entry.path);
                    let entry_path = entry.path.clone();
                    let entry_name = entry.name.clone();
                    let entry_kind = entry.kind;
                    let entry_size = entry.size_bytes;
                    let entry_mtime = entry.modified;
                    let key_for_copy = format!("{prefix}{entry_name}");
                    let drag_set = ui::dnd::effective_local_drag_set(entry, &selected, &entries);
                    let drag_id = egui::Id::new("local_row").with(&entry_path);

                    body.row(18.0, |mut row| {
                        row.col(|ui| {
                            ui.label(ui::kind_icon(entry_kind));
                        });
                        row.col(|ui| {
                            let resp = ui
                                .dnd_drag_source(
                                    drag_id,
                                    ui::dnd::DragPayload::Local(drag_set),
                                    |ui| ui.selectable_label(is_selected, &entry_name),
                                )
                                .inner;

                            if resp.double_clicked() && entry_kind == EntryKind::Directory {
                                nav_path = Some(entry_path.clone());
                            } else if resp.clicked() {
                                toggle_path = Some(entry_path.clone());
                            }

                            resp.context_menu(|ui| {
                                if entry_kind == EntryKind::File {
                                    if ui.button("Copy to S3").clicked() {
                                        copy_to_s3.push((
                                            entry_path.clone(),
                                            key_for_copy.clone(),
                                            entry_size,
                                        ));
                                        ui.close();
                                    }
                                    if ui.button("Delete").clicked() {
                                        delete_paths.push(entry_path.clone());
                                        ui.close();
                                    }
                                } else if has_bucket {
                                    if ui.button("Upload folder to S3 →").clicked() {
                                        upload_folder = Some(entry_path.clone());
                                        ui.close();
                                    }
                                    ui.weak("(uploads all files recursively)");
                                } else {
                                    ui.weak("Select an S3 bucket first");
                                }
                            });
                        });
                        row.col(|ui| {
                            if entry_kind == EntryKind::File {
                                ui.label(ui::format_bytes(entry_size));
                            }
                        });
                        row.col(|ui| {
                            ui.label(entry_mtime.format("%Y-%m-%d %H:%M").to_string());
                        });
                    });
                }
            });
    });

    // Apply accumulated actions after the table is done.
    if let Some(path) = nav_path {
        app.load_local_directory(&path);
    } else if let Some(path) = toggle_path {
        if app.local_selected.contains(&path) {
            app.local_selected.remove(&path);
        } else {
            app.local_selected.insert(path);
        }
    }

    for (local, key, size) in copy_to_s3 {
        if !bucket.is_empty() {
            app.enqueue_transfer(
                TransferKind::Upload {
                    local,
                    bucket: bucket.clone(),
                    key,
                },
                size,
            );
        }
    }

    if let Some(folder) = upload_folder {
        app.start_folder_upload(&folder, false);
    }

    if !delete_paths.is_empty() {
        app.request_delete_local(delete_paths);
    }

    if let Some(payload) = dropped {
        if let ui::dnd::DragPayload::S3(items) = payload.as_ref().clone() {
            let is_move = ui.input(|i| i.modifiers.shift);
            app.handle_s3_payload_dropped_on_local(items, is_move);
        }
    }
}
```

- [ ] **Step 2: Format, lint, build**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo build`
Expected: all three succeed with no warnings/errors.

- [ ] **Step 3: Manual regression check**

Run: `cargo run`. Confirm, with no dragging involved:
- The local pane still lists files/folders, click still selects, double-click still navigates.
- The right-click context menu ("Copy to S3", "Delete", "Upload folder to S3 →") still works exactly as before.
- The pane still fills the available panel height and its column resize handles still work (checks that wrapping the table in `dnd_drop_zone` didn't break layout).

- [ ] **Step 4: Commit**

```bash
git add src/ui/local_pane.rs
git commit -m "feat: wire drag source and drop zone into the local pane"
```

---

### Task 5: Wire drag-and-drop into the S3 pane

**Files:**
- Modify: `src/ui/s3_pane.rs` (full function body of `draw_prefix_contents`)

**Interfaces:**
- Consumes: `ui::dnd::DragPayload`, `ui::dnd::effective_s3_drag_set` (Task 1), `S3ExplorerApp::handle_local_payload_dropped_on_s3`, `start_folder_download(&mut self, &str, bool)` (Task 2).

- [ ] **Step 1: Replace `draw_prefix_contents` in `src/ui/s3_pane.rs` in full**

Replace the function currently at lines 103-230 (`#[allow(clippy::too_many_lines)] fn draw_prefix_contents(...) { ... }`) with:

```rust
#[allow(clippy::too_many_lines)]
fn draw_prefix_contents(app: &mut S3ExplorerApp, ui: &mut egui::Ui) {
    let entries = app.s3_entries.clone();
    let selected = app.s3_selected.clone();
    let bucket = app.s3_location.bucket.clone();
    let current_loc = app.s3_location.clone();
    let local_root = app.local_path.clone();

    let mut navigate_to: Option<S3Location> = None;
    let mut toggle_key: Option<String> = None;
    let mut download_items: Vec<(String, String, u64)> = Vec::new(); // (key, name, size)
    let mut delete_items: Vec<(String, String)> = Vec::new(); // (bucket, key)
    let mut download_folder: Option<String> = None; // S3 prefix to download recursively

    // Dropping an `S3` payload here (a Local payload dropped back onto the
    // S3 pane) is intentionally ignored below — only cross-pane drops act.
    let (_, dropped) = ui.dnd_drop_zone::<ui::dnd::DragPayload, _>(egui::Frame::NONE, |ui| {
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .auto_shrink(false)
            .column(Column::auto()) // icon
            .column(Column::remainder()) // name
            .column(Column::initial(80.0)) // size
            .column(Column::initial(160.0)) // last modified
            .header(20.0, |mut header| {
                header.col(|_ui| {});
                header.col(|ui| {
                    ui.strong("Name");
                });
                header.col(|ui| {
                    ui.strong("Size");
                });
                header.col(|ui| {
                    ui.strong("Last Modified");
                });
            })
            .body(|mut body| {
                for entry in &entries {
                    let is_selected = selected.contains(&entry.key);
                    let entry_key = entry.key.clone();
                    let entry_name = entry.name.clone();
                    let entry_kind = entry.kind;
                    let entry_size = entry.size_bytes;
                    let entry_mtime = entry.last_modified;
                    let bucket_for_action = bucket.clone();
                    let loc_for_nav = current_loc.clone();
                    let drag_set = ui::dnd::effective_s3_drag_set(entry, &selected, &entries);
                    let drag_id = egui::Id::new("s3_row").with(&entry_key);

                    body.row(18.0, |mut row| {
                        row.col(|ui| {
                            ui.label(ui::kind_icon(entry_kind));
                        });
                        row.col(|ui| {
                            let resp = ui
                                .dnd_drag_source(
                                    drag_id,
                                    ui::dnd::DragPayload::S3(drag_set),
                                    |ui| ui.selectable_label(is_selected, &entry_name),
                                )
                                .inner;

                            if resp.double_clicked() && entry_kind == EntryKind::Directory {
                                navigate_to = Some(loc_for_nav.enter(&entry_name));
                            } else if resp.clicked() {
                                toggle_key = Some(entry_key.clone());
                            }

                            resp.context_menu(|ui| {
                                if entry_kind == EntryKind::File {
                                    if ui.button("Download").clicked() {
                                        download_items.push((
                                            entry_key.clone(),
                                            entry_name.clone(),
                                            entry_size,
                                        ));
                                        ui.close();
                                    }
                                    if ui.button("Delete").clicked() {
                                        delete_items
                                            .push((bucket_for_action.clone(), entry_key.clone()));
                                        ui.close();
                                    }
                                } else {
                                    // Directory/prefix
                                    if ui.button("Download folder to local ↓").clicked() {
                                        download_folder = Some(entry_key.clone());
                                        ui.close();
                                    }
                                    ui.weak("(downloads all objects recursively)");
                                }
                            });
                        });
                        row.col(|ui| {
                            if entry_kind == EntryKind::File {
                                ui.label(ui::format_bytes(entry_size));
                            }
                        });
                        row.col(|ui| {
                            if let Some(mtime) = entry_mtime {
                                ui.label(mtime.format("%Y-%m-%d %H:%M").to_string());
                            }
                        });
                    });
                }
            });
    });

    // Apply accumulated actions.
    if let Some(loc) = navigate_to {
        app.load_s3_prefix(&loc);
    } else if let Some(key) = toggle_key {
        if app.s3_selected.contains(&key) {
            app.s3_selected.remove(&key);
        } else {
            app.s3_selected.insert(key);
        }
    }

    for (key, name, size) in download_items {
        let local = local_root.join(&name);
        app.enqueue_transfer(
            TransferKind::Download {
                bucket: bucket.clone(),
                key,
                local,
            },
            size,
        );
    }

    if !delete_items.is_empty() {
        app.request_delete_remote(delete_items);
    }

    if let Some(folder_prefix) = download_folder {
        app.start_folder_download(&folder_prefix, false);
    }

    if let Some(payload) = dropped {
        if let ui::dnd::DragPayload::Local(items) = payload.as_ref().clone() {
            let is_move = ui.input(|i| i.modifiers.shift);
            app.handle_local_payload_dropped_on_s3(items, is_move);
        }
    }
}
```

`draw`, `draw_bucket_list`, and `go_up` in this file are unchanged (no drop zone in the bucket-list view — a bucket must be selected before drag-and-drop targets it, per the spec's confirmed scope).

- [ ] **Step 2: Format, lint, build**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo build`
Expected: all three succeed with no warnings/errors.

- [ ] **Step 3: Manual regression check**

Run: `cargo run`. Confirm, with no dragging involved:
- Selecting a bucket still lists its contents; click/double-click/navigation still work.
- The right-click context menu ("Download", "Delete", "Download folder to local ↓") still works exactly as before.
- The pane still fills the available height and column resizing still works.

- [ ] **Step 4: Commit**

```bash
git add src/ui/s3_pane.rs
git commit -m "feat: wire drag source and drop zone into the S3 pane"
```

---

### Task 6: End-to-end manual verification

No further code changes are expected in this task unless verification surfaces a bug — if it does, fix it in the relevant file, re-run the specific check that failed, then continue.

**Files:** none expected (bugfix-only, in whichever file the bug lives in).

- [ ] **Step 1: Single-file copy, each direction**

Run: `cargo run`. Drag one local file onto the S3 pane (no Shift). Confirm it's queued as an `Upload` in the transfer panel and the local file still exists afterward. Then drag one S3 object onto the Local pane (no Shift). Confirm it's queued as a `Download` and the S3 object still exists afterward (check via the S3 pane's refresh).

- [ ] **Step 2: Single-file move, each direction**

With `confirm_before_delete` at its default (`true`, per `~/.config/aws-s3-explorer/config.json`), drag one local file onto the S3 pane while holding Shift. Confirm the "Confirm Move" dialog appears showing that one item; click "Move". Confirm the upload completes, then the local file disappears (refresh the local pane to check). Repeat S3 → local with Shift held.

- [ ] **Step 3: Multi-select drag**

Select 3 local files (click, then Ctrl/Shift-click per existing selection semantics — check `local_selected` behavior in `local_pane.rs` if unsure), then start dragging one of the selected files onto the S3 pane. Confirm all 3 are queued, not just the one under the cursor. Repeat for S3 multi-select onto the Local pane.

- [ ] **Step 4: Folder copy, each direction**

Drag a local folder containing a few files onto the S3 pane (no Shift). Confirm it behaves the same as today's "Upload folder to S3 →" context-menu action (same destination-prefix naming, same file count in the status message). Repeat for an S3 "folder" (prefix) dragged onto the Local pane.

- [ ] **Step 5: Folder move, each direction**

Drag a local folder onto the S3 pane with Shift held. Confirm the "Confirm Move" dialog appears only after the recursive scan completes, listing every file in the folder (not just the folder name). Click "Move". Confirm every file uploads and is then deleted from the local folder, and the (now empty) local subdirectory itself is left behind on disk. Repeat for an S3 prefix dragged onto the Local pane with Shift held — confirm every object downloads and is then deleted from S3.

- [ ] **Step 6: Partial-failure safety**

Set up a move where one file will fail (e.g. make one target file read-only so the download's local write fails, or revoke write permission on one file so `DeleteLocal` fails after a successful upload in a move). Confirm: a file whose *copy* fails is never deleted from the source (its `Failed` status shows in the transfer panel, and the file remains on disk/in S3). A file whose copy succeeds but whose *companion delete* fails also shows as `Failed` in the panel, distinctly from the successful ones.

- [ ] **Step 7: Cancel the move-confirmation dialog**

Drag a file with Shift held, then click "Cancel" in the dialog. Confirm nothing was enqueued — no rows appear in the transfer panel, and the source is untouched.

- [ ] **Step 8: Same-pane drop is a no-op**

Drag a local file and drop it back onto the Local pane (not the S3 pane). Confirm nothing happens — no transfer is queued, no dialog appears.

- [ ] **Step 9: Final full-suite check and commit (only if fixes were needed)**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo build && cargo test`
Expected: all succeed. If any step above required a code fix, commit it now:

```bash
git add -A
git commit -m "fix: <describe the specific bug found during drag-and-drop UAT>"
```

If no fixes were needed, there is nothing to commit for this task.
