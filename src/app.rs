//! Application state and eframe integration.
//!
//! `S3ExplorerApp` owns all runtime state: the tokio Handle, the flume channels,
//! the AWS S3 client, and all display state. The transfer worker task is spawned
//! once at startup and runs for the lifetime of the process.

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

// ── App struct ────────────────────────────────────────────────────────────────

#[allow(clippy::struct_excessive_bools)]
pub struct S3ExplorerApp {
    // ── Communication ────────────────────────────────────────────────────────
    pub tokio_handle: tokio::runtime::Handle,
    pub msg_tx: flume::Sender<AppMsg>,
    pub msg_rx: flume::Receiver<AppMsg>,
    /// Send jobs to the dedicated transfer worker task.
    pub job_tx: flume::Sender<TransferJob>,
    /// egui Context stored so `load_*` methods called outside `ui()` can
    /// call `request_repaint()` without requiring access to `&mut Ui`.
    pub egui_ctx: egui::Context,

    // ── AWS ──────────────────────────────────────────────────────────────────
    pub s3_client: aws_sdk_s3::Client,
    pub config: AppConfig,

    // ── Persisted state (serialised by eframe) ────────────────────────────────
    pub settings: AppSettings,

    // ── Local pane state ─────────────────────────────────────────────────────
    pub local_path: PathBuf,
    pub local_entries: Vec<LocalEntry>,
    pub local_loading: bool,
    pub local_selected: HashSet<PathBuf>,

    // ── S3 pane state ────────────────────────────────────────────────────────
    pub s3_location: S3Location,
    pub s3_entries: Vec<S3Entry>,
    pub s3_loading: bool,
    pub s3_selected: HashSet<String>,
    pub bucket_list: Vec<String>,
    pub buckets_loaded: bool,

    // ── Transfer panel state ──────────────────────────────────────────────────
    pub transfer_jobs: Vec<TransferJob>,
    pub next_job_id: u64,

    // ── Sync state ───────────────────────────────────────────────────────────
    pub pending_sync_plan: Option<SyncPlan>,
    pub sync_options: SyncOptions,

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

    // ── Move (drag-and-drop) state ────────────────────────────────────────────
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

// ── Construction ──────────────────────────────────────────────────────────────

impl S3ExplorerApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        tokio_handle: tokio::runtime::Handle,
        s3_client: aws_sdk_s3::Client,
        config: AppConfig,
    ) -> Self {
        let settings: AppSettings = cc
            .storage
            .and_then(|s| eframe::get_value(s, eframe::APP_KEY))
            .unwrap_or_default();

        let (msg_tx, msg_rx) = flume::unbounded::<AppMsg>();
        let (job_tx, job_rx) = flume::unbounded::<TransferJob>();

        let egui_ctx = cc.egui_ctx.clone();

        // Spawn the transfer worker — runs for the lifetime of the process.
        let worker_s3 = s3_client.clone();
        let worker_msg_tx = msg_tx.clone();
        let worker_ctx = egui_ctx.clone();
        let worker_storage_class = config.upload_storage_class;
        tokio_handle.spawn(async move {
            while let Ok(job) = job_rx.recv_async().await {
                worker_msg_tx.send(AppMsg::TransferStarted(job.id)).ok();
                worker_ctx.request_repaint();

                let result = execute_transfer(&worker_s3, &job, worker_storage_class).await;

                match result {
                    Ok(()) => {
                        worker_msg_tx.send(AppMsg::TransferDone(job.id)).ok();
                    }
                    Err(e) => {
                        worker_msg_tx
                            .send(AppMsg::TransferFailed {
                                id: job.id,
                                error: e.to_string(),
                            })
                            .ok();
                    }
                }
                worker_ctx.request_repaint();
            }
        });

        let local_path = settings
            .last_local_path
            .clone()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")));

        let s3_location = settings.last_s3_location.clone();
        let sync_options = settings.sync_options.clone();

        let mut app = Self {
            tokio_handle,
            msg_tx,
            msg_rx,
            job_tx,
            egui_ctx,
            s3_client,
            config,
            settings,
            local_path: local_path.clone(),
            local_entries: Vec::new(),
            local_loading: false,
            local_selected: HashSet::new(),
            s3_location: s3_location.clone(),
            s3_entries: Vec::new(),
            s3_loading: false,
            s3_selected: HashSet::new(),
            bucket_list: Vec::new(),
            buckets_loaded: false,
            transfer_jobs: Vec::new(),
            next_job_id: 0,
            pending_sync_plan: None,
            sync_options,
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

        app.load_local_directory(&local_path);
        app.load_buckets();
        if !s3_location.bucket.is_empty() {
            app.load_s3_prefix(&s3_location);
        }

        app
    }

    fn alloc_job_id(&mut self) -> JobId {
        let id = JobId(self.next_job_id);
        self.next_job_id += 1;
        id
    }
}

