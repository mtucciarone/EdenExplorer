use crate::core::fs::{FileItem, MY_PC_PATH};
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::utils::SortColumn;
use crate::gui::windows::containers::enums::ItemViewerAction;
use crate::gui::windows::containers::itemviewer::draw_item_viewer;
use crate::gui::windows::containers::structs::{
    DragState, ExplorerState, FilterState, ItemViewerFolderSizeState, RenameState, TabInfo,
    TabState, TabbarAction, TabsAction,
};
use crate::gui::windows::containers::tabs::{draw_tabbar, draw_tabs};
use crate::gui::windows::mainwindow_imp::tab_title_for;
use crate::gui::windows::structs::{SettingsWindow, SidebarState, ThemeCustomizer};
use eframe::egui;
use egui::ScrollArea;
use egui_phosphor::regular;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use windows::Win32::Foundation::HWND;

pub fn draw_explorer(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    palette: &ThemePalette,
    hwnd: Option<HWND>,
    tabs: &mut Vec<TabState>,
    active_tab: usize,
    tab_infos_cache: &mut Vec<TabInfo>,
    tab_infos_dirty: &mut bool,
    pending_tab_scroll_id: &mut Option<u64>,
    sidebar_state: &SidebarState,
    files: &Vec<FileItem>,
    folder_sizes: &HashMap<PathBuf, ItemViewerFolderSizeState>,
    clipboard_has_files: bool,
    clipboard_set: &HashSet<PathBuf>,
    clipboard_is_cut: bool,
    sort_column: SortColumn,
    sort_ascending: bool,
    rename_state: &mut Option<RenameState>,
    file_type_cache: &mut HashMap<String, String>,
    file_size_text_cache: &mut HashMap<PathBuf, (u64, String)>,
    folder_size_text_cache: &mut HashMap<PathBuf, (u64, bool, String)>,
    drive_size_text_cache: &mut HashMap<PathBuf, (u64, u64, String)>,
    external_drag_to_internal_hover: &mut bool,
    drag_state: &mut DragState,
    item_viewer_filter_state: &mut FilterState,
    is_loading: bool,
    explorer_state: &mut ExplorerState,
    theme_customizer: &mut ThemeCustomizer,
    settings_window: &mut SettingsWindow,
) -> (TabsAction, Option<TabbarAction>, Option<ItemViewerAction>) {
    let mut tabs_action = TabsAction::default();
    let mut tabbar_action: Option<TabbarAction> = None;
    let mut pending_action: Option<ItemViewerAction> = None;

    let tabs_width = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(tabs_width, ui.available_height()),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            let old_spacing = ui.spacing().item_spacing;
            ui.spacing_mut().item_spacing.y = 0.0;

            if *tab_infos_dirty || tab_infos_cache.len() != tabs.len() {
                *tab_infos_cache = tabs
                    .iter()
                    .map(|tab| TabInfo {
                        id: tab.id,
                        title: tab_title_for(&tab.nav),
                        full_path: if tab.nav.is_root() {
                            PathBuf::from(MY_PC_PATH)
                        } else {
                            tab.nav.current.clone()
                        },
                        is_pinned: settings_window
                            .current_settings
                            .pinned_tabs
                            .iter()
                            .any(|p| p == &tab.nav.current),
                    })
                    .collect();
                *tab_infos_dirty = false;
            }

            let active_id = tabs[active_tab].id;

            egui::Frame::NONE.show(ui, |ui| {
                ui.add_space(8.0);
                let scroll_to_id = *pending_tab_scroll_id;
                tabs_action = draw_tabs(
                    ui,
                    tab_infos_cache,
                    active_id,
                    palette,
                    hwnd,
                    scroll_to_id,
                    drag_state,
                );
                if scroll_to_id.is_some() {
                    *pending_tab_scroll_id = None;
                }
            });

            let container = egui::Frame::NONE
                .stroke(egui::Stroke::NONE)
                .fill(egui::Color32::TRANSPARENT)
                .inner_margin(egui::Margin::symmetric(10, 8));

            let active_index = active_tab;
            let is_drive_view = tabs[active_index].nav.is_root();
            let display_files = files;

            container.show(ui, |ui| {
                tabbar_action = {
                    let tab = &mut tabs[active_index];
                    let is_favorited = sidebar_state
                        .favorites
                        .iter()
                        .any(|fav| fav.path == tab.nav.current);

                    Some(draw_tabbar(
                        ui,
                        icon_cache,
                        tab,
                        palette,
                        is_favorited,
                        drag_state,
                    ))
                };

                ui.add_space(4.0);

                let status_height = palette.text_size + 6.0;
                let status_bottom_gap = 4.0;
                let item_viewer_height =
                    (ui.available_height() - status_height - status_bottom_gap).max(0.0);
                let mut hovered_drop_target: Option<PathBuf> = None;

                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), item_viewer_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ScrollArea::horizontal()
                            .id_salt("item_viewer_horizontal_scroll")
                            .show(ui, |ui| {
                                pending_action = draw_item_viewer(
                                    ui,
                                    display_files,
                                    folder_sizes,
                                    clipboard_has_files,
                                    clipboard_set,
                                    clipboard_is_cut,
                                    is_drive_view,
                                    sort_column,
                                    sort_ascending,
                                    icon_cache,
                                    rename_state,
                                    palette,
                                    file_type_cache,
                                    file_size_text_cache,
                                    folder_size_text_cache,
                                    drive_size_text_cache,
                                    external_drag_to_internal_hover,
                                    &mut tabbar_action,
                                    drag_state,
                                    item_viewer_filter_state,
                                    &mut hovered_drop_target,
                                    is_loading,
                                    explorer_state,
                                    theme_customizer,
                                    settings_window,
                                    hwnd,
                                );
                            });
                    },
                );

                let pointer_released = ui
                    .ctx()
                    .input(|i| i.pointer.any_released() && i.pointer.interact_pos().is_some());

                if drag_state.active && pointer_released && !drag_state.source_items.is_empty() {
                    if let Some(target_dir) = tabs_action.move_files_to_tab_dir.clone() {
                        if pending_action.is_none() {
                            pending_action = Some(ItemViewerAction::MoveFilesToTabDirectory {
                                sources: drag_state.source_items.clone(),
                                target_dir,
                            });
                        }
                    } else if let Some(target_dir) = tabbar_action
                        .as_ref()
                        .and_then(|a| a.move_files_to_breadcrumb_dir.clone())
                    {
                        if pending_action.is_none() {
                            pending_action =
                                Some(ItemViewerAction::MoveFilesToBreadcrumbDirectory {
                                    sources: drag_state.source_items.clone(),
                                    target_dir,
                                });
                        }
                    } else if let Some(target_dir) = hovered_drop_target {
                        if pending_action.is_none() {
                            pending_action = Some(ItemViewerAction::MoveItems {
                                sources: drag_state.source_items.clone(),
                                target_dir,
                            });
                        }
                    }

                    drag_state.active = false;
                    drag_state.start_pos = None;
                    drag_state.source_items.clear();
                }

                if !is_drive_view {
                    let mut dir_count = 0usize;
                    let mut file_count = 0usize;
                    for &idx in item_viewer_filter_state.cached_indices.iter() {
                        if display_files[idx].is_dir {
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
                                let selected_count = explorer_state.selected_paths.len();
                                let selected_label =
                                    if selected_count == 1 { "Item" } else { "Items" };

                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} {}",
                                        regular::FILE,
                                        file_count
                                    ))
                                    .size(text_size)
                                    .color(text_color),
                                );
                                ui.label(
                                    egui::RichText::new("|")
                                        .size(text_size)
                                        .color(text_color.linear_multiply(0.6)),
                                );
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} {}",
                                        regular::FOLDER_SIMPLE,
                                        dir_count
                                    ))
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
                                            "{selected_count} {selected_label} Selected"
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
            });

            ui.spacing_mut().item_spacing = old_spacing;
        },
    );

    (tabs_action, tabbar_action, pending_action)
}
