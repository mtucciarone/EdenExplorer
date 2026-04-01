use crate::core::drives::is_raw_physical_drive_path;
use crate::core::fs::FileItem;
use crate::gui::icons::IconCache;
use crate::gui::theme::{ThemePalette, apply_checkbox_colors};
use crate::gui::utils::{
    SortColumn, clear_clipboard_files, draw_object_drag_ghost, drive_usage_bar, format_size,
    fuzzy_match, get_file_type_name, truncate_item_text,
};
use crate::gui::windows::containers::enums::{ItemViewerAction, ItemViewerContextAction};
use crate::gui::windows::containers::structs::{
    DragState, ExplorerState, FilterState, ItemViewerFolderSizeState, ItemViewerLayout,
    RenameState, TabbarAction,
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
    paste_enabled: bool,
    clipboard_set: &HashSet<PathBuf>,
    is_cut_mode: bool,
    is_drive_view: bool,
    sort_column: SortColumn,
    sort_ascending: bool,
    icon_cache: &IconCache,
    rename_state: &mut Option<RenameState>,
    palette: &ThemePalette,
    file_type_cache: &mut HashMap<String, String>,
    file_size_text_cache: &mut HashMap<PathBuf, (u64, String)>,
    folder_size_text_cache: &mut HashMap<PathBuf, (u64, bool, String)>,
    drive_size_text_cache: &mut HashMap<PathBuf, (u64, u64, String)>,
    external_drag_to_internal_hover: &mut bool,
    tabbar_action: &mut Option<TabbarAction>,
    drag_state: &mut DragState,
    filter_state: &mut FilterState,
    is_loading: bool,
    explorer_state: &mut ExplorerState,
) -> Option<ItemViewerAction> {
    let font_id = FontId::new(palette.text_size, FontFamily::Proportional);
    let mut hovered_drop_target: Option<PathBuf> = None;
    let mut hovered_drop_target_rect: Option<egui::Rect> = None;
    draw_external_to_internal_drag_overlay(ui, *external_drag_to_internal_hover);

    let layout = compute_layout(ui, is_drive_view);

    let mut action: Option<ItemViewerAction> = None;
    let mut any_row_hovered = false;

    if files.is_empty() {
        // Handle drag and drop for empty folders
        if ui.ctx().input(|i| !i.raw.dropped_files.is_empty()) {
            let dropped_paths: Vec<PathBuf> = ui.ctx().input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .filter_map(|f| f.path.clone())
                    .collect()
            });

            if !dropped_paths.is_empty() {
                action = Some(ItemViewerAction::FilesDropped(dropped_paths));
            }
        }

        ui.centered_and_justified(|ui| {
            if is_loading {
                ui.add(egui::Spinner::new().size(28.0));
            } else {
                ui.label("This folder is empty");
            }
        });
    }

    if filter_state.dirty
        || filter_state.query != filter_state.last_query
        || filter_state.last_files_len != files.len()
    {
        filter_state.cached_indices = files
            .iter()
            .enumerate()
            .filter(|(_, f)| fuzzy_match(&f.name, &filter_state.query))
            .map(|(i, _)| i)
            .collect();

        filter_state.last_query = filter_state.query.clone();
        filter_state.last_files_len = files.len();
        filter_state.dirty = false;
    }

    // 🔥 Ensure selection is valid within filtered view
    if let Some(selected) = explorer_state.selected_paths.iter().next() {
        if !filter_state
            .cached_indices
            .iter()
            .any(|&i| &files[i].path == selected)
        {
            explorer_state.selected_paths.clear();
            explorer_state.selection_anchor = None;
            explorer_state.selection_focus = None;
        }
    }

    if drag_state.active {
        let label = if drag_state.source_items.len() == 1 {
            drag_state.source_items[0]
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown")
                .to_string()
        } else {
            format!("{} items", drag_state.source_items.len())
        };

        draw_object_drag_ghost(ui, palette, &label, false);
    }

    if let Some(global_action) = handle_global_actions(
        ui,
        files,
        palette,
        tabbar_action,
        rename_state,
        filter_state,
        drag_state,
        explorer_state,
        is_cut_mode,
    ) {
        action = Some(global_action);
    }

    let mut current_hovered_drop_target: Option<PathBuf> = None;
    let mut current_hovered_drop_target_rect: Option<egui::Rect> = None;
    let mut best_hovered_row: Option<(f32, bool, PathBuf, egui::Rect)> = None;
    let drag_hover_active = ui
        .ctx()
        .input(|i| drag_state.active || i.raw.hovered_files.iter().any(|file| file.path.is_some()));
    let pointer_pos = ui
        .ctx()
        .input(|i| i.pointer.interact_pos().or_else(|| i.pointer.hover_pos()));

    if !files.is_empty() {
        let modifiers = ui.ctx().input(|i| i.modifiers);

        let arrow_nav = ui
            .ctx()
            .input(|i| i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowUp));

        let mut table = TableBuilder::new(ui)
            .striped(false)
            .sense(if layout.is_drive_view {
                egui::Sense::click()
            } else {
                egui::Sense::click_and_drag()
            })
            .animate_scrolling(true)
            .resizable(true)
            .id_salt("item_viewer_table");

        // If we have a newly created row, scroll to it and select it
        if let Some(new_path) = &explorer_state.newly_created_path {
            if let Some(idx) = filter_state
                .cached_indices
                .iter()
                .position(|&i| files[i].path == *new_path)
            {
                table = table.scroll_to_row(idx, Some(egui::Align::Center));

                // Auto-select the newly created/renamed item
                explorer_state.selected_paths.clear();
                explorer_state.selected_paths.insert(new_path.clone());
                explorer_state.selection_anchor = Some(idx);
                explorer_state.selection_focus = Some(idx);

                explorer_state.newly_created_path = None;
            }
        }

        // If selection changed via keyboard, keep it in view
        if arrow_nav {
            if let Some(focus_idx) = explorer_state.selection_focus {
                if focus_idx < filter_state.cached_indices.len() {
                    table = table.scroll_to_row(focus_idx, Some(egui::Align::Center));
                }
            }
        }

        if !layout.is_drive_view {
            table = table.column(Column::exact(20.0));
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
            ); // Type

        if layout.is_drive_view {
            table = table.column(
                Column::initial(layout.available_width * 0.14)
                    .at_least(120.0)
                    .resizable(true),
            ); // Size
        } else {
            table = table.column(
                Column::initial(layout.available_width * 0.1)
                    .at_least(75.0)
                    .resizable(true),
            ); // Size
        }

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
                    &filter_state.cached_indices,
                    files,
                    sort_column,
                    sort_ascending,
                    &palette,
                    explorer_state,
                ) {
                    action = Some(a);
                }
            })
            .body(|body| {
                let hovered_drop_target = &mut hovered_drop_target;
                let filtered_indices = &filter_state.cached_indices;
                body.rows(layout.row_height, filtered_indices.len(), |mut row| {
                    let idx = row.index();
                    let file = &files[filtered_indices[idx]];
                    let is_non_ntfs_drive =
                        layout.is_drive_view && is_raw_physical_drive_path(&file.path);

                    // Determine if this row is selected
                    let is_selected = explorer_state.selected_paths.contains(&file.path);
                    row.set_selected(is_selected);

                    // ✅ Step 3: Detect if file is cut
                    let is_cut = is_cut_mode && clipboard_set.contains(&file.path);

                    // Checkbox column (only show for non-drive views)
                    if !layout.is_drive_view {
                        row.col(|ui| {
                            let mut checked = is_selected;
                            ui.scope(|ui| {
                                apply_checkbox_colors(ui, palette, checked);
                                if ui.checkbox(&mut checked, "").clicked() {
                                    if checked {
                                        action = Some(ItemViewerAction::Select(file.path.clone()));
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
                            file_size_text_cache,
                            folder_size_text_cache,
                            drive_size_text_cache,
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

                    if drag_hover_active {
                        if let Some(pointer) = pointer_pos {
                            if row_resp.rect.contains(pointer) {
                                let is_dir = file.is_dir;
                                let row_top = row_resp.rect.top();
                                let row_rect = {
                                    let row_min = row_resp.rect.min;
                                    let row_max = egui::pos2(
                                        row_resp.rect.max.x,
                                        row_resp.rect.min.y + layout.row_height,
                                    );
                                    egui::Rect::from_min_max(row_min, row_max)
                                };
                                match &best_hovered_row {
                                    Some((best_top, _, _, _)) if *best_top >= row_top => {}
                                    _ => {
                                        best_hovered_row =
                                            Some((row_top, is_dir, file.path.clone(), row_rect));
                                    }
                                }
                            }
                        }
                    }

                    if row_resp.drag_started() && !is_non_ntfs_drive {
                        drag_state.start_pos = row_resp.interact_pointer_pos();
                        drag_state.active = false; // threshold not passed yet
                        drag_state.source_items.clear();

                        // ✅ LOCK drag payload immediately
                        drag_state.source_items =
                            if explorer_state.selected_paths.contains(&file.path) {
                                explorer_state.selected_paths.iter().cloned().collect()
                            } else {
                                vec![file.path.clone()]
                            };
                    }

                    if let (Some(start), Some(current)) = (
                        drag_state.start_pos,
                        row_resp.ctx.input(|i| i.pointer.hover_pos()),
                    ) {
                        if !drag_state.active && start.distance(current) > 4.0 {
                            drag_state.active = true;
                        }
                    }

                    if row_resp.clicked() && !drag_state.active {
                        if is_non_ntfs_drive {
                            explorer_state.non_ntfs_popup_path = Some(file.path.clone());
                        } else if let Some(a) = handle_row_click(
                            idx,
                            file,
                            modifiers,
                            &filter_state.cached_indices,
                            files,
                            drag_state,
                            explorer_state,
                        ) {
                            action = Some(a);
                        }
                    }

                    if row_resp.middle_clicked() && file.is_dir && !is_non_ntfs_drive {
                        action = Some(ItemViewerAction::OpenInNewTab(file.path.clone()));
                    }

                    if drag_hover_active {
                        // Avoid double-hover visuals during drag; rely on drop highlight.
                        row.set_hovered(false);
                    } else if row_resp.hovered() {
                        row.set_hovered(true);
                        any_row_hovered = true;
                    }

                    if !is_non_ntfs_drive {
                        row_resp.context_menu(|ui| {
                            handle_context_menu_actions(
                                ui,
                                file,
                                is_selected,
                                paste_enabled,
                                layout.is_drive_view,
                                is_cut,
                                &mut action,
                                palette,
                                explorer_state,
                            );
                        });
                    }
                });
                if let Some((_, is_dir, path, rect)) = best_hovered_row.take() {
                    if is_dir {
                        current_hovered_drop_target = Some(path);
                        current_hovered_drop_target_rect = Some(rect);
                    }
                }

                *hovered_drop_target = current_hovered_drop_target;
                hovered_drop_target_rect = current_hovered_drop_target_rect;
            });

        ui.add_space(layout.header_gap);

        if let Some(rect) = hovered_drop_target_rect {
            let painter = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drop_highlight"),
            ));
            painter.rect_filled(
                rect,
                egui::CornerRadius::same(palette.medium_radius),
                palette.primary.linear_multiply(0.1),
            );
            painter.rect_stroke(
                rect,
                egui::CornerRadius::same(palette.medium_radius),
                egui::Stroke::new(1.5, palette.primary_active),
                egui::StrokeKind::Inside,
            );
        }

        let pointer_released = ui
            .ctx()
            .input(|i| i.pointer.any_released() && i.pointer.interact_pos().is_some());

        if drag_state.active && pointer_released {
            if let Some(target_dir) = hovered_drop_target {
                // Drop into hovered folder
                action = Some(ItemViewerAction::MoveItems {
                    sources: drag_state.source_items.clone(),
                    target_dir,
                });
            } else {
                // Optional: drop into current directory
                // action = Some(ItemViewerAction::MoveItems {
                //     sources: drag_state.source_items.clone(),
                //     target_dir: current_directory.clone(),
                // });
            }

            drag_state.active = false;
            drag_state.start_pos = None;
            drag_state.source_items.clear();
        }

        if let Some(a) = handle_keyboard_navigation(
            ui.ctx(),
            &filter_state.cached_indices,
            files,
            layout.is_drive_view,
            explorer_state,
        ) {
            action = Some(a);
        }

        // --- Drag and Drop Detection ---
        // Check for external drag and drop

        if ui.ctx().input(|i| !i.raw.dropped_files.is_empty()) {
            let dropped_paths: Vec<PathBuf> = ui.ctx().input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .filter_map(|f| f.path.clone())
                    .collect()
            });

            if !dropped_paths.is_empty() {
                action = Some(ItemViewerAction::FilesDropped(dropped_paths));
            }
        }

        // Update drag hover state
        *external_drag_to_internal_hover = ui
            .ctx()
            .input(|i| i.raw.hovered_files.iter().any(|f| f.path.is_some()));

        // 👇 Fill remaining space so empty area is interactable
        let remaining_rect = ui.available_rect_before_wrap();

        let bg_response = ui.allocate_rect(remaining_rect, egui::Sense::click());

        if bg_response.clicked() {
            action = Some(ItemViewerAction::DeselectAll);
        }

        bg_response.context_menu(|ui| {
            // 👇 Only show when NOT clicking on a row
            if !any_row_hovered {
                if ui
                    .add_enabled(paste_enabled, egui::Button::new("Paste"))
                    .clicked()
                {
                    action = Some(ItemViewerAction::Context(ItemViewerContextAction::Paste));
                    ui.close();
                }
            }
        });

        if let Some(_path) = explorer_state.non_ntfs_popup_path.clone() {
            let mut open = true;
            egui::Window::new("Non-NTFS Drive")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .open(&mut open)
                .show(ui.ctx(), |ui| {
                    ui.label(
                        egui::RichText::new("This is a non-NTFS drive. Please mount it first if you'd like to explore it, or use an external tool to access this filesystem.")
                            .size(palette.text_size)
                            .color(palette.tooltip_text_color)
                            .font(font_id.clone()),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(format!("{} Ok", regular::CHECK)).clicked() {
                                explorer_state.non_ntfs_popup_path = None;
                            }
                        });
                    });
                });
            if !open {
                explorer_state.non_ntfs_popup_path = None;
            }
        }

        action
    } else {
        return action;
    }
}