// ── Background task launchers ─────────────────────────────────────────────────

impl S3ExplorerApp {
    pub fn load_local_directory(&mut self, path: &std::path::Path) {
        // Update local_path FIRST so apply_message can match the arriving response.
        self.local_path = path.to_path_buf();
        self.local_loading = true;
        self.local_selected.clear();
        let tx = self.msg_tx.clone();
        let ctx = self.egui_ctx.clone();
        let p = path.to_path_buf();
        self.tokio_handle.spawn(async move {
            match crate::fs::local::list_directory(&p).await {
                Ok(entries) => tx.send(AppMsg::LocalListingDone { path: p, entries }).ok(),
                Err(e) => tx
                    .send(AppMsg::LocalListingError {
                        path: p,
                        error: e.to_string(),
                    })
                    .ok(),
            };
            ctx.request_repaint();
        });
    }

    pub fn load_buckets(&mut self) {
        self.s3_loading = true;
        let tx = self.msg_tx.clone();
        let ctx = self.egui_ctx.clone();
        let s3 = self.s3_client.clone();
        self.tokio_handle.spawn(async move {
            match crate::s3::client::list_buckets(&s3).await {
                Ok(names) => tx.send(AppMsg::BucketsLoaded(names)).ok(),
                Err(e) => tx.send(AppMsg::BucketsError(e.to_string())).ok(),
            };
            ctx.request_repaint();
        });
    }

    pub fn load_s3_prefix(&mut self, location: &S3Location) {
        // Update s3_location FIRST so apply_message can match the arriving response.
        self.s3_location = location.clone();
        self.s3_loading = true;
        self.s3_selected.clear();
        let tx = self.msg_tx.clone();
        let ctx = self.egui_ctx.clone();
        let s3 = self.s3_client.clone();
        let loc = location.clone();
        self.tokio_handle.spawn(async move {
            match crate::s3::client::list_prefix(&s3, &loc).await {
                Ok(entries) => tx
                    .send(AppMsg::S3ListingDone {
                        location: loc,
                        entries,
                    })
                    .ok(),
                Err(e) => tx
                    .send(AppMsg::S3ListingError {
                        location: loc,
                        error: e.to_string(),
                    })
                    .ok(),
            };
            ctx.request_repaint();
        });
    }

    /// Queue a transfer job: push to the display list and send to the worker.
    pub fn enqueue_transfer(&mut self, kind: TransferKind, size_bytes: u64) {
        let job = TransferJob {
            id: self.alloc_job_id(),
            kind,
            size_bytes,
            status: TransferStatus::Queued,
        };
        self.job_tx.send(job.clone()).ok();
        self.transfer_jobs.push(job);
    }

    /// Compute a sync plan in the background and show the confirmation dialog on completion.
    pub fn start_sync(&mut self, direction: SyncDirection) {
        let local_entries = self.local_entries.clone();
        let s3_entries = self.s3_entries.clone();
        let options = self.sync_options.clone();
        let bucket = self.s3_location.bucket.clone();
        let s3_prefix = self.s3_location.prefix.clone();
        let local_root = self.local_path.clone();
        let mut counter = self.next_job_id;
        let tx = self.msg_tx.clone();
        let ctx = self.egui_ctx.clone();

        self.tokio_handle.spawn(async move {
            let plan = match direction {
                SyncDirection::LocalToS3 => crate::sync::engine::compute_plan_local_to_s3(
                    &local_entries,
                    &s3_entries,
                    &options,
                    &mut counter,
                    &bucket,
                    &s3_prefix,
                ),
                SyncDirection::S3ToLocal => crate::sync::engine::compute_plan_s3_to_local(
                    &s3_entries,
                    &local_entries,
                    &options,
                    &mut counter,
                    &bucket,
                    &local_root,
                ),
            };
            tx.send(AppMsg::SyncPlanReady(plan)).ok();
            ctx.request_repaint();
        });
    }

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

