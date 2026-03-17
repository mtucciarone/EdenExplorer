use eframe::egui;
use egui_extras::{TableBuilder, Column};
use std::collections::HashMap;
use std::path::PathBuf;
use crate::state::FileItem;
use super::sorting::SortColumn;
use super::formatting::format_size;
use crate::app::icons::IconCache;
use crate::app::utils::drive_usage_bar;
use egui_phosphor::regular;

pub enum ItemViewerAction {
    Sort(SortColumn),
    Select(std::path::PathBuf),
    Open(std::path::PathBuf),
    OpenInNewTab(std::path::PathBuf),
    Context(ItemViewerContextAction),
}

#[derive(Clone)]
pub enum ItemViewerContextAction {
    Cut(std::path::PathBuf),
    Copy(std::path::PathBuf),
    Paste,
    Rename(std::path::PathBuf),
    Delete(std::path::PathBuf),
    Properties(std::path::PathBuf),
}

#[derive(Clone, Copy)]
pub struct ItemViewerFolderSizeState {
    pub bytes: u64,
    pub done: bool,
}

pub fn draw_item_viewer(
    ui: &mut egui::Ui,
    files: &Vec<FileItem>,
    folder_sizes: &HashMap<PathBuf, ItemViewerFolderSizeState>,
    selected_path: Option<&PathBuf>,
    paste_enabled: bool,
    sort_column: SortColumn,
    sort_ascending: bool,
    icon_cache: &IconCache,
) -> Option<ItemViewerAction> {
    let text_height = 22.0; // 🔥 unified row height
    let header_height = text_height + 6.0;
    let header_gap = 6.0;
    let available_width = ui.available_width();
    let context_menu_open = ui.ctx().is_popup_open();
    let mut any_row_hovered = false;

    let mut action: Option<ItemViewerAction> = None;

    // 🔥 detect drive view
    let is_drive_view = files.iter().any(|f| f.total_space.is_some());

    let mut table = TableBuilder::new(ui)
        .striped(true)
        .sense(egui::Sense::click_and_drag())
        .animate_scrolling(true)
        .resizable(true)
        .column(Column::exact(available_width * 0.5).resizable(true)) // Name
        .column(Column::exact(available_width * 0.2).resizable(true)); // Size

    if is_drive_view {
        table = table.column(Column::exact(available_width * 0.3).resizable(true)); // Usage
    } else {
        table = table.column(Column::exact(available_width * 0.3).resizable(true)); // Modified
    }

    table
        .header(header_height, |mut header| {
            // Name
            header.col(|ui| {
                ui.add_space(2.0);
                let (label, arrow) = match sort_column {
                    SortColumn::Name => (
                        "Name",
                        if sort_ascending { regular::CARET_UP } else { regular::CARET_DOWN },
                    ),
                    _ => ("Name", ""),
                };

                let resp = ui.add(
                    egui::Label::new(format!("{label} {arrow}").trim_end())
                        .selectable(false)
                        .sense(egui::Sense::click()),
                );
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    action = Some(ItemViewerAction::Sort(SortColumn::Name));
                }
            });

            // Size
            header.col(|ui| {
                ui.add_space(2.0);
                let (label, arrow) = match sort_column {
                    SortColumn::Size => (
                        "Size",
                        if sort_ascending { regular::CARET_UP } else { regular::CARET_DOWN },
                    ),
                    _ => ("Size", ""),
                };

                let resp = ui.add(
                    egui::Label::new(format!("{label} {arrow}").trim_end())
                        .selectable(false)
                        .sense(egui::Sense::click()),
                );
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    action = Some(ItemViewerAction::Sort(SortColumn::Size));
                }
            });

            // Usage
            if is_drive_view {
                header.col(|ui| {
                    ui.add(
                        egui::Label::new("Usage")
                            .selectable(false)
                            .sense(egui::Sense::click()),
                    );
                });
            } else {
                header.col(|ui| {
                    let (label, arrow) = match sort_column {
                        SortColumn::Modified => (
                            "Modified",
                            if sort_ascending { regular::CARET_UP } else { regular::CARET_DOWN },
                        ),
                        _ => ("Modified", ""),
                    };

                    let resp = ui.add(
                        egui::Label::new(format!("{label} {arrow}").trim_end())
                            .selectable(false)
                            .sense(egui::Sense::click()),
                    );
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        action = Some(ItemViewerAction::Sort(SortColumn::Modified));
                    }
                });
            }
        })
        .body(|mut body| {
            if files.is_empty() {
                body.row(text_height, |mut row| {
                    row.col(|ui| {
                        ui.label("This folder is empty");
                    });
                });
                return;
            }

            body.rows(text_height, files.len(), |mut row| {
                let idx = row.index();
                let file = &files[idx];

                let is_selected = selected_path
                    .map(|p| p == &file.path)
                    .unwrap_or(false);

                row.set_selected(is_selected);

                // Name
                row.col(|ui| {
                    ui.horizontal(|ui| {
                        if let Some(icon) = icon_cache.get(&file.path, file.is_dir) {
                            ui.add(
                                egui::Image::new(&icon)
                                    .fit_to_exact_size(egui::vec2(16.0, 16.0)),
                            );
                        } else {
                            ui.add_space(16.0);
                        }

                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&file.name).size(13.0),
                            )
                            .sense(egui::Sense::click()),
                        );
                    });
                });

                // Size
                row.col(|ui| {
                    if let (Some(total), Some(free)) = (file.total_space, file.free_space) {
                        let gb = 1024.0 * 1024.0 * 1024.0;

                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{:.1} / {:.1} GB",
                                        free as f64 / gb,
                                        total as f64 / gb
                                    ))
                                    .size(13.0),
                                );
                            },
                        );
                    } else if file.is_dir {
                        if let Some(state) = folder_sizes.get(&file.path) {
                            let label = format_size(state.bytes);
                            ui.label(
                                egui::RichText::new(if state.done {
                                    label
                                } else {
                                    format!("⏳ {}", label)
                                })
                                .size(13.0),
                            );
                        } else {
                            ui.label(egui::RichText::new("—").size(13.0));
                        }
                    } else if let Some(size) = file.file_size {
                        ui.label(egui::RichText::new(format_size(size)).size(13.0));
                    } else {
                        ui.label(egui::RichText::new("—").size(13.0));
                    }
                });

                if is_drive_view {
                    row.col(|ui| {
                        if let (Some(total), Some(free)) = (file.total_space, file.free_space) {
                            let bar_height = text_height * 0.85;
                            let vertical_padding = (text_height - bar_height) * 0.5;

                            ui.add_space(vertical_padding);
                            drive_usage_bar(ui, total, free, bar_height);
                        }
                    });
                } else {
                    row.col(|ui| {
                        if let Some(m) = &file.modified_time {
                            ui.label(egui::RichText::new(m).size(13.0));
                        } else {
                            ui.label(egui::RichText::new("—").size(13.0));
                        }
                    });
                }

                let row_resp = row.response();

                // 🔥 FIX hover
                if !context_menu_open && row_resp.hovered() {
                    row.set_hovered(true);
                    any_row_hovered = true;
                }

                if row_resp.clicked() {
                    action = Some(ItemViewerAction::Select(file.path.clone()));
                }

                if row_resp.double_clicked() && file.is_dir {
                    action = Some(ItemViewerAction::Open(file.path.clone()));
                }
                if row_resp.middle_clicked() && file.is_dir {
                    action = Some(ItemViewerAction::OpenInNewTab(file.path.clone()));
                }

                row_resp.context_menu(|ui| {
                    if ui.button("Cut").clicked() {
                        action = Some(ItemViewerAction::Context(ItemViewerContextAction::Cut(file.path.clone())));
                        ui.close();
                    }
                    if ui.button("Copy").clicked() {
                        action = Some(ItemViewerAction::Context(ItemViewerContextAction::Copy(file.path.clone())));
                        ui.close();
                    }
                    if ui.add_enabled(paste_enabled, egui::Button::new("Paste")).clicked() {
                        action = Some(ItemViewerAction::Context(ItemViewerContextAction::Paste));
                        ui.close();
                    }
                    if ui.button("Rename").clicked() {
                        action = Some(ItemViewerAction::Context(ItemViewerContextAction::Rename(file.path.clone())));
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        action = Some(ItemViewerAction::Context(ItemViewerContextAction::Delete(file.path.clone())));
                        ui.close();
                    }
                    if ui.button("Properties").clicked() {
                        action = Some(ItemViewerAction::Context(ItemViewerContextAction::Properties(file.path.clone())));
                        ui.close();
                    }
                    if ui.button("Open in new tab").clicked() {
                        action = Some(ItemViewerAction::OpenInNewTab(file.path.clone()));
                        ui.close();
                    }
                });
            });
        });

    ui.add_space(header_gap);

    if !context_menu_open && any_row_hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    action
}