fn draw_external_to_internal_drag_overlay(
    ui: &mut egui::Ui,
    external_drag_to_internal_hover: bool,
) {
    if external_drag_to_internal_hover {
        let rect = ui.max_rect();

        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(6),
            ui.visuals().selection.bg_fill.linear_multiply(0.15),
        );

        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Move to this folder",
            egui::TextStyle::Heading.resolve(ui.style()),
            ui.visuals().text_color(),
        );
    }
}

fn compute_layout(ui: &egui::Ui, is_drive_view: bool) -> ItemViewerLayout {
    let text_height = 14.0;
    let row_padding = 6.0;
    let row_height = text_height + row_padding;

    let header_padding = 0.0;
    let header_height = row_height + header_padding;

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
    paste_enabled: bool,
    is_drive_view: bool,
    is_cut: bool,
    action: &mut Option<ItemViewerAction>,
    palette: &ThemePalette,
    explorer_state: &mut ExplorerState,
) {
    // ✅ Match Explorer behavior: right-click selects if not already selected
    if !is_selected {
        *action = Some(ItemViewerAction::ReplaceSelection(file.path.clone()));
    }

    // 🚗 DRIVE VIEW MODE → ONLY PROPERTIES
    if is_drive_view {
        if ui.button("Properties").clicked() {
            let targets: Vec<PathBuf> = if is_selected {
                explorer_state.selected_paths.iter().cloned().collect()
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

    // Determine if "Open in new tab" should be enabled
    // Enable only if a single path is selected
    let enable_open_in_tab = explorer_state.selected_paths.len() == 1;

    // Add the button
    if ui
        .add_enabled(enable_open_in_tab, egui::Button::new("Open in new tab"))
        .clicked()
    {
        if let Some(path) = explorer_state.selected_paths.iter().next() {
            *action = Some(ItemViewerAction::OpenInNewTab(path.clone()));
            ui.close();
        }
    }

    // Determine button label based on selection count
    let label = if explorer_state.selected_paths.len() == 1 {
        "Open in Default Program"
    } else {
        "Open Files in Default Program"
    };

    // Check if all selected files are not directories
    let all_files = explorer_state
        .selected_paths
        .iter()
        .all(|path| !path.is_dir());

    // Add the button with dynamic label
    if ui
        .add_enabled(all_files, egui::Button::new(label))
        .clicked()
    {
        let paths: Vec<PathBuf> = explorer_state.selected_paths.iter().cloned().collect();
        *action = Some(ItemViewerAction::OpenWithDefault(paths));
        ui.close();
    }

    ui.separator();

    if ui.add_enabled(!is_cut, egui::Button::new("Cut")).clicked() {
        let paths = if !explorer_state.selected_paths.is_empty() {
            explorer_state.selected_paths.iter().cloned().collect()
        } else {
            vec![file.path.clone()]
        };

        *action = Some(ItemViewerAction::Context(ItemViewerContextAction::Cut(
            paths,
        )));
        ui.close();
    }
    if ui.button("Copy").clicked() {
        let paths = if !explorer_state.selected_paths.is_empty() {
            explorer_state.selected_paths.iter().cloned().collect()
        } else {
            vec![file.path.clone()]
        };

        *action = Some(ItemViewerAction::Context(ItemViewerContextAction::Copy(
            paths,
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
        let paths = if !explorer_state.selected_paths.is_empty() {
            explorer_state.selected_paths.iter().cloned().collect()
        } else {
            vec![file.path.clone()]
        };

        *action = Some(ItemViewerAction::Context(ItemViewerContextAction::Delete(
            paths,
        )));
        ui.close();
    }

    ui.separator();

    // Properties (multi-select aware)
    if ui.button("Properties").clicked() {
        let targets: Vec<PathBuf> = if is_selected {
            explorer_state.selected_paths.iter().cloned().collect()
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

    let text_rect =
        egui::Rect::from_min_max(egui::pos2(rect.min.x + text_offset_x, rect.min.y), rect.max);

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

    let text_width = available_width - text_offset_x;
    let color = get_text_color(is_selected, is_cut, palette);

    let (display_name, _) = truncate_item_text(ui, &file.name, text_width, font_id, color);

    let text_pos = egui::pos2(rect.min.x + text_offset_x, rect.center().y);

    ui.painter().text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        display_name,
        font_id.clone(),
        color,
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
    let color = get_text_color(is_selected, is_cut, palette);

    let type_text = if file.is_dir {
        "Folder"
    } else if let Some(ext) = file.path.extension().and_then(|ext| ext.to_str()) {
        get_file_type_name(ext, file_type_cache)
    } else {
        get_file_type_name("", file_type_cache)
    };

    let mut rich_text = egui::RichText::new(type_text)
        .size(palette.text_size)
        .color(color);

    if is_cut {
        rich_text = rich_text.italics();
    }

    let resp = ui.add(
        egui::Label::new(rich_text.font(font_id.clone()))
            .truncate() // 🔥 THIS fixes multi-line + resizing
            .sense(egui::Sense::hover()),
    );

    resp.on_hover_cursor(egui::CursorIcon::Default);
}

fn handle_draw_col_size(
    ui: &mut egui::Ui,
    file: &FileItem,
    folder_sizes: &HashMap<PathBuf, ItemViewerFolderSizeState>,
    is_selected: bool,
    is_cut: bool,
    palette: &ThemePalette,
    font_id: &egui::FontId,
    file_size_text_cache: &mut HashMap<PathBuf, (u64, String)>,
    folder_size_text_cache: &mut HashMap<PathBuf, (u64, bool, String)>,
    drive_size_text_cache: &mut HashMap<PathBuf, (u64, u64, String)>,
) {
    let text_color = get_text_color(is_selected, is_cut, palette);

    // --- DRIVE VIEW (free / total) ---
    if let (Some(total), Some(free)) = (file.total_space, file.free_space) {
        let key = &file.path;
        let text = if let Some((cached_total, cached_free, cached_text)) =
            drive_size_text_cache.get(key)
        {
            if *cached_total == total && *cached_free == free {
                cached_text.as_str()
            } else {
                ""
            }
        } else {
            ""
        };

        let display_text = if text.is_empty() {
            let formatted = format!("{} / {}", format_size(free), format_size(total));
            drive_size_text_cache.insert(file.path.clone(), (total, free, formatted));
            drive_size_text_cache
                .get(key)
                .map(|(_, _, t)| t.as_str())
                .unwrap_or("")
        } else {
            text
        };

        ui.add(
            egui::Label::new(
                egui::RichText::new(display_text)
                    .size(palette.text_size)
                    .color(text_color)
                    .font(font_id.clone()),
            )
            .truncate(),
        );

        return;
    }

    // --- FOLDER SIZE ---
    if file.is_dir {
        if let Some(state) = folder_sizes.get(&file.path) {
            let cached = folder_size_text_cache.get(&file.path);
            let text = match cached {
                Some((bytes, done, value)) if *bytes == state.bytes && *done == state.done => {
                    value.as_str()
                }
                _ => {
                    let label = format_size(state.bytes);
                    let value = if state.done {
                        label
                    } else {
                        format!("⏳ {}", label)
                    };
                    folder_size_text_cache
                        .insert(file.path.clone(), (state.bytes, state.done, value));
                    folder_size_text_cache
                        .get(&file.path)
                        .map(|(_, _, v)| v.as_str())
                        .unwrap_or("")
                }
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
        let cached = file_size_text_cache.get(&file.path);
        let text = match cached {
            Some((cached_size, value)) if *cached_size == size => value.as_str(),
            _ => {
                let value = format_size(size);
                file_size_text_cache.insert(file.path.clone(), (size, value));
                file_size_text_cache
                    .get(&file.path)
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("")
            }
        };
        ui.label(
            egui::RichText::new(text)
                .size(palette.text_size)
                .color(text_color)
                .font(font_id.clone()),
        )
        .on_hover_cursor(egui::CursorIcon::Default);
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
        let color = get_text_color(is_selected, is_cut, palette);

        if let Some(m) = &file.modified_time {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(m)
                        .size(palette.text_size)
                        .color(color)
                        .font(font_id.clone()),
                )
                .truncate()
                .sense(egui::Sense::hover()),
            );
        } else {
            draw_placeholder(ui, palette, font_id, color);
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
    let color = get_text_color(is_selected, is_cut, palette);

    let text = file.created_time.as_deref().unwrap_or("—");

    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(palette.text_size)
                .color(color)
                .font(font_id.clone()),
        )
        .truncate()
        .sense(egui::Sense::hover()),
    );
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
    )
    .on_hover_cursor(egui::CursorIcon::Default);
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
        palette.item_viewer_row_text_selected
    } else {
        palette.item_viewer_row_text_normal
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

        let edit_id = ui.id().with("rename_input").with(&file.path);
        let edit_response = ui.add(
            egui::TextEdit::singleline(&mut rename_state.new_name)
                .id(edit_id)
                .desired_width(f32::INFINITY)
                .font(FontId::new(palette.text_size, FontFamily::Proportional)),
        );

        // ✅ Focus once
        if rename_state.should_focus {
            ui.memory_mut(|mem| mem.request_focus(edit_id));
            edit_response.request_focus();
            if edit_response.has_focus() {
                rename_state.should_focus = false;
            }
        }

        // ✅ Input handling (same pattern as tabs)
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));

        if enter {
            let new_name = rename_state.new_name.trim().to_string();

            action = Some(ItemViewerAction::Context(
                ItemViewerContextAction::RenameRequest(file.path.clone(), new_name),
            ));
        } else if escape {
            action = Some(ItemViewerAction::Context(
                ItemViewerContextAction::RenameCancel,
            ));
        } else if edit_response.lost_focus() {
            // 👈 matches Windows: clicking away cancels rename
            action = Some(ItemViewerAction::Context(
                ItemViewerContextAction::RenameCancel,
            ));
        }
    });

    action
}

fn handle_global_actions(
    ui: &mut egui::Ui,
    files: &[FileItem],
    palette: &ThemePalette,
    tabbar_action: &mut Option<TabbarAction>,
    rename_state: &mut Option<RenameState>,
    filter_state: &mut FilterState,
    drag_state: &mut DragState,
    explorer_state: &mut ExplorerState,
    is_cut_mode: bool,
) -> Option<ItemViewerAction> {
    let filtered_indices = &filter_state.cached_indices;
    let mut action: Option<ItemViewerAction> = None;

    let is_text_edit_active = tabbar_action
        .as_ref()
        .is_some_and(|t| t.is_breadcrumb_path_edit_active);

    // =====================================================
    // 🥇 PRIORITY 1: RENAME MODE (let TextEdit own everything)
    // =====================================================
    if rename_state.is_some() || is_text_edit_active {
        return None;
    }

    if is_cut_mode {
        let cancel_called = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if cancel_called {
            clear_clipboard_files();
        }
    }

    // =====================================================
    // 🥈 PRIORITY 2: FILTER MODE (TextEdit owns input)
    // =====================================================
    if filter_state.active {
        let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));

        if cancel {
            let text_edit_id = ui.id().with("filter_input");
            ui.memory_mut(|mem| {
                mem.data
                    .remove::<egui::text_edit::TextEditState>(text_edit_id)
            });
            *filter_state = FilterState::default();
            return None;
        }

        let text_edit_id = ui.id().with("filter_input");

        let response = ui.add(
            egui::TextEdit::singleline(&mut filter_state.query)
                .id(text_edit_id)
                .desired_width(200.0)
                .font(FontId::new(
                    palette.text_size,
                    egui::FontFamily::Proportional,
                )),
        );

        if !filter_state.focus_requested {
            response.request_focus();
            filter_state.focus_requested = true;
        }

        if response.clicked_elsewhere() {
            ui.memory_mut(|mem| {
                mem.data
                    .remove::<egui::text_edit::TextEditState>(text_edit_id)
            });
            *filter_state = FilterState::default();
        }

        return None;
    }

    // =====================================================
    // 🥉 PRIORITY 3: DRAG STATE
    // =====================================================
    if drag_state.active {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            drag_state.active = false;
            drag_state.source_items.clear();
            drag_state.start_pos = None;
        }
    }

    // =====================================================
    // 🔹 GLOBAL SHORTCUTS
    // =====================================================
    ui.input(|i| {
        for event in &i.events {
            match event {
                egui::Event::Copy => {
                    if !explorer_state.selected_paths.is_empty() {
                        action = Some(ItemViewerAction::Context(ItemViewerContextAction::Copy(
                            explorer_state.selected_paths.iter().cloned().collect(),
                        )));
                    }
                }
                egui::Event::Cut => {
                    if !explorer_state.selected_paths.is_empty() {
                        action = Some(ItemViewerAction::Context(ItemViewerContextAction::Cut(
                            explorer_state.selected_paths.iter().cloned().collect(),
                        )));
                    }
                }
                _ => {}
            }
        }
    });
    ui.input(|i| {
        if i.key_pressed(egui::Key::Backspace) {
            action = Some(ItemViewerAction::BackNavigation);
        }
        if i.key_pressed(egui::Key::Enter) {
            let selected_paths: Vec<PathBuf> = explorer_state
                .selected_paths
                .iter()
                .filter_map(|p| {
                    files
                        .iter()
                        .find(|f| &f.path == p && !f.is_dir)
                        .map(|_| p.clone())
                })
                .collect();

            if !selected_paths.is_empty() {
                action = Some(ItemViewerAction::OpenWithDefault(selected_paths));
            }

            // Optionally handle directories separately:
            for dir_path in explorer_state
                .selected_paths
                .iter()
                .filter(|p| files.iter().any(|f| &f.path == *p && f.is_dir))
            {
                action = Some(ItemViewerAction::Open(dir_path.clone()));
            }
        }
        if i.modifiers.command && i.key_pressed(egui::Key::A) {
            action = Some(ItemViewerAction::SelectAll);
        }
        if i.modifiers.command && i.key_released(egui::Key::V) {
            // Any other key functions won't work with egui v0.33.x
            action = Some(ItemViewerAction::Context(ItemViewerContextAction::Paste));
        }
        if i.key_pressed(egui::Key::Delete) {
            let paths: Vec<PathBuf> = if !explorer_state.selected_paths.is_empty() {
                explorer_state.selected_paths.iter().cloned().collect()
            } else if !filtered_indices.is_empty() {
                vec![files[filtered_indices[0]].path.clone()]
            } else {
                return;
            };

            action = Some(ItemViewerAction::Context(ItemViewerContextAction::Delete(
                paths,
            )));
        }
    });

    // =====================================================
    // PRIORITY 4: GLOBAL INPUT (navigation + shortcuts)
    // =====================================================
    let mut start_filter = String::new();

    ui.input(|i| {
        if i.modifiers.command || i.modifiers.ctrl {
            return;
        }
        for event in &i.events {
            if let egui::Event::Text(text) = event {
                if text.chars().all(|c| !c.is_control()) {
                    start_filter.push_str(text);
                }
            }
        }
    });

    if !start_filter.is_empty() {
        filter_state.active = true;
        filter_state.query.push_str(&start_filter);
        filter_state.last_input_time = ui.input(|i| i.time);
        return None;
    }

    action
}

