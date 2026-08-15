use crate::core::drives::is_raw_physical_drive_path;
use crate::core::fs::FileItem;
use crate::core::utils::text::apply_eden_text_overrides;
use crate::core::utils::thumbnails::{ThumbnailPriority, ThumbnailService};
use crate::core::utils::widgets::{
    apply_eden_dropdown_visual_color_overrides, apply_eden_visual_overrides, draw_dropdown,
};
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::utils::{SortColumn, truncate_item_text};
use crate::gui::windows::containers::enums::{ItemViewerAction, ItemViewerContextAction};
use crate::gui::windows::containers::itemviewer_helper::{
    handle_context_menu_actions, handle_editing_file_name, handle_keyboard_navigation,
    handle_row_click,
};
use crate::gui::windows::containers::structs::{
    DragState, ExplorerState, GalleryState, GalleryThumbnailSize, RenameState, TagsState,
};
use crate::gui::windows::structs::{SettingsWindow, ThemeCustomizer};
use eframe::egui;
use egui::containers::{Popup, PopupCloseBehavior};
use egui::{FontFamily, FontId};
use egui_phosphor::regular;
use std::collections::HashSet;
use std::path::PathBuf;
use windows::Win32::Foundation::HWND;

const LABEL_HEIGHT: f32 = 34.0;
const TOOLBAR_HEIGHT: f32 = 24.0;
const GALLERY_TOOLBAR_GAP: f32 = 6.0;

