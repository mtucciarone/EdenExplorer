use crate::core::drives::is_raw_physical_drive_path;
use crate::core::fs::FileItem;
use crate::core::i18n::I18n;
use crate::core::icons::IconCache;
use crate::core::utils::files::{format_size, get_file_type_name};
use crate::core::utils::text::apply_eden_text_overrides;
use crate::core::utils::thumbnails::{ThumbnailPriority, ThumbnailService};
use crate::core::utils::widgets::{
    apply_eden_dropdown_visual_color_overrides, apply_eden_visual_overrides, draw_dropdown,
};
use crate::gui::icon_system::{AppIcon, paint_icon};
use crate::gui::theme::ThemePalette;
use crate::gui::utils::{SortColumn, truncate_item_text};
use crate::gui::windows::containers::enums::{ItemViewerAction, ItemViewerContextAction};
use crate::gui::windows::containers::itemviewer_helper::{
    draw_empty_item_viewer_area, draw_view_submenu, handle_context_menu_actions,
    handle_editing_file_name, handle_keyboard_navigation, handle_row_click,
};
use crate::gui::windows::containers::structs::{
    DragState, ExplorerState, GalleryState, GalleryThumbnailSize, ItemViewerDisplayMode,
    RenameState, ViewPreset,
};
use crate::gui::windows::structs::{SettingsWindow, ThemeCustomizer};
use eframe::egui;
use egui::containers::{Popup, PopupCloseBehavior};
use egui::{FontFamily, FontId};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use windows::Win32::Foundation::HWND;

const LABEL_HEIGHT: f32 = 34.0;
const TOOLBAR_HEIGHT: f32 = 24.0;
const GALLERY_TOOLBAR_GAP: f32 = 6.0;
/// Room left under a sideways-scrolling List for its scrollbar.
const HORIZONTAL_SCROLLBAR_ROOM: f32 = 14.0;

/// Where every tile goes for one of the icon-style display modes (everything
/// except Details). Row-major modes fill rows and scroll vertically; List is
/// column-major and scrolls sideways, like Windows Explorer.
#[derive(Clone, Copy)]
struct TileLayout {
    mode: ItemViewerDisplayMode,
    tile: egui::Vec2,
    col_pitch: f32,
    row_pitch: f32,
    columns: usize,
    rows: usize,
    column_major: bool,
    /// Edge length of the icon / thumbnail box.
    icon: f32,
    line_height: f32,
    /// Gallery only: padding around the thumbnail.
    padding: f32,
}

impl TileLayout {
    fn new(
        mode: ItemViewerDisplayMode,
        gallery_state: &GalleryState,
        palette: &ThemePalette,
        viewport: egui::Vec2,
        count: usize,
    ) -> Self {
        let line_height = (palette.text_size * 1.4).ceil();
        let compact_row = (line_height + 6.0).max(22.0);

        // (tile size, column gap, row gap, icon edge, column-major)
        let (tile, gap_x, gap_y, icon, column_major) = match mode {
            ItemViewerDisplayMode::SmallIcons => {
                (egui::vec2(240.0, compact_row), 8.0, 2.0, 16.0, false)
            }
            ItemViewerDisplayMode::List => (egui::vec2(280.0, compact_row), 8.0, 2.0, 16.0, true),
            ItemViewerDisplayMode::Tiles => (
                egui::vec2(300.0, (line_height * 3.0 + 12.0).max(64.0)),
                8.0,
                8.0,
                48.0,
                false,
            ),
            ItemViewerDisplayMode::Content => (
                egui::vec2(viewport.x.max(200.0), (line_height * 2.0 + 20.0).max(64.0)),
                0.0,
                0.0,
                48.0,
                false,
            ),
            // Gallery (Details never reaches this view).
            _ => {
                let thumb = gallery_state.thumbnail_size.pixel_size();
                let pad = gallery_state.thumbnail_padding;
                let gap = gallery_state.thumbnail_gap;
                (
                    egui::vec2(thumb + pad * 2.0 + 26.0, thumb + pad * 2.0 + LABEL_HEIGHT),
                    gap,
                    gap,
                    thumb,
                    false,
                )
            }
        };

        let col_pitch = tile.x + gap_x;
        let row_pitch = tile.y + gap_y;
        let (columns, rows) = if column_major {
            let usable_height = (viewport.y - HORIZONTAL_SCROLLBAR_ROOM).max(row_pitch);
            let rows = ((usable_height + gap_y) / row_pitch).floor().max(1.0) as usize;
            (count.div_ceil(rows).max(1), rows)
        } else {
            let columns = ((viewport.x.max(tile.x) + gap_x) / col_pitch)
                .floor()
                .max(1.0) as usize;
            (columns, count.div_ceil(columns))
        };

        Self {
            mode,
            tile,
            col_pitch,
            row_pitch,
            columns,
            rows,
            column_major,
            icon,
            line_height,
            padding: gallery_state.thumbnail_padding,
        }
    }

