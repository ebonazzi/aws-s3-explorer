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

                body.row(18.0, |mut row| {
                    row.col(|ui| {
                        ui.label(ui::kind_icon(entry_kind));
                    });
                    row.col(|ui| {
                        let resp = ui.selectable_label(is_selected, &entry_name);

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
}