    /// Execute the confirmed sync plan: enqueue all transfers and deletes.
    pub fn execute_sync_plan(&mut self) {
        if let Some(plan) = self.pending_sync_plan.take() {
            // Advance the job id counter past what the engine allocated.
            // Jobs already have IDs from the engine; push directly to worker.
            for job in plan.to_transfer.into_iter().chain(plan.to_delete) {
                self.job_tx.send(job.clone()).ok();
                self.transfer_jobs.push(job);
            }
        }
    }

    /// Request deletion of S3 objects or local files.
    /// If `confirm_before_delete` is set, shows the dialog; otherwise executes immediately.
    pub fn request_delete_remote(&mut self, items: Vec<(String, String)>) {
        // items: Vec<(bucket, key)>
        let jobs: Vec<TransferJob> = items
            .into_iter()
            .map(|(bucket, key)| TransferJob {
                id: self.alloc_job_id(),
                kind: TransferKind::DeleteRemote { bucket, key },
                size_bytes: 0,
                status: TransferStatus::Queued,
            })
            .collect();

        if self.config.confirm_before_delete {
            self.delete_confirm_items = jobs.iter().map(TransferJob::description).collect();
            self.pending_delete_jobs = jobs;
            self.show_delete_confirm = true;
        } else {
            for job in jobs {
                self.job_tx.send(job.clone()).ok();
                self.transfer_jobs.push(job);
            }
        }
    }

    pub fn request_delete_local(&mut self, paths: Vec<PathBuf>) {
        let jobs: Vec<TransferJob> = paths
            .into_iter()
            .map(|path| TransferJob {
                id: self.alloc_job_id(),
                kind: TransferKind::DeleteLocal { path },
                size_bytes: 0,
                status: TransferStatus::Queued,
            })
            .collect();

        if self.config.confirm_before_delete {
            self.delete_confirm_items = jobs.iter().map(TransferJob::description).collect();
            self.pending_delete_jobs = jobs;
            self.show_delete_confirm = true;
        } else {
            for job in jobs {
                self.job_tx.send(job.clone()).ok();
                self.transfer_jobs.push(job);
            }
        }
    }

    /// Confirm pending deletions (from the dialog).
    pub fn confirm_delete(&mut self) {
        for job in self.pending_delete_jobs.drain(..) {
            self.job_tx.send(job.clone()).ok();
            self.transfer_jobs.push(job);
        }
        self.delete_confirm_items.clear();
        self.show_delete_confirm = false;
    }

    /// Begin a drag-and-drop move: seed with plain files whose size is
    /// already known, and wait for `dir_count` recursive folder scans (if
    /// any) before finalizing.
    ///
    /// Only called from `handle_local_payload_dropped_on_s3` and
    /// `handle_s3_payload_dropped_on_local`, which are themselves wired up
    /// to the UI in a later task — until then this is reachable only via
    /// those two entry points, hence `#[allow(dead_code)]`.
    #[allow(dead_code)]
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
    ///
    /// Consumed by the move-confirmation dialog added in a later task; kept
    /// here now so `Self`'s move API surface is complete for that task.
    #[allow(dead_code)]
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
    ///
    /// Not yet called from the UI — wired up to drag-and-drop in a later
    /// task, hence `#[allow(dead_code)]`.
    #[allow(dead_code)]
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
    ///
    /// Not yet called from the UI — wired up to drag-and-drop in a later
    /// task, hence `#[allow(dead_code)]`.
    #[allow(dead_code)]
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
}

// ── Message handling ──────────────────────────────────────────────────────────

