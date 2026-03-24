use crate::core::state::FileItem;
use crate::gui::icons::IconCache;
use crate::gui::theme::{ThemePalette, apply_checkbox_colors};
use crate::gui::utils::{
    SortColumn, drive_usage_bar, format_size, get_cut_paths, get_file_type_name,
};
use crate::gui::windows::containers::enums::{ItemViewerAction, ItemViewerContextAction};
use crate::gui::windows::containers::structs::{
    ItemViewerFolderSizeState, ItemViewerLayout, RenameState, TabbarAction,
};
use eframe::egui;
use egui::{FontFamily, FontId};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub fn draw_item_viewer(
    ui: &mut egui::Ui,
    files: &Vec<FileItem>,
    folder_sizes: &HashMap<PathBuf, ItemViewerFolderSizeState>,
    selected_path: &mut Option<PathBuf>,
    selected_paths: &HashSet<PathBuf>,
    paste_enabled: bool,
    sort_column: SortColumn,
    sort_ascending: bool,
    icon_cache: &IconCache,
    rename_state: &mut Option<RenameState>,
    palette: &ThemePalette,
    file_type_cache: &mut HashMap<String, String>,
    drag_hover: &mut bool,
    selection_anchor: &mut Option<usize>,
    selection_focus: &mut Option<usize>,
    tabbar_action: &mut Option<TabbarAction>,
) -> Option<ItemViewerAction> {
    let cut_paths = get_cut_paths();

    draw_drag_overlay(ui, *drag_hover);

    let layout = compute_layout(ui, files);

    let mut action: Option<ItemViewerAction> = None;
    let mut any_row_hovered = false;

    if files.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label("This folder is empty");
        });
        return action;
    }

    // Wrap table in a scroll area for horizontal scrolling
    egui::ScrollArea::both()
        .show(ui, |ui| {
            if let Some(global_action) = handle_global_actions(
                ui,
                files,
                selected_path,
                selected_paths,
                paste_enabled,
                palette,
                selection_anchor,
                tabbar_action,
            ) {
                action = Some(global_action);
            }

            let modifiers = ui.ctx().input(|i| i.modifiers);

            let mut table = TableBuilder::new(ui)
                .striped(false)
                .sense(egui::Sense::click_and_drag())
                .animate_scrolling(true)
                .resizable(true)
                .id_salt("item_viewer_table");

            // Conditionally add checkbox column
            if !layout.is_drive_view {
                table = table.column(Column::exact(20.0)); // Checkbox
            }

            table = table
                .column(
                    Column::initial(layout.available_width * 0.35)
                        .at_least(200.0)
                        .resizable(true),
                ) // Name
                .column(
                    Column::initial(layout.available_width * 0.1)
                        .at_least(60.0)
                        .resizable(true),
                ) // Type
                .column(
                    Column::initial(layout.available_width * 0.15)
                        .at_least(80.0)
                        .resizable(true),
                ); // Size

            if layout.is_drive_view {
                table = table.column(Column::remainder().at_least(150.0).resizable(true));
            // Usage
            } else {
                table = table
                    .column(
                        Column::initial(layout.available_width * 0.2)
                            .at_least(120.0)
                            .resizable(true),
                    ) // Modified
                    .column(Column::remainder().at_least(120.0).resizable(true));
                // Created
            }

            table
                .header(layout.header_height, |mut header| {
                    if let Some(a) = draw_item_viewer_header(
                        &mut header,
                        layout.is_drive_view,
                        files,
                        selected_paths,
                        sort_column,
                        sort_ascending,
                        &palette,
                    ) {
                        action = Some(a);
                    }
                })
                .body(|body| {
                    body.rows(layout.row_height, files.len(), |mut row| {
                        let font_id = FontId::new(palette.text_size, FontFamily::Proportional);
                        let idx = row.index();
                        let file = &files[idx];

                        // Determine if this row is selected
                        let is_selected = selected_paths.contains(&file.path);
                        row.set_selected(is_selected);

                        // ✅ Step 3: Detect if file is cut
                        let is_cut = cut_paths.contains(&file.path);

                        // Checkbox column (only show for non-drive views)
                        if !layout.is_drive_view {
                            row.col(|ui| {
                                let mut checked = is_selected;
                                ui.scope(|ui| {
                                    apply_checkbox_colors(ui, palette, checked);
                                    if ui.checkbox(&mut checked, "").clicked() {
                                        if checked {
                                            action =
                                                Some(ItemViewerAction::Select(file.path.clone()));
                                        } else {
                                            action =
                                                Some(ItemViewerAction::Deselect(file.path.clone()));
                                        }
                                    }
                                });
                            });
                        }

                        row.col(|ui| {
                            if let Some(a) = handle_draw_col_name(
                                ui,
                                file,
                                &layout,
                                icon_cache,
                                is_selected,
                                is_cut,
                                palette,
                                &font_id,
                                rename_state,
                            ) {
                                action = Some(a);
                            }
                        });

                        row.col(|ui| {
                            handle_draw_col_type(
                                ui,
                                file,
                                is_selected,
                                is_cut,
                                palette,
                                &font_id,
                                file_type_cache,
                            );
                        });

                        row.col(|ui| {
                            handle_draw_col_size(
                                ui,
                                file,
                                folder_sizes,
                                is_selected,
                                is_cut,
                                palette,
                                &font_id,
                            );
                        });

                        row.col(|ui| {
                            handle_draw_col_modified(
                                ui,
                                file,
                                &layout,
                                is_selected,
                                is_cut,
                                palette,
                                &font_id,
                            );
                        });

                        if !layout.is_drive_view {
                            row.col(|ui| {
                                handle_draw_col_created(
                                    ui,
                                    file,
                                    is_selected,
                                    is_cut,
                                    palette,
                                    &font_id,
                                );
                            });
                        }

                        let row_resp = row.response();

                        if row_resp.clicked() {
                            if modifiers.shift {
                                if let Some(anchor_idx) = *selection_anchor {
                                    let current_idx = idx;

                                    let range_start = anchor_idx.min(current_idx);
                                    let range_end = anchor_idx.max(current_idx);

                                    let range_paths: Vec<PathBuf> = files[range_start..=range_end]
                                        .iter()
                                        .map(|f| f.path.clone())
                                        .collect();

                                    action = Some(ItemViewerAction::RangeSelect(range_paths));
                                    *selection_focus = Some(current_idx);
                                } else {
                                    *selection_anchor = Some(idx);
                                    *selection_focus = Some(idx);
                                    action = Some(ItemViewerAction::Select(file.path.clone()));
                                }
                            } else if modifiers.ctrl {
                                // Ctrl toggle
                                if is_selected {
                                    action = Some(ItemViewerAction::Deselect(file.path.clone()));
                                } else {
                                    action = Some(ItemViewerAction::Select(file.path.clone()));
                                }
                            } else {
                                // 🔥 NEW LOGIC: detect "already selected single item"
                                let is_single_selected = selected_paths.len() == 1
                                    && selected_paths.contains(&file.path);

                                if is_single_selected {
                                    // 🚀 Open instead of re-select
                                    action = Some(if file.is_dir {
                                        ItemViewerAction::Open(file.path.clone())
                                    } else {
                                        ItemViewerAction::OpenWithDefault(file.path.clone())
                                    });
                                } else {
                                    // Normal selection
                                    action =
                                        Some(ItemViewerAction::ReplaceSelection(file.path.clone()));
                                    *selection_anchor = Some(idx);
                                    *selection_focus = Some(idx);
                                }
                            }
                        }

                        if row_resp.middle_clicked() && file.is_dir {
                            action = Some(ItemViewerAction::OpenInNewTab(file.path.clone()));
                        }

                        if row_resp.hovered() {
                            row.set_hovered(true);
                            any_row_hovered = true;
                        }

                        row_resp.context_menu(|ui| {
                            handle_context_menu_actions(
                                ui,
                                file,
                                is_selected,
                                selected_paths,
                                paste_enabled,
                                layout.is_drive_view,
                                is_cut,
                                &mut action,
                            );
                        });
                    });
                });

            ui.add_space(layout.header_gap);

            if any_row_hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
            }

            if let Some(_pos) = ui.ctx().pointer_hover_pos() {
                // Optionally, check if pos is inside table rect if you want
                ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
            }

            let input_state = ui.ctx().input(|i| i.clone());

            // ✅ Shift+Up/Down for extended selection
            if input_state.modifiers.shift && !files.is_empty() {
                // Find current focused item index
                let current_index = if let Some(selected) = selected_path {
                    files.iter().position(|f| {
                        f.path.as_ref() as &std::path::Path == selected.as_ref() as &std::path::Path
                    })
                } else {
                    None
                };

                if let Some(current_idx) = current_index {
                    // ✅ Initialize anchor + focus if not set
                    if selection_anchor.is_none() {
                        *selection_anchor = Some(current_idx);
                        *selection_focus = Some(current_idx);
                    }

                    let anchor_idx = selection_anchor.unwrap();
                    let focus_idx = selection_focus.unwrap_or(current_idx);

                    // 🔼 Shift + Up
                    if input_state.key_pressed(egui::Key::ArrowUp) && focus_idx > 0 {
                        let new_focus = focus_idx - 1;
                        *selection_focus = Some(new_focus);

                        let range_start = anchor_idx.min(new_focus);
                        let range_end = anchor_idx.max(new_focus);

                        let range_paths: Vec<PathBuf> = files[range_start..=range_end]
                            .iter()
                            .map(|f| f.path.clone())
                            .collect();

                        action = Some(ItemViewerAction::RangeSelect(range_paths));
                    }
                    // 🔽 Shift + Down
                    else if input_state.key_pressed(egui::Key::ArrowDown)
                        && focus_idx < files.len() - 1
                    {
                        let new_focus = focus_idx + 1;
                        *selection_focus = Some(new_focus);

                        let range_start = anchor_idx.min(new_focus);
                        let range_end = anchor_idx.max(new_focus);

                        let range_paths: Vec<PathBuf> = files[range_start..=range_end]
                            .iter()
                            .map(|f| f.path.clone())
                            .collect();

                        action = Some(ItemViewerAction::RangeSelect(range_paths));
                    }
                } else if !files.is_empty() {
                    // No current selection → start from first item
                    if input_state.key_pressed(egui::Key::ArrowDown) {
                        action = Some(ItemViewerAction::Select(files[0].path.clone()));
                        *selection_anchor = Some(0);
                        *selection_focus = Some(0);
                    }
                }
            }
            if !input_state.modifiers.shift {
                *selection_anchor = None;
                *selection_focus = None;
            }

            // ✅ Regular arrow navigation (without Shift)
            if !input_state.modifiers.shift && !files.is_empty() {
                // Reset anchor when doing regular navigation
                *selection_anchor = None;

                if let Some(current_idx) = selected_path.as_ref().and_then(|selected| {
                    files.iter().position(|f| {
                        f.path.as_ref() as &std::path::Path == selected.as_ref() as &std::path::Path
                    })
                }) {
                    if input_state.key_pressed(egui::Key::ArrowUp) && current_idx > 0 {
                        // Move selection up
                        let target_path = files[current_idx - 1].path.clone();
                        action = Some(ItemViewerAction::ReplaceSelection(target_path));
                    } else if input_state.key_pressed(egui::Key::ArrowDown)
                        && current_idx < files.len() - 1
                    {
                        // Move selection down
                        let target_path = files[current_idx + 1].path.clone();
                        action = Some(ItemViewerAction::ReplaceSelection(target_path));
                    }
                } else if files.len() > 0 && input_state.key_pressed(egui::Key::ArrowDown) {
                    // No selection, select first item
                    action = Some(ItemViewerAction::Select(files[0].path.clone()));
                }
            }

            // --- Drag and Drop Detection ---
            // Check for external drag and drop
            let input_state = ui.ctx().input(|i| i.clone());
            if !input_state.raw.dropped_files.is_empty() {
                let dropped_paths: Vec<PathBuf> = input_state
                    .raw
                    .dropped_files
                    .iter()
                    .filter_map(|file| file.path.clone())
                    .collect();

                if !dropped_paths.is_empty() {
                    action = Some(ItemViewerAction::FilesDropped(dropped_paths));
                }
            }

            // Update drag hover state
            *drag_hover = input_state
                .raw
                .hovered_files
                .iter()
                .any(|file| file.path.is_some());

            action
        })
        .inner
}

