use eframe::egui;
use egui_extras::{TableBuilder, Column};
use std::collections::HashMap;
use std::path::PathBuf;
use crate::state::FileItem;
use super::sorting::SortColumn;
use super::formatting::format_size;
use crate::app::icons::IconCache; // 🔥 FIX

pub enum TableAction {
    Sort(SortColumn),
    Open(std::path::PathBuf),
}

pub fn draw_table(
    ui: &mut egui::Ui,
    files: &Vec<FileItem>,
    folder_sizes: &HashMap<PathBuf, u64>,
    sort_column: SortColumn,
    sort_ascending: bool,
    icon_cache: &IconCache,
) -> Option<TableAction> {
    let text_height = 24.0;
    let available_width = ui.available_width();

    let mut action: Option<TableAction> = None;

    TableBuilder::new(ui)
        .striped(true)
        .column(Column::exact(available_width * 0.6))
        .column(Column::exact(available_width * 0.2))
        .column(Column::exact(available_width * 0.2))
        .header(text_height, |mut header| {
            header.col(|ui| {
                let label = match sort_column {
                    SortColumn::Name => {
                        if sort_ascending { "Name ▲" } else { "Name ▼" }
                    }
                    _ => "Name",
                };

                if ui.button(label).clicked() {
                    action = Some(TableAction::Sort(SortColumn::Name));
                }
            });

            header.col(|ui| {
                let label = match sort_column {
                    SortColumn::Size => {
                        if sort_ascending { "Size ▲" } else { "Size ▼" }
                    }
                    _ => "Size",
                };

                if ui.button(label).clicked() {
                    action = Some(TableAction::Sort(SortColumn::Size));
                }
            });

            header.col(|ui| {
                let label = match sort_column {
                    SortColumn::Modified => {
                        if sort_ascending { "Modified ▲" } else { "Modified ▼" }
                    }
                    _ => "Modified",
                };

                if ui.button(label).clicked() {
                    action = Some(TableAction::Sort(SortColumn::Modified));
                }
            });
        })
        .body(|mut body| {
            for file in files.iter() {
                body.row(text_height, |mut row| {
                    row.col(|ui| {
                        let mut label_clicked = false;
                        let mut icon_clicked = false;

                        let row_resp = ui
                            .horizontal(|ui| {
                                if let Some(icon) = icon_cache.get(&file.path, file.is_dir) {
                                    let icon_resp = ui.add(
                                        egui::Image::new(&icon)
                                            .fit_to_exact_size(egui::vec2(20.0, 20.0)),
                                    );
                                    icon_clicked = icon_resp.clicked();
                                } else {
                                    ui.add_space(20.0);
                                }

                                let label_resp = ui.selectable_label(false, &file.name);
                                label_clicked = label_resp.clicked();
                            })
                            .response;

                        if file.is_dir && (row_resp.clicked() || label_clicked || icon_clicked) {
                            action = Some(TableAction::Open(file.path.clone()));
                        }
                    });

                    row.col(|ui| {
                        if let (Some(total), Some(free)) =
                            (file.total_space, file.free_space)
                        {
                            let gb = 1024 * 1024 * 1024;
                            let used = total.saturating_sub(free);
                            let used_ratio = if total == 0 {
                                0.0
                            } else {
                                used as f32 / total as f32
                            };

                            let bar_color = if used_ratio > 0.95 {
                                egui::Color32::from_rgb(200, 72, 72)
                            } else if used_ratio >= 0.85 && used_ratio <= 0.95 {
                                egui::Color32::from_rgb(214, 170, 76)
                            } else {
                                egui::Color32::from_rgb(88, 170, 120)
                            };

                            ui.label(format!(
                                "{:.1} / {:.1} GB",
                                free as f64 / gb as f64,
                                total as f64 / gb as f64
                            ));
                            ui.add(
                                egui::ProgressBar::new(used_ratio)
                                    .fill(bar_color)
                                    .show_percentage(),
                            );
                        } else if file.is_dir {
                            if let Some(size) = folder_sizes.get(&file.path) {
                                ui.label(format_size(*size));
                            } else {
                                ui.label("Calculating...");
                            }
                        } else if let Some(size) = file.file_size {
                            ui.label(format_size(size));
                        } else {
                            ui.label("—");
                        }
                    });

                    row.col(|ui| {
                        if let Some(m) = &file.modified_time {
                            ui.label(m);
                        } else {
                            ui.label("—");
                        }
                    });
                });
            }
        });

    action
}