    fn index(&self, row: usize, col: usize) -> usize {
        if self.column_major {
            col * self.rows + row
        } else {
            row * self.columns + col
        }
    }

    /// Size the scroll content has to cover (at least the visible area).
    fn content_size(&self, viewport: egui::Vec2) -> egui::Vec2 {
        if self.column_major {
            egui::vec2(
                (self.columns as f32 * self.col_pitch).max(viewport.x),
                (viewport.y - HORIZONTAL_SCROLLBAR_ROOM).max(self.row_pitch),
            )
        } else {
            egui::vec2(
                viewport.x.max(self.tile.x),
                (self.rows as f32 * self.row_pitch).max(viewport.y),
            )
        }
    }

    /// Big enough boxes to be worth loading image thumbnails for.
    fn shows_thumbnails(&self) -> bool {
        matches!(
            self.mode,
            ItemViewerDisplayMode::Gallery
                | ItemViewerDisplayMode::Tiles
                | ItemViewerDisplayMode::Content
        )
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw_gallery_view(
    ui: &mut egui::Ui,
    i18n: &I18n,
    display_mode: ItemViewerDisplayMode,
    current_view: ViewPreset,
    file_type_cache: &mut HashMap<String, String>,
    files: &[FileItem],
    filtered_indices: &[usize],
    sort_column: SortColumn,
    sort_ascending: bool,
    explorer_state: &mut ExplorerState,
    rename_state: &mut Option<RenameState>,
    drag_state: &mut DragState,
    thumbnail_service: &mut ThumbnailService,
    gallery_state: &mut GalleryState,
    paste_enabled: bool,
    clipboard_set: &HashSet<PathBuf>,
    is_cut_mode: bool,
    icon_cache: &IconCache,
    palette: &ThemePalette,
    external_drag_to_internal_hover: &mut bool,
    drag_active: bool,
    native_drag_active: bool,
    drag_hover_target: Option<PathBuf>,
    hovered_drop_target_out: &mut Option<PathBuf>,
    hovered_drop_target_rect_out: &mut Option<egui::Rect>,
    _theme_customizer_window: &mut ThemeCustomizer,
    settings_window: &mut SettingsWindow,
    hwnd: Option<HWND>,
    modal_input_blocked: bool,
    is_focused: bool,
    active_tab_id: u64,
    current_dir: PathBuf,
) -> Option<ItemViewerAction> {
    thumbnail_service.pump_completed(ui.ctx());

    let mut action = draw_gallery_toolbar(
        ui,
        i18n,
        palette,
        gallery_state,
        display_mode,
        current_view,
        sort_column,
        sort_ascending,
    );

    if filtered_indices.is_empty() {
        return draw_empty_item_viewer_area(
            ui,
            i18n,
            palette,
            paste_enabled,
            false,
            false,
            &current_dir,
            drag_state,
            modal_input_blocked,
            false,
            current_view,
        )
        .or(action);
    }

    let modifiers = ui.ctx().input(|i| i.modifiers);
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
    let font_id = FontId::new(palette.text_size, FontFamily::Proportional);
    let mut best_hovered_tile: Option<(f32, bool, PathBuf, egui::Rect)> = None;
    let mut current_hovered_drop_target: Option<PathBuf> = None;
    let mut current_hovered_drop_target_rect: Option<egui::Rect> = None;
    let gallery_rect = ui.available_rect_before_wrap();
    let scroll_rect = egui::Rect::from_min_max(
        egui::pos2(
            gallery_rect.left(),
            gallery_rect.top() + GALLERY_TOOLBAR_GAP,
        ),
        gallery_rect.right_bottom(),
    );

    let layout = TileLayout::new(
        display_mode,
        gallery_state,
        palette,
        scroll_rect.size(),
        filtered_indices.len(),
    );
    let (rows, columns) = (layout.rows, layout.columns);
    let (row_pitch, col_pitch) = (layout.row_pitch, layout.col_pitch);
    let content_size = layout.content_size(scroll_rect.size());

    // List scrolls sideways; let the normal wheel drive it, like Explorer.
    if layout.column_major && ui.rect_contains_pointer(scroll_rect) {
        ui.ctx().input_mut(|i| {
            if i.smooth_scroll_delta.x == 0.0 {
                i.smooth_scroll_delta.x = i.smooth_scroll_delta.y;
                i.smooth_scroll_delta.y = 0.0;
            }
        });
    }

    ui.scope_builder(egui::UiBuilder::new().max_rect(scroll_rect), |ui| {
        let scroll_area = if layout.column_major {
            egui::ScrollArea::horizontal()
        } else {
            egui::ScrollArea::vertical()
        };
        scroll_area
            .id_salt((
                "item_viewer_gallery_scroll",
                active_tab_id,
                layout.column_major,
            ))
            .auto_shrink([false, true])
            .show_viewport(ui, |ui, viewport| {
                if layout.column_major {
                    ui.set_min_width(content_size.x);
                    ui.set_height(content_size.y);
                } else {
                    ui.set_width(content_size.x);
                    ui.set_min_height(content_size.y);
                }

                let visible_start_row = (viewport.top() / row_pitch).floor().max(0.0) as usize;
                let visible_end_row = (viewport.bottom() / row_pitch).ceil().max(0.0) as usize;
                let visible_start_col = (viewport.left() / col_pitch).floor().max(0.0) as usize;
                let visible_end_col =
                    ((viewport.right() / col_pitch).ceil().max(0.0) as usize + 1).min(columns);

                if layout.shows_thumbnails() {
                    let nearby_start_row = visible_start_row.saturating_sub(2);
                    let nearby_end_row = (visible_end_row + 3).min(rows);
                    for row in nearby_start_row..nearby_end_row {
                        for col in 0..columns {
                            let Some(&file_index) = filtered_indices.get(layout.index(row, col))
                            else {
                                continue;
                            };
                            let priority = if row >= visible_start_row && row <= visible_end_row {
                                ThumbnailPriority::Visible
                            } else {
                                ThumbnailPriority::Nearby
                            };
                            thumbnail_service.request(&files[file_index], priority);
                        }
                    }
                }

                let content_origin = ui.min_rect().min;

                let bg_rect = egui::Rect::from_min_size(content_origin, content_size);
                let bg_response = ui.interact(
                    bg_rect,
                    ui.id().with(("gallery_background", active_tab_id)),
                    egui::Sense::click(),
                );

                for row in visible_start_row.saturating_sub(1)..(visible_end_row + 2).min(rows) {
                    for col in visible_start_col.saturating_sub(1)..visible_end_col {
                        let item_index = layout.index(row, col);
                        let Some(&file_index) = filtered_indices.get(item_index) else {
                            continue;
                        };

                        let file = &files[file_index];
                        let rect = egui::Rect::from_min_size(
                            content_origin
                                + egui::vec2(col as f32 * col_pitch, row as f32 * row_pitch),
                            layout.tile,
                        );
                        let (response, tile_action) = draw_gallery_tile(
                            i18n,
                            ui,
                            file,
                            item_index,
                            rect,
                            &layout,
                            file_type_cache,
                            icon_cache,
                            thumbnail_service,
                            explorer_state,
                            rename_state,
                            clipboard_set,
                            is_cut_mode,
                            palette,
                            &font_id,
                        );

                        if let Some(a) = tile_action {
                            action = Some(a);
                        }

                        if drag_hover_active {
                            if let Some(target) = hovered_target_ref {
                                if &file.path == target && file.is_dir {
                                    current_hovered_drop_target = Some(file.path.clone());
                                    current_hovered_drop_target_rect = Some(response.rect);
                                }
                            } else if let Some(pointer) = pointer_pos {
                                if response.rect.contains(pointer) {
                                    match &best_hovered_tile {
                                        Some((best_top, _, _, _))
                                            if *best_top >= response.rect.top() => {}
                                        _ => {
                                            best_hovered_tile = Some((
                                                response.rect.top(),
                                                file.is_dir,
                                                file.path.clone(),
                                                response.rect,
                                            ));
                                        }
                                    }
                                }
                            }
                        }

                        if response.drag_started() && !is_raw_physical_drive_path(&file.path) {
                            drag_state.start_pos = response.interact_pointer_pos();
                            drag_state.active = false;
                            drag_state.source_items.clear();

                            if explorer_state.selected_paths.contains(&file.path) {
                                drag_state.source_items =
                                    explorer_state.selected_paths.iter().cloned().collect();
                            } else {
                                explorer_state.selected_paths.clear();
                                explorer_state.selected_paths.insert(file.path.clone());
                                explorer_state.selection_anchor = Some(item_index);
                                explorer_state.selection_focus = Some(item_index);
                                drag_state.source_items = vec![file.path.clone()];
                            }
                        }

                        if !drag_state.active && !response.ctx.input(|i| i.pointer.primary_down()) {
                            drag_state.start_pos = None;
                            drag_state.source_items.clear();
                        }

                        if let (Some(start), Some(current)) = (
                            drag_state.start_pos,
                            response.ctx.input(|i| i.pointer.hover_pos()),
                        ) {
                            if !drag_state.active
                                && !drag_state.source_items.is_empty()
                                && start.distance(current) > 4.0
                            {
                                drag_state.active = true;
                            }
                        }

                        if response.clicked() && !drag_state.active && !modal_input_blocked {
                            if let Some(a) = handle_row_click(
                                item_index,
                                file,
                                modifiers,
                                filtered_indices,
                                files,
                                drag_state,
                                explorer_state,
                                false,
                            ) {
                                action = Some(a);
                            }
                        }

                        if response.middle_clicked() && file.is_dir {
                            action = Some(ItemViewerAction::OpenInBackgroundTab(file.path.clone()));
                        }

                        Popup::context_menu(&response)
                            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                egui::ScrollArea::vertical()
                                    .max_height(ui.ctx().viewport_rect().height() * 0.75)
                                    .show(ui, |ui| {
                                        handle_context_menu_actions(
                                            ui,
                                            i18n,
                                            file,
                                            explorer_state.selected_paths.contains(&file.path),
                                            paste_enabled,
                                            false,
                                            false,
                                            is_cut_mode && clipboard_set.contains(&file.path),
                                            &mut action,
                                            palette,
                                            explorer_state,
                                            settings_window,
                                            hwnd,
                                        );
                                    });
                            });
                    }
                }

                if let Some((_, is_dir, path, rect)) = best_hovered_tile.take()
                    && is_dir
                {
                    current_hovered_drop_target = Some(path);
                    current_hovered_drop_target_rect = Some(rect);
                }

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
                }

                if !modal_input_blocked {
                    if bg_response.clicked() {
                        action = Some(ItemViewerAction::DeselectAll);
                    }

                    Popup::context_menu(&bg_response)
                        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            apply_eden_text_overrides(ui, palette);
                            draw_view_submenu(ui, palette, current_view, &mut action);
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

                            if ui
                                .add_enabled(paste_enabled, egui::Button::new("Paste"))
                                .clicked()
                            {
                                action =
                                    Some(ItemViewerAction::Context(ItemViewerContextAction::Paste));
                                ui.close();
                            }
                            if ui.button("Properties").clicked() {
                                action = Some(ItemViewerAction::Context(
                                    ItemViewerContextAction::Properties(vec![current_dir.clone()]),
                                ));
                                ui.close();
                            }
                        });
                }
            });
    });
    if let Some(rect) = current_hovered_drop_target_rect {
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new(("gallery_drop_highlight", active_tab_id)),
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

    *hovered_drop_target_out = current_hovered_drop_target;
    *hovered_drop_target_rect_out = current_hovered_drop_target_rect;
    *external_drag_to_internal_hover = native_drag_active || external_file_hover;

    if !modal_input_blocked
        && is_focused
        && let Some(a) = handle_keyboard_navigation(
            ui.ctx(),
            filtered_indices,
            &files.to_vec(),
            false,
            explorer_state,
        )
    {
        action = Some(a);
    }

    action
}

