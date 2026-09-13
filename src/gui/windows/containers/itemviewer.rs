use crate::core::drives::is_raw_physical_drive_path;
use crate::core::fs::FileItem;
use crate::core::utils::text::apply_eden_text_overrides;
use crate::core::utils::widgets::draw_checkbox;
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::utils::{draw_object_drag_ghost, fuzzy_match};
use crate::gui::windows::containers::enums::{
    ItemViewerAction, ItemViewerContextAction, ItemViewerHeaderColumn, ItemViewerNavAction,
};
use crate::gui::windows::containers::itemviewer_columns::{
    draw_column_preview_view, draw_columns_view,
};
use crate::gui::windows::containers::itemviewer_gallery::draw_gallery_view;
use crate::gui::windows::containers::itemviewer_helper::*;
use crate::gui::windows::containers::itemviewer_preview::{draw_preview_pane, draw_preview_view};
use crate::gui::windows::containers::structs::{
    ItemViewerDisplayMode, ItemViewerFolderSizeState, ItemViewerLayout, ItemViewerNavBarAction,
    RenameState, TabView, TagsState,
};
use crate::gui::windows::shell_context_menu::ShellContextMenu;
use crate::gui::windows::structs::{SettingsWindow, ThemeCustomizer};
use eframe::egui;
use egui::containers::{Popup, PopupCloseBehavior};
use egui::{FontFamily, FontId};
use egui_extras::{Column, Size, StripBuilder, TableBuilder};
use egui_phosphor::regular;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use windows::Win32::Foundation::HWND;