impl S3ExplorerApp {
    #[allow(clippy::too_many_lines)]
    pub fn apply_message(&mut self, msg: AppMsg) {
        match msg {
            AppMsg::BucketsLoaded(names) => {
                info!("Loaded {} buckets", names.len());
                self.bucket_list = names;
                self.buckets_loaded = true;
                self.s3_loading = false;
            }
            AppMsg::BucketsError(e) => {
                error!("Bucket list error: {e}");
                self.status_message = format!("Error loading buckets: {e}");
                self.s3_loading = false;
            }
            AppMsg::S3ListingDone { location, entries } => {
                if location == self.s3_location {
                    self.s3_entries = entries;
                    self.s3_loading = false;
                }
            }
            AppMsg::S3ListingError { location, error } => {
                if location == self.s3_location {
                    self.status_message = format!("S3 error: {error}");
                    self.s3_loading = false;
                }
            }
            AppMsg::LocalListingDone { path, entries } => {
                if path == self.local_path {
                    self.local_entries = entries;
                    self.local_loading = false;
                }
            }
            AppMsg::LocalListingError { path, error } => {
                if path == self.local_path {
                    self.status_message = format!("Local error: {error}");
                    self.local_loading = false;
                }
            }
            AppMsg::SyncPlanReady(plan) => {
                self.pending_sync_plan = Some(plan);
                self.show_sync_dialog = true;
            }
            AppMsg::SyncPlanError(e) => {
                self.status_message = format!("Sync error: {e}");
            }
            AppMsg::TransferStarted(id) => {
                if let Some(job) = self.transfer_jobs.iter_mut().find(|j| j.id == id) {
                    job.status = TransferStatus::InProgress;
                }
            }
            AppMsg::TransferDone(id) => {
                if let Some(job) = self.transfer_jobs.iter_mut().find(|j| j.id == id) {
                    job.status = TransferStatus::Done;
                }
                if let Some(companion) = self.move_followups.remove(&id) {
                    self.enqueue_transfer(companion, 0);
                }
                self.prune_completed();
            }
            AppMsg::TransferFailed { id, error } => {
                if let Some(job) = self.transfer_jobs.iter_mut().find(|j| j.id == id) {
                    job.status = TransferStatus::Failed(error.clone());
                }
                self.status_message = format!("Transfer failed: {error}");
            }
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
            AppMsg::BackgroundError(e) => {
                error!("Background error: {e}");
                self.status_message = format!("Error: {e}");
            }
        }
    }

    /// Remove oldest Done/Skipped rows when the list exceeds the config limit.
    fn prune_completed(&mut self) {
        let limit = self.config.max_completed_transfers_shown;
        let completed_count = self
            .transfer_jobs
            .iter()
            .filter(|j| matches!(j.status, TransferStatus::Done | TransferStatus::Skipped))
            .count();

        if completed_count > limit {
            let to_remove = completed_count - limit;
            let mut removed = 0usize;
            self.transfer_jobs.retain(|j| {
                if removed < to_remove
                    && matches!(j.status, TransferStatus::Done | TransferStatus::Skipped)
                {
                    removed += 1;
                    false
                } else {
                    true
                }
            });
        }
    }
}

// ── UI drawing ────────────────────────────────────────────────────────────────

