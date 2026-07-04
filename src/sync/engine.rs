//! Pure sync plan computation — no I/O.
//!
//! Compares source and destination entry lists and produces a `SyncPlan`
//! describing what needs to be transferred or deleted. This module is
//! fully unit-testable without any network or filesystem access.
//!
//! Match criterion: size in bytes AND last-modified timestamp match within
//! 2 seconds (tolerance accounts for FAT/NTFS/S3 timestamp precision differences).

use std::path::Path;

use crate::types::{
    EntryKind, JobId, LocalEntry, S3Entry, SyncOptions, SyncPlan, TransferJob, TransferKind,
    TransferStatus,
};

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Returns `true` if size and mtime are considered equal (within 2-second tolerance).
fn entries_match(
    size_a: u64,
    mtime_a: chrono::DateTime<chrono::Utc>,
    size_b: u64,
    mtime_b: Option<chrono::DateTime<chrono::Utc>>,
) -> bool {
    if size_a != size_b {
        return false;
    }
    mtime_b.is_some_and(|mb| (mtime_a - mb).num_seconds().abs() <= 2)
}

fn new_job_id(counter: &mut u64) -> JobId {
    let id = JobId(*counter);
    *counter += 1;
    id
}

// ── Local → S3 ────────────────────────────────────────────────────────────────

/// Compare a local directory listing against an S3 prefix listing and produce
/// a sync plan for uploading local files to S3.
///
/// Only `EntryKind::File` entries are considered; directories are ignored
/// (Phase 1: single-level sync only).
#[must_use]
pub fn compute_plan_local_to_s3(
    source_entries: &[LocalEntry],
    destination_entries: &[S3Entry],
    options: &SyncOptions,
    next_job_id: &mut u64,
    bucket: &str,
    s3_prefix: &str,
) -> SyncPlan {
    // Build lookup: destination name → S3Entry.
    let dest_map: std::collections::HashMap<&str, &S3Entry> = destination_entries
        .iter()
        .filter(|e| e.kind == EntryKind::File)
        .map(|e| (e.name.as_str(), e))
        .collect();

    let mut plan = SyncPlan::default();

    for src in source_entries.iter().filter(|e| e.kind == EntryKind::File) {
        match dest_map.get(src.name.as_str()) {
            Some(dest)
                if entries_match(
                    src.size_bytes,
                    src.modified,
                    dest.size_bytes,
                    dest.last_modified,
                ) =>
            {
                plan.already_current += 1;
            }
            None | Some(_) => {
                plan.to_transfer.push(TransferJob {
                    id: new_job_id(next_job_id),
                    kind: TransferKind::Upload {
                        local: src.path.clone(),
                        bucket: bucket.to_owned(),
                        key: format!("{}{}", s3_prefix, src.name),
                    },
                    size_bytes: src.size_bytes,
                    status: TransferStatus::Queued,
                });
            }
        }
    }

    if options.delete_extra {
        let source_names: std::collections::HashSet<&str> = source_entries
            .iter()
            .filter(|e| e.kind == EntryKind::File)
            .map(|e| e.name.as_str())
            .collect();

        for dest in destination_entries
            .iter()
            .filter(|e| e.kind == EntryKind::File)
        {
            if !source_names.contains(dest.name.as_str()) {
                plan.to_delete.push(TransferJob {
                    id: new_job_id(next_job_id),
                    kind: TransferKind::DeleteRemote {
                        bucket: bucket.to_owned(),
                        key: dest.key.clone(),
                    },
                    size_bytes: 0,
                    status: TransferStatus::Queued,
                });
            }
        }
    }

    plan
}

// ── S3 → Local ────────────────────────────────────────────────────────────────

