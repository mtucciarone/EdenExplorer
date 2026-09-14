use crate::core::fs::SETTINGS_PATH;
use crate::gui::dragdrop::DropTargets;
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::containers::enums::ItemViewerAction;
use crate::gui::windows::containers::itemviewer::draw_item_viewer;
use crate::gui::windows::containers::itemviewer_navbar::{
    ADDRESS_BAR_TOTAL_HEIGHT, TOOLBAR_ROW_VERTICAL_PADDING, draw_itemviewer_navigation_bar,
};
use crate::gui::windows::containers::structs::{
    FavoriteItem, ItemViewerDisplayMode, ItemViewerNavBarAction, RenameState, TabView, TagsState,
};
use crate::gui::windows::settings::draw_settings_page;
use crate::gui::windows::structs::{SettingsWindow, ThemeCustomizer};
use crate::gui::utils::format_size;
use eframe::egui;
use egui::{RichText, ScrollArea};
use egui_extras::{Size, StripBuilder};
use egui_phosphor::regular;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use windows::Win32::Foundation::HWND;

const RIGHT_MARGIN: f32 = 20.0;
const COLUMN_SPACING: f32 = 20.0;
const STATUS_ICON_GAP: f32 = 4.0;
const STATUS_BAR_VPAD: f32 = 6.0;

