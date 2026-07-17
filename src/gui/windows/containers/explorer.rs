use crate::gui::dragdrop::DropTargets;
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::containers::enums::ItemViewerAction;
use crate::gui::windows::containers::itemviewer::draw_item_viewer;
use crate::gui::windows::containers::itemviewer_navbar::draw_itemviewer_navigation_bar;
use crate::gui::windows::containers::structs::{
    ItemViewerNavBarAction, RenameState, TabView, TagsState,
};
use crate::gui::windows::structs::{SettingsWindow, ThemeCustomizer};
use eframe::egui;
use egui::ScrollArea;
use egui_extras::{Size, StripBuilder};
use egui_phosphor::regular;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use windows::Win32::Foundation::HWND;

const RIGHT_MARGIN: f32 = 8.0;
const COLUMN_SPACING: f32 = 20.0;
const FILES_COLUMN_WIDTH: f32 = 56.0;
const FOLDERS_COLUMN_WIDTH: f32 = 56.0;
const STATUS_ICON_GAP: f32 = 4.0;

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
    let is_drive_view = view.nav.is_root();
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
                ));
            });

            strip.cell(|ui| {
                ScrollArea::horizontal()
                    .id_salt(("item_viewer_horizontal_scroll", tab_id))
                    .show(ui, |ui| {
                        pending_action = draw_item_viewer(
                            ui,
                            i18n,
                            &view.files,
                            folder_sizes,
                            clipboard_has_files,
                            clipboard_set,
                            clipboard_is_cut,
                            is_drive_view,
                            view.sort_column,
                            view.sort_ascending,
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
                            &mut view.drag_state,
                            drag_active,
                            native_drag_active,
                            drag_hover_target.clone(),
                            view.nav.current.clone(),
                            &mut view.item_viewer_filter_state,
                            &mut hovered_drop_target,
                            &mut hovered_drop_target_rect,
                            view.is_loading,
                            &mut view.explorer_state,
                            tags_state,
                            theme_customizer,
                            settings_window,
                            hwnd,
                            is_focused,
                            tab_id,
                        );
                    });
            });

            drop_targets.item_target.target = hovered_drop_target.clone();
            drop_targets.item_target.rect = hovered_drop_target_rect;
            drop_targets.breadcrumb_target.target = tabbar_action
                .as_ref()
                .and_then(|a| a.move_files_to_breadcrumb_dir.clone());
            drop_targets.breadcrumb_target.rect = tabbar_action
                .as_ref()
                .and_then(|a| a.move_files_to_breadcrumb_dir_rect);

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

                    let text_color = ui.visuals().text_color();
                    let font_id = egui::FontId::proportional(palette.text_size);

                    let selected_count = view.explorer_state.selected_paths.len();
                    let selected_label = if selected_count == 1 {
                        i18n.tr("item_capital")
                    } else {
                        i18n.tr("items_capital")
                    };

                    let painter = ui.painter();
                    let center_y = rect.center().y;

                    let mut right = rect.right() - RIGHT_MARGIN;

                    // Files
                    draw_status_counter(
                        painter,
                        right,
                        center_y,
                        regular::FILE,
                        file_count,
                        &font_id,
                        text_color,
                    );

                    right -= FILES_COLUMN_WIDTH + COLUMN_SPACING;

                    // Folders
                    draw_status_counter(
                        painter,
                        right,
                        center_y,
                        regular::FOLDER_SIMPLE,
                        dir_count,
                        &font_id,
                        text_color,
                    );

                    right -= FOLDERS_COLUMN_WIDTH + COLUMN_SPACING;

                    // Selected
                    if selected_count > 0 {
                        let selected_text =
                            format!("{selected_count} {selected_label} {}", i18n.tr("selected"));

                        draw_status_selected(
                            painter,
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
    painter: &egui::Painter,
    right: f32,
    center_y: f32,
    icon: &str,
    count: usize,
    font_id: &egui::FontId,
    color: egui::Color32,
) {
    let number_right = right;
    let icon_left = number_right + STATUS_ICON_GAP;

    painter.text(
        egui::pos2(number_right, center_y),
        egui::Align2::RIGHT_CENTER,
        count.to_string(),
        font_id.clone(),
        color,
    );

    painter.text(
        egui::pos2(icon_left, center_y),
        egui::Align2::LEFT_CENTER,
        icon,
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
