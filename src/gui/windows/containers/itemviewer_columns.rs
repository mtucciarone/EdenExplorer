use crate::core::fs::FileItem;
use crate::core::audio::AudioPreviewService;
use crate::core::preview::PreviewService;
use crate::core::video::VideoPreviewService;
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::utils::truncate_item_text;
use crate::gui::windows::containers::enums::ItemViewerAction;
use crate::gui::windows::containers::itemviewer_helper::{
    handle_context_menu_actions, handle_editing_file_name,
};
use crate::gui::windows::containers::itemviewer_preview::{PREVIEW_PANE_WIDTH, draw_preview_pane};
use crate::gui::windows::containers::structs::{
    ColumnEntry, ColumnItem, ColumnsViewState, ExplorerState, FindInPreviewState, RenameState,
    TagsState,
};
use crate::gui::windows::structs::SettingsWindow;
use eframe::egui;
use egui::containers::{Popup, PopupCloseBehavior};
use egui_extras::{Size, StripBuilder};
use egui_phosphor::regular;
use std::collections::HashSet;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::HWND;

const MIN_COLUMN_WIDTH: f32 = 180.0;
const MAX_COLUMN_WIDTH: f32 = 420.0;
const ROW_HEIGHT: f32 = 22.0;

/// macOS Finder-style column browser: each folder you click opens another column
/// to its right showing that folder's contents. Single-clicking a folder drills
/// into it immediately; files just get selected on a single click and need a
/// double-click to open (matching the rest of the app).
#[allow(clippy::too_many_arguments)]
pub fn draw_columns_view(
    ui: &mut egui::Ui,
    i18n: &I18n,
    columns_state: &mut ColumnsViewState,
    current_dir: &Path,
    show_hidden_files_folders: bool,
    icon_cache: &IconCache,
    palette: &ThemePalette,
    explorer_state: &mut ExplorerState,
    tags_state: &mut TagsState,
    settings_window: &mut SettingsWindow,
    hwnd: Option<HWND>,
    paste_enabled: bool,
    clipboard_set: &HashSet<PathBuf>,
    is_cut_mode: bool,
    rename_state: &mut Option<RenameState>,
) -> Option<ItemViewerAction> {
    draw_columns_core(
        ui,
        i18n,
        columns_state,
        current_dir,
        show_hidden_files_folders,
        icon_cache,
        palette,
        None,
        explorer_state,
        tags_state,
        settings_window,
        hwnd,
        paste_enabled,
        clipboard_set,
        is_cut_mode,
        rename_state,
    )
}

/// The column browser, plus a preview pane on the right showing the content of
/// whichever file is currently selected (if it's previewable).
#[allow(clippy::too_many_arguments)]
pub fn draw_column_preview_view(
    ui: &mut egui::Ui,
    i18n: &I18n,
    columns_state: &mut ColumnsViewState,
    current_dir: &Path,
    show_hidden_files_folders: bool,
    preview_service: &mut PreviewService,
    video_service: &mut VideoPreviewService,
    audio_service: &mut AudioPreviewService,
    find_in_preview: &mut FindInPreviewState,
    icon_cache: &IconCache,
    palette: &ThemePalette,
    explorer_state: &mut ExplorerState,
    tags_state: &mut TagsState,
    settings_window: &mut SettingsWindow,
    hwnd: Option<HWND>,
    paste_enabled: bool,
    clipboard_set: &HashSet<PathBuf>,
    is_cut_mode: bool,
    rename_state: &mut Option<RenameState>,
) -> Option<ItemViewerAction> {
    preview_service.pump(ui.ctx());

    let mut action = None;

    StripBuilder::new(ui)
        .size(Size::remainder())
        .size(Size::exact(PREVIEW_PANE_WIDTH))
        .horizontal(|mut strip| {
            strip.cell(|ui| {
                action = draw_columns_core(
                    ui,
                    i18n,
                    columns_state,
                    current_dir,
                    show_hidden_files_folders,
                    icon_cache,
                    palette,
                    Some(preview_service),
                    explorer_state,
                    tags_state,
                    settings_window,
                    hwnd,
                    paste_enabled,
                    clipboard_set,
                    is_cut_mode,
                    rename_state,
                );
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
                    columns_state.selected_file.as_deref(),
                    palette,
                );
            });
        });

    action
}