/// Draws one view's breadcrumb + item viewer + status bar (the content of
/// either half of a split tab, or the whole content area for an unsplit tab).
#[allow(clippy::too_many_arguments)]
pub fn draw_tab_content(
    ui: &mut egui::Ui,
    i18n: &mut I18n,
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
    favorites: &mut Vec<FavoriteItem>,
    drop_targets: &mut DropTargets,
    is_focused: bool,
    is_split_pane: bool,
    saved_search_count: usize,
) -> (Option<ItemViewerNavBarAction>, Option<ItemViewerAction>) {
    let viewport_width = ui.available_width();
    let is_drive_view = view.nav.is_root();
    let is_recycle_bin_view = view.nav.is_recycle_bin();
    let is_settings_view = view.nav.current.to_string_lossy() == SETTINGS_PATH;
    let tag_view_group_id = crate::core::fs::parse_tag_view_path(&view.nav.current);
    let mut hovered_drop_target: Option<PathBuf> = None;
    let mut hovered_drop_target_rect: Option<egui::Rect> = None;
    let mut pending_action: Option<ItemViewerAction> = None;
    let mut tabbar_action = None;

    let font_id = egui::FontId::proportional(palette.text_size);
    let status_height =
        ui.fonts_mut(|f| f.row_height(&font_id)) + 2.0 + 2.0 * STATUS_BAR_VPAD;
    // approximate height of your breadcrumb/tabbar - a split pane stacks the
    // address bar above the toolbar as its own row, so it needs roughly
    // double (each row also reserves `TOOLBAR_ROW_VERTICAL_PADDING` above
    // and below itself).
    //
    // The row containing the address bar must be at least
    // `ADDRESS_BAR_TOTAL_HEIGHT` tall or its own bottom border gets clipped
    // by the file list starting right where this budget says the row
    // should end - previously this was a separately-hardcoded "+8.0" fudge
    // factor that turned out to leave only ~2px of slack once the address
    // bar's real height was accounted for precisely, an amount easily lost
    // to rounding (exactly what happened: the bottom border kept vanishing
    // no matter how the address bar's own code was fixed, because the
    // *budget* around it was still cutting it off by a couple of pixels).
    // Computing this from the address bar's own real constant, with a
    // deliberate `ADDRESS_BAR_SLACK` on top, means the two can't drift out
    // of sync with each other again the way two independent guessed
    // numbers already did once.
    const ADDRESS_BAR_SLACK: f32 = 10.0;
    let address_bar_row_height = ADDRESS_BAR_TOTAL_HEIGHT + ADDRESS_BAR_SLACK;
    let toolbar_row_height = 30.0;
    let tabbar_height = if is_split_pane {
        address_bar_row_height + toolbar_row_height + 4.0 * TOOLBAR_ROW_VERTICAL_PADDING
    } else {
        address_bar_row_height.max(toolbar_row_height) + 2.0 * TOOLBAR_ROW_VERTICAL_PADDING
    };

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
                    is_split_pane,
                    &tags_state.groups,
                    saved_search_count,
                ));
            });

            strip.cell(|ui| {
                let content_rect = ui.max_rect();

                if is_settings_view {
                    let (action, tags_item_action) = draw_settings_page(
                        ui,
                        settings_window,
                        i18n,
                        palette,
                        icon_cache,
                        theme_customizer,
                        tags_state,
                        favorites,
                    );
                    if action.is_some() {
                        settings_window.pending_action = action;
                    }
                    if tags_item_action.is_some() {
                        pending_action = tags_item_action;
                    }
                } else if let Some(group_id) = tag_view_group_id {
                    pending_action = crate::gui::windows::containers::tags::draw_tag_view(
                        ui,
                        i18n,
                        icon_cache,
                        palette,
                        tags_state,
                        group_id,
                        file_type_cache,
                        settings_window.current_settings.date_style,
                        settings_window.current_settings.time_format_24h,
                        &settings_window.current_settings.custom_date_format,
                    );
                } else if view.display_mode == ItemViewerDisplayMode::Gallery {
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
                    ScrollArea::horizontal()
                        .id_salt(("item_viewer_horizontal_scroll", tab_id))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_min_width(viewport_width);
                            // The type-to-filter box (drawn by `draw_item_viewer`
                            // via `handle_global_actions`) draws its own border
                            // only a few px inset from this ui's own edge - a
                            // clip rect set exactly to the scroll viewport eats
                            // that border's top/left edge, the same class of bug
                            // already fixed for the navbar's address bar and
                            // search box (see CLAUDE.md). A few px of slack lets
                            // the border render fully without letting real
                            // overflow escape the scroll area visibly.
                            ui.set_clip_rect(ui.clip_rect().expand(3.0));
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
                }

                // A subtle box border around the file/folder view, so a dual-pane
                // split makes it clear at a glance where each pane's view ends.
                ui.painter().rect_stroke(
                    content_rect,
                    egui::CornerRadius::same(4),
                    egui::Stroke::new(1.5, palette.borders_default),
                    egui::StrokeKind::Inside,
                );
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
                // Not shown for the drive root ("This PC") or Settings - neither
                // has a meaningful item/size count. Every other view (Details,
                // Columns, Gallery, Preview and their variants, Recycle Bin, and
                // tag views) shares this same status bar, including each half of
                // a split pane independently, since `draw_tab_content` is called
                // once per pane.
                if !is_drive_view && !is_settings_view {
                    let counts = if let Some(group_id) = tag_view_group_id {
                        // The tag view renders its own local row list (see
                        // `tags::draw_tag_view`) rather than populating
                        // `view.files`, so counts have to be computed from the
                        // tag group's own item list instead - `view.files` would
                        // just be stale data left over from whatever real
                        // folder this tab last showed.
                        status_counts_for_tag_group(tags_state, group_id, &view.explorer_state.selected_paths)
                    } else {
                        status_counts_for_view(view, folder_sizes)
                    };

                    let text_color = palette.status_bar_text_color;
                    let icon_color = palette.status_bar_icon_color;
                    let font_id = egui::FontId::proportional(palette.text_size);
                    let folder_scanning_enabled =
                        settings_window.current_settings.folder_scanning_enabled;

                    ui.spacing_mut().item_spacing.x = STATUS_ICON_GAP;

                    // An explicit fixed inner margin, rather than relying on
                    // `Align::Center` to split the row's extra height evenly -
                    // the icon glyphs' own line-box metrics carry a lot more
                    // empty space above the visible glyph than below it, so
                    // an equal top/bottom margin still reads as "mostly
                    // touching the bottom" once the glyph's own internal
                    // lopsidedness is added on top. Skewing the margin
                    // (less on top, more on bottom) compensates for that and
                    // was tuned by pixel-sampling a live screenshot.
                    egui::Frame::NONE
                        .fill(palette.status_bar_bg_color)
                        .inner_margin(egui::Margin {
                            left: 0,
                            right: 0,
                            top: (STATUS_BAR_VPAD - 4.0) as i8,
                            bottom: (STATUS_BAR_VPAD + 4.0) as i8,
                        })
                        .show(ui, |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(RIGHT_MARGIN);

                        status_counter(
                            ui,
                            regular::FILE,
                            counts.file_count,
                            &font_id,
                            text_color,
                            icon_color,
                            &i18n.tr("status_files_tooltip"),
                        );
                        ui.add_space(COLUMN_SPACING);
                        status_counter(
                            ui,
                            regular::FOLDER_SIMPLE,
                            counts.dir_count,
                            &font_id,
                            text_color,
                            icon_color,
                            &i18n.tr("status_folders_tooltip"),
                        );
                        ui.add_space(COLUMN_SPACING);
                        status_counter(
                            ui,
                            regular::STACK_SIMPLE,
                            counts.dir_count + counts.file_count,
                            &font_id,
                            text_color,
                            icon_color,
                            &i18n.tr("status_total_items_tooltip"),
                        );
                        ui.add_space(COLUMN_SPACING);
                        status_size(
                            ui,
                            regular::HARD_DRIVE,
                            counts.total_size,
                            &font_id,
                            text_color,
                            icon_color,
                            &i18n.tr(if folder_scanning_enabled {
                                "status_total_size_tooltip"
                            } else {
                                "status_total_size_tooltip_files_only"
                            }),
                        );

                        if counts.selected_count > 0 {
                            ui.add_space(COLUMN_SPACING);
                            let selected_label = if counts.selected_count == 1 {
                                i18n.tr("item_capital")
                            } else {
                                i18n.tr("items_capital")
                            };
                            let selected_text = format!(
                                "{} {selected_label} {} \u{2022} {}",
                                counts.selected_count,
                                i18n.tr("selected"),
                                format_size(counts.selected_size),
                            );
                            ui.label(RichText::new(selected_text).font(font_id.clone()).color(text_color));
                        }
                    });
                        });
                }
            });
        });

    ui.spacing_mut().item_spacing.y = old_spacing;

    (tabbar_action, pending_action)
}