#[allow(clippy::too_many_arguments)]
fn draw_gallery_toolbar(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    gallery_state: &mut GalleryState,
    display_mode: ItemViewerDisplayMode,
    current_view: ViewPreset,
    sort_column: SortColumn,
    sort_ascending: bool,
) -> Option<ItemViewerAction> {
    let mut action = None;
    let rect = ui
        .allocate_exact_size(
            egui::vec2(ui.available_width(), TOOLBAR_HEIGHT),
            egui::Sense::hover(),
        )
        .0;

    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Min)),
        |ui| {
            ui.add_space(2.25);
            apply_eden_visual_overrides(ui, palette);
            apply_eden_dropdown_visual_color_overrides(ui, palette);
            apply_eden_text_overrides(ui, palette);
            ui.style_mut().visuals.widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
            ui.style_mut().visuals.widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
            ui.style_mut().visuals.widgets.open.bg_fill = egui::Color32::TRANSPARENT;
            ui.style_mut().visuals.widgets.open.weak_bg_fill = egui::Color32::TRANSPARENT;

            const GALLERY_SORT_COMBO_WIDTH: f32 = 120.0;
            const VIEW_COMBO_WIDTH: f32 = 150.0;

            let view_label = ViewPreset::MENU
                .iter()
                .find(|(preset, _)| *preset == current_view)
                .map(|(_, label)| *label)
                .unwrap_or_else(|| match current_view {
                    ViewPreset::Icons(size) => size.label(),
                    _ => "",
                });

            draw_dropdown(
                ui,
                palette,
                "gallery_view_selector",
                VIEW_COMBO_WIDTH,
                format!("View: {view_label}"),
                |ui| {
                    for (preset, label) in ViewPreset::MENU {
                        if ui.radio(current_view == preset, label).clicked() {
                            action = Some(ItemViewerAction::SetViewPreset(preset));
                            ui.close();
                        }
                    }
                },
            );

            draw_dropdown(
                ui,
                palette,
                "gallery_sort_selector",
                GALLERY_SORT_COMBO_WIDTH,
                format!(
                    "{}: {} {}",
                    i18n.tr("gallery_sort_by"),
                    sort_label(i18n, sort_column),
                    if sort_ascending { "↑" } else { "↓" }
                ),
                |ui| {
                    for (column, label) in [
                        (SortColumn::Name, i18n.tr("explorer_cols_name")),
                        (SortColumn::Type, i18n.tr("explorer_cols_type")),
                        (SortColumn::Size, i18n.tr("explorer_cols_size")),
                        (SortColumn::Modified, i18n.tr("explorer_cols_modified")),
                        (SortColumn::Created, i18n.tr("explorer_cols_created")),
                    ] {
                        if ui.selectable_label(sort_column == column, label).clicked() {
                            let modifiers = ui.input(|input| input.modifiers);
                            action = Some(ItemViewerAction::Sort {
                                column,
                                additive: modifiers.shift,
                                remove: modifiers.ctrl,
                            });
                            ui.close();
                        }
                    }
                },
            );

            // Fine-grained thumbnail sizes only make sense for the icon grid.
            if display_mode == ItemViewerDisplayMode::Gallery {
                for size in [
                    GalleryThumbnailSize::ExtraSmall,
                    GalleryThumbnailSize::Small,
                    GalleryThumbnailSize::Medium,
                    GalleryThumbnailSize::Large,
                    GalleryThumbnailSize::ExtraLarge,
                ] {
                    if ui
                        .selectable_label(
                            gallery_state.thumbnail_size == size,
                            egui::RichText::new(size.label()).color(palette.text_header_section),
                        )
                        .clicked()
                    {
                        gallery_state.set_thumbnail_size(size);
                    }
                }
            }
        },
    );

    action
}