impl S3ExplorerApp {
    fn draw_toolbar(&mut self, ui: &mut egui::Ui) {
        // Panel::top is not resizable by default; no size needed for toolbar.
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui::toolbar::draw(self, ui);
        });
    }

    fn draw_main_panels(&mut self, ui: &mut egui::Ui) {
        // Bottom panel must be declared before the side/central panels.
        egui::Panel::bottom("transfer_panel")
            .resizable(true)
            .default_size(180.0)
            .show(ui, |ui| {
                ui::transfer_panel::draw(self, ui);
            });
        egui::Panel::left("local_pane")
            .resizable(true)
            .default_size(500.0)
            .show(ui, |ui| {
                ui::local_pane::draw(self, ui);
            });
        egui::CentralPanel::default().show(ui, |ui| {
            ui::s3_pane::draw(self, ui);
        });
    }

    fn draw_sync_dialog(&mut self, ui: &mut egui::Ui) {
        let mut open = self.show_sync_dialog;
        egui::Window::new("Sync Plan")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(ui.ctx(), |ui| {
                if let Some(plan) = &self.pending_sync_plan {
                    if plan.is_empty() {
                        ui.label("Nothing to do — all files are already up to date.");
                    } else {
                        ui.label(format!(
                            "{} action(s) total ({} bytes to copy)",
                            plan.action_count(),
                            plan.total_bytes()
                        ));
                        ui.label(format!("{} file(s) to copy", plan.to_transfer.len()));
                        if !plan.to_delete.is_empty() {
                            ui.label(format!("{} file(s) to delete", plan.to_delete.len()));
                        }
                    }
                    ui.label(format!(
                        "{} file(s) already up to date",
                        plan.already_current
                    ));
                }
                ui.horizontal(|ui| {
                    if ui.button("Execute").clicked() {
                        self.execute_sync_plan();
                        self.show_sync_dialog = false;
                    }
                    if ui.button("Dry Run Only").clicked() {
                        self.pending_sync_plan = None;
                        self.show_sync_dialog = false;
                        "Dry run complete — no transfers executed."
                            .clone_into(&mut self.status_message);
                    }
                    if ui.button("Cancel").clicked() {
                        self.pending_sync_plan = None;
                        self.show_sync_dialog = false;
                    }
                });
            });
        if !open {
            self.show_sync_dialog = false;
            self.pending_sync_plan = None;
        }
    }

    fn draw_delete_confirm_dialog(&mut self, ui: &mut egui::Ui) {
        let mut open = self.show_delete_confirm;
        egui::Window::new("Confirm Delete")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(ui.ctx(), |ui| {
                ui.label("Delete the following items?");
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        for item in &self.delete_confirm_items {
                            ui.label(item);
                        }
                    });
                ui.horizontal(|ui| {
                    if ui.button("Delete").clicked() {
                        self.confirm_delete();
                    }
                    if ui.button("Cancel").clicked() {
                        self.pending_delete_jobs.clear();
                        self.delete_confirm_items.clear();
                        self.show_delete_confirm = false;
                    }
                });
            });
        if !open {
            self.pending_delete_jobs.clear();
            self.delete_confirm_items.clear();
            self.show_delete_confirm = false;
        }
    }

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

    fn draw_fatal_error(&mut self, ui: &mut egui::Ui) {
        if let Some(ref msg) = self.fatal_error.clone() {
            egui::Window::new("Startup Error")
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label(msg);
                    if ui.button("OK").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
        }
    }
}

// ── eframe::App implementation ────────────────────────────────────────────────

impl eframe::App for S3ExplorerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 1. Drain the message channel before any drawing.
        while let Ok(msg) = self.msg_rx.try_recv() {
            self.apply_message(msg);
        }

        // 2. Fatal startup error takes over the whole UI.
        if self.fatal_error.is_some() {
            self.draw_fatal_error(ui);
            return;
        }

        // 3. Render layout regions (order matters for egui panel allocation).
        self.draw_toolbar(ui);
        self.draw_main_panels(ui);

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

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.settings.last_local_path = Some(self.local_path.clone());
        self.settings.last_s3_location = self.s3_location.clone();
        self.settings.sync_options = self.sync_options.clone();
        eframe::set_value(storage, eframe::APP_KEY, &self.settings);
    }
}

// ── Transfer execution ────────────────────────────────────────────────────────

/// Execute a single transfer job. Called by the worker task.
async fn execute_transfer(
    s3: &aws_sdk_s3::Client,
    job: &TransferJob,
    storage_class: crate::types::UploadStorageClass,
) -> Result<()> {
    match &job.kind {
        TransferKind::Upload { local, bucket, key } => {
            crate::s3::client::upload_file(s3, local, bucket, key, storage_class).await?;
            info!("Uploaded {key}");
        }
        TransferKind::Download { bucket, key, local } => {
            crate::s3::client::download_object(s3, bucket, key, local).await?;
            info!("Downloaded {key}");
        }
        TransferKind::DeleteRemote { bucket, key } => {
            crate::s3::client::delete_object(s3, bucket, key).await?;
            info!("Deleted s3://{bucket}/{key}");
        }
        TransferKind::DeleteLocal { path } => {
            tokio::fs::remove_file(path).await?;
            info!("Deleted local {path:?}");
        }
    }
    Ok(())
}
