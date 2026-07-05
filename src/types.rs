//! Core domain types shared across the application.
//!
//! This module has no imports from the rest of `aws-s3-explorer`.
//! Every other module may import from here without risk of circular deps.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── Entry types (what the panes display) ──────────────────────────────────────

/// Whether a filesystem or S3 entry is a file/object or a directory/prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    /// A local directory or an S3 "common prefix" (virtual folder).
    Directory,
}

/// One row in the local filesystem pane.
#[derive(Debug, Clone)]
pub struct LocalEntry {
    /// Filename only — used for display and sorting.
    pub name: String,
    /// Full absolute path — used for I/O operations.
    pub path: PathBuf,
    /// Zero for directories.
    pub size_bytes: u64,
    pub modified: DateTime<Utc>,
    pub kind: EntryKind,
}

/// One row in the S3 pane — either a real object or a common prefix.
#[derive(Debug, Clone)]
pub struct S3Entry {
    /// Full S3 key, e.g. `"photos/2023/Italy/IMG_001.jpg"`.
    pub key: String,
    /// Last path component, e.g. `"IMG_001.jpg"` — used for display.
    pub name: String,
    /// Zero for common prefixes.
    pub size_bytes: u64,
    /// `None` for common prefixes (they carry no `LastModified`).
    pub last_modified: Option<DateTime<Utc>>,
    /// MD5 or multipart `ETag` from S3. `None` for prefixes.
    /// Stored for potential future exact-match comparison;
    /// sync uses size+mtime by default.
    #[allow(dead_code)]
    pub e_tag: Option<String>,
    pub kind: EntryKind,
}

// ── Navigation state ──────────────────────────────────────────────────────────

/// The S3 "location" currently displayed in the right pane.
///
/// `prefix` is always either empty (bucket root) or ends with `'/'`.
/// Example: `{ bucket: "photos-yuri-budilov", prefix: "2023/Italy/" }`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct S3Location {
    pub bucket: String,
    /// Empty string means bucket root.  Otherwise always ends with `'/'`.
    pub prefix: String,
}

impl S3Location {
    /// Breadcrumb path for the address bar: `"photos-yuri-budilov/2023/Italy/"`.
    #[must_use]
    pub fn display_path(&self) -> String {
        if self.prefix.is_empty() {
            self.bucket.clone()
        } else {
            format!("{}/{}", self.bucket, self.prefix)
        }
    }

    /// Returns a new location one level deeper into `sub`.
    /// `sub` may or may not have a trailing `'/'`; this method normalises it.
    #[must_use]
    pub fn enter(&self, sub: &str) -> Self {
        let sub = sub.trim_end_matches('/');
        Self {
            bucket: self.bucket.clone(),
            prefix: format!("{}{}/", self.prefix, sub),
        }
    }

    /// Returns the parent location, or `None` if already at bucket root.
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        if self.prefix.is_empty() {
            return None;
        }
        // "2023/Italy/" → trim trailing '/' → "2023/Italy"
        //               → rfind('/') → "2023/"  (or "" for top-level prefix)
        let trimmed = self.prefix.trim_end_matches('/');
        let parent_prefix = match trimmed.rfind('/') {
            Some(i) => trimmed[..=i].to_string(), // includes the trailing '/'
            None => String::new(),                // was a top-level prefix
        };
        Some(Self {
            bucket: self.bucket.clone(),
            prefix: parent_prefix,
        })
    }

    /// True when at the bucket root (no prefix selected yet).
    #[must_use]
    #[allow(dead_code)]
    pub fn is_root(&self) -> bool {
        self.prefix.is_empty()
    }
}

// ── Transfer queue ────────────────────────────────────────────────────────────

/// Opaque monotonically-increasing identifier for a transfer job.
/// Generated on the UI thread. Safe to use as a `HashMap` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobId(pub u64);

/// What a single transfer job does.
#[derive(Debug, Clone)]
pub enum TransferKind {
    /// Local file → S3 object.
    Upload {
        local: PathBuf,
        bucket: String,
        key: String,
    },
    /// S3 object → local file.
    Download {
        bucket: String,
        key: String,
        local: PathBuf,
    },
    /// Delete an S3 object (`sync --delete` equivalent).
    DeleteRemote { bucket: String, key: String },
    /// Delete a local file (`sync --delete` in S3→local direction).
    DeleteLocal { path: PathBuf },
}

/// Lifecycle state of a transfer job.
#[derive(Debug, Clone)]
pub enum TransferStatus {
    Queued,
    InProgress,
    Done,
    /// Human-readable error message from anyhow or the AWS SDK.
    Failed(String),
    /// Sync determined source and destination already match; no transfer needed.
    #[allow(dead_code)]
    Skipped,
}

/// One entry in the transfer queue panel.
#[derive(Debug, Clone)]
pub struct TransferJob {
    pub id: JobId,
    pub kind: TransferKind,
    /// Used to display size and estimate transfer time. Zero for deletes.
    pub size_bytes: u64,
    pub status: TransferStatus,
}

impl TransferJob {
    /// One-line description for the transfer panel rows.
    #[must_use]
    pub fn description(&self) -> String {
        match &self.kind {
            TransferKind::Upload { local, key, .. } => {
                let name = local.file_name().unwrap_or_default().to_string_lossy();
                format!("↑  {name}  →  {key}")
            }
            TransferKind::Download { key, local, .. } => {
                let name = local.file_name().unwrap_or_default().to_string_lossy();
                format!("↓  {key}  →  {name}")
            }
            TransferKind::DeleteRemote { key, .. } => {
                format!("✕  s3:{key}")
            }
            TransferKind::DeleteLocal { path } => {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                format!("✕  local:{name}")
            }
        }
    }
}