fn draw_drag_overlay(ui: &mut egui::Ui, drag_hover: bool) {
    if drag_hover {
        let rect = ui.max_rect();

        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(6),
            ui.visuals().selection.bg_fill.linear_multiply(0.15),
        );

        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Copy to this folder",
            egui::TextStyle::Heading.resolve(ui.style()),
            ui.visuals().text_color(),
        );
    }
}

fn compute_layout(ui: &egui::Ui, files: &Vec<FileItem>) -> ItemViewerLayout {
    let text_height = 14.0;
    let row_padding = 6.0;
    let row_height = text_height + row_padding;

    let header_padding = 0.0;
    let header_height = row_height + header_padding;

    let is_drive_view = files.iter().any(|f| f.total_space.is_some());

    ItemViewerLayout {
        row_height,
        header_height,
        header_gap: 6.0,
        available_width: ui.available_width(),
        is_drive_view,
    }
}

fn handle_context_menu_actions(
    ui: &mut egui::Ui,
    file: &FileItem,
    is_selected: bool,
    selected_paths: &HashSet<PathBuf>,
    paste_enabled: bool,
    is_drive_view: bool,
    is_cut: bool,
    action: &mut Option<ItemViewerAction>,
) {
    // ✅ Match Explorer behavior: right-click selects if not already selected
    if !is_selected {
        *action = Some(ItemViewerAction::ReplaceSelection(file.path.clone()));
    }

    // 🚗 DRIVE VIEW MODE → ONLY PROPERTIES
    if is_drive_view {
        if ui.button("Properties").clicked() {
            let targets: Vec<PathBuf> = if is_selected {
                selected_paths.iter().cloned().collect()
            } else {
                vec![file.path.clone()]
            };

            *action = Some(ItemViewerAction::Context(
                ItemViewerContextAction::Properties(targets),
            ));
            ui.close();
        }

        return; // 🔥 Early exit — nothing else allowed
    }

    // --- NORMAL FILE VIEW ---

    // First section: Open
    if ui
        .add_enabled(file.is_dir, egui::Button::new("Open in new tab"))
        .clicked()
    {
        *action = Some(ItemViewerAction::OpenInNewTab(file.path.clone()));
        ui.close();
    }

    ui.separator();

    // Second section: file operations + undo/redo
    let cut_label = if is_cut { "Cut (pending)" } else { "Cut" };
    if ui
        .add_enabled(!is_cut, egui::Button::new(cut_label))
        .clicked()
    {
        *action = Some(ItemViewerAction::Context(ItemViewerContextAction::Cut(
            file.path.clone(),
        )));
        ui.close();
    }
    if is_cut {
        if ui.button("Cancel Cut").clicked() {
            *action = Some(ItemViewerAction::Context(
                ItemViewerContextAction::ClearCut(file.path.clone()),
            ));
            ui.close();
        }

        ui.separator();
    }
    if ui.button("Copy").clicked() {
        *action = Some(ItemViewerAction::Context(ItemViewerContextAction::Copy(
            file.path.clone(),
        )));
        ui.close();
    }
    if ui
        .add_enabled(paste_enabled, egui::Button::new("Paste"))
        .clicked()
    {
        *action = Some(ItemViewerAction::Context(ItemViewerContextAction::Paste));
        ui.close();
    }

    ui.separator();

    if ui.button("Rename").clicked() {
        *action = Some(ItemViewerAction::StartEdit(file.path.clone()));
        ui.close();
    }
    if ui.button("Delete").clicked() {
        *action = Some(ItemViewerAction::Context(ItemViewerContextAction::Delete(
            file.path.clone(),
        )));
        ui.close();
    }

    ui.separator();

    if ui.button("Undo").clicked() {
        *action = Some(ItemViewerAction::Context(ItemViewerContextAction::Undo));
        ui.close();
    }
    if ui.button("Redo").clicked() {
        *action = Some(ItemViewerAction::Context(ItemViewerContextAction::Redo));
        ui.close();
    }

    ui.separator();

    // Properties (multi-select aware)
    if ui.button("Properties").clicked() {
        let targets: Vec<PathBuf> = if is_selected {
            selected_paths.iter().cloned().collect()
        } else {
            vec![file.path.clone()]
        };

        *action = Some(ItemViewerAction::Context(
            ItemViewerContextAction::Properties(targets),
        ));
        ui.close();
    }
}

