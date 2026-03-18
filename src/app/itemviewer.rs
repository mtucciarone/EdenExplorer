use super::formatting::format_size;
use super::sorting::SortColumn;
use crate::app::icons::IconCache;
use crate::app::utils::drive_usage_bar;
use crate::state::FileItem;
use eframe::egui;
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_TYPENAME, SHGFI_USEFILEATTRIBUTES};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::core::PCWSTR;

fn get_file_type_name(ext: &str, cache: &mut HashMap<String, String>) -> String {
    // Check cache first
    if let Some(cached) = cache.get(ext) {
        return cached.clone();
    }

    // Ensure extension starts with "."
    let ext_formatted = if ext.starts_with('.') {
        ext.to_string()
    } else {
        format!(".{}", ext)
    };

    let wide: Vec<u16> = OsStr::new(&ext_formatted)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut info = SHFILEINFOW::default();

    let result = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_TYPENAME | SHGFI_USEFILEATTRIBUTES,
        )
    };

    // Convert UTF-16 buffer to Rust String
    let len = info.szTypeName.iter().position(|&c| c == 0).unwrap_or(0);
    let type_name = String::from_utf16_lossy(&info.szTypeName[..len]);

    // Cache the result
    cache.insert(ext.to_string(), type_name.clone());

    type_name
}

pub enum ItemViewerAction {
    Sort(SortColumn),
    Select(PathBuf),
    Open(PathBuf),
    OpenInNewTab(PathBuf),
    Context(ItemViewerContextAction),
    RenameRequest(PathBuf, String),
    RenameCancel,
    StartEdit(PathBuf),
}

#[derive(Clone)]
pub enum ItemViewerContextAction {
    Cut(PathBuf),
    Copy(PathBuf),
    Paste,
    Rename(PathBuf),
    Delete(PathBuf),
    Properties(PathBuf),
    Undo,
    Redo,
}

#[derive(Clone, Copy)]
pub struct ItemViewerFolderSizeState {
    pub bytes: u64,
    pub done: bool,
}

