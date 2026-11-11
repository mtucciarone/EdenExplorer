use crate::core::i18n::I18n;
use crate::core::icons::IconCache;
use crate::core::utils::files::format_size;
use crate::gui::dragdrop::DropTargets;
use crate::gui::icon_system::{AppIcon, StyledIcon, paint_icon};
use crate::gui::theme::ThemePalette;
use crate::gui::windows::containers::enums::ItemViewerAction;
use crate::gui::windows::containers::itemviewer::draw_item_viewer;
use crate::gui::windows::containers::itemviewer_gallery::draw_gallery_toolbar;
use crate::gui::windows::containers::itemviewer_navbar::draw_itemviewer_navigation_bar;
use crate::gui::windows::containers::structs::{
    ItemViewerDisplayMode, ItemViewerNavBarAction, RenameState, TabView, TagsState, ViewPreset,
};
use crate::gui::windows::structs::{SettingsWindow, ThemeCustomizer};
use eframe::egui;
use egui::ScrollArea;
use egui_extras::{Size, StripBuilder};
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use windows::Win32::Foundation::HWND;

const RIGHT_MARGIN: f32 = 20.0;
const COLUMN_SPACING: f32 = 20.0;
const FILES_COLUMN_WIDTH: f32 = 56.0;
const FOLDERS_COLUMN_WIDTH: f32 = 56.0;
const SIZE_COLUMN_WIDTH: f32 = 76.0;
const STATUS_ICON_GAP: f32 = 4.0;
/// Space between the View / Sort By bar and the Details table header.
const DETAILS_TOOLBAR_GAP: f32 = 6.0;