/// The status bar's four right-aligned counters plus the selection summary,
/// gathered into one place so both the normal-directory and tag-view sources
/// (see `status_counts_for_view`/`status_counts_for_tag_group`) can feed the
/// same rendering code.
struct StatusCounts {
    dir_count: usize,
    file_count: usize,
    /// Sum of every visible file's size, plus every visible folder's size
    /// where a folder-size scan has already produced one (see
    /// `folder_sizes` in `ItemViewerFolderSizeState`) - folders with no scan
    /// result yet (scanning disabled, or still in progress) simply don't
    /// contribute, which is why the size tooltip differs when folder-size
    /// scanning is off.
    total_size: u64,
    selected_count: usize,
    selected_size: u64,
}

fn status_counts_for_view(
    view: &TabView,
    folder_sizes: &HashMap<PathBuf, crate::gui::windows::containers::structs::ItemViewerFolderSizeState>,
) -> StatusCounts {
    let mut dir_count = 0usize;
    let mut file_count = 0usize;
    let mut total_size = 0u64;

    let item_size = |file: &crate::core::fs::FileItem| -> u64 {
        if file.is_dir {
            folder_sizes.get(&file.path).map(|s| s.bytes).unwrap_or(0)
        } else {
            file.file_size.unwrap_or(0)
        }
    };

    for &idx in &view.item_viewer_filter_state.cached_indices {
        let file = &view.files[idx];
        if file.is_dir {
            dir_count += 1;
        } else {
            file_count += 1;
        }
        total_size += item_size(file);
    }

    let selected_paths = &view.explorer_state.selected_paths;
    let selected_count = selected_paths.len();
    let selected_size = if selected_count == 0 {
        0
    } else {
        view.files
            .iter()
            .filter(|file| selected_paths.contains(&file.path))
            .map(item_size)
            .sum()
    };

    StatusCounts {
        dir_count,
        file_count,
        total_size,
        selected_count,
        selected_size,
    }
}

fn status_counts_for_tag_group(
    tags_state: &TagsState,
    group_id: u64,
    selected_paths: &HashSet<PathBuf>,
) -> StatusCounts {
    let mut counts = StatusCounts {
        dir_count: 0,
        file_count: 0,
        total_size: 0,
        selected_count: 0,
        selected_size: 0,
    };

    let Some(group) = tags_state.groups.iter().find(|g| g.id == group_id) else {
        return counts;
    };

    for path in &group.items {
        let Ok(metadata) = std::fs::metadata(path) else {
            continue;
        };
        let is_selected = selected_paths.contains(path);
        if metadata.is_dir() {
            counts.dir_count += 1;
        } else {
            counts.file_count += 1;
            counts.total_size += metadata.len();
            if is_selected {
                counts.selected_size += metadata.len();
            }
        }
        if is_selected {
            counts.selected_count += 1;
        }
    }

    counts
}

fn status_counter(
    ui: &mut egui::Ui,
    icon: &str,
    count: usize,
    font_id: &egui::FontId,
    text_color: egui::Color32,
    icon_color: egui::Color32,
    tooltip: &str,
) {
    // Added in right-to-left order (icon first) so the icon ends up on the
    // right and the number to its left, matching how this status bar has
    // always read: "12 🗎" rather than "🗎 12".
    ui.label(RichText::new(icon).font(font_id.clone()).color(icon_color))
        .on_hover_text(tooltip);
    ui.label(RichText::new(count.to_string()).font(font_id.clone()).color(text_color))
        .on_hover_text(tooltip);
}

fn status_size(
    ui: &mut egui::Ui,
    icon: &str,
    total_size: u64,
    font_id: &egui::FontId,
    text_color: egui::Color32,
    icon_color: egui::Color32,
    tooltip: &str,
) {
    // Reverse order from `status_counter`: a byte-size reads more naturally
    // as "💾 1.2 GB" (icon first) than trailing the icon after the number.
    ui.label(RichText::new(format_size(total_size)).font(font_id.clone()).color(text_color))
        .on_hover_text(tooltip);
    ui.label(RichText::new(icon).font(font_id.clone()).color(icon_color))
        .on_hover_text(tooltip);
}