pub fn draw_item_viewer(
    ui: &mut egui::Ui,
    i18n: &I18n,
    view: &mut TabView,
    folder_sizes: &HashMap<PathBuf, ItemViewerFolderSizeState>,
    paste_enabled: bool,
    clipboard_set: &HashSet<PathBuf>,
    is_cut_mode: bool,
    is_drive_view: bool,
    is_recycle_bin_view: bool,
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
    drag_active: bool,
    native_drag_active: bool,
    drag_hover_target: Option<PathBuf>,
    hovered_drop_target_out: &mut Option<PathBuf>,
    hovered_drop_target_rect_out: &mut Option<egui::Rect>,
    tags_state: &mut TagsState,
    theme_customizer_window: &mut ThemeCustomizer,
    settings_window: &mut SettingsWindow,
    hwnd: Option<HWND>,
    is_focused: bool,
    active_tab_id: u64,
    viewport_width: f32,
) -> Option<ItemViewerAction> {
    let display_mode = view.display_mode;
    let files = &view.files;
    let sort_column = view.sort_column;
    let sort_ascending = view.sort_ascending;
    let sort_keys = view.sort_keys.clone();
    let column_state = &mut view.column_state;
    let filter_state = &mut view.item_viewer_filter_state;
    let drag_state = &mut view.drag_state;
    let explorer_state = &mut view.explorer_state;
    let preview_service = &mut view.preview_service;
    let video_service = &mut view.video_service;
    let audio_service = &mut view.audio_service;
    let find_in_preview = &mut view.find_in_preview;
    let preview_selection = &mut view.preview_selection;
    let current_dir = view.nav.current.clone();
    let is_loading = view.is_loading;
    let is_search_view = crate::core::fs::parse_search_view_path(&current_dir).is_some();
    let font_id = FontId::new(palette.text_size, FontFamily::Proportional);
    let mut hovered_drop_target: Option<PathBuf> = None;
    let mut hovered_drop_target_rect: Option<egui::Rect> = None;
    draw_external_to_internal_drag_overlay(ui, i18n, *external_drag_to_internal_hover);

    let layout = compute_layout(
        ui,
        is_drive_view,
        is_recycle_bin_view,
        settings_window.current_settings.show_selection_checkboxes,
        palette,
    );
    let modal_input_blocked = tags_state.picker.is_some()
        || ui.ctx().memory(|mem| {
            mem.data
                .get_temp::<bool>(egui::Id::new(BLOCKING_MODAL_MEMORY_ID))
                .unwrap_or(false)
        });

    let mut action: Option<ItemViewerAction> = None;

    // Applies to Details/Detail Preview and (via their own call sites)
    // Gallery/Preview - not Columns/Column Preview, which only rename via
    // the right-click menu.
    let now = ui.input(|i| i.time);
    poll_click_to_rename(files, explorer_state, rename_state, now);
    if let Some((_, armed_at)) = explorer_state.click_to_rename_arm {
        let remaining = CLICK_TO_RENAME_DELAY - (now - armed_at);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(remaining.max(0.0)));
    }

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

    let is_custom_layout = matches!(
        display_mode,
        ItemViewerDisplayMode::Gallery
            | ItemViewerDisplayMode::Columns
            | ItemViewerDisplayMode::ColumnPreview
            | ItemViewerDisplayMode::Preview
            | ItemViewerDisplayMode::DetailPreview
    );

    let is_detail_preview = display_mode == ItemViewerDisplayMode::DetailPreview
        && !is_drive_view
        && !is_recycle_bin_view;
    if is_detail_preview {
        preview_service.pump(ui.ctx());
    }

    // Drawn *before* the "empty" message below: `handle_global_actions`
    // is what actually paints the type-to-filter box (when
    // `filter_state.active`), and `ui.centered_and_justified` for the
    // empty-state message claims the *entire* remaining rect for its own
    // content, advancing this `ui`'s cursor all the way to the bottom of
    // it - so if the filter box were drawn afterward using the same `ui`,
    // it would render wherever that now-bottomed-out cursor landed instead
    // of at the top, which is exactly what happened when a filter query
    // matched nothing (an otherwise-ordinary, very reproducible case).
    // Doing the reverse - filter box first, claiming the top, then
    // centering the "no results" message in whatever's left below it -
    // is the correct order regardless of which one calls
    // `centered_and_justified`.
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
            is_drive_view,
            is_recycle_bin_view,
            settings_window,
        ) {
            action = Some(global_action);
        }
    }

    if visible_items_empty && !is_custom_layout {
        let empty_rect = ui.available_rect_before_wrap();
        ui.centered_and_justified(|ui| {
            if is_loading && files.is_empty() {
                ui.add(egui::Spinner::new().size(28.0));
            } else {
                ui.label(i18n.tr("folder_is_empty"));
            }
        });

        // The populated-folder double-click-to-go-up handler below lives
        // inside the `!visible_items_empty` table-drawing branch (it's
        // wired through `table_background_response`, which only makes
        // sense once there's a table), so an empty folder never reached
        // it at all - double-clicking anywhere in the "This folder is
        // empty" area silently did nothing. This mirrors just the
        // navigate-up behavior for that case, on the same settings gate.
        if !modal_input_blocked
            && !is_recycle_bin_view
            && settings_window.current_settings.double_click_navigates_up
        {
            let empty_resp =
                ui.interact(empty_rect, ui.id().with("empty_folder_bg"), egui::Sense::click());
            if empty_resp.double_clicked() {
                tabbar_action.get_or_insert_with(Default::default).nav =
                    Some(ItemViewerNavAction::Up);
            }
        }
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
            &i18n.tr("drag_items")
        };

        draw_object_drag_ghost(ui, palette, label, false);
    }

    if display_mode == ItemViewerDisplayMode::Columns && !is_drive_view && !is_recycle_bin_view {
        return draw_columns_view(
            ui,
            i18n,
            &mut view.columns_view_state,
            &current_dir,
            show_hidden_files_folders,
            icon_cache,
            palette,
            explorer_state,
            tags_state,
            settings_window,
            hwnd,
            paste_enabled,
            clipboard_set,
            is_cut_mode,
            rename_state,
        )
        .or(action);
    }

    if display_mode == ItemViewerDisplayMode::ColumnPreview
        && !is_drive_view
        && !is_recycle_bin_view
    {
        return draw_column_preview_view(
            ui,
            i18n,
            &mut view.columns_view_state,
            &current_dir,
            show_hidden_files_folders,
            preview_service,
            video_service,
            audio_service,
            find_in_preview,
            icon_cache,
            palette,
            explorer_state,
            tags_state,
            settings_window,
            hwnd,
            paste_enabled,
            clipboard_set,
            is_cut_mode,
            rename_state,
        )
        .or(action);
    }

    if display_mode == ItemViewerDisplayMode::Preview && !is_drive_view && !is_recycle_bin_view {
        return draw_preview_view(
            ui,
            i18n,
            files,
            &filter_state.cached_indices,
            explorer_state,
            preview_service,
            video_service,
            audio_service,
            find_in_preview,
            preview_selection,
            icon_cache,
            palette,
            rename_state,
        )
        .or(action);
    }

    if display_mode == ItemViewerDisplayMode::Gallery && !is_drive_view && !is_recycle_bin_view {
        return draw_gallery_view(
            ui,
            i18n,
            files,
            &filter_state.cached_indices,
            sort_column,
            sort_ascending,
            explorer_state,
            rename_state,
            drag_state,
            &mut view.thumbnail_service,
            &mut view.gallery_state,
            paste_enabled,
            clipboard_set,
            is_cut_mode,
            icon_cache,
            palette,
            external_drag_to_internal_hover,
            drag_active,
            native_drag_active,
            drag_hover_target,
            hovered_drop_target_out,
            hovered_drop_target_rect_out,
            tags_state,
            theme_customizer_window,
            settings_window,
            hwnd,
            modal_input_blocked,
            is_focused,
            active_tab_id,
            current_dir,
        )
        .or(action);
    }

    // Everything from here down draws the Details table itself. It's wrapped
    // in a closure so `ItemViewerDisplayMode::DetailPreview` can run it inside
    // the left half of a split (table + preview pane) instead of directly on
    // the full-width `ui`, while the plain `Details` mode still just calls it
    // once on the whole area below.
    let draw_details_table = |ui: &mut egui::Ui,
                              preview_service: &mut crate::core::preview::PreviewService,
                              preview_selection: &mut Option<PathBuf>|
     -> Option<ItemViewerAction> {
        if visible_items_empty && is_detail_preview {
            let empty_rect = ui.available_rect_before_wrap();
            ui.centered_and_justified(|ui| {
                if is_loading && files.is_empty() {
                    ui.add(egui::Spinner::new().size(28.0));
                } else {
                    ui.label(i18n.tr("folder_is_empty"));
                }
            });

            // See the matching comment on the plain-Details empty case
            // above - the populated-folder handler lives inside the
            // `!visible_items_empty` branch below and never runs here.
            if !modal_input_blocked
                && !is_recycle_bin_view
                && settings_window.current_settings.double_click_navigates_up
            {
                let empty_resp = ui.interact(
                    empty_rect,
                    ui.id().with("empty_folder_bg_detail_preview"),
                    egui::Sense::click(),
                );
                if empty_resp.double_clicked() {
                    tabbar_action.get_or_insert_with(Default::default).nav =
                        Some(ItemViewerNavAction::Up);
                }
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

            let column_layout = compute_item_viewer_column_layout(
                ui,
                i18n,
                column_state,
                filter_state,
                files,
                folder_sizes,
                is_drive_view,
                is_recycle_bin_view,
                is_search_view,
                show_item_viewer_icons,
                palette,
                &font_id,
                file_type_cache,
                file_size_text_cache,
                folder_size_text_cache,
                drive_size_text_cache,
                viewport_width,
            );

            if column_layout.column_sizes_changed {
                action = Some(ItemViewerAction::ColumnSizesChanged);
            }

            // Bottom padding so the last row isn't flush against the
            // window/pane border - reserved as real, unused rect space
            // (rather than an extra table row) so it doesn't depend on
            // TableBuilder's own height/scroll bookkeeping.
            const BOTTOM_PADDING: f32 = 12.0;
            let available_height = (ui.available_height() - BOTTOM_PADDING).max(0.0);
            // IMPORTANT: register background interaction before TableBuilder.
            let bg_response = table_background_response(ui);
            let ctx = ui.ctx().clone();
            let table_rect = ui.available_rect_before_wrap();
            let left_margin = 8.0;
            let table_rect = egui::Rect::from_min_max(
                egui::pos2(table_rect.left() + left_margin, table_rect.top()),
                table_rect.right_bottom() - egui::vec2(0.0, BOTTOM_PADDING),
            );

            ui.scope_builder(egui::UiBuilder::new().max_rect(table_rect), |ui| {
                ui.visuals_mut().widgets.noninteractive.bg_stroke =
                    egui::Stroke::new(1.5, palette.borders_default);
                let table_id_salt = egui::Id::new((
                    "item_viewer_table",
                    active_tab_id,
                    &current_dir,
                    is_drive_view,
                    is_recycle_bin_view,
                    column_state.layout_generation,
                ));
                let table_state_id = ui.id().with(table_id_salt);
                let mut table = TableBuilder::new(ui)
                    .vscroll(true)
                    .max_scroll_height(available_height)
                    .min_scrolled_height(0.0)
                    .auto_shrink([false, false])
                    .striped(false)
                    // .sense(egui::Sense::hover())
                    .sense(if layout.is_drive_view {
                        egui::Sense::click()
                    } else {
                        egui::Sense::click_and_drag()
                    })
                    .animate_scrolling(true)
                    .resizable(true)
                    .id_salt(table_id_salt);

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

                if layout.show_checkboxes {
                    table = table.column(Column::exact(16.0));
                }

                for &column in &column_layout.ordered_columns {
                    let column = match column {
                        ItemViewerHeaderColumn::Name => Column::initial(column_layout.name_width)
                            .at_least(180.0)
                            .resizable(true),

                        ItemViewerHeaderColumn::OriginalDirectory => {
                            Column::initial(column_layout.original_directory_width)
                                .at_least(200.0)
                                .resizable(true)
                        }

                        ItemViewerHeaderColumn::Type => Column::initial(column_layout.type_width)
                            .at_least(60.0)
                            .resizable(true),

                        ItemViewerHeaderColumn::Size => Column::initial(column_layout.size_width)
                            .at_least(if is_drive_view { 120.0 } else { 75.0 })
                            .resizable(true),

                        ItemViewerHeaderColumn::Modified => {
                            Column::initial(column_layout.modified_width)
                                .at_least(100.0)
                                .resizable(true)
                        }

                        ItemViewerHeaderColumn::Created => {
                            Column::initial(column_layout.created_width)
                                .at_least(100.0)
                                .resizable(true)
                        }

                        ItemViewerHeaderColumn::Deleted => {
                            Column::initial(column_layout.deleted_width)
                                .at_least(100.0)
                                .resizable(true)
                        }

                        ItemViewerHeaderColumn::Usage => Column::initial(column_layout.usage_width)
                            .at_least(150.0)
                            .resizable(true),

                        ItemViewerHeaderColumn::Tags => Column::initial(column_layout.tags_width)
                            .at_least(100.0)
                            .resizable(true),
                    };

                    table = table.column(column);
                }

                let mut actual_column_widths = Vec::new();

                table
                    .header(layout.header_height, |mut header| {
                        if let Some(a) = draw_item_viewer_header(
                            i18n,
                            &mut header,
                            layout.is_drive_view,
                            layout.is_recycle_bin_view,
                            layout.show_checkboxes,
                            &column_layout.ordered_columns,
                            &filter_state.cached_indices,
                            files,
                            &sort_keys,
                            &palette,
                            explorer_state,
                            &*column_state,
                        ) {
                            action = Some(a);
                        }
                    })
                    .body(|body| {
                        let table_body_rect = body.max_rect();
                        // Capture the actual widths after TableBuilder has applied any
                        // user-driven column resizing.
                        actual_column_widths.extend_from_slice(body.widths());
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

                            if layout.show_checkboxes {
                                row.col(|ui| {
                                    let mut checked = is_selected;

                                    if draw_checkbox(ui, palette, &mut checked, &file.path)
                                        .clicked()
                                    {
                                        if checked {
                                            action =
                                                Some(ItemViewerAction::Select(file.path.clone()));
                                        } else {
                                            action =
                                                Some(ItemViewerAction::Deselect(file.path.clone()));
                                        }
                                    }
                                });
                            }

                            for &column in &column_layout.ordered_columns {
                                row.col(|ui| {
                                    draw_item_viewer_row_column(
                                        ui,
                                        column,
                                        file,
                                        &layout,
                                        i18n,
                                        icon_cache,
                                        is_selected,
                                        is_cut,
                                        palette,
                                        &font_id,
                                        rename_state,
                                        show_item_viewer_icons,
                                        folder_sizes,
                                        file_type_cache,
                                        file_size_text_cache,
                                        folder_size_text_cache,
                                        drive_size_text_cache,
                                        tags_state,
                                        &mut action,
                                    );
                                });
                            }

                            let row_resp = row.response();

                            draw_tag_row_background(
                                row_resp.rect,
                                table_body_rect,
                                layout.row_height,
                                tag_color,
                                palette,
                                &ctx,
                                active_tab_id,
                                &file.path,
                            );

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
                                                best_hovered_row = Some((
                                                    row_top,
                                                    is_dir,
                                                    file.path.clone(),
                                                    row_rect,
                                                ));
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
                                    is_recycle_bin_view,
                                    is_drive_view,
                                    row_resp.double_clicked(),
                                    row_resp.ctx.input(|i| i.time),
                                ) {
                                    if is_detail_preview
                                        && !file.is_dir
                                        && !matches!(
                                            a,
                                            ItemViewerAction::Open(_)
                                                | ItemViewerAction::OpenWithDefault(_)
                                        )
                                    {
                                        preview_service.request(&file.path);
                                        *preview_selection = Some(file.path.clone());
                                    }
                                    action = Some(a);
                                }
                            }

                            if row_resp.middle_clicked()
                                && file.is_dir
                                && !is_non_ntfs_drive
                                && !is_recycle_bin_view
                                && settings_window.current_settings.middle_click_opens_new_tab
                            {
                                action = Some(ItemViewerAction::OpenInNewTab(file.path.clone()));
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
                                            is_recycle_bin_view,
                                            is_cut,
                                            &mut action,
                                            palette,
                                            explorer_state,
                                            tags_state,
                                            settings_window,
                                            hwnd,
                                            icon_cache,
                                            is_search_view,
                                        );
                                    });
                            }
                        });

                        // ------------------- END OF ROW HANDLING -------------------

                        if let Some((_, is_dir, path, rect)) = best_hovered_row.take() {
                            if is_dir {
                                current_hovered_drop_target = Some(path);
                                current_hovered_drop_target_rect = Some(rect);
                            }
                        }

                        *hovered_drop_target = current_hovered_drop_target.clone();
                        hovered_drop_target_rect = current_hovered_drop_target_rect;
                    });

                // ------------------- END OF TABLE VIEW -------------------
                let checkbox_column_count = if layout.show_checkboxes { 1 } else { 0 };

                // Search view has no persisted column-width storage of its
                // own (it reuses the fixed column set computed in
                // `visible_order` rather than a fourth
                // `Vec<ItemViewerHeaderColumn>`/sizes pair alongside file/
                // drive/recycle-bin's) - skip the write-back entirely there
                // rather than let every frame see a "not the stored width"
                // mismatch (since there's nothing to compare against) and
                // fire `ColumnSizesChanged` in a loop.
                if !is_search_view {
                    for (column_index, &column) in column_layout.ordered_columns.iter().enumerate()
                    {
                        let table_index = column_index + checkbox_column_count;

                        let Some(&actual_width) = actual_column_widths.get(table_index) else {
                            continue;
                        };

                        let stored_width =
                            column_state.column_width(is_drive_view, is_recycle_bin_view, column);

                        if stored_width.map_or(true, |stored| (stored - actual_width).abs() > 0.5) {
                            column_state.set_column_width(
                                is_drive_view,
                                is_recycle_bin_view,
                                column,
                                actual_width,
                            );

                            action = Some(ItemViewerAction::ColumnSizesChanged);
                        }
                    }
                }

                let divider_tolerance = ui.style().interaction.resize_grab_radius_side + 3.0;
                let divider_double_clicked_column = ui.ctx().input(|input| {
                    let Some(pointer_pos) = input.pointer.interact_pos() else {
                        return None;
                    };
                    if !input
                        .pointer
                        .button_double_clicked(egui::PointerButton::Primary)
                        || !table_rect.contains(pointer_pos)
                    {
                        return None;
                    }

                    let spacing_x = ui.spacing().item_spacing.x;
                    let mut divider_x = table_rect.left() - spacing_x * 0.5;
                    for (table_index, width) in actual_column_widths.iter().enumerate() {
                        divider_x += *width + spacing_x;
                        if (pointer_pos.x - divider_x).abs() <= divider_tolerance {
                            return table_index.checked_sub(checkbox_column_count).and_then(
                                |column_index| {
                                    column_layout.ordered_columns.get(column_index).copied()
                                },
                            );
                        }
                    }
                    None
                });

                if let Some(column) = divider_double_clicked_column {
                    action = Some(ItemViewerAction::FitColumn(column));
                }

                let divider_double_clicked_column =
                    (0..actual_column_widths.len()).find_map(|index| {
                        let resize_id = table_state_id.with("resize_column").with(index);
                        ui.ctx()
                            .read_response(resize_id)
                            .filter(|response| response.double_clicked())
                            .and_then(|_| {
                                index
                                    .checked_sub(checkbox_column_count)
                                    .and_then(|column_index| {
                                        column_layout.ordered_columns.get(column_index).copied()
                                    })
                            })
                    });

                if let Some(column) = divider_double_clicked_column {
                    action = Some(ItemViewerAction::FitColumn(column));
                }

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

                    // The drag ends here regardless of whether it landed on
                    // a valid target. Clearing `active` alone isn't enough:
                    // the per-row re-arm check above (`start.distance(current)
                    // > 4.0`) never verifies the mouse button is still down,
                    // so a leftover `start_pos` from this finished drag would
                    // flip `active` back to true the moment the pointer next
                    // drifted more than 4px from it - reappearing the ghost/
                    // highlight with no drag actually happening. Clear
                    // `start_pos` and `source_items` too, matching the
                    // Escape-cancel handler in itemviewer_helper.rs.
                    drag_state.active = false;
                    drag_state.start_pos = None;
                    drag_state.source_items.clear();
                }

                if !modal_input_blocked {
                    if bg_response.clicked() {
                        action = Some(ItemViewerAction::DeselectAll);
                    }

                    if bg_response.double_clicked()
                        && !is_recycle_bin_view
                        && settings_window.current_settings.double_click_navigates_up
                    {
                        tabbar_action.get_or_insert_with(Default::default).nav =
                            Some(ItemViewerNavAction::Up);
                    }

                    if !layout.is_drive_view && !is_recycle_bin_view {
                        Popup::context_menu(&bg_response)
                            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                apply_eden_text_overrides(ui, palette);
                                if ui.button("New Folder").clicked() {
                                    action = Some(ItemViewerAction::CreateFolder);
                                    ui.close();
                                }
                                if ui.button("New File").clicked() {
                                    action = Some(ItemViewerAction::CreateFile);
                                    ui.close();
                                }
                                if ui.button("Refresh").clicked() {
                                    action = Some(ItemViewerAction::RefreshCurrentDirectory);
                                    ui.close();
                                }
                                if ui.button("Open Terminal").clicked() {
                                    action = Some(ItemViewerAction::OpenTerminal);
                                    ui.close();
                                }

                                ui.separator();

                                ui.menu_button(i18n.tr("view_menu"), |ui| {
                                    apply_eden_text_overrides(ui, palette);
                                    for (mode, label_key) in [
                                        (ItemViewerDisplayMode::Details, "view_details"),
                                        (ItemViewerDisplayMode::Gallery, "view_gallery"),
                                        (ItemViewerDisplayMode::Columns, "view_columns"),
                                        (
                                            ItemViewerDisplayMode::ColumnPreview,
                                            "view_column_preview",
                                        ),
                                        (ItemViewerDisplayMode::Preview, "view_preview"),
                                        (
                                            ItemViewerDisplayMode::DetailPreview,
                                            "view_detail_preview",
                                        ),
                                    ] {
                                        // A plain `selectable_label` here ends up low-contrast
                                        // against this app's custom theme (the same issue fixed
                                        // in the preview pane's tab strip) - draw it explicitly
                                        // with a checkmark and a color that's always legible
                                        // instead of relying on egui's selection fill/text colors.
                                        let is_selected = view.display_mode == mode;
                                        let color = if is_selected {
                                            palette.primary
                                        } else {
                                            ui.visuals().text_color()
                                        };
                                        let check = if is_selected { regular::CHECK } else { "" };

                                        let resp = ui.add(
                                            egui::Button::new(
                                                egui::RichText::new(format!(
                                                    "{check:<2}{}",
                                                    i18n.tr(label_key)
                                                ))
                                                .color(color),
                                            )
                                            .fill(egui::Color32::TRANSPARENT)
                                            .stroke(egui::Stroke::NONE),
                                        );

                                        if resp.clicked() {
                                            view.display_mode = mode;
                                            ui.close();
                                        }
                                    }
                                });

                                // `draw_custom_context_menu_group` draws its own leading
                                // separator only when it actually has background-scoped
                                // entries to show, so View/Paste stay adjacent (no double
                                // separator) when there are none. The separator below is
                                // unconditional, closing off the group before Paste either way.
                                draw_custom_context_menu_group(
                                    ui,
                                    i18n,
                                    icon_cache,
                                    &settings_window.current_settings.custom_context_menu,
                                    false,
                                    false,
                                    true,
                                    &[],
                                    &mut action,
                                );
                                ui.separator();

                                if ui
                                    .add_enabled(paste_enabled, egui::Button::new("Paste"))
                                    .clicked()
                                {
                                    action = Some(ItemViewerAction::Context(
                                        ItemViewerContextAction::Paste,
                                    ));
                                    ui.close();
                                }
                                if ui.button("Properties").clicked() {
                                    action = Some(ItemViewerAction::Context(
                                        ItemViewerContextAction::Properties(vec![
                                            current_dir.clone(),
                                        ]),
                                    ));
                                    ui.close();
                                }

                                if settings_window
                                    .current_settings
                                    .windows_context_menu_enabled
                                {
                                    ui.separator();
                                    let bg_key = vec![current_dir.clone()];
                                    draw_windows_context_submenu(
                                        ui,
                                        i18n,
                                        palette,
                                        explorer_state,
                                        hwnd,
                                        bg_key,
                                        |hwnd| ShellContextMenu::for_background(&current_dir, hwnd),
                                    );
                                }
                                // }
                            });
                    } else if is_recycle_bin_view {
                        Popup::context_menu(&bg_response)
                            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                apply_eden_text_overrides(ui, palette);
                                if ui.button("Refresh").clicked() {
                                    action = Some(ItemViewerAction::RefreshCurrentDirectory);
                                    ui.close();
                                }
                            });
                    }
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
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .button(format!("{} {}", regular::CHECK, i18n.tr("ok")))
                                            .clicked()
                                        {
                                            explorer_state.non_ntfs_popup_path = None;
                                        }
                                    },
                                );
                            });
                        });
                    if !open {
                        explorer_state.non_ntfs_popup_path = None;
                    }
                }
            });

            action
        } else {
            return action;
        }
    };

    if is_detail_preview {
        let mut result = None;
        StripBuilder::new(ui)
            .size(Size::relative(0.5))
            .size(Size::relative(0.5))
            .horizontal(|mut strip| {
                strip.cell(|ui| {
                    result = draw_details_table(ui, preview_service, preview_selection);
                });
                strip.cell(|ui| {
                    let divider_rect = ui.max_rect();
                    ui.painter().vline(
                        divider_rect.left(),
                        divider_rect.y_range(),
                        egui::Stroke::new(1.5, palette.borders_default),
                    );
                    draw_preview_pane(
                        ui,
                        i18n,
                        preview_service,
                        video_service,
                        audio_service,
                        find_in_preview,
                        preview_selection.as_deref(),
                        palette,
                    );
                });
            });
        result
    } else {
        draw_details_table(ui, preview_service, preview_selection)
    }
}