fn handle_draw_col_name(
    ui: &mut egui::Ui,
    file: &FileItem,
    layout: &ItemViewerLayout,
    icon_cache: &IconCache,
    is_selected: bool,
    is_cut: bool,
    palette: &ThemePalette,
    font_id: &egui::FontId,
    rename_state: &mut Option<RenameState>,
) -> Option<ItemViewerAction> {
    let available_width = ui.available_width();

    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(available_width, layout.row_height),
        egui::Sense::hover(),
    );

    // --- ICON ---
    let icon_size = egui::vec2(18.0, 18.0);
    let icon_padding = 4.0;

    let text_offset_x = if let Some(icon) = icon_cache.get(&file.path, file.is_dir) {
        let icon_pos = egui::pos2(rect.min.x + 4.0, rect.center().y - icon_size.y / 2.0);

        ui.painter().image(
            (&icon).into(),
            egui::Rect::from_min_size(icon_pos, icon_size),
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1.0, 1.0)),
            if is_cut {
                palette.icon_colored_hover.linear_multiply(0.5)
            } else {
                palette.icon_colored_hover
            },
        );

        8.0 + icon_size.x + icon_padding
    } else {
        8.0 + 16.0 + icon_padding
    };

    // --- TEXT / RENAME ---
    let text_rect =
        egui::Rect::from_min_max(egui::pos2(rect.min.x + text_offset_x, rect.min.y), rect.max);

    // ⚠️ Important: clone path BEFORE mutable borrow
    let editing_path = rename_state.as_ref().map(|rs| rs.path.clone());

    if let Some(path) = editing_path {
        if path == file.path {
            return handle_editing_file_name(
                ui,
                file,
                is_selected,
                palette,
                text_rect,
                rename_state,
            );
        }
    }

    // --- DISPLAY TEXT ---
    let text_width = available_width - text_offset_x;
    let max_chars = (text_width / 7.0) as usize;

    let display_name = if file.name.len() > max_chars && max_chars > 3 {
        let mut char_count = 0;
        let mut byte_end = 0;
        for (i, _) in file.name.char_indices() {
            if char_count >= max_chars - 3 {
                break;
            }
            char_count += 1;
            byte_end = i;
        }
        format!("{}...", &file.name[..byte_end])
    } else {
        file.name.clone()
    };

    let text_pos = egui::pos2(rect.min.x + text_offset_x, rect.center().y);

    ui.painter().text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        display_name,
        font_id.clone(),
        get_text_color(is_selected, is_cut, palette),
    );

    None
}