// ── Upload storage class ──────────────────────────────────────────────────────
//
// Defined here rather than using aws_sdk_s3::types::StorageClass directly,
// so that types.rs remains free of AWS SDK imports.
// Conversion to the SDK type lives in s3/client.rs.

/// The S3 storage class applied to every upload (`PutObject`) call.
///
/// Serialises to/from canonical AWS uppercase strings via `SCREAMING_SNAKE_CASE`,
/// making the config.json human-readable: `"STANDARD_IA"`, `"STANDARD"`, etc.
///
/// Valid `config.json` values:
///   `"STANDARD"`, `"STANDARD_IA"`, `"ONEZONE_IA"`,
///   `"INTELLIGENT_TIERING"`, `"GLACIER"`, `"GLACIER_IR"`, `"DEEP_ARCHIVE"`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UploadStorageClass {
    Standard,
    StandardIa,
    OnezoneIa,
    IntelligentTiering,
    Glacier,
    GlacierIr,
    DeepArchive,
}

impl UploadStorageClass {
    /// Short display label for the status bar.
    #[must_use]
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Standard => "STANDARD",
            Self::StandardIa => "STANDARD_IA",
            Self::OnezoneIa => "ONEZONE_IA",
            Self::IntelligentTiering => "INTELLIGENT_TIERING",
            Self::Glacier => "GLACIER",
            Self::GlacierIr => "GLACIER_IR",
            Self::DeepArchive => "DEEP_ARCHIVE",
        }
    }
}

// ── Sync ──────────────────────────────────────────────────────────────────────

/// Direction of a sync operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    LocalToS3,
    S3ToLocal,
}

/// Options controlling a sync operation.
///
/// Persisted across sessions as part of `AppSettings` (eframe persistence).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncOptions {
    /// Delete destination items absent from the source.
    /// Mirrors `aws s3 sync --delete`. Default: `false` (safe — no accidental deletes).
    pub delete_extra: bool,
    /// Compute and display the plan without executing any transfers.
    pub dry_run: bool,
}

/// The output of `sync::engine::compute_plan_*()`.
///
/// Produced by comparing source and destination entry lists using size+mtime.
/// The engine is pure logic with no I/O. The UI displays this plan and the
/// user confirms before execution begins.
#[derive(Debug, Default)]
pub struct SyncPlan {
    /// Files/objects that need to be copied (new or size/mtime mismatch).
    pub to_transfer: Vec<TransferJob>,
    /// Destination items absent from source — populated only when
    /// `SyncOptions::delete_extra` is `true`.
    pub to_delete: Vec<TransferJob>,
    /// Count of entries already matching — displayed as "N files up to date".
    pub already_current: usize,
}

impl SyncPlan {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.to_transfer.is_empty() && self.to_delete.is_empty()
    }

    /// Total bytes to transfer (excludes deletes, which carry no size).
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.to_transfer.iter().map(|j| j.size_bytes).sum()
    }

    /// Total number of actions (transfers + deletes).
    #[must_use]
    pub fn action_count(&self) -> usize {
        self.to_transfer.len() + self.to_delete.len()
    }
}

// ── App → UI messaging ────────────────────────────────────────────────────────

/// Messages sent from tokio background tasks → UI thread via the flume channel.
///
/// The UI thread drains this channel at the top of every `App::ui()` call
/// via `while let Ok(msg) = self.msg_rx.try_recv() { self.apply_message(msg); }`.
///
/// After `apply_message` mutates `App` state, egui automatically re-renders
/// the new state on the next frame.
///
/// `AppMsg` must be `Send` (it crosses a thread boundary via flume).
#[derive(Debug)]
pub enum AppMsg {
    // ── S3 ────────────────────────────────────────────────────────────────────
    /// Bucket list loaded on startup.
    BucketsLoaded(Vec<String>),
    BucketsError(String),

    /// Directory listing for an S3 prefix completed.
    S3ListingDone {
        location: S3Location,
        entries: Vec<S3Entry>,
    },
    S3ListingError {
        location: S3Location,
        error: String,
    },

    // ── Local ─────────────────────────────────────────────────────────────────
    /// Directory listing for a local path completed.
    LocalListingDone {
        path: PathBuf,
        entries: Vec<LocalEntry>,
    },
    LocalListingError {
        path: PathBuf,
        error: String,
    },

    // ── Sync ──────────────────────────────────────────────────────────────────
    SyncPlanReady(SyncPlan),
    #[allow(dead_code)]
    SyncPlanError(String),

    // ── Transfer lifecycle ─────────────────────────────────────────────────────
    TransferStarted(JobId),
    TransferDone(JobId),
    TransferFailed {
        id: JobId,
        error: String,
    },

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

    // ── General ───────────────────────────────────────────────────────────────
    /// Non-job-specific background error (e.g. credential load failure).
    BackgroundError(String),
}

// ── Persisted application settings ───────────────────────────────────────────

/// The subset of application state that survives between sessions.
///
/// Stored by eframe's built-in persistence mechanism:
///   Linux   → `~/.local/share/aws-s3-explorer/`
///   Windows → `%APPDATA%\aws-s3-explorer\`
///
/// Kept separate from `App` because `App` contains non-serialisable items
/// (tokio Handle, flume channel ends, AWS client).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppSettings {
    /// Last local directory the user was browsing.
    pub last_local_path: Option<PathBuf>,
    /// Last S3 location (bucket + prefix) the user was browsing.
    pub last_s3_location: S3Location,
    /// Sync options — persisted so `delete_extra` stays off by default
    /// but remembers if the user explicitly enabled it last session.
    pub sync_options: SyncOptions,
}
