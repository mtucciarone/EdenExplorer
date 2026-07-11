use crate::core::fs::MY_PC_PATH;
use crate::gui::dragdrop::{DragDropBackend, DropTargets};
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::containers::enums::ItemViewerAction;
use crate::gui::windows::containers::itemviewer::draw_item_viewer;
use crate::gui::windows::containers::structs::{
    RenameState, TabInfo, TabState, TabView, TabbarAction, TagsState,
};
use crate::gui::windows::containers::tabs::draw_tabbar;
use crate::gui::windows::mainwindow_imp::tab_title_for;
use crate::gui::windows::structs::{SettingsWindow, ThemeCustomizer};
use eframe::egui;
use egui::ScrollArea;
use egui_phosphor::regular;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use windows::Win32::Foundation::HWND;

/// Rebuilds the tab-strip display cache from the window-global tab list, if dirty.
pub fn update_tab_infos_cache(
    tabs: &[TabState],
    tab_infos_cache: &mut Vec<TabInfo>,
    tab_infos_dirty: &mut bool,
    settings_window: &SettingsWindow,
) {
    if *tab_infos_dirty || tab_infos_cache.len() != tabs.len() {
        *tab_infos_cache = tabs
            .iter()
            .map(|tab| TabInfo {
                id: tab.id,
                title: tab_title_for(&tab.primary_view.nav),
                full_path: if tab.primary_view.nav.is_root() {
                    PathBuf::from(MY_PC_PATH)
                } else {
                    tab.primary_view.nav.current.clone()
                },
                is_pinned: settings_window
                    .current_settings
                    .pinned_tabs
                    .iter()
                    .any(|p| p == &tab.primary_view.nav.current),
            })
            .collect();
        *tab_infos_dirty = false;
    }
}

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
    dragdrop: Option<&dyn DragDropBackend>,
    tags_state: &mut TagsState,
    theme_customizer: &mut ThemeCustomizer,
    settings_window: &mut SettingsWindow,
    drop_targets: &mut DropTargets,
    is_focused: bool,
) -> (Option<TabbarAction>, Option<ItemViewerAction>) {
    let is_drive_view = view.nav.is_root();

    let mut tabbar_action = Some(draw_tabbar(
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

    ui.add_space(4.0);

    let status_height = palette.text_size + 6.0;
    let status_bottom_gap = 4.0;
    let item_viewer_height = (ui.available_height() - status_height - status_bottom_gap).max(0.0);
    let mut hovered_drop_target: Option<PathBuf> = None;
    let mut hovered_drop_target_rect: Option<egui::Rect> = None;
    let mut pending_action: Option<ItemViewerAction> = None;

    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), item_viewer_height),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
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
                        dragdrop,
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
        },
    );

    drop_targets.item_target.target = hovered_drop_target.clone();
    drop_targets.item_target.rect = hovered_drop_target_rect;
    drop_targets.breadcrumb_target.target = tabbar_action
        .as_ref()
        .and_then(|a| a.move_files_to_breadcrumb_dir.clone());
    drop_targets.breadcrumb_target.rect = tabbar_action
        .as_ref()
        .and_then(|a| a.move_files_to_breadcrumb_dir_rect);

    if !is_drive_view {
        let mut dir_count = 0usize;
        let mut file_count = 0usize;
        for &idx in view.item_viewer_filter_state.cached_indices.iter() {
            if view.files[idx].is_dir {
                dir_count += 1;
            } else {
                file_count += 1;
            }
        }

        let status_frame = egui::Frame::NONE
            .fill(egui::Color32::TRANSPARENT)
            .inner_margin(egui::Margin {
                left: 10,
                right: 10,
                top: 2,
                bottom: 2,
            });

        status_frame.show(ui, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), status_height),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    let text_color = ui.visuals().text_color();
                    let text_size = palette.text_size;
                    let selected_count = view.explorer_state.selected_paths.len();
                    let selected_label = if selected_count == 1 {
                        i18n.tr("item_capital")
                    } else {
                        i18n.tr("items_capital")
                    };

                    ui.label(
                        egui::RichText::new(format!("{} {}", regular::FILE, file_count))
                            .size(text_size)
                            .color(text_color),
                    );
                    ui.label(
                        egui::RichText::new("|")
                            .size(text_size)
                            .color(text_color.linear_multiply(0.6)),
                    );
                    ui.label(
                        egui::RichText::new(format!("{} {}", regular::FOLDER_SIMPLE, dir_count))
                            .size(text_size)
                            .color(text_color),
                    );
                    if selected_count > 0 {
                        ui.label(
                            egui::RichText::new("|")
                                .size(text_size)
                                .color(text_color.linear_multiply(0.6)),
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "{selected_count} {selected_label} {}",
                                i18n.tr("selected")
                            ))
                            .size(text_size)
                            .color(text_color),
                        );
                    }
                },
            );
        });
    }

    ui.add_space(status_bottom_gap);

    (tabbar_action, pending_action)
}