/// Compare an S3 prefix listing against a local directory listing and produce
/// a sync plan for downloading S3 objects to the local filesystem.
///
/// Only `EntryKind::File` entries are considered; prefixes/directories are ignored
/// (Phase 1: single-level sync only).
#[must_use]
pub fn compute_plan_s3_to_local(
    source_entries: &[S3Entry],
    destination_entries: &[LocalEntry],
    options: &SyncOptions,
    next_job_id: &mut u64,
    bucket: &str,
    local_root: &Path,
) -> SyncPlan {
    // Build lookup: destination name → LocalEntry.
    let dest_map: std::collections::HashMap<&str, &LocalEntry> = destination_entries
        .iter()
        .filter(|e| e.kind == EntryKind::File)
        .map(|e| (e.name.as_str(), e))
        .collect();

    let mut plan = SyncPlan::default();

    for src in source_entries.iter().filter(|e| e.kind == EntryKind::File) {
        match dest_map.get(src.name.as_str()) {
            Some(dest)
                if entries_match(
                    src.size_bytes,
                    dest.modified,
                    dest.size_bytes,
                    src.last_modified,
                ) =>
            {
                plan.already_current += 1;
            }
            None | Some(_) => {
                plan.to_transfer.push(TransferJob {
                    id: new_job_id(next_job_id),
                    kind: TransferKind::Download {
                        bucket: bucket.to_owned(),
                        key: src.key.clone(),
                        local: local_root.join(&src.name),
                    },
                    size_bytes: src.size_bytes,
                    status: TransferStatus::Queued,
                });
            }
        }
    }

    if options.delete_extra {
        let source_names: std::collections::HashSet<&str> = source_entries
            .iter()
            .filter(|e| e.kind == EntryKind::File)
            .map(|e| e.name.as_str())
            .collect();

        for dest in destination_entries
            .iter()
            .filter(|e| e.kind == EntryKind::File)
        {
            if !source_names.contains(dest.name.as_str()) {
                plan.to_delete.push(TransferJob {
                    id: new_job_id(next_job_id),
                    kind: TransferKind::DeleteLocal {
                        path: dest.path.clone(),
                    },
                    size_bytes: 0,
                    status: TransferStatus::Queued,
                });
            }
        }
    }

    plan
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use chrono::{TimeZone as _, Utc};

    fn make_local(name: &str, size: u64, secs: i64) -> LocalEntry {
        LocalEntry {
            name: name.to_owned(),
            path: PathBuf::from(name),
            size_bytes: size,
            modified: Utc.timestamp_opt(secs, 0).unwrap(),
            kind: EntryKind::File,
        }
    }

    fn make_s3(name: &str, size: u64, secs: Option<i64>) -> S3Entry {
        S3Entry {
            key: format!("prefix/{name}"),
            name: name.to_owned(),
            size_bytes: size,
            last_modified: secs.map(|s| Utc.timestamp_opt(s, 0).unwrap()),
            e_tag: None,
            kind: EntryKind::File,
        }
    }

    fn no_delete_opts() -> SyncOptions {
        SyncOptions {
            delete_extra: false,
            dry_run: false,
        }
    }

    fn delete_opts() -> SyncOptions {
        SyncOptions {
            delete_extra: true,
            dry_run: false,
        }
    }

    // ── Local → S3 ───────────────────────────────────────────────────────────

    #[test]
    fn local_to_s3_new_file_goes_to_transfer() {
        let src = vec![make_local("img.jpg", 1000, 1000)];
        let dst: Vec<S3Entry> = vec![];
        let plan = compute_plan_local_to_s3(&src, &dst, &no_delete_opts(), &mut 0, "bucket", "p/");
        assert_eq!(plan.to_transfer.len(), 1);
        assert_eq!(plan.already_current, 0);
        assert!(plan.to_delete.is_empty());
    }

    #[test]
    fn local_to_s3_matching_file_is_skipped() {
        let src = vec![make_local("img.jpg", 1000, 1000)];
        let dst = vec![make_s3("img.jpg", 1000, Some(1000))];
        let plan = compute_plan_local_to_s3(&src, &dst, &no_delete_opts(), &mut 0, "bucket", "p/");
        assert!(plan.to_transfer.is_empty());
        assert_eq!(plan.already_current, 1);
    }

    #[test]
    fn local_to_s3_size_mismatch_triggers_upload() {
        let src = vec![make_local("img.jpg", 2000, 1000)];
        let dst = vec![make_s3("img.jpg", 1000, Some(1000))];
        let plan = compute_plan_local_to_s3(&src, &dst, &no_delete_opts(), &mut 0, "bucket", "p/");
        assert_eq!(plan.to_transfer.len(), 1);
        assert_eq!(plan.already_current, 0);
    }

    #[test]
    fn local_to_s3_extra_at_dest_ignored_when_delete_false() {
        let src: Vec<LocalEntry> = vec![];
        let dst = vec![make_s3("extra.jpg", 500, Some(1000))];
        let plan = compute_plan_local_to_s3(&src, &dst, &no_delete_opts(), &mut 0, "bucket", "p/");
        assert!(plan.to_transfer.is_empty());
        assert!(plan.to_delete.is_empty());
    }

    #[test]
    fn local_to_s3_extra_at_dest_deleted_when_delete_true() {
        let src: Vec<LocalEntry> = vec![];
        let dst = vec![make_s3("extra.jpg", 500, Some(1000))];
        let plan = compute_plan_local_to_s3(&src, &dst, &delete_opts(), &mut 0, "bucket", "p/");
        assert!(plan.to_transfer.is_empty());
        assert_eq!(plan.to_delete.len(), 1);
    }

    #[test]
    fn local_to_s3_timestamp_within_2s_tolerance_is_current() {
        // mtime differs by exactly 2 seconds — should be "already current".
        let src = vec![make_local("img.jpg", 1000, 1002)];
        let dst = vec![make_s3("img.jpg", 1000, Some(1000))];
        let plan = compute_plan_local_to_s3(&src, &dst, &no_delete_opts(), &mut 0, "bucket", "p/");
        assert!(plan.to_transfer.is_empty());
        assert_eq!(plan.already_current, 1);
    }

    #[test]
    fn local_to_s3_timestamp_beyond_2s_triggers_upload() {
        // mtime differs by 3 seconds — should trigger upload.
        let src = vec![make_local("img.jpg", 1000, 1003)];
        let dst = vec![make_s3("img.jpg", 1000, Some(1000))];
        let plan = compute_plan_local_to_s3(&src, &dst, &no_delete_opts(), &mut 0, "bucket", "p/");
        assert_eq!(plan.to_transfer.len(), 1);
    }

    // ── S3 → Local ───────────────────────────────────────────────────────────

    #[test]
    fn s3_to_local_new_object_goes_to_transfer() {
        let src = vec![make_s3("img.jpg", 1000, Some(1000))];
        let dst: Vec<LocalEntry> = vec![];
        let plan = compute_plan_s3_to_local(
            &src,
            &dst,
            &no_delete_opts(),
            &mut 0,
            "bucket",
            Path::new("/local"),
        );
        assert_eq!(plan.to_transfer.len(), 1);
        assert_eq!(plan.already_current, 0);
    }

    #[test]
    fn s3_to_local_matching_object_is_skipped() {
        let src = vec![make_s3("img.jpg", 1000, Some(1000))];
        let dst = vec![make_local("img.jpg", 1000, 1000)];
        let plan = compute_plan_s3_to_local(
            &src,
            &dst,
            &no_delete_opts(),
            &mut 0,
            "bucket",
            Path::new("/local"),
        );
        assert!(plan.to_transfer.is_empty());
        assert_eq!(plan.already_current, 1);
    }

    #[test]
    fn s3_to_local_extra_local_deleted_when_delete_true() {
        let src: Vec<S3Entry> = vec![];
        let dst = vec![make_local("old.jpg", 500, 1000)];
        let plan = compute_plan_s3_to_local(
            &src,
            &dst,
            &delete_opts(),
            &mut 0,
            "bucket",
            Path::new("/local"),
        );
        assert!(plan.to_transfer.is_empty());
        assert_eq!(plan.to_delete.len(), 1);
        assert!(matches!(
            plan.to_delete[0].kind,
            TransferKind::DeleteLocal { .. }
        ));
    }
}