fn draw_item_viewer_header(
    header: &mut egui_extras::TableRow<'_, '_>,
    is_drive_view: bool,
    filtered_indices: &[usize],
    files: &[FileItem],
    sort_column: SortColumn,
    sort_ascending: bool,
    palette: &crate::gui::theme::ThemePalette,
    explorer_state: &mut ExplorerState,
) -> Option<ItemViewerAction> {
    let font_id = FontId::new(palette.text_size, FontFamily::Proportional);
    let mut action: Option<ItemViewerAction> = None;
    if !is_drive_view {
        header.col(|ui| {
            let mut all_selected = !filtered_indices.is_empty()
                && filtered_indices
                    .iter()
                    .all(|&i| explorer_state.selected_paths.contains(&files[i].path));

            ui.scope(|ui| {
                apply_checkbox_colors(ui, palette, all_selected);
                if ui.checkbox(&mut all_selected, "").clicked() {
                    if all_selected {
                        action = Some(ItemViewerAction::SelectAll);
                    } else {
                        action = Some(ItemViewerAction::DeselectAll);
                    }
                }
            });
        });
    }

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
                    .color(palette.item_viewer_col_header_text),
            )
            .selectable(false)
            .sense(egui::Sense::click()),
        );
        if resp.clicked() {
            action = Some(ItemViewerAction::Sort(SortColumn::Name));
        }
    });

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
                    .color(palette.item_viewer_col_header_text),
            )
            .selectable(false)
            .sense(egui::Sense::click()),
        );
        if resp.clicked() {
            action = Some(ItemViewerAction::Sort(SortColumn::Type));
        }
    });

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
                    .color(palette.item_viewer_col_header_text),
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

    if is_drive_view {
        header.col(|ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("Usage").trim_end())
                        .font(font_id.clone())
                        .size(palette.text_size)
                        .color(palette.item_viewer_col_header_text),
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
                        .color(palette.item_viewer_col_header_text),
                )
                .selectable(false)
                .sense(egui::Sense::click()),
            );
            if resp.clicked() {
                action = Some(ItemViewerAction::Sort(SortColumn::Modified));
            }
        });

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
                        .color(palette.item_viewer_col_header_text),
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

