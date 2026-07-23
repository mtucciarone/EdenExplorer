use crate::core::drives::is_raw_physical_drive_path;
use crate::core::fs::FileItem;
use crate::core::utils::widgets::draw_checkbox;
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::utils::{SortColumn, draw_object_drag_ghost, fuzzy_match};
use crate::gui::windows::containers::enums::{ItemViewerAction, ItemViewerContextAction};
use crate::gui::windows::containers::itemviewer_helper::*;
use crate::gui::windows::containers::structs::{
    DragState, ExplorerState, FilterState, ItemViewerFolderSizeState, ItemViewerNavBarAction,
    RenameState, TagsState,
};
use crate::gui::windows::structs::{SettingsWindow, ThemeCustomizer};
use eframe::egui;
use egui::containers::{Popup, PopupCloseBehavior};
use egui::{FontFamily, FontId};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use windows::Win32::Foundation::HWND;

pub fn draw_item_viewer(
    ui: &mut egui::Ui,
    i18n: &I18n,
    files: &Vec<FileItem>,
    folder_sizes: &HashMap<PathBuf, ItemViewerFolderSizeState>,
    paste_enabled: bool,
    clipboard_set: &HashSet<PathBuf>,
    is_cut_mode: bool,
    is_drive_view: bool,
    sort_column: SortColumn,
    sort_ascending: bool,
    show_hidden_files_folders: bool,
    show_item_viewer_icons: bool,
    icon_cache: &IconCache,
    rename_state: &mut Option<RenameState>,
    palette: &ThemePalette,
    file_type_cache: &mut HashMap<String, String>,
    file_size_text_cache: &mut HashMap<PathBuf, (u64, String)>,
    folder_size_text_cache: &mut HashMap<PathBuf, (u64, bool, String)>,
    drive_size_text_cache: &mut HashMap<PathBuf, (u64, u64, String)>,
    external_drag_to_internal_hover: &mut bool,
    tabbar_action: &mut Option<ItemViewerNavBarAction>,
    drag_state: &mut DragState,
    drag_active: bool,
    native_drag_active: bool,
    drag_hover_target: Option<PathBuf>,
    current_dir: PathBuf,
    filter_state: &mut FilterState,
    hovered_drop_target_out: &mut Option<PathBuf>,
    hovered_drop_target_rect_out: &mut Option<egui::Rect>,
    is_loading: bool,
    explorer_state: &mut ExplorerState,
    tags_state: &mut TagsState,
    theme_customizer_window: &mut ThemeCustomizer,
    settings_window: &mut SettingsWindow,
    hwnd: Option<HWND>,
    is_focused: bool,
    active_tab_id: u64,
) -> Option<ItemViewerAction> {
    let font_id = FontId::new(palette.text_size, FontFamily::Proportional);
    let mut hovered_drop_target: Option<PathBuf> = None;
    let mut hovered_drop_target_rect: Option<egui::Rect> = None;
    draw_external_to_internal_drag_overlay(ui, i18n, *external_drag_to_internal_hover);

    let layout = compute_layout(ui, is_drive_view, palette);
    let modal_input_blocked =
        tags_state.picker.is_some() || theme_customizer_window.open || settings_window.open;

    let mut action: Option<ItemViewerAction> = None;
    let mut any_row_hovered = false;

    let filter_changed = filter_state.dirty
        || filter_state.query != filter_state.last_query
        || filter_state.last_files_len != files.len()
        || filter_state.last_show_hidden_files_folders != show_hidden_files_folders;

    if filter_changed {
        filter_state.cached_indices = files
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                (show_hidden_files_folders || !f.is_hidden)
                    && fuzzy_match(&f.name, &filter_state.query)
            })
            .map(|(i, _)| i)
            .collect();

        filter_state.last_query = filter_state.query.clone();
        filter_state.last_files_len = files.len();
        filter_state.last_show_hidden_files_folders = show_hidden_files_folders;
        filter_state.dirty = false;
    }

    let visible_items_empty = filter_state.cached_indices.is_empty();
    if filter_changed
        && explorer_state.selected_paths.iter().any(|selected| {
            !filter_state
                .cached_indices
                .iter()
                .any(|&i| &files[i].path == selected)
        })
    {
        explorer_state.selected_paths.clear();
        explorer_state.selection_anchor = None;
        explorer_state.selection_focus = None;
    }

    if visible_items_empty {
        ui.centered_and_justified(|ui| {
            if is_loading && files.is_empty() {
                ui.add(egui::Spinner::new().size(28.0));
            } else {
                ui.label(i18n.tr("folder_is_empty"));
            }
        });
    }

    if drag_active {
        let unknown_label = &i18n.tr("unknown");
        let label = if drag_state.source_items.len() == 1 {
            drag_state
                .source_items
                .first()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or(unknown_label)
        } else {
            &i18n.tr("items")
        };

        draw_object_drag_ghost(ui, palette, label, false);
    }

    if !modal_input_blocked && is_focused {
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
            theme_customizer_window,
            settings_window,
        ) {
            action = Some(global_action);
        }
    }

    let mut current_hovered_drop_target: Option<PathBuf> = None;
    let mut current_hovered_drop_target_rect: Option<egui::Rect> = None;
    let mut best_hovered_row: Option<(f32, bool, PathBuf, egui::Rect)> = None;
    let drag_hover_active = ui.ctx().input(|i| {
        drag_active
            || native_drag_active
            || i.raw.hovered_files.iter().any(|file| file.path.is_some())
    });
    let external_file_hover = ui
        .ctx()
        .input(|i| i.raw.hovered_files.iter().any(|file| file.path.is_some()));
    let pointer_pos = ui
        .ctx()
        .input(|i| i.pointer.interact_pos().or_else(|| i.pointer.hover_pos()));
    let pointer_released = ui.ctx().input(|i| i.pointer.primary_released());
    let hovered_target_ref = drag_hover_target.as_ref();

    if !visible_items_empty {
        let modifiers = ui.ctx().input(|i| i.modifiers);
        let arrow_nav = ui.ctx().input(|i| {
            i.key_pressed(egui::Key::ArrowDown)
                || i.key_pressed(egui::Key::ArrowUp)
                || i.key_pressed(egui::Key::Home)
                || i.key_pressed(egui::Key::End)
        });
        let current_width = ui.available_width();
        let available_height = ui.available_height();

        let mut table = TableBuilder::new(ui)
            .vscroll(true)
            .max_scroll_height(available_height)
            .min_scrolled_height(0.0)
            .striped(false)
            .sense(if layout.is_drive_view {
                egui::Sense::click()
            } else {
                egui::Sense::click_and_drag()
            })
            .animate_scrolling(true)
            .resizable(true)
            .id_salt(("item_viewer_table", active_tab_id));

        // If we have a pending selection from a refresh, scroll to it and select it
        if let Some(pending_paths) = explorer_state.pending_selection_paths.clone() {
            let mut selected_indices = Vec::with_capacity(pending_paths.len());
            let mut all_found = true;

            for path in &pending_paths {
                if let Some(idx) = filter_state
                    .cached_indices
                    .iter()
                    .position(|&i| &files[i].path == path)
                {
                    selected_indices.push(idx);
                } else {
                    all_found = false;
                    break;
                }
            }

            if all_found && !selected_indices.is_empty() {
                selected_indices.sort_unstable();
                table = table.scroll_to_row(selected_indices[0], Some(egui::Align::Center));

                explorer_state.selected_paths.clear();
                for path in pending_paths {
                    explorer_state.selected_paths.insert(path);
                }
                explorer_state.selection_anchor = Some(selected_indices[0]);
                explorer_state.selection_focus = Some(*selected_indices.last().unwrap());
                explorer_state.pending_selection_paths = None;
            }
        }

        // If we have a navigation selection, scroll to it and select it
        if let Some(nav_path) = &explorer_state.navigation_selection {
            if let Some(idx) = filter_state
                .cached_indices
                .iter()
                .position(|&i| &files[i].path == nav_path)
            {
                table = table.scroll_to_row(idx, Some(egui::Align::Center));

                // Auto-select the navigation item
                explorer_state.selected_paths.clear();
                explorer_state.selected_paths.insert(nav_path.clone());
                explorer_state.selection_anchor = Some(idx);
                explorer_state.selection_focus = Some(idx);

                explorer_state.navigation_selection = None;
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
            table = table.column(Column::exact(16.0));
        }

        table = table
            .column(
                Column::initial(current_width * 0.35)
                    .at_least(200.0)
                    .resizable(true),
            ) // Name
            .column(
                Column::initial(current_width * 0.1)
                    .at_least(60.0)
                    .resizable(true),
            ); // Type

        if layout.is_drive_view {
            table = table.column(
                Column::initial(current_width * 0.14)
                    .at_least(120.0)
                    .resizable(true),
            ); // Size
        } else {
            table = table.column(
                Column::initial(current_width * 0.1)
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
                    Column::initial(current_width * 0.2)
                        .at_least(120.0)
                        .resizable(true),
                ) // Modified
                .column(Column::remainder().at_least(120.0).resizable(true));
            // Created
        }

        table
            .header(layout.header_height, |mut header| {
                if let Some(a) = draw_item_viewer_header(
                    i18n,
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
                    let is_selected = explorer_state.selected_paths.contains(&file.path);
                    let tag_color = tags_state.tag_color_for_path(&file.path);
                    row.set_selected(is_selected);
                    let is_cut = is_cut_mode && clipboard_set.contains(&file.path);

                    if !layout.is_drive_view {
                        row.col(|ui| {
                            let mut checked = is_selected;

                            if draw_checkbox(ui, palette, &mut checked, &file.path).clicked() {
                                if checked {
                                    action = Some(ItemViewerAction::Select(file.path.clone()));
                                } else {
                                    action = Some(ItemViewerAction::Deselect(file.path.clone()));
                                }
                            }
                        });
                    }

                    row.col(|ui| {
                        if let Some(a) = handle_draw_col_name(
                            ui,
                            i18n,
                            file,
                            &layout,
                            icon_cache,
                            is_selected,
                            is_cut,
                            palette,
                            &font_id,
                            rename_state,
                            show_item_viewer_icons,
                        ) {
                            action = Some(a);
                        }
                    });

                    row.col(|ui| {
                        handle_draw_col_type(
                            ui,
                            file,
                            &layout,
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
                            &layout,
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
                                &layout,
                                is_selected,
                                is_cut,
                                palette,
                                &font_id,
                            );
                        });
                    }

                    let row_resp = row.response();

                    if let Some(tag_color) = tag_color {
                        let tag_rect = egui::Rect::from_min_size(
                            row_resp.rect.min,
                            egui::vec2(row_resp.rect.width(), layout.row_height),
                        )
                        .shrink2(egui::vec2(0.0, 1.0));
                        let painter = row_resp.ctx.layer_painter(egui::LayerId::new(
                            egui::Order::Background,
                            egui::Id::new(("tag_row_bg", active_tab_id, &file.path)),
                        ));
                        painter.rect_filled(tag_rect, egui::CornerRadius::same(palette.medium_radius), tag_color.linear_multiply(0.18));
                    }

                    if drag_hover_active {
                        if let Some(target) = hovered_target_ref {
                            if &file.path == target && file.is_dir {
                                current_hovered_drop_target = Some(file.path.clone());
                                current_hovered_drop_target_rect = Some(row_resp.rect);
                            }
                        } else if let Some(pointer) = pointer_pos {
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

                        if explorer_state.selected_paths.contains(&file.path) {
                            drag_state.source_items =
                                explorer_state.selected_paths.iter().cloned().collect();
                        } else {
                            // Click-drag on a row should promote that row into the selection.
                            explorer_state.selected_paths.clear();
                            explorer_state.selected_paths.insert(file.path.clone());
                            explorer_state.selection_anchor = Some(idx);
                            explorer_state.selection_focus = Some(idx);
                            drag_state.source_items = vec![file.path.clone()];
                        }
                    }

                    if let (Some(start), Some(current)) = (
                        drag_state.start_pos,
                        row_resp.ctx.input(|i| i.pointer.hover_pos()),
                    ) {
                        if !drag_state.active
                            && !drag_state.source_items.is_empty()
                            && start.distance(current) > 4.0
                        {
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
                        Popup::context_menu(&row_resp)
                            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                handle_context_menu_actions(
                                    ui,
                                    i18n,
                                    file,
                                    is_selected,
                                    paste_enabled,
                                    layout.is_drive_view,
                                    is_cut,
                                    &mut action,
                                    palette,
                                    explorer_state,
                                    tags_state,
                                    settings_window,
                                    hwnd,
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

                *hovered_drop_target = current_hovered_drop_target.clone();
                hovered_drop_target_rect = current_hovered_drop_target_rect;
            });

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
                egui::StrokeKind::Outside,
            );
        }

        *hovered_drop_target_out = hovered_drop_target.clone();
        *hovered_drop_target_rect_out = hovered_drop_target_rect;

        if !modal_input_blocked {
            if let Some(a) = handle_keyboard_navigation(
                ui.ctx(),
                &filter_state.cached_indices,
                files,
                layout.is_drive_view,
                explorer_state,
            ) {
                action = Some(a);
            }
        }

        // --- Drag and Drop Detection ---
        *external_drag_to_internal_hover = native_drag_active || external_file_hover;
        // Fill remaining space so empty area is interactable
        let remaining_rect = ui.available_rect_before_wrap();

        let bg_response = ui.allocate_rect(remaining_rect, egui::Sense::click());

        if drag_state.active && pointer_released {
            let target_dir = current_hovered_drop_target
                .clone()
                .or_else(|| bg_response.hovered().then(|| current_dir.clone()));

            if let Some(target_dir) = target_dir {
                action = Some(ItemViewerAction::MoveItems {
                    sources: drag_state.source_items.clone(),
                    target_dir,
                });
            }

            drag_state.active = false;
            drag_state.start_pos = None;
            drag_state.source_items.clear();
        }

        if !modal_input_blocked {
            if bg_response.clicked() {
                action = Some(ItemViewerAction::DeselectAll);
            }

            Popup::context_menu(&bg_response)
                .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    if !any_row_hovered {
                        if ui
                            .add_enabled(paste_enabled, egui::Button::new("Paste"))
                            .clicked()
                        {
                            action =
                                Some(ItemViewerAction::Context(ItemViewerContextAction::Paste));
                            ui.close();
                        }
                    }
                });
        }

        if let Some(_path) = explorer_state.non_ntfs_popup_path.clone() {
            let mut open = true;
            egui::Window::new(i18n.tr("non_nftsdrive"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .open(&mut open)
                .show(ui.ctx(), |ui| {
                    ui.label(
                        egui::RichText::new(i18n.tr("non_nftsdrive_fulllabel"))
                            .size(palette.text_size)
                            .color(palette.tooltip_text_color)
                            .font(font_id.clone()),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .button(format!("{} {}", regular::CHECK, i18n.tr("ok")))
                                .clicked()
                            {
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