#[allow(clippy::too_many_arguments)]
fn draw_gallery_tile(
    i18n: &I18n,
    ui: &mut egui::Ui,
    file: &FileItem,
    item_index: usize,
    rect: egui::Rect,
    layout: &TileLayout,
    file_type_cache: &mut HashMap<String, String>,
    icon_cache: &IconCache,
    thumbnail_service: &mut ThumbnailService,
    explorer_state: &ExplorerState,
    rename_state: &mut Option<RenameState>,
    clipboard_set: &HashSet<PathBuf>,
    is_cut_mode: bool,
    palette: &ThemePalette,
    font_id: &FontId,
) -> (egui::Response, Option<ItemViewerAction>) {
    let response = ui.interact(
        rect,
        ui.id().with(("gallery_tile", item_index, &file.path)),
        egui::Sense::click_and_drag(),
    );

    let is_selected = explorer_state.selected_paths.contains(&file.path);
    let is_cut = is_cut_mode && clipboard_set.contains(&file.path);

    let bg = if is_selected {
        palette.primary_active
    } else if response.hovered() {
        palette.primary_hover.linear_multiply(0.35)
    } else {
        egui::Color32::TRANSPARENT
    };

    // Respect the parent/ScrollArea clip rect while also preventing anything
    // rendered by this tile from escaping its own bounds.
    let tile_painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));

    if bg != egui::Color32::TRANSPARENT {
        tile_painter.rect_filled(rect, egui::CornerRadius::same(palette.medium_radius), bg);
    }

    let text_color = if is_selected {
        palette.item_viewer_row_text_selected
    } else {
        palette.text_normal
    };
    let secondary_color = text_color.gamma_multiply(0.65);

    let name_rect = if layout.mode == ItemViewerDisplayMode::Gallery {
        let thumb_size = layout.icon;
        let preview_rect = egui::Rect::from_center_size(
            egui::pos2(
                rect.center().x,
                rect.top() + layout.padding + thumb_size * 0.5,
            ),
            egui::vec2(thumb_size, thumb_size),
        );

        tile_painter.rect_filled(
            preview_rect,
            egui::CornerRadius::same(palette.medium_radius),
            palette.application_bg_color.linear_multiply(0.65),
        );

        tile_painter.rect_stroke(
            preview_rect,
            egui::CornerRadius::same(palette.medium_radius),
            egui::Stroke::new(1.0, palette.borders_default.linear_multiply(0.8)),
            egui::StrokeKind::Inside,
        );

        paint_file_visual(
            ui,
            &tile_painter,
            file,
            preview_rect,
            (thumb_size * 0.35).clamp(26.0, 80.0),
            true,
            is_cut,
            icon_cache,
            thumbnail_service,
            palette,
        );

        egui::Rect::from_min_max(
            egui::pos2(rect.left() + layout.padding, preview_rect.bottom() + 8.0),
            egui::pos2(rect.right() - layout.padding, rect.bottom() - 4.0),
        )
    } else {
        // Icon on the left, text lines to its right.
        let left_pad = if layout.mode == ItemViewerDisplayMode::Content {
            12.0
        } else {
            6.0
        };
        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + left_pad, rect.center().y - layout.icon * 0.5),
            egui::vec2(layout.icon, layout.icon),
        );
        paint_file_visual(
            ui,
            &tile_painter,
            file,
            icon_rect,
            layout.icon,
            layout.shows_thumbnails(),
            is_cut,
            icon_cache,
            thumbnail_service,
            palette,
        );

        let text_left = icon_rect.right() + if layout.icon > 20.0 { 10.0 } else { 6.0 };
        let text_right = rect.right() - 8.0;
        let line_h = layout.line_height;

        let mut type_text = || -> String {
            if file.is_dir {
                "Folder".to_string()
            } else {
                let ext = file
                    .path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("");
                get_file_type_name(ext, file_type_cache).to_string()
            }
        };
        let size_text = || -> Option<String> {
            (!file.is_dir)
                .then_some(file.file_size)
                .flatten()
                .map(format_size)
        };

        // Content: date modified and size in a right-hand column when there is room.
        let mut name_right = text_right;
        if layout.mode == ItemViewerDisplayMode::Content && rect.width() > 560.0 {
            const DETAIL_COLUMN_WIDTH: f32 = 240.0;
            let detail_left = text_right - DETAIL_COLUMN_WIDTH;
            name_right = detail_left - 16.0;

            let details: Vec<String> = [
                file.modified_time
                    .as_deref()
                    .map(|t| format!("Date modified: {t}")),
                size_text().map(|s| format!("Size: {s}")),
            ]
            .into_iter()
            .flatten()
            .collect();
            paint_text_lines(
                ui,
                &tile_painter,
                details.iter().map(|t| (t.as_str(), secondary_color)),
                detail_left,
                text_right,
                rect.center().y - details.len() as f32 * line_h * 0.5,
                line_h,
                font_id,
            );
        }

        let mut lines: Vec<(String, egui::Color32)> = vec![(file.name.clone(), text_color)];
        match layout.mode {
            ItemViewerDisplayMode::Tiles => {
                lines.push((type_text(), secondary_color));
                if let Some(size) = size_text() {
                    lines.push((size, secondary_color));
                }
            }
            ItemViewerDisplayMode::Content => lines.push((type_text(), secondary_color)),
            _ => {}
        }

        let block_top = rect.center().y - lines.len() as f32 * line_h * 0.5;
        // While the name is being edited the text box takes the first line.
        let skip = usize::from(rename_state.as_ref().is_some_and(|rs| rs.path == file.path));
        paint_text_lines(
            ui,
            &tile_painter,
            lines[skip..]
                .iter()
                .map(|(text, color)| (text.as_str(), *color)),
            text_left,
            name_right,
            block_top + skip as f32 * line_h,
            line_h,
            font_id,
        );

        egui::Rect::from_min_max(
            egui::pos2(text_left, block_top),
            egui::pos2(name_right, block_top + line_h),
        )
    };

    if rename_state.as_ref().is_some_and(|rs| rs.path == file.path) {
        let action = handle_editing_file_name(
            ui,
            i18n,
            file,
            is_selected,
            palette,
            name_rect,
            rename_state,
        );
        return (response, action);
    }

    if layout.mode == ItemViewerDisplayMode::Gallery {
        let (name, _) = truncate_item_text(ui, &file.name, name_rect.width(), font_id, text_color);
        tile_painter.text(
            name_rect.center_top(),
            egui::Align2::CENTER_TOP,
            name,
            font_id.clone(),
            text_color,
        );
    }

    (response.on_hover_text(&file.name), None)
}