fn handle_draw_col_type(
    ui: &mut egui::Ui,
    file: &FileItem,
    is_selected: bool,
    is_cut: bool,
    palette: &ThemePalette,
    font_id: &egui::FontId,
    file_type_cache: &mut HashMap<String, String>,
) {
    let available_width = ui.available_width();
    let max_chars = (available_width / 7.0) as usize;

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
        let mut char_count = 0;
        let mut byte_end = 0;
        for (i, _) in type_text.char_indices() {
            if char_count >= max_chars - 3 {
                break;
            }
            char_count += 1;
            byte_end = i;
        }
        format!("{}...", &type_text[..byte_end])
    } else {
        type_text.clone()
    };

    let mut rich_text = egui::RichText::new(display_type)
        .size(palette.text_size)
        .color(get_text_color(is_selected, is_cut, palette));

    if is_cut {
        rich_text = rich_text.italics();
    }

    let label = egui::Label::new(rich_text.font(font_id.clone())).sense(egui::Sense::hover());

    let resp = ui.add(label);

    if resp.hovered() && type_text.len() > max_chars && max_chars > 3 {
        resp.on_hover_text(
            egui::RichText::new(&type_text)
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        );
    }
}

fn handle_draw_col_size(
    ui: &mut egui::Ui,
    file: &FileItem,
    folder_sizes: &HashMap<PathBuf, ItemViewerFolderSizeState>,
    is_selected: bool,
    is_cut: bool,
    palette: &ThemePalette,
    font_id: &egui::FontId,
) {
    let text_color = get_text_color(is_selected, is_cut, palette);

    // --- DRIVE VIEW (free / total) ---
    if let (Some(total), Some(free)) = (file.total_space, file.free_space) {
        let gb = 1024.0 * 1024.0 * 1024.0;

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!(
                    "{:.1} / {:.1} GB",
                    free as f64 / gb,
                    total as f64 / gb
                ))
                .size(palette.text_size)
                .color(text_color)
                .font(font_id.clone()),
            );
        });

        return;
    }

    // --- FOLDER SIZE ---
    if file.is_dir {
        if let Some(state) = folder_sizes.get(&file.path) {
            let label = format_size(state.bytes);

            let text = if state.done {
                label
            } else {
                format!("⏳ {}", label)
            };

            ui.label(
                egui::RichText::new(text)
                    .size(palette.text_size)
                    .color(text_color)
                    .font(font_id.clone()),
            );
        } else {
            draw_placeholder(ui, palette, font_id, text_color);
        }

        return;
    }

    // --- FILE SIZE ---
    if let Some(size) = file.file_size {
        ui.label(
            egui::RichText::new(format_size(size))
                .size(palette.text_size)
                .color(text_color)
                .font(font_id.clone()),
        );
    } else {
        draw_placeholder(ui, palette, font_id, text_color);
    }
}