/// Shared column-browser rendering. `current_dir` is the tab's actual navigated
/// directory (the last column); if it doesn't match what's cached, the column
/// list is rebuilt starting fresh from `current_dir` (e.g. the display mode was
/// just switched to a column layout, or the user navigated via the address bar).
/// When `preview_service` is `Some`, selecting a file also queues it for preview.
#[allow(clippy::too_many_arguments)]
fn draw_columns_core(
    ui: &mut egui::Ui,
    i18n: &I18n,
    columns_state: &mut ColumnsViewState,
    current_dir: &Path,
    show_hidden_files_folders: bool,
    icon_cache: &IconCache,
    palette: &ThemePalette,
    mut preview_service: Option<&mut PreviewService>,
    explorer_state: &mut ExplorerState,
    tags_state: &mut TagsState,
    settings_window: &mut SettingsWindow,
    hwnd: Option<HWND>,
    paste_enabled: bool,
    clipboard_set: &HashSet<PathBuf>,
    is_cut_mode: bool,
    rename_state: &mut Option<RenameState>,
) -> Option<ItemViewerAction> {
    // Keep every column up to (and including) whichever one already represents
    // `current_dir`, so clicking deeper into the tree never hides the columns
    // that led there. Only reset from scratch if `current_dir` isn't part of the
    // current chain at all.
    match columns_state
        .columns
        .iter()
        .position(|column| column.path == current_dir)
    {
        Some(idx) => columns_state.columns.truncate(idx + 1),
        None => columns_state.columns = vec![load_column(current_dir)],
    }

    // A navigation/refresh happened since these columns were last read from
    // disk (see `TabView::columns_view_state.needs_reload`'s doc comment) -
    // re-read every still-open column's contents so newly created, renamed,
    // or deleted files (from this app or externally) actually show up.
    if columns_state.needs_reload {
        for column in &mut columns_state.columns {
            *column = load_column(&column.path);
        }
        columns_state.needs_reload = false;
    }

    let mut action = None;
    let mut clicked_folder: Option<(usize, PathBuf)> = None;
    let mut selected_file: Option<PathBuf> = None;
    let mut opened_file: Option<PathBuf> = None;
    let mut context_menu_action: Option<ItemViewerAction> = None;

    let name_font_id = egui::FontId::proportional(palette.text_size);

    egui::ScrollArea::horizontal()
        .id_salt("columns_view_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                // Left padding to match every other layout's file/folder list,
                // which otherwise sits flush against the pane's edge.
                ui.add_space(8.0);

                let column_count = columns_state.columns.len();

                for (col_idx, column) in columns_state.columns.iter().enumerate() {
                    let column_width =
                        compute_column_width(ui, column, show_hidden_files_folders, &name_font_id);

                    ui.allocate_ui_with_layout(
                        egui::vec2(column_width, ui.available_height()),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt(("columns_view_column", col_idx))
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let selected_next_path = columns_state
                                        .columns
                                        .get(col_idx + 1)
                                        .map(|next| next.path.clone());
                                    let is_last_column = col_idx + 1 == column_count;

                                    for item in &column.items {
                                        if item.is_hidden && !show_hidden_files_folders {
                                            continue;
                                        }

                                        let is_selected = if item.is_dir {
                                            selected_next_path.as_ref() == Some(&item.path)
                                        } else {
                                            is_last_column
                                                && columns_state.selected_file.as_ref()
                                                    == Some(&item.path)
                                        };

                                        let (rect, resp) = ui.allocate_exact_size(
                                            egui::vec2(ui.available_width(), ROW_HEIGHT),
                                            egui::Sense::click(),
                                        );

                                        if is_selected || resp.hovered() {
                                            ui.painter().rect_filled(
                                                rect,
                                                egui::CornerRadius::same(palette.small_radius),
                                                if is_selected {
                                                    palette.primary_active
                                                } else {
                                                    palette.primary_hover
                                                },
                                            );
                                        }

                                        const ICON_SIZE: f32 = 16.0;
                                        let icon_area = egui::Rect::from_min_size(
                                            rect.left_center() - egui::vec2(0.0, ICON_SIZE * 0.5),
                                            egui::vec2(ICON_SIZE, ICON_SIZE),
                                        );
                                        let text_x = if let Some(glyph) = icon_cache
                                            .get_custom_folder_icon(&item.path, item.is_dir)
                                        {
                                            ui.painter().text(
                                                icon_area.center(),
                                                egui::Align2::CENTER_CENTER,
                                                glyph,
                                                egui::FontId::proportional(ICON_SIZE),
                                                palette.icon_colored_hover,
                                            );
                                            icon_area.right() + 6.0
                                        } else if let Some(texture) =
                                            icon_cache.get(&item.path, item.is_dir)
                                        {
                                            ui.painter().image(
                                                (&texture).into(),
                                                icon_area,
                                                egui::Rect::from_min_size(
                                                    egui::pos2(0.0, 0.0),
                                                    egui::vec2(1.0, 1.0),
                                                ),
                                                palette.icon_colored_hover,
                                            );
                                            icon_area.right() + 6.0
                                        } else {
                                            rect.left() + 6.0
                                        };

                                        let is_renaming = rename_state
                                            .as_ref()
                                            .map(|rs| rs.path == item.path)
                                            .unwrap_or(false);

                                        if is_renaming {
                                            let stub_file = column_item_to_file_item(item);
                                            let text_rect = egui::Rect::from_min_max(
                                                egui::pos2(text_x, rect.top()),
                                                rect.right_bottom(),
                                            );

                                            if let Some(rename_action) = handle_editing_file_name(
                                                ui,
                                                i18n,
                                                &stub_file,
                                                is_selected,
                                                palette,
                                                text_rect,
                                                &mut *rename_state,
                                            ) {
                                                context_menu_action = Some(rename_action);
                                            }

                                            continue;
                                        }

                                        // Reserve room for the drill-in caret on folders so
                                        // long names truncate before reaching it, instead of
                                        // overflowing past the column into whatever's next
                                        // (the next column, or the preview pane).
                                        let caret_reserved = if item.is_dir { 16.0 } else { 4.0 };
                                        let max_text_width =
                                            (rect.right() - text_x - caret_reserved).max(0.0);
                                        let name_font_id =
                                            egui::FontId::proportional(palette.text_size);
                                        let text_color = ui.visuals().text_color();
                                        let (display_name, _) = truncate_item_text(
                                            ui,
                                            &item.name,
                                            max_text_width,
                                            &name_font_id,
                                            text_color,
                                        );

                                        ui.painter().text(
                                            egui::pos2(text_x, rect.center().y),
                                            egui::Align2::LEFT_CENTER,
                                            display_name,
                                            name_font_id,
                                            text_color,
                                        );

                                        if item.is_dir {
                                            ui.painter().text(
                                                rect.right_center() - egui::vec2(6.0, 0.0),
                                                egui::Align2::RIGHT_CENTER,
                                                regular::CARET_RIGHT,
                                                egui::FontId::proportional(palette.text_size),
                                                ui.visuals().weak_text_color(),
                                            );
                                        }

                                        if resp.hovered() {
                                            ui.ctx()
                                                .set_cursor_icon(egui::CursorIcon::PointingHand);
                                        }

                                        if item.is_dir {
                                            // Folders drill in on a single click - that's
                                            // the whole point of a column browser.
                                            if resp.clicked() {
                                                clicked_folder = Some((col_idx, item.path.clone()));
                                            }
                                            if resp.middle_clicked()
                                                && settings_window
                                                    .current_settings
                                                    .middle_click_opens_new_tab
                                            {
                                                context_menu_action =
                                                    Some(ItemViewerAction::OpenInNewTab(
                                                        item.path.clone(),
                                                    ));
                                            }
                                        } else {
                                            // Files: single click selects (and previews,
                                            // if a preview pane is present); double-click
                                            // opens, matching every other view in the app.
                                            if resp.clicked() {
                                                selected_file = Some(item.path.clone());
                                            }
                                            if resp.double_clicked() {
                                                opened_file = Some(item.path.clone());
                                            }
                                        }

                                        if resp.secondary_clicked() && !item.is_dir {
                                            selected_file = Some(item.path.clone());
                                        }

                                        let stub_file = column_item_to_file_item(item);
                                        let item_is_cut =
                                            is_cut_mode && clipboard_set.contains(&item.path);

                                        Popup::context_menu(&resp)
                                            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                                            .show(|ui| {
                                                handle_context_menu_actions(
                                                    ui,
                                                    i18n,
                                                    &stub_file,
                                                    is_selected,
                                                    paste_enabled,
                                                    false,
                                                    false,
                                                    item_is_cut,
                                                    &mut context_menu_action,
                                                    palette,
                                                    &mut *explorer_state,
                                                    &mut *tags_state,
                                                    &mut *settings_window,
                                                    hwnd,
                                                    icon_cache,
                                                    false,
                                                );
                                            });
                                    }

                                    if column.items.is_empty() {
                                        ui.add_space(8.0);
                                        ui.weak(i18n.tr("folder_is_empty"));
                                    }

                                    // Bottom padding so the last item isn't
                                    // flush against the pane border.
                                    ui.add_space(12.0);
                                });
                        },
                    );

                    if col_idx + 1 < column_count {
                        ui.separator();
                    }
                }
            });
        });

    if let Some((col_idx, path)) = clicked_folder {
        columns_state.columns.truncate(col_idx + 1);
        columns_state.columns.push(load_column(&path));
        columns_state.selected_file = None;
        action = Some(ItemViewerAction::Open(path));
    } else if let Some(path) = opened_file {
        action = Some(ItemViewerAction::OpenWithDefault(vec![path]));
    } else if let Some(path) = selected_file {
        if let Some(service) = preview_service.as_deref_mut() {
            service.request(&path);
        }
        columns_state.selected_file = Some(path);
    }

    if context_menu_action.is_some() {
        action = context_menu_action;
    }

    action
}