fn handle_keyboard_navigation(
    ctx: &egui::Context,
    filtered_indices: &[usize],
    files: &Vec<FileItem>,
    is_drive_view: bool,
    explorer_state: &mut ExplorerState,
) -> Option<ItemViewerAction> {
    if filtered_indices.is_empty() {
        return None;
    }

    let is_selectable = |row_idx: usize| -> bool {
        if !is_drive_view {
            return true;
        }
        let file_idx = filtered_indices[row_idx];
        !is_raw_physical_drive_path(&files[file_idx].path)
    };

    let next_selectable = |start: usize, dir: i32| -> Option<usize> {
        let mut i = start as i32;
        loop {
            i += dir;
            if i < 0 || i >= filtered_indices.len() as i32 {
                return None;
            }
            let idx = i as usize;
            if is_selectable(idx) {
                return Some(idx);
            }
        }
    };

    let mut action: Option<ItemViewerAction> = None;

    let current_index = explorer_state
        .selected_paths
        .iter()
        .next()
        .and_then(|selected| {
            filtered_indices
                .iter()
                .position(|&i| &files[i].path == selected)
        });

    let current_idx = match current_index {
        Some(idx) => idx,
        None => {
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                let first_idx = (0..filtered_indices.len()).find(|&i| is_selectable(i))?;
                let first = files[filtered_indices[first_idx]].path.clone();

                explorer_state.selection_anchor = Some(first_idx);
                explorer_state.selection_focus = Some(first_idx);

                return Some(ItemViewerAction::ReplaceSelection(first));
            }

            if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                let last_idx = (0..filtered_indices.len())
                    .rev()
                    .find(|&i| is_selectable(i))?;
                let last = files[filtered_indices[last_idx]].path.clone();

                explorer_state.selection_anchor = Some(last_idx);
                explorer_state.selection_focus = Some(last_idx);

                return Some(ItemViewerAction::ReplaceSelection(last));
            }

            return None;
        }
    };

    // 🔥 SHIFT RANGE
    if ctx.input(|i| i.modifiers.shift) {
        let anchor = explorer_state.selection_anchor.unwrap_or(current_idx);
        let focus = explorer_state.selection_focus.unwrap_or(current_idx);

        let mut new_focus = focus;

        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            if let Some(next) = next_selectable(focus, 1) {
                new_focus = next;
            }
        }

        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            if let Some(prev) = next_selectable(focus, -1) {
                new_focus = prev;
            }
        }

        explorer_state.selection_anchor = Some(anchor);
        explorer_state.selection_focus = Some(new_focus);

        let range_start = anchor.min(new_focus);
        let range_end = anchor.max(new_focus);

        let range_paths: Vec<PathBuf> = filtered_indices[range_start..=range_end]
            .iter()
            .filter(|&&i| {
                if !is_drive_view {
                    true
                } else {
                    !is_raw_physical_drive_path(&files[i].path)
                }
            })
            .map(|&i| files[i].path.clone())
            .collect();

        action = Some(ItemViewerAction::RangeSelect(range_paths));
    }
    // 🔹 NORMAL NAV
    else {
        let mut new_idx = current_idx;

        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            if let Some(next) = next_selectable(current_idx, 1) {
                new_idx = next;
            }
        }

        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            if let Some(prev) = next_selectable(current_idx, -1) {
                new_idx = prev;
            }
        }

        if new_idx != current_idx {
            let new_path = files[filtered_indices[new_idx]].path.clone();

            explorer_state.selection_anchor = Some(new_idx);
            explorer_state.selection_focus = Some(new_idx);

            action = Some(ItemViewerAction::ReplaceSelection(new_path));
        }
    }

    action
}