fn handle_draw_col_modified(
    ui: &mut egui::Ui,
    file: &FileItem,
    layout: &ItemViewerLayout,
    is_selected: bool,
    is_cut: bool,
    palette: &ThemePalette,
    font_id: &egui::FontId,
) {
    if layout.is_drive_view {
        if let (Some(total), Some(free)) = (file.total_space, file.free_space) {
            let bar_height = layout.row_height * 0.85;
            let vertical_padding = (layout.row_height - bar_height) * 0.5;
            ui.add_space(vertical_padding);
            drive_usage_bar(ui, total, free, bar_height, palette);
        } else {
            draw_placeholder(
                ui,
                palette,
                font_id,
                get_text_color(is_selected, is_cut, palette),
            );
        }
    } else {
        let text_color = get_text_color(is_selected, is_cut, palette);
        if let Some(m) = &file.modified_time {
            ui.label(
                egui::RichText::new(m)
                    .size(palette.text_size)
                    .color(text_color)
                    .font(font_id.clone()),
            );
        } else {
            draw_placeholder(ui, palette, font_id, text_color);
        }
    }
}

fn handle_draw_col_created(
    ui: &mut egui::Ui,
    file: &FileItem,
    is_selected: bool,
    is_cut: bool,
    palette: &ThemePalette,
    font_id: &egui::FontId,
) {
    let text_color = get_text_color(is_selected, is_cut, palette);
    if let Some(c) = &file.created_time {
        ui.label(
            egui::RichText::new(c)
                .size(palette.text_size)
                .color(text_color)
                .font(font_id.clone()),
        );
    } else {
        ui.label(
            egui::RichText::new("—")
                .size(palette.text_size)
                .color(text_color)
                .font(font_id.clone()),
        );
    }
}