/// Paints `lines` left-aligned between `left` and `right`, one per `line_h`
/// starting at `top`, truncating each line with an ellipsis.
#[allow(clippy::too_many_arguments)]
fn paint_text_lines<'a>(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    lines: impl Iterator<Item = (&'a str, egui::Color32)>,
    left: f32,
    right: f32,
    top: f32,
    line_h: f32,
    font_id: &FontId,
) {
    let width = (right - left).max(0.0);
    for (i, (text, color)) in lines.enumerate() {
        let (text, _) = truncate_item_text(ui, text, width, font_id, color);
        painter.text(
            egui::pos2(left, top + (i as f32 + 0.5) * line_h),
            egui::Align2::LEFT_CENTER,
            text,
            font_id.clone(),
            color,
        );
    }
}

/// Draws a file's thumbnail (when allowed and available) or its icon into
/// `rect`, with icons drawn at `icon_size` in the middle of it.
#[allow(clippy::too_many_arguments)]
fn paint_file_visual(
    ui: &mut egui::Ui,
    tile_painter: &egui::Painter,
    file: &FileItem,
    rect: egui::Rect,
    icon_size: f32,
    allow_thumbnail: bool,
    is_cut: bool,
    icon_cache: &IconCache,
    thumbnail_service: &mut ThumbnailService,
    palette: &ThemePalette,
) {
    // Clip image/icon rendering to the visual's box.
    let painter = tile_painter.with_clip_rect(rect.intersect(tile_painter.clip_rect()));
    let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(icon_size, icon_size));
    let full_uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
    let icon_tint = if is_cut {
        palette.icon_colored_hover.linear_multiply(0.45)
    } else {
        palette.icon_colored_hover
    };

    // Keep the app's custom folder glyphs ahead of both Windows thumbnails and
    // shell icons so recognized folders keep their visual identity.
    if let Some(glyph) = icon_cache.get_custom_folder_icon(&file.path, file.is_dir) {
        ui.scope_builder(egui::UiBuilder::new().max_rect(icon_rect), |icon_ui| {
            paint_icon(
                icon_ui,
                icon_ui.max_rect(),
                glyph.regular(),
                icon_size,
                icon_tint,
            );
        });
    } else if file.is_dir {
        // Directory thumbnails are often just Windows' generic folder preview.
        // Use the path-specific shell icon instead, including desktop.ini
        // customizations, and only show the neutral placeholder while it loads.
        if let Some(icon) = icon_cache.get(&file.path, true) {
            painter.image((&icon).into(), icon_rect, full_uv, icon_tint);
        } else {
            ui.scope_builder(egui::UiBuilder::new().max_rect(icon_rect), |icon_ui| {
                paint_icon(
                    icon_ui,
                    icon_ui.max_rect(),
                    AppIcon::FolderSimple.regular(),
                    icon_size,
                    palette.text_header_section,
                );
            });
        }
    } else if let Some(texture) = allow_thumbnail
        .then(|| thumbnail_service.texture_for(ui.ctx(), file))
        .flatten()
    {
        painter.image(
            texture.id(),
            fit_rect(texture.size_vec2(), rect),
            full_uv,
            if is_cut {
                egui::Color32::WHITE.linear_multiply(0.45)
            } else {
                egui::Color32::WHITE
            },
        );
    } else if let Some(icon) = icon_cache.get(&file.path, file.is_dir) {
        painter.image((&icon).into(), icon_rect, full_uv, icon_tint);
    } else {
        ui.scope_builder(egui::UiBuilder::new().max_rect(icon_rect), |icon_ui| {
            paint_icon(
                icon_ui,
                icon_ui.max_rect(),
                AppIcon::ImageSquare.regular(),
                icon_size,
                palette.text_header_section,
            );
        });
    }
}

fn fit_rect(image_size: egui::Vec2, bounds: egui::Rect) -> egui::Rect {
    if image_size.x <= 0.0 || image_size.y <= 0.0 {
        return bounds;
    }

    let scale = (bounds.width() / image_size.x)
        .min(bounds.height() / image_size.y)
        .min(1.0);
    egui::Rect::from_center_size(bounds.center(), image_size * scale)
}

fn sort_label(i18n: &I18n, column: SortColumn) -> String {
    match column {
        SortColumn::Name => i18n.tr("explorer_cols_name"),
        SortColumn::Type => i18n.tr("explorer_cols_type"),
        SortColumn::Size => i18n.tr("explorer_cols_size"),
        SortColumn::Modified => i18n.tr("explorer_cols_modified"),
        SortColumn::Created => i18n.tr("explorer_cols_created"),
        SortColumn::Deleted => i18n.tr("explorer_cols_deleted"),
        SortColumn::OriginalDirectory => i18n.tr("explorer_cols_original_directory"),
        SortColumn::Tags => i18n.tr("explorer_cols_tags"),
    }
}