fn handle_row_click(
    row_idx: usize,
    file: &FileItem,
    modifiers: egui::Modifiers,
    filtered_indices: &[usize],
    files: &[FileItem],
    drag_state: &DragState,
    explorer_state: &mut ExplorerState,
) -> Option<ItemViewerAction> {
    if drag_state.active {
        return None;
    }

    if modifiers.shift {
        if let Some(anchor_idx) = explorer_state.selection_anchor {
            let current_idx = row_idx;
            let range_start = anchor_idx.min(current_idx);
            let range_end = anchor_idx.max(current_idx);

            let range_paths: Vec<PathBuf> = filtered_indices[range_start..=range_end]
                .iter()
                .map(|&i| files[i].path.clone())
                .collect();

            explorer_state.selection_focus = Some(current_idx);

            Some(ItemViewerAction::RangeSelect(range_paths))
        } else {
            explorer_state.selection_anchor = Some(row_idx);
            explorer_state.selection_focus = Some(row_idx);

            Some(ItemViewerAction::Select(file.path.clone()))
        }
    } else if modifiers.ctrl {
        if !explorer_state.selected_paths.contains(&file.path) {
            explorer_state.selected_paths.insert(file.path.clone());
        }

        explorer_state.selection_anchor = Some(row_idx);
        explorer_state.selection_focus = Some(row_idx);

        Some(ItemViewerAction::Select(file.path.clone()))
    } else {
        let is_single_selected = explorer_state.selected_paths.len() == 1
            && explorer_state.selected_paths.contains(&file.path);

        if is_single_selected {
            return Some(if file.is_dir {
                ItemViewerAction::Open(file.path.clone())
            } else {
                ItemViewerAction::OpenWithDefault(vec![file.path.clone()])
            });
        } else {
            explorer_state.selection_anchor = Some(row_idx);
            explorer_state.selection_focus = Some(row_idx);

            Some(ItemViewerAction::ReplaceSelection(file.path.clone()))
        }
    }
}