fn draw_placeholder(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    font_id: &egui::FontId,
    color: egui::Color32,
) {
    ui.label(
        egui::RichText::new("—")
            .size(palette.text_size)
            .color(color)
            .font(font_id.clone()),
    );
}

fn get_text_color(is_selected: bool, is_cut: bool, palette: &ThemePalette) -> egui::Color32 {
    let base_color = get_row_color(is_selected, palette);
    if is_cut {
        base_color.linear_multiply(0.5)
    } else {
        base_color
    }
}

fn get_row_color(
    is_multi_selected: bool,
    palette: &crate::gui::theme::ThemePalette,
) -> egui::Color32 {
    if is_multi_selected {
        palette.row_label_selected
    } else {
        palette.row_label_default
    }
}

fn handle_editing_file_name(
    ui: &mut egui::Ui,
    file: &FileItem,
    is_selected: bool,
    palette: &ThemePalette,
    text_rect: egui::Rect,
    rename_state: &mut Option<RenameState>,
) -> Option<ItemViewerAction> {
    let Some(rename_state) = rename_state else {
        return None;
    };
    if rename_state.path != file.path {
        return None;
    }

    let mut action: Option<ItemViewerAction> = None;
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(text_rect));

    child_ui.scope(|ui| {
        let visuals = ui.visuals_mut();
        let bg = if is_selected {
            palette.row_selected_bg
        } else {
            palette.row_bg
        };

        visuals.widgets.inactive.bg_fill = bg;
        visuals.widgets.hovered.bg_fill = bg;
        visuals.widgets.active.bg_fill = bg;
        visuals.widgets.inactive.bg_stroke.width = 0.0;
        visuals.widgets.hovered.bg_stroke.width = 0.0;
        visuals.widgets.active.bg_stroke.width = 0.0;

        visuals.override_text_color = Some(get_row_color(is_selected, palette));

        let edit_response = ui.add(
            egui::TextEdit::singleline(&mut rename_state.new_name)
                .desired_width(f32::INFINITY)
                .font(FontId::new(palette.text_size, FontFamily::Proportional)),
        );

        if rename_state.should_focus {
            edit_response.request_focus();
            rename_state.should_focus = false;
        }

        if edit_response.lost_focus() {
            let input = edit_response.ctx.input(|i| i.clone());

            if input.key_pressed(egui::Key::Enter) {
                let new_name = rename_state.new_name.trim().to_string();
                action = Some(ItemViewerAction::RenameRequest(file.path.clone(), new_name));
            } else if input.key_pressed(egui::Key::Escape) {
                action = Some(ItemViewerAction::RenameCancel);
            } else {
                action = Some(ItemViewerAction::RenameCancel);
            }
        }
    });

    action
}

