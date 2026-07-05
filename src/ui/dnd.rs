//! Drag-and-drop payload and selection-aware drag-set helpers shared by
//! both panes.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::types::{LocalEntry, S3Entry};

/// What is being dragged between the two panes.
#[allow(dead_code)]
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
#[allow(dead_code)]
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

/// Compute the set of S3 entries that should be dragged when the user starts
/// dragging `clicked`: the whole current selection if `clicked` is part of
/// it, otherwise just `clicked` alone.
#[allow(dead_code)]
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
