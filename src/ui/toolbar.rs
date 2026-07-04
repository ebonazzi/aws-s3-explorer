//! Top toolbar: address bars, action buttons, and status bar.

use std::path::PathBuf;

use crate::app::S3ExplorerApp;
use crate::types::{EntryKind, SyncDirection, TransferKind};

/// Draw the toolbar into the given `ui`.
pub fn draw(app: &mut S3ExplorerApp, ui: &mut egui::Ui) {
    // Row 1: local-side controls | S3-side controls.
    ui.horizontal(|ui| {
        // ── Local side ───────────────────────────────────────────────────────
        ui.label("Local:");
        ui.label(app.local_path.display().to_string());
        ui.separator();

        if ui.button("Choose Folder").clicked() {
            // Blocking native dialog — pauses the render loop while open.
            if let Some(picked) = rfd::FileDialog::new().pick_folder() {
                app.load_local_directory(&picked);
            }
        }

        let has_local_selection = !app.local_selected.is_empty();

        if ui
            .add_enabled(
                has_local_selection && !app.s3_location.bucket.is_empty(),
                egui::Button::new("↑ Upload"),
            )
            .clicked()
        {
            upload_selected(app);
        }

        let can_sync = !app.local_path.as_os_str().is_empty() && !app.s3_location.bucket.is_empty();

        if ui
            .add_enabled(can_sync, egui::Button::new("⇄ Sync →"))
            .clicked()
        {
            app.start_sync(SyncDirection::LocalToS3);
        }

        ui.separator();

        // ── S3 side ──────────────────────────────────────────────────────────
        ui.label("S3:");
        let s3_addr = if app.s3_location.bucket.is_empty() {
            if app.buckets_loaded {
                "(bucket list)".to_owned()
            } else {
                "(loading…)".to_owned()
            }
        } else {
            app.s3_location.display_path()
        };
        ui.label(s3_addr);
        ui.separator();

        let has_s3_selection = !app.s3_selected.is_empty();

        if ui
            .add_enabled(has_s3_selection, egui::Button::new("↓ Download"))
            .clicked()
        {
            download_selected(app);
        }

        if ui
            .add_enabled(can_sync, egui::Button::new("⇄ Sync ←"))
            .clicked()
        {
            app.start_sync(SyncDirection::S3ToLocal);
        }
    });

    ui.separator();

    // Row 2: status bar.
    ui.horizontal(|ui| {
        ui.label(&app.status_message);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(format!(
                "Storage class: {}",
                app.config.upload_storage_class.display_name()
            ));
        });
    });
}

// ── Action helpers ────────────────────────────────────────────────────────────

fn upload_selected(app: &mut S3ExplorerApp) {
    let bucket = app.s3_location.bucket.clone();
    let prefix = app.s3_location.prefix.clone();

    // Only files can be uploaded; directories are not supported in Phase 1.
    let has_dirs = app
        .local_entries
        .iter()
        .any(|e| e.kind == EntryKind::Directory && app.local_selected.contains(&e.path));

    let uploads: Vec<(PathBuf, String, u64)> = app
        .local_entries
        .iter()
        .filter(|e| e.kind == EntryKind::File && app.local_selected.contains(&e.path))
        .map(|e| {
            let key = format!("{prefix}{}", e.name);
            (e.path.clone(), key, e.size_bytes)
        })
        .collect();

    if uploads.is_empty() {
        if has_dirs {
            "Folder upload is not supported — select individual files to upload."
                .clone_into(&mut app.status_message);
        }
        return;
    }

    for (local, key, size) in uploads {
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

fn download_selected(app: &mut S3ExplorerApp) {
    let bucket = app.s3_location.bucket.clone();
    let local_root = app.local_path.clone();
    let downloads: Vec<(String, String, u64)> = app
        .s3_entries
        .iter()
        .filter(|e| e.kind == EntryKind::File && app.s3_selected.contains(&e.key))
        .map(|e| (e.key.clone(), e.name.clone(), e.size_bytes))
        .collect();

    for (key, name, size) in downloads {
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
}