fn handle_global_actions(
    ui: &mut egui::Ui,
    files: &Vec<FileItem>,
    selected_path: &mut Option<PathBuf>,
    selected_paths: &HashSet<PathBuf>,
    paste_enabled: bool,
    palette: &crate::gui::theme::ThemePalette,
    selection_anchor: &mut Option<usize>,
    tabbar_action: &mut Option<TabbarAction>,
) -> Option<ItemViewerAction> {
    // 🔥 TEMP: disable focus blocking for now (fix later with rename_state)
    let is_text_edit_active = tabbar_action
        .as_ref()
        .is_some_and(|t| t.is_breadcrumb_path_edit_active);
    if is_text_edit_active {
        return None;
    }

    let mut action: Option<ItemViewerAction> = None;

    // =========================
    // 🔥 EVENT-BASED SHORTCUTS
    // =========================
    ui.input(|i| {
        for event in &i.events {
            // println!("EVENT: {:?}", event); // keep debug

            match event {
                // =========================
                // 🔥 CLIPBOARD EVENTS (CORRECT WAY)
                // =========================
                egui::Event::Copy => {
                    if is_text_edit_active {
                        continue;
                    }

                    let copy_path = if let Some(selected) = selected_path {
                        selected.clone()
                    } else if !files.is_empty() {
                        files[0].path.clone()
                    } else {
                        continue;
                    };

                    println!("🔥 Copy event detected");
                    action = Some(ItemViewerAction::Context(ItemViewerContextAction::Copy(
                        copy_path,
                    )));
                }

                egui::Event::Cut => {
                    if is_text_edit_active {
                        continue;
                    }

                    let cut_path = if let Some(selected) = selected_path {
                        selected.clone()
                    } else if !files.is_empty() {
                        files[0].path.clone()
                    } else {
                        continue;
                    };

                    println!("🔥 Cut event detected");
                    action = Some(ItemViewerAction::Context(ItemViewerContextAction::Cut(
                        cut_path,
                    )));
                }

                // Fegui::Event::Paste(content) => {
                //     action = Some(ItemViewerAction::Context(ItemViewerContextAction::Paste));
                // }

                // egui::Event::Paste(_) => {
                //     if is_text_edit_active || !paste_enabled {
                //         continue;
                //     }

                //     action = Some(ItemViewerAction::Context(ItemViewerContextAction::Paste));
                // }

                // =========================
                // 🔥 FALLBACK: KEY EVENTS (YOUR CASE)
                // =========================
                egui::Event::Key {
                    key,
                    pressed: false, // 👈 IMPORTANT: your system emits false
                    modifiers,
                    ..
                } => {
                    if modifiers.command && !is_text_edit_active {
                        match key {
                            egui::Key::V => {
                                println!("🔥 Ctrl+V detected (fallback)");
                                action =
                                    Some(ItemViewerAction::Context(ItemViewerContextAction::Paste));
                            }
                            egui::Key::C => {
                                println!("🔥 Ctrl+C detected (fallback)");

                                let copy_path = if let Some(selected) = selected_path {
                                    selected.clone()
                                } else if !files.is_empty() {
                                    files[0].path.clone()
                                } else {
                                    continue;
                                };

                                action = Some(ItemViewerAction::Context(
                                    ItemViewerContextAction::Copy(copy_path),
                                ));
                            }
                            egui::Key::X => {
                                println!("🔥 Ctrl+X detected (fallback)");

                                let cut_path = if let Some(selected) = selected_path {
                                    selected.clone()
                                } else if !files.is_empty() {
                                    files[0].path.clone()
                                } else {
                                    continue;
                                };

                                action = Some(ItemViewerAction::Context(
                                    ItemViewerContextAction::Cut(cut_path),
                                ));
                            }
                            egui::Key::Z => {
                                println!("🔥 Ctrl+Z detected");
                                action =
                                    Some(ItemViewerAction::Context(ItemViewerContextAction::Undo));
                            }
                            egui::Key::Y => {
                                println!("🔥 Ctrl+Y detected");
                                action =
                                    Some(ItemViewerAction::Context(ItemViewerContextAction::Redo));
                            }
                            egui::Key::A => {
                                println!("🔥 Ctrl+A detected");
                                action = Some(ItemViewerAction::SelectAll);
                            }
                            _ => {}
                        }
                    }
                }

                _ => {}
            }
        }

        // =========================
        // 🔹 NON-MODIFIER KEYS
        // =========================

        if i.key_pressed(egui::Key::Escape) {
            action = Some(ItemViewerAction::DeselectAll);
        }

        if let Some(selected) = selected_path {
            if i.key_pressed(egui::Key::Delete) {
                action = Some(ItemViewerAction::Context(ItemViewerContextAction::Delete(
                    selected.clone(),
                )));
            }
        }

        if i.key_pressed(egui::Key::Backspace) {
            action = Some(ItemViewerAction::BackNavigation);
        }

        // =========================
        // 🔹 ARROW NAVIGATION
        // =========================

        if !files.is_empty() {
            if let Some(current) = selected_path {
                if let Some(idx) = files.iter().position(|f| &f.path == current) {
                    if i.key_pressed(egui::Key::ArrowDown) && idx + 1 < files.len() {
                        action = Some(ItemViewerAction::Select(files[idx + 1].path.clone()));
                    }
                    if i.key_pressed(egui::Key::ArrowUp) && idx > 0 {
                        action = Some(ItemViewerAction::Select(files[idx - 1].path.clone()));
                    }
                }
            } else if i.key_pressed(egui::Key::ArrowDown) {
                action = Some(ItemViewerAction::Select(files[0].path.clone()));
            }
        }
    });

    // =========================
    // 🔹 BOX SELECTION (UNCHANGED)
    // =========================

    let start_pos = ui.ctx().memory_mut(|mem| {
        mem.data
            .get_temp::<egui::Pos2>("box_selection_start".into())
    });

    if let Some(pointer_pos) = ui.ctx().pointer_hover_pos() {
        if ui.input(|i| i.pointer.primary_pressed()) {
            ui.ctx().memory_mut(|mem| {
                mem.data
                    .insert_temp("box_selection_start".into(), pointer_pos);
            });
        }

        if ui.input(|i| i.pointer.primary_down()) {
            if let Some(start_pos) = start_pos {
                let selection_rect = egui::Rect::from_min_max(start_pos, pointer_pos);

                ui.painter().rect_filled(
                    selection_rect,
                    egui::CornerRadius::same(0),
                    palette.primary,
                );
                ui.painter().rect_stroke(
                    selection_rect,
                    egui::CornerRadius::same(0),
                    egui::Stroke::new(1.0, palette.box_selection_stroke),
                    egui::StrokeKind::Inside,
                );

                let selected_files: Vec<PathBuf> = ui.ctx().memory(|mem| {
                    mem.data
                        .get_temp::<Vec<egui::Rect>>("table_row_rects".into())
                        .unwrap_or_default()
                        .iter()
                        .enumerate()
                        .filter_map(|(idx, row_rect)| {
                            if selection_rect.intersects(*row_rect) {
                                Some(files[idx].path.clone())
                            } else {
                                None
                            }
                        })
                        .collect()
                });

                if !selected_files.is_empty() {
                    return Some(ItemViewerAction::BoxSelect(selected_files));
                }
            }
        }
    }

    if ui.input(|i| i.pointer.primary_released()) {
        ui.ctx().memory_mut(|mem| {
            mem.data.remove::<egui::Pos2>("box_selection_start".into());
        });
    }

    action
}