fn draw_item_viewer_row_column(
    ui: &mut egui::Ui,
    column: ItemViewerHeaderColumn,
    file: &FileItem,
    layout: &ItemViewerLayout,
    i18n: &I18n,
    icon_cache: &IconCache,
    is_selected: bool,
    is_cut: bool,
    palette: &ThemePalette,
    font_id: &FontId,
    rename_state: &mut Option<RenameState>,
    show_item_viewer_icons: bool,
    folder_sizes: &HashMap<PathBuf, ItemViewerFolderSizeState>,
    file_type_cache: &mut HashMap<String, String>,
    file_size_text_cache: &mut HashMap<PathBuf, (u64, String)>,
    folder_size_text_cache: &mut HashMap<PathBuf, (u64, bool, String)>,
    drive_size_text_cache: &mut HashMap<PathBuf, (u64, u64, String)>,
    tags_state: &TagsState,
    action: &mut Option<ItemViewerAction>,
) {
    match column {
        ItemViewerHeaderColumn::Name => {
            if let Some(a) = handle_draw_col_name(
                ui,
                i18n,
                file,
                layout,
                icon_cache,
                is_selected,
                is_cut,
                palette,
                font_id,
                rename_state,
                show_item_viewer_icons,
            ) {
                *action = Some(a);
            }
        }

        ItemViewerHeaderColumn::OriginalDirectory => {
            handle_draw_col_original_directory(
                ui,
                file,
                layout,
                is_selected,
                is_cut,
                palette,
                font_id,
            );
        }

        ItemViewerHeaderColumn::Type => {
            handle_draw_col_type(
                ui,
                file,
                layout,
                is_selected,
                is_cut,
                palette,
                font_id,
                file_type_cache,
            );
        }

        ItemViewerHeaderColumn::Size => {
            handle_draw_col_size(
                ui,
                file,
                layout,
                folder_sizes,
                is_selected,
                is_cut,
                palette,
                font_id,
                file_size_text_cache,
                folder_size_text_cache,
                drive_size_text_cache,
            );
        }

        ItemViewerHeaderColumn::Modified => {
            handle_draw_col_modified(ui, file, layout, is_selected, is_cut, palette, font_id);
        }

        ItemViewerHeaderColumn::Created => {
            handle_draw_col_created(ui, file, layout, is_selected, is_cut, palette, font_id);
        }

        ItemViewerHeaderColumn::Deleted => {
            handle_draw_col_deleted(ui, file, layout, is_selected, is_cut, palette, font_id);
        }

        ItemViewerHeaderColumn::Usage => {
            handle_draw_col_modified(ui, file, layout, is_selected, is_cut, palette, font_id);
        }

        ItemViewerHeaderColumn::Tags => {
            handle_draw_col_tags(ui, file, layout, tags_state, font_id);
        }
    }
}

fn draw_tag_row_background(
    row_rect: egui::Rect,
    table_clip_rect: egui::Rect,
    row_height: f32,
    tag_color: Option<egui::Color32>,
    palette: &ThemePalette,
    ctx: &egui::Context,
    active_tab_id: u64,
    path: &std::path::Path,
) {
    let Some(tag_color) = tag_color else {
        return;
    };

    let tag_rect =
        egui::Rect::from_min_size(row_rect.min, egui::vec2(row_rect.width(), row_height))
            .shrink2(egui::vec2(0.0, 1.0))
            .intersect(table_clip_rect);

    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new(("tag_row_bg", active_tab_id, path)),
    ));

    painter.rect_filled(
        tag_rect,
        egui::CornerRadius::same(palette.medium_radius),
        tag_color.linear_multiply(0.18),
    );
}
