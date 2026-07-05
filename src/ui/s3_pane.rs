//! Right pane: S3 bucket and prefix browser.

use egui_extras::{Column, TableBuilder};

use crate::app::S3ExplorerApp;
use crate::types::{EntryKind, S3Location, TransferKind};
use crate::ui;

/// Draw the S3 pane into the given `ui`.
pub fn draw(app: &mut S3ExplorerApp, ui: &mut egui::Ui) {
    ui.heading("S3");

    // Address bar + navigation buttons.
    let mut refresh_requested = false;
    ui.horizontal(|ui| {
        let can_go_up = !app.s3_location.bucket.is_empty();
        if ui
            .add_enabled(can_go_up, egui::Button::new("[..]"))
            .clicked()
        {
            go_up(app);
        }
        if ui
            .button("⟳")
            .on_hover_text("Refresh current S3 listing")
            .clicked()
        {
            refresh_requested = true;
        }
        ui.label(if app.s3_location.bucket.is_empty() {
            "Buckets".to_owned()
        } else {
            app.s3_location.display_path()
        });
    });
    if refresh_requested {
        if app.s3_location.bucket.is_empty() {
            app.load_buckets();
        } else {
            let loc = app.s3_location.clone();
            app.load_s3_prefix(&loc);
        }
    }

    if app.s3_loading {
        ui.spinner();
        return;
    }

    // Show bucket list when no bucket is selected.
    if app.s3_location.bucket.is_empty() {
        draw_bucket_list(app, ui);
        return;
    }

    draw_prefix_contents(app, ui);
}

// ── Bucket list ───────────────────────────────────────────────────────────────

fn draw_bucket_list(app: &mut S3ExplorerApp, ui: &mut egui::Ui) {
    let buckets = app.bucket_list.clone();
    let mut navigate_to: Option<S3Location> = None;

    TableBuilder::new(ui)
        .striped(true)
        .resizable(false)
        .auto_shrink(false)
        .column(Column::auto()) // icon
        .column(Column::remainder()) // name
        .header(20.0, |mut header| {
            header.col(|_ui| {});
            header.col(|ui| {
                ui.strong("Bucket");
            });
        })
        .body(|mut body| {
            for bucket in &buckets {
                let bucket_name = bucket.clone();
                body.row(18.0, |mut row| {
                    row.col(|ui| {
                        ui.label("🪣");
                    });
                    row.col(|ui| {
                        if ui.selectable_label(false, &bucket_name).clicked() {
                            navigate_to = Some(S3Location {
                                bucket: bucket_name.clone(),
                                prefix: String::new(),
                            });
                        }
                    });
                });
            }
        });

    if let Some(loc) = navigate_to {
        app.load_s3_prefix(&loc);
    }
}

// ── Prefix contents ───────────────────────────────────────────────────────────

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

    if let Some(payload) = dropped
        && let ui::dnd::DragPayload::Local(items) = payload.as_ref().clone()
    {
        let is_move = ui.input(|i| i.modifiers.shift);
        app.handle_local_payload_dropped_on_s3(items, is_move);
    }
}

// ── Navigation ────────────────────────────────────────────────────────────────

fn go_up(app: &mut S3ExplorerApp) {
    if let Some(parent) = app.s3_location.parent() {
        app.load_s3_prefix(&parent);
    } else {
        // Already at bucket root — go back to bucket list.
        app.s3_location = S3Location::default();
        app.s3_entries.clear();
    }
}
