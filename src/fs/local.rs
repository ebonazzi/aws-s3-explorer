//! Local filesystem operations.
//!
//! All functions run on a `tokio::task::spawn_blocking` thread to avoid
//! blocking the async scheduler with synchronous `std::fs` calls.

use std::path::PathBuf;

use anyhow::Result;

use crate::types::{EntryKind, LocalEntry};

/// List one directory level, returning entries sorted directories-first.
///
/// # Errors
///
/// Returns an error if the directory cannot be read or any entry's metadata
/// is inaccessible.
pub async fn list_directory(path: &std::path::Path) -> Result<Vec<LocalEntry>> {
    use std::time::SystemTime;
    let path = path.to_path_buf();

    tokio::task::spawn_blocking(move || {
        let mut entries = Vec::new();

        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            let Ok(metadata) = entry.metadata() else {
                continue; // skip entries we can't read (permissions etc.)
            };
            let name = entry.file_name().to_string_lossy().into_owned();

            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let modified: chrono::DateTime<chrono::Utc> = modified.into();

            let kind = if metadata.is_dir() {
                EntryKind::Directory
            } else {
                EntryKind::File
            };

            entries.push(LocalEntry {
                name,
                path: entry.path(),
                size_bytes: if metadata.is_dir() { 0 } else { metadata.len() },
                modified,
                kind,
            });
        }

        // Sort: directories first, then by name case-insensitively.
        entries.sort_by(|a, b| match (&a.kind, &b.kind) {
            (EntryKind::Directory, EntryKind::File) => std::cmp::Ordering::Less,
            (EntryKind::File, EntryKind::Directory) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });

        Ok(entries)
    })
    .await?
}

/// Walk a directory tree and return every file with its size in bytes.
///
/// Skips inaccessible entries (permission-denied etc.) without failing.
/// Symlinks are not followed.
///
/// # Errors
///
/// Returns an error only if the `spawn_blocking` thread panics.
pub async fn collect_files_recursive(root: &std::path::Path) -> Result<Vec<(PathBuf, u64)>> {
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut files = Vec::new();
        for entry in walkdir::WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let size = entry.metadata().map_or(0, |m| m.len());
            files.push((entry.into_path(), size));
        }
        Ok(files)
    })
    .await?
}