/// Draws one view's breadcrumb + item viewer + status bar (the content of
/// either half of a split tab, or the whole content area for an unsplit tab).
#[allow(clippy::too_many_arguments)]
pub fn draw_tab_content(
    ui: &mut egui::Ui,
    i18n: &I18n,
    icon_cache: &IconCache,
    palette: &ThemePalette,
    hwnd: Option<HWND>,
    view: &mut TabView,
    tab_id: u64,
    is_favorited: bool,
    folder_sizes: &HashMap<
        PathBuf,
        crate::gui::windows::containers::structs::ItemViewerFolderSizeState,
    >,
    clipboard_has_files: bool,
    clipboard_set: &HashSet<PathBuf>,
    clipboard_is_cut: bool,
    show_hidden_files_folders: bool,
    show_item_viewer_icons: bool,
    extra_actions_bar: bool,
    rename_state: &mut Option<RenameState>,
    file_type_cache: &mut HashMap<String, String>,
    file_size_text_cache: &mut HashMap<PathBuf, (u64, String)>,
    folder_size_text_cache: &mut HashMap<PathBuf, (u64, bool, String)>,
    drive_size_text_cache: &mut HashMap<PathBuf, (u64, u64, String)>,
    external_drag_to_internal_hover: &mut bool,
    drag_active: bool,
    native_drag_active: bool,
    drag_hover_target: Option<PathBuf>,
    tags_state: &mut TagsState,
    theme_customizer: &mut ThemeCustomizer,
    settings_window: &mut SettingsWindow,
    drop_targets: &mut DropTargets,
    is_focused: bool,
) -> (Option<ItemViewerNavBarAction>, Option<ItemViewerAction>) {
    let viewport_width = ui.available_width();
    let is_drive_view = view.nav.is_root();
    let is_recycle_bin_view = view.nav.is_recycle_bin();
    let mut hovered_drop_target: Option<PathBuf> = None;
    let mut hovered_drop_target_rect: Option<egui::Rect> = None;
    let mut pending_action: Option<ItemViewerAction> = None;
    let mut tabbar_action = None;

    let font_id = egui::FontId::proportional(palette.text_size);
    let status_height = ui.fonts_mut(|f| f.row_height(&font_id)) + 2.0;
    // approximate height of your breadcrumb/tabbar
    let tabbar_height = 30.0;

    let old_spacing = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 0.0;
    let content_rect = ui.available_rect_before_wrap();

    StripBuilder::new(ui)
        .size(Size::exact(tabbar_height)) // Tabbar
        .size(Size::remainder()) // Item viewer
        .size(Size::exact(status_height)) // Footer
        .vertical(|mut strip| {
            strip.cell(|ui| {
                tabbar_action = Some(draw_itemviewer_navigation_bar(
                    ui,
                    i18n,
                    icon_cache,
                    view,
                    tab_id,
                    palette,
                    is_favorited,
                    drag_active,
                    drag_hover_target.clone(),
                    extra_actions_bar,
                    clipboard_has_files,
                ));
            });

            strip.cell(|ui| {
                let viewer_rect = ui.max_rect();
                if view.display_mode != ItemViewerDisplayMode::Details {
                    pending_action = draw_item_viewer(
                        ui,
                        i18n,
                        view,
                        folder_sizes,
                        clipboard_has_files,
                        clipboard_set,
                        clipboard_is_cut,
                        is_drive_view,
                        is_recycle_bin_view,
                        show_hidden_files_folders,
                        show_item_viewer_icons,
                        icon_cache,
                        rename_state,
                        palette,
                        file_type_cache,
                        file_size_text_cache,
                        folder_size_text_cache,
                        drive_size_text_cache,
                        external_drag_to_internal_hover,
                        &mut tabbar_action,
                        drag_active,
                        native_drag_active,
                        drag_hover_target.clone(),
                        &mut hovered_drop_target,
                        &mut hovered_drop_target_rect,
                        tags_state,
                        theme_customizer,
                        settings_window,
                        hwnd,
                        is_focused,
                        tab_id,
                        viewport_width,
                    );
                } else {
                    // Same View / Sort By bar the icon views draw for themselves.
                    let toolbar_action = if !is_drive_view && !is_recycle_bin_view {
                        let current_view = ViewPreset::of(view);
                        let action = draw_gallery_toolbar(
                            ui,
                            i18n,
                            palette,
                            &mut view.gallery_state,
                            view.display_mode,
                            current_view,
                            view.sort_column,
                            view.sort_ascending,
                        );
                        ui.add_space(DETAILS_TOOLBAR_GAP);
                        action
                    } else {
                        None
                    };

                    ScrollArea::horizontal()
                        .id_salt(("item_viewer_horizontal_scroll", tab_id))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_min_width(viewport_width);
                            pending_action = draw_item_viewer(
                                ui,
                                i18n,
                                view,
                                folder_sizes,
                                clipboard_has_files,
                                clipboard_set,
                                clipboard_is_cut,
                                is_drive_view,
                                is_recycle_bin_view,
                                show_hidden_files_folders,
                                show_item_viewer_icons,
                                icon_cache,
                                rename_state,
                                palette,
                                file_type_cache,
                                file_size_text_cache,
                                folder_size_text_cache,
                                drive_size_text_cache,
                                external_drag_to_internal_hover,
                                &mut tabbar_action,
                                drag_active,
                                native_drag_active,
                                drag_hover_target.clone(),
                                &mut hovered_drop_target,
                                &mut hovered_drop_target_rect,
                                tags_state,
                                theme_customizer,
                                settings_window,
                                hwnd,
                                is_focused,
                                tab_id,
                                viewport_width,
                            );
                        });

                    if pending_action.is_none() {
                        pending_action = toolbar_action;
                    }
                }

                // View changes apply to this pane directly (not the focused one), and
                // the per-folder snapshot taken after drawing persists them.
                if let Some(ItemViewerAction::SetViewPreset(preset)) = pending_action {
                    preset.apply(view);
                    pending_action = None;
                }

                // Ctrl + mouse wheel steps through the views like Windows Explorer.
                if !is_drive_view && !is_recycle_bin_view {
                    let steps = ctrl_wheel_steps(ui, viewer_rect, tab_id);
                    if steps != 0 {
                        ViewPreset::of(view).stepped(steps).apply(view);
                    }
                }
            });

            // A split view renders both panes into the same DropTargets value.
            // Keep an already detected target so rendering the other pane does
            // not erase the first pane's drop region.
            if hovered_drop_target.is_some() {
                drop_targets.item_target.target = hovered_drop_target.clone();
                drop_targets.item_target.rect = hovered_drop_target_rect;
            }
            drop_targets
                .item_background_targets
                .push(crate::gui::dragdrop::DropTargetRegion {
                    target: Some(view.nav.current.clone()),
                    rect: Some(egui::Rect::from_min_max(
                        egui::pos2(content_rect.left(), content_rect.top() + tabbar_height),
                        content_rect.right_bottom(),
                    )),
                });
            if let Some(target) = tabbar_action
                .as_ref()
                .and_then(|a| a.move_files_to_breadcrumb_dir.clone())
            {
                drop_targets.breadcrumb_target.target = Some(target);
                drop_targets.breadcrumb_target.rect = tabbar_action
                    .as_ref()
                    .and_then(|a| a.move_files_to_breadcrumb_dir_rect);
            }

            strip.cell(|ui| {
                let rect = ui.max_rect();

                if !is_drive_view {
                    let mut dir_count = 0usize;
                    let mut file_count = 0usize;

                    for &idx in &view.item_viewer_filter_state.cached_indices {
                        if view.files[idx].is_dir {
                            dir_count += 1;
                        } else {
                            file_count += 1;
                        }
                    }

                    // Sum visible files and completed recursive folder sizes.
                    let total_size = view
                        .item_viewer_filter_state
                        .cached_indices
                        .iter()
                        .map(|&idx| {
                            let item = &view.files[idx];
                            if item.is_dir {
                                folder_sizes
                                    .get(&item.path)
                                    .filter(|state| state.done)
                                    .map(|state| state.bytes)
                                    .unwrap_or(0)
                            } else {
                                item.file_size.unwrap_or(0)
                            }
                        })
                        .fold(0u64, u64::saturating_add);

                    let text_color = ui.visuals().text_color();
                    let font_id = egui::FontId::proportional(palette.text_size);

                    let selected_count = view.explorer_state.selected_paths.len();
                    let selected_label = if selected_count == 1 {
                        i18n.tr("item_capital")
                    } else {
                        i18n.tr("items_capital")
                    };

                    let center_y = rect.center().y;

                    let mut right = rect.right() - RIGHT_MARGIN;

                    // Files
                    draw_status_counter(
                        ui,
                        right,
                        center_y,
                        AppIcon::File.regular(),
                        file_count,
                        &font_id,
                        text_color,
                    );

                    right -= FILES_COLUMN_WIDTH + COLUMN_SPACING;

                    // Folders
                    draw_status_counter(
                        ui,
                        right,
                        center_y,
                        AppIcon::FolderSimple.regular(),
                        dir_count,
                        &font_id,
                        text_color,
                    );

                    right -= FOLDERS_COLUMN_WIDTH + COLUMN_SPACING;

                    // Total size, immediately to the left of the folder count.
                    draw_status_size(
                        ui.painter(),
                        right,
                        center_y,
                        if settings_window.current_settings.folder_scanning_enabled {
                            format_size(total_size)
                        } else {
                            "-".to_string()
                        },
                        &font_id,
                        text_color,
                    );

                    right -= SIZE_COLUMN_WIDTH + COLUMN_SPACING;

                    // Selected
                    if selected_count > 0 {
                        let selected_text =
                            format!("{selected_count} {selected_label} {}", i18n.tr("selected"));

                        draw_status_selected(
                            ui.painter(),
                            right,
                            center_y,
                            &selected_text,
                            &font_id,
                            text_color,
                        );
                    }
                }
            });
        });

    ui.spacing_mut().item_spacing.y = old_spacing;

    (tabbar_action, pending_action)
}