/// Builds a minimal `FileItem` stub from a `ColumnItem` for reuse with the
/// shared context-menu handler, which only reads `path`/`is_dir` from it -
/// the column browser doesn't track the extra metadata (size, dates) that a
/// full directory scan would provide.
fn column_item_to_file_item(item: &ColumnItem) -> FileItem {
    FileItem::new(
        item.name.clone(),
        item.path.clone(),
        item.is_dir,
        item.is_hidden,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

/// Sizes a column to fit the longest visible item name in it (icon and
/// drill-in caret included), clamped to a sane range so one huge file name
/// can't blow the column out and an empty/all-short folder doesn't shrink it
/// to nothing.
fn compute_column_width(
    ui: &egui::Ui,
    column: &ColumnEntry,
    show_hidden_files_folders: bool,
    font_id: &egui::FontId,
) -> f32 {
    const ICON_AND_PADDING: f32 = 16.0 + 6.0 + 6.0;
    const CARET_RESERVED: f32 = 16.0;

    let max_name_width = column
        .items
        .iter()
        .filter(|item| show_hidden_files_folders || !item.is_hidden)
        .map(|item| {
            ui.painter()
                .layout_no_wrap(
                    item.name.clone(),
                    font_id.clone(),
                    ui.visuals().text_color(),
                )
                .size()
                .x
        })
        .fold(0.0_f32, f32::max);

    (max_name_width + ICON_AND_PADDING + CARET_RESERVED).clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH)
}

fn load_column(path: &Path) -> ColumnEntry {
    let mut items = Vec::new();

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };

            let name = entry.file_name().to_string_lossy().to_string();
            let is_hidden = metadata
                .file_attributes()
                & 0x2 // FILE_ATTRIBUTE_HIDDEN
                != 0;

            items.push(ColumnItem {
                name,
                path: entry.path(),
                is_dir: metadata.is_dir(),
                is_hidden,
            });
        }
    }

    items.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        })
    });

    ColumnEntry {
        path: path.to_path_buf(),
        items,
    }
}