pub struct RenameState {
    pub path: PathBuf,
    pub new_name: String,
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
    mut rename_state: Option<&mut RenameState>,
    palette: &crate::app::features::ThemePalette,
    file_type_cache: &mut HashMap<String, String>,
) -> Option<ItemViewerAction> {
    let row_height = ui.text_style_height(&egui::TextStyle::Button) + 4.0;
    let header_height = row_height + 6.0;
    let header_gap = 6.0;
    let available_width = ui.available_width();
    let context_menu_open = ui.ctx().is_popup_open();
    let mut any_row_hovered = false;
    let mut action: Option<ItemViewerAction> = None;

    let is_drive_view = files.iter().any(|f| f.total_space.is_some());

    // Check if we're currently editing a file
    let editing_path = rename_state.as_ref().map(|rs| rs.path.clone());

    if files.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label("This folder is empty");
        });
        return action;
    }

    // Wrap table in a scroll area for horizontal scrolling
    egui::ScrollArea::both().show(ui, |ui| {
        let mut table = TableBuilder::new(ui)
            .striped(false)
            .sense(egui::Sense::click_and_drag())
            .animate_scrolling(true)
            .resizable(true)
            .id_salt("item_viewer_table")
            .column(Column::initial(available_width * 0.35).at_least(200.0).resizable(true)) // Name
            .column(Column::initial(available_width * 0.1).at_least(60.0).resizable(true)) // Type
            .column(Column::initial(available_width * 0.15).at_least(80.0).resizable(true)); // Size

        if is_drive_view {
            table = table.column(Column::remainder().at_least(150.0).resizable(true));
        // Usage
        } else {
            table = table
                .column(Column::initial(available_width * 0.2).at_least(120.0).resizable(true)) // Modified
                .column(Column::remainder().at_least(120.0).resizable(true)); // Created
        }

    table
        .header(header_height, |mut header| {
            // Name column
            header.col(|ui| {
                ui.add_space(2.0);
                let (label, arrow) = match sort_column {
                    SortColumn::Name => (
                        "Name",
                        if sort_ascending {
                            regular::CARET_UP
                        } else {
                            regular::CARET_DOWN
                        },
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

            // Type column
            header.col(|ui| {
                ui.add_space(2.0);
                let (label, arrow) = match sort_column {
                    SortColumn::Type => (
                        "Type",
                        if sort_ascending {
                            regular::CARET_UP
                        } else {
                            regular::CARET_DOWN
                        },
                    ),
                    _ => ("Type", ""),
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
                    action = Some(ItemViewerAction::Sort(SortColumn::Type));
                }
            });

            // Size column
            header.col(|ui| {
                ui.add_space(2.0);
                let (label, arrow) = match sort_column {
                    SortColumn::Size => (
                        "Size",
                        if sort_ascending {
                            regular::CARET_UP
                        } else {
                            regular::CARET_DOWN
                        },
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

            // Usage / Modified column
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
                            if sort_ascending {
                                regular::CARET_UP
                            } else {
                                regular::CARET_DOWN
                            },
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

                // Created column
                header.col(|ui| {
                    let (label, arrow) = match sort_column {
                        SortColumn::Created => (
                            "Created",
                            if sort_ascending {
                                regular::CARET_UP
                            } else {
                                regular::CARET_DOWN
                            },
                        ),
                        _ => ("Created", ""),
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
                        action = Some(ItemViewerAction::Sort(SortColumn::Created));
                    }
                });
            }
        })
        .body(|mut body| {
            body.rows(row_height, files.len(), |mut row| {
                let idx = row.index();
                let file = &files[idx];
                let is_selected = selected_path.map(|p| p == &file.path).unwrap_or(false);
                row.set_selected(is_selected);

                row.col(|ui| {
                    let available_width = ui.available_width();

                    let (rect, item_resp) =
                        ui.allocate_exact_size(egui::vec2(available_width, row_height), egui::Sense::hover());

                    // --- ICON ---
                    let icon_size = egui::vec2(16.0, 16.0);
                    let icon_padding = 4.0;

                    let text_offset_x = if let Some(icon) = icon_cache.get(&file.path, file.is_dir) {
                        let icon_pos = egui::pos2(
                            rect.min.x + 4.0,
                            rect.center().y - icon_size.y / 2.0,
                        );

                        ui.painter().image(
                            (&icon).into(),
                            egui::Rect::from_min_size(icon_pos, icon_size),
                            egui::Rect::from_min_size(
                                egui::pos2(0.0, 0.0),
                                egui::vec2(1.0, 1.0),
                            ),
                            egui::Color32::WHITE,
                        );

                        8.0 + icon_size.x + icon_padding
                    } else {
                        8.0 + 16.0 + icon_padding
                    };

                    // --- TEXT / RENAME ---
                    let text_rect = egui::Rect::from_min_max(
                        egui::pos2(rect.min.x + text_offset_x, rect.min.y),
                        rect.max,
                    );

                    if let Some(ref editing_path) = editing_path {
                        if editing_path == &file.path {
                            if let Some(ref mut rename_state) = rename_state {
                                let mut child_ui = ui.new_child(
                                    egui::UiBuilder::new().max_rect(text_rect)
                                );

                                let edit_response = child_ui.add(
                                    egui::TextEdit::singleline(&mut rename_state.new_name)
                                        .desired_width(f32::INFINITY),
                                );

                                if edit_response.lost_focus() {
                                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                        let new_name = rename_state.new_name.trim().to_string();
                                        action = Some(ItemViewerAction::RenameRequest(
                                            file.path.clone(),
                                            new_name,
                                        ));
                                    } else {
                                        action = Some(ItemViewerAction::RenameCancel);
                                    }
                                }
                            }

                            return; // ✅ just exit closure
                        }
                    }

                    // --- DISPLAY TEXT ---
                    let text_width = available_width - text_offset_x;
                    let max_chars = (text_width / 7.0) as usize;

                    let display_name = if file.name.len() > max_chars && max_chars > 3 {
                        format!("{}...", &file.name[..(max_chars - 3)])
                    } else {
                        file.name.clone()
                    };

                    let text_pos = egui::pos2(rect.min.x + text_offset_x, rect.center().y);

                    ui.painter().text(
                        text_pos,
                        egui::Align2::LEFT_CENTER,
                        display_name,
                        egui::TextStyle::Button.resolve(ui.style()),
                        ui.style().visuals.text_color(),
                    );
                });

                // Type
                row.col(|ui| {
                    let available_width = ui.available_width();
                    let max_chars = (available_width / 7.0) as usize; // Approximate 7px per character for type names
                    
                    let type_text = if file.is_dir {
                        "Folder".to_string()
                    } else {
                        file.path
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .map(|ext| get_file_type_name(ext, file_type_cache))
                            .unwrap_or_else(|| get_file_type_name("", file_type_cache))
                    };
                    
                    let display_type = if type_text.len() > max_chars && max_chars > 3 {
                        format!("{}...", &type_text[..(max_chars - 3)])
                    } else {
                        type_text.clone()
                    };
                    
                    let label = egui::Label::new(egui::RichText::new(display_type).size(13.0))
                        .sense(egui::Sense::hover());
                    let resp = ui.add(label);
                    if resp.hovered() && type_text.len() > max_chars && max_chars > 3 {
                        resp.on_hover_text(&type_text);
                    }
                });

                // Size
                row.col(|ui| {
                    if let (Some(total), Some(free)) = (file.total_space, file.free_space) {
                        let gb = 1024.0 * 1024.0 * 1024.0;
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{:.1} / {:.1} GB",
                                    free as f64 / gb,
                                    total as f64 / gb
                                ))
                                .size(13.0),
                            );
                        });
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

                // Usage / Modified column
                row.col(|ui| {
                    if is_drive_view {
                        if let (Some(total), Some(free)) = (file.total_space, file.free_space) {
                            let bar_height = row_height * 0.85;
                            let vertical_padding = (row_height - bar_height) * 0.5;
                            ui.add_space(vertical_padding);
                            drive_usage_bar(ui, total, free, bar_height, &palette);
                        }
                    } else {
                        if let Some(m) = &file.modified_time {
                            ui.label(egui::RichText::new(m).size(13.0));
                        } else {
                            ui.label(egui::RichText::new("—").size(13.0));
                        }
                    }
                });

                // Created column (only for non-drive views)
                if !is_drive_view {
                    row.col(|ui| {
                        if let Some(c) = &file.created_time {
                            ui.label(egui::RichText::new(c).size(13.0));
                        } else {
                            ui.label(egui::RichText::new("—").size(13.0));
                        }
                    });
                }

                let row_resp = row.response();
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
                    // First section: Open
                    if ui.button("Open in new tab").clicked() {
                        action = Some(ItemViewerAction::OpenInNewTab(file.path.clone()));
                        ui.close();
                    }

                    ui.separator(); // 🔥 separator after "Open in new tab"

                    // Second section: file operations + undo/redo
                    if ui.button("Cut").clicked() {
                        action = Some(ItemViewerAction::Context(ItemViewerContextAction::Cut(
                            file.path.clone(),
                        )));
                        ui.close();
                    }
                    if ui.button("Copy").clicked() {
                        action = Some(ItemViewerAction::Context(ItemViewerContextAction::Copy(
                            file.path.clone(),
                        )));
                        ui.close();
                    }
                    if ui
                        .add_enabled(paste_enabled, egui::Button::new("Paste"))
                        .clicked()
                    {
                        action = Some(ItemViewerAction::Context(ItemViewerContextAction::Paste));
                        ui.close();
                    }

                    ui.separator(); // 🔥 separator after "Open in new tab"

                    if ui.button("Rename").clicked() {
                        action = Some(ItemViewerAction::StartEdit(file.path.clone()));
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        action = Some(ItemViewerAction::Context(ItemViewerContextAction::Delete(
                            file.path.clone(),
                        )));
                        ui.close();
                    }

                    ui.separator(); // 🔥 separator after "Open in new tab"

                    if ui.button("Undo").clicked() {
                        action = Some(ItemViewerAction::Context(ItemViewerContextAction::Undo));
                        ui.close();
                    }
                    if ui.button("Redo").clicked() {
                        action = Some(ItemViewerAction::Context(ItemViewerContextAction::Redo));
                        ui.close();
                    }

                    ui.separator(); // 🔥 separator before "Properties"

                    // Third section: Properties
                    if ui.button("Properties").clicked() {
                        action = Some(ItemViewerAction::Context(
                            ItemViewerContextAction::Properties(file.path.clone()),
                        ));
                        ui.close();
                    }
                });
            });
        });

        ui.add_space(header_gap);
        if !context_menu_open && any_row_hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        // 🔥 Keyboard shortcuts
        let input_state = ui.ctx().input(|i| i.clone()); // clone to avoid lifetime issues
        if let Some(selected) = selected_path {
            if input_state.key_pressed(egui::Key::Delete) {
                action = Some(ItemViewerAction::Context(ItemViewerContextAction::Delete(
                    selected.clone(),
                )));
            }
        }

        if input_state.modifiers.ctrl {
            if input_state.key_pressed(egui::Key::Z) {
                action = Some(ItemViewerAction::Context(ItemViewerContextAction::Undo));
            }
            if input_state.key_pressed(egui::Key::Y) {
                action = Some(ItemViewerAction::Context(ItemViewerContextAction::Redo));
            }
        }

        action
    }).inner
}
