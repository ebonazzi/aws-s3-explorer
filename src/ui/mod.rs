//! UI module: re-exports and shared helpers.

pub mod dnd;
pub mod local_pane;
pub mod s3_pane;
pub mod toolbar;
pub mod transfer_panel;

use humansize::{BINARY, format_size};

use crate::types::TransferStatus;

/// Format bytes as a human-readable binary string: "1.4 MiB", "230 KiB".
#[must_use]
pub fn format_bytes(bytes: u64) -> String {
    format_size(bytes, BINARY)
}

/// Colour used to render a transfer status cell in the panel.
#[must_use]
pub fn status_colour(status: &TransferStatus) -> egui::Color32 {
    match status {
        TransferStatus::Queued => egui::Color32::GRAY,
        TransferStatus::InProgress => egui::Color32::YELLOW,
        TransferStatus::Done => egui::Color32::GREEN,
        TransferStatus::Failed(_) => egui::Color32::RED,
        TransferStatus::Skipped => egui::Color32::DARK_GRAY,
    }
}

/// Unicode icon for a directory or file entry.
#[must_use]
pub fn kind_icon(kind: crate::types::EntryKind) -> &'static str {
    match kind {
        crate::types::EntryKind::Directory => "📁",
        crate::types::EntryKind::File => "📄",
    }
}