fn draw_item_viewer_header(
    header: &mut egui_extras::TableRow<'_, '_>,
    is_drive_view: bool,
    files: &Vec<FileItem>,
    selected_paths: &HashSet<PathBuf>,
    sort_column: SortColumn,
    sort_ascending: bool,
    palette: &crate::gui::theme::ThemePalette,
) -> Option<ItemViewerAction> {
    let font_id = FontId::new(palette.text_size, FontFamily::Proportional);
    let mut action: Option<ItemViewerAction> = None;
    // Checkbox column (only show for non-drive views)
    if !is_drive_view {
        header.col(|ui| {
            // Add a small checkbox in header for select all functionality
            let mut all_selected = files.iter().all(|f| selected_paths.contains(&f.path));

            ui.scope(|ui| {
                apply_checkbox_colors(ui, palette, all_selected);
                if ui.checkbox(&mut all_selected, "").clicked() {
                    if all_selected {
                        // Select all
                        action = Some(ItemViewerAction::SelectAll);
                    } else {
                        // Deselect all
                        action = Some(ItemViewerAction::DeselectAll);
                    }
                }
            });
        });
    }

    // Name column
    header.col(|ui| {
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
            egui::Label::new(
                egui::RichText::new(format!("{label} {arrow}").trim_end())
                    .font(font_id.clone())
                    .size(palette.text_size)
                    .color(palette.itemviewer_header_color),
            )
            .selectable(false)
            .sense(egui::Sense::click()),
        );
        if resp.clicked() {
            action = Some(ItemViewerAction::Sort(SortColumn::Name));
        }
    });

    // Type column
    header.col(|ui| {
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
            egui::Label::new(
                egui::RichText::new(format!("{label} {arrow}").trim_end())
                    .font(font_id.clone())
                    .size(palette.text_size)
                    .color(palette.itemviewer_header_color),
            )
            .selectable(false)
            .sense(egui::Sense::click()),
        );
        if resp.clicked() {
            action = Some(ItemViewerAction::Sort(SortColumn::Type));
        }
    });

    // Size column
    header.col(|ui| {
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
            egui::Label::new(
                egui::RichText::new(format!("{label} {arrow}").trim_end())
                    .font(font_id.clone())
                    .size(palette.text_size)
                    .color(palette.itemviewer_header_color),
            )
            .selectable(false)
            .sense(egui::Sense::click()),
        );
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
        }
        if resp.clicked() {
            action = Some(ItemViewerAction::Sort(SortColumn::Size));
        }
    });

    // Usage / Modified column
    if is_drive_view {
        header.col(|ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("Usage").trim_end())
                        .font(font_id.clone())
                        .size(palette.text_size)
                        .color(palette.itemviewer_header_color),
                )
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
                egui::Label::new(
                    egui::RichText::new(format!("{label} {arrow}").trim_end())
                        .font(font_id.clone())
                        .size(palette.text_size)
                        .color(palette.itemviewer_header_color),
                )
                .selectable(false)
                .sense(egui::Sense::click()),
            );
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
                egui::Label::new(
                    egui::RichText::new(format!("{label} {arrow}").trim_end())
                        .font(font_id.clone())
                        .size(palette.text_size)
                        .color(palette.itemviewer_header_color),
                )
                .selectable(false)
                .sense(egui::Sense::click()),
            );

            if resp.clicked() {
                action = Some(ItemViewerAction::Sort(SortColumn::Created));
            }
        });
    }

    action
}
