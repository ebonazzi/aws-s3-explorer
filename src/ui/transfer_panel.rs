//! Bottom panel: transfer queue display.

use egui_extras::{Column, TableBuilder};

use crate::app::S3ExplorerApp;
use crate::types::TransferStatus;
use crate::ui;

/// Draw the transfer panel into the given `ui`.
pub fn draw(app: &mut S3ExplorerApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.strong("Transfers");
        if ui.button("Clear Completed").clicked() {
            app.transfer_jobs
                .retain(|j| !matches!(j.status, TransferStatus::Done | TransferStatus::Skipped));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let active = app
                .transfer_jobs
                .iter()
                .filter(|j| matches!(j.status, TransferStatus::InProgress))
                .count();
            let queued = app
                .transfer_jobs
                .iter()
                .filter(|j| matches!(j.status, TransferStatus::Queued))
                .count();
            if active > 0 || queued > 0 {
                ui.label(format!("{active} active, {queued} queued"));
            }
        });
    });

    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .column(Column::remainder()) // description
                .column(Column::initial(80.0)) // size
                .column(Column::initial(100.0)) // status
                .header(18.0, |mut header| {
                    header.col(|ui| {
                        ui.strong("Description");
                    });
                    header.col(|ui| {
                        ui.strong("Size");
                    });
                    header.col(|ui| {
                        ui.strong("Status");
                    });
                })
                .body(|mut body| {
                    for job in &app.transfer_jobs {
                        let description = job.description();
                        let size = job.size_bytes;
                        let status_text = status_label(&job.status);
                        let colour = ui::status_colour(&job.status);

                        body.row(16.0, |mut row| {
                            row.col(|ui| {
                                ui.label(&description);
                            });
                            row.col(|ui| {
                                if size > 0 {
                                    ui.label(ui::format_bytes(size));
                                } else {
                                    ui.label("—");
                                }
                            });
                            row.col(|ui| {
                                ui.colored_label(colour, status_text);
                            });
                        });
                    }
                });
        });
}

fn status_label(status: &TransferStatus) -> String {
    match status {
        TransferStatus::Queued => "Queued".to_owned(),
        TransferStatus::InProgress => "⟳ In Progress".to_owned(),
        TransferStatus::Done => "✓ Done".to_owned(),
        TransferStatus::Failed(msg) => format!("✕ {msg}"),
        TransferStatus::Skipped => "— Skipped".to_owned(),
    }
}