fn draw_status_counter(
    ui: &mut egui::Ui,
    right: f32,
    center_y: f32,
    icon: StyledIcon,
    count: usize,
    font_id: &egui::FontId,
    color: egui::Color32,
) {
    let number_right = right;
    let icon_left = number_right + STATUS_ICON_GAP;

    ui.painter().text(
        egui::pos2(number_right, center_y),
        egui::Align2::RIGHT_CENTER,
        count.to_string(),
        font_id.clone(),
        color,
    );

    let icon_size = font_id.size + 2.0;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(icon_left + icon_size * 0.5, center_y),
        egui::vec2(icon_size, icon_size),
    );
    paint_icon(ui, icon_rect, icon, icon_size, color);
}

fn draw_status_size(
    painter: &egui::Painter,
    right: f32,
    center_y: f32,
    size: String,
    font_id: &egui::FontId,
    color: egui::Color32,
) {
    painter.text(
        egui::pos2(right, center_y),
        egui::Align2::RIGHT_CENTER,
        size,
        font_id.clone(),
        color,
    );
}

fn draw_status_selected(
    painter: &egui::Painter,
    right: f32,
    center_y: f32,
    text: &str,
    font_id: &egui::FontId,
    color: egui::Color32,
) {
    painter.text(
        egui::pos2(right, center_y),
        egui::Align2::RIGHT_CENTER,
        text,
        font_id.clone(),
        color,
    );
}

/// Whole wheel notches turned with Ctrl held while the pointer is over `rect`
/// (positive = away from the user). egui turns Ctrl+wheel into zoom rather than
/// scrolling, so the raw wheel events are read here; smooth/touchpad deltas are
/// accumulated so one gesture still moves one step at a time.
fn ctrl_wheel_steps(ui: &egui::Ui, rect: egui::Rect, tab_id: u64) -> i32 {
    if !ui.rect_contains_pointer(rect) {
        return 0;
    }

    let delta: f32 = ui.input(|i| {
        i.events
            .iter()
            .filter_map(|event| match event {
                egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } if modifiers.command => Some(match unit {
                    egui::MouseWheelUnit::Line => delta.y,
                    egui::MouseWheelUnit::Page => delta.y * 3.0,
                    egui::MouseWheelUnit::Point => delta.y / 50.0,
                }),
                _ => None,
            })
            .sum()
    });

    let id = egui::Id::new(("item_viewer_ctrl_wheel", tab_id));
    if delta == 0.0 {
        if !ui.input(|i| i.modifiers.command) {
            ui.data_mut(|d| d.remove::<f32>(id));
        }
        return 0;
    }

    let total = ui.data(|d| d.get_temp::<f32>(id).unwrap_or(0.0)) + delta;
    let steps = total.trunc();
    ui.data_mut(|d| d.insert_temp(id, total - steps));
    steps as i32
}