#[allow(clippy::too_many_arguments)]
pub fn draw_gallery_view(
    ui: &mut egui::Ui,
    i18n: &I18n,
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
    tags_state: &mut TagsState,
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
        sort_column,
        sort_ascending,
    );

    if filtered_indices.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(i18n.tr("folder_is_empty"));
        });
        return action;
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
    let thumb_size = gallery_state.thumbnail_size.pixel_size();
    let tile_width = thumb_size + gallery_state.thumbnail_padding * 2.0 + 26.0;
    let tile_height = thumb_size + gallery_state.thumbnail_padding * 2.0 + LABEL_HEIGHT;
    let row_pitch = tile_height + gallery_state.thumbnail_gap;
    let col_pitch = tile_width + gallery_state.thumbnail_gap;
    let available_width = ui.available_width().max(tile_width);
    let columns = ((available_width + gallery_state.thumbnail_gap) / col_pitch)
        .floor()
        .max(1.0) as usize;
    let rows = filtered_indices.len().div_ceil(columns);
    let content_height = (rows as f32 * row_pitch).max(ui.available_height());
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

    ui.scope_builder(egui::UiBuilder::new().max_rect(scroll_rect), |ui| {
        egui::ScrollArea::vertical()
            .id_salt(("item_viewer_gallery_scroll", active_tab_id))
            .auto_shrink([false, true])
            .show_viewport(ui, |ui, viewport| {
                ui.set_width(available_width);
                ui.set_min_height(content_height);

                let visible_start_row = (viewport.top() / row_pitch).floor().max(0.0) as usize;
                let visible_end_row = (viewport.bottom() / row_pitch).ceil().max(0.0) as usize;
                let nearby_start_row = visible_start_row.saturating_sub(2);
                let nearby_end_row = (visible_end_row + 3).min(rows);

                for row in nearby_start_row..nearby_end_row {
                    for col in 0..columns {
                        let item_index = row * columns + col;
                        let Some(&file_index) = filtered_indices.get(item_index) else {
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

                let content_origin = ui.min_rect().min;

                let bg_rect = egui::Rect::from_min_size(
                    content_origin,
                    egui::vec2(available_width, content_height),
                );
                let bg_response = ui.interact(
                    bg_rect,
                    ui.id().with(("gallery_background", active_tab_id)),
                    egui::Sense::click(),
                );

                for row in visible_start_row.saturating_sub(1)..(visible_end_row + 2).min(rows) {
                    for col in 0..columns {
                        let item_index = row * columns + col;
                        let Some(&file_index) = filtered_indices.get(item_index) else {
                            continue;
                        };

                        let file = &files[file_index];
                        let rect = egui::Rect::from_min_size(
                            content_origin
                                + egui::vec2(col as f32 * col_pitch, row as f32 * row_pitch),
                            egui::vec2(tile_width, tile_height),
                        );
                        let (response, tile_action) = draw_gallery_tile(
                            i18n,
                            ui,
                            file,
                            item_index,
                            rect,
                            thumb_size,
                            icon_cache,
                            thumbnail_service,
                            explorer_state,
                            rename_state,
                            clipboard_set,
                            is_cut_mode,
                            palette,
                            &font_id,
                            &gallery_state,
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
                            action = Some(ItemViewerAction::OpenInNewTab(file.path.clone()));
                        }

                        Popup::context_menu(&response)
                            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
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
                                    tags_state,
                                    settings_window,
                                    hwnd,
                                );
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

fn draw_gallery_toolbar(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    gallery_state: &mut GalleryState,
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

            draw_dropdown(
                ui,
                palette,
                "gallery_sort_selector",
                GALLERY_SORT_COMBO_WIDTH,
                format!(
                    "{}: {} {}",
                    i18n.tr("gallery_sort_by"),
                    sort_label(i18n, sort_column),
                    if sort_ascending {
                        regular::CARET_UP
                    } else {
                        regular::CARET_DOWN
                    }
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
                            action = Some(ItemViewerAction::Sort(column));
                            ui.close();
                        }
                    }
                },
            );

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
                        egui::RichText::new(size.label()).color(
                            if gallery_state.thumbnail_size == size {
                                palette.text_header_section
                            } else {
                                palette.text_header_section
                            },
                        ),
                    )
                    .clicked()
                {
                    gallery_state.thumbnail_size = size;
                    gallery_state.thumbnail_gap = match size {
                        GalleryThumbnailSize::ExtraSmall => 0.0,
                        GalleryThumbnailSize::Small => 4.0,
                        GalleryThumbnailSize::Medium => 8.0,
                        GalleryThumbnailSize::Large => 12.0,
                        GalleryThumbnailSize::ExtraLarge => 16.0,
                    };
                    gallery_state.thumbnail_padding = match size {
                        GalleryThumbnailSize::ExtraSmall => 0.0,
                        GalleryThumbnailSize::Small => 2.0,
                        GalleryThumbnailSize::Medium => 6.0,
                        GalleryThumbnailSize::Large => 8.0,
                        GalleryThumbnailSize::ExtraLarge => 10.0,
                    };
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
    thumb_size: f32,
    icon_cache: &IconCache,
    thumbnail_service: &mut ThumbnailService,
    explorer_state: &ExplorerState,
    rename_state: &mut Option<RenameState>,
    clipboard_set: &HashSet<PathBuf>,
    is_cut_mode: bool,
    palette: &ThemePalette,
    font_id: &FontId,
    gallery_state: &GalleryState,
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

    let preview_rect = egui::Rect::from_center_size(
        egui::pos2(
            rect.center().x,
            rect.top() + gallery_state.thumbnail_padding + thumb_size * 0.5,
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

    // Clip image/icon rendering specifically to the preview rectangle.
    let preview_painter = tile_painter.with_clip_rect(preview_rect.intersect(ui.clip_rect()));

    if let Some(texture) = thumbnail_service.texture_for(ui.ctx(), file) {
        let image_rect = fit_rect(texture.size_vec2(), preview_rect);

        preview_painter.image(
            texture.id(),
            image_rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            if is_cut {
                egui::Color32::WHITE.linear_multiply(0.45)
            } else {
                egui::Color32::WHITE
            },
        );
    } else if let Some(icon) = icon_cache.get(&file.path, file.is_dir) {
        let icon_size = (thumb_size * 0.35).clamp(24.0, 80.0);

        let icon_rect =
            egui::Rect::from_center_size(preview_rect.center(), egui::vec2(icon_size, icon_size));

        preview_painter.image(
            (&icon).into(),
            icon_rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            if is_cut {
                palette.icon_colored_hover.linear_multiply(0.45)
            } else {
                palette.icon_colored_hover
            },
        );
    } else {
        preview_painter.text(
            preview_rect.center(),
            egui::Align2::CENTER_CENTER,
            regular::IMAGE_SQUARE,
            FontId::proportional(thumb_size * 0.35),
            palette.text_header_section,
        );
    }

    let text_rect = egui::Rect::from_min_max(
        egui::pos2(
            rect.left() + gallery_state.thumbnail_padding,
            preview_rect.bottom() + 8.0,
        ),
        egui::pos2(
            rect.right() - gallery_state.thumbnail_padding,
            rect.bottom() - 4.0,
        ),
    );

    let editing_path = rename_state.as_ref().map(|rs| rs.path.clone());

    if let Some(path) = editing_path {
        if path == file.path {
            let action = handle_editing_file_name(
                ui,
                i18n,
                file,
                is_selected,
                palette,
                text_rect,
                rename_state,
            );

            return (response, action);
        }
    }

    let text_color = if is_selected {
        palette.item_viewer_row_text_selected
    } else {
        palette.text_normal
    };

    let (name, _) = truncate_item_text(ui, &file.name, text_rect.width(), font_id, text_color);

    tile_painter.text(
        text_rect.center_top(),
        egui::Align2::CENTER_TOP,
        name,
        font_id.clone(),
        text_color,
    );

    (response.on_hover_text(&file.name), None)
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
    }
}
