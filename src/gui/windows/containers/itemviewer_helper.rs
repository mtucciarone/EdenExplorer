use crate::core::context_menu_settings::{CustomContextMenuEntry, CustomContextMenuIcon};
use crate::core::drives::is_raw_physical_drive_path;
use crate::core::fs::FileItem;
use crate::core::indexer::{
    default_item_viewer_drive_column_size, default_item_viewer_file_column_size,
    default_recycle_bin_column_size,
};
use crate::core::utils::files::filename_has_valid_characters_realtime;
use crate::core::utils::text::apply_eden_text_overrides;
use crate::core::utils::widgets::{
    apply_eden_visual_color_overrides, apply_eden_visual_overrides, draw_checkbox,
};
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::{ThemePalette, apply_checkbox_colors};
use crate::gui::utils::{
    SortColumn, SortKey, clear_clipboard_files, drive_usage_bar, format_size, get_file_type_name,
    truncate_item_text,
};
use crate::gui::windows::containers::enums::{
    ItemViewerAction, ItemViewerContextAction, ItemViewerHeaderColumn, ItemViewerNavAction,
};
use crate::gui::windows::containers::structs::{
    DragState, ExplorerState, FilterState, ItemViewerColumnFitRequest, ItemViewerColumnLayout,
    ItemViewerColumnState, ItemViewerColumnWidths, ItemViewerFolderSizeState, ItemViewerLayout,
    ItemViewerNavBarAction, RenameState, TagsState,
};
use crate::gui::windows::shell_context_menu::{ShellContextMenu, ShellContextMenuItem};
use crate::gui::windows::structs::SettingsWindow;
use eframe::egui;
use egui::ScrollArea;
use egui::containers::{Popup, PopupCloseBehavior};
use egui::{FontFamily, FontId};
use egui_phosphor::regular;
use std::collections::HashMap;
use std::path::PathBuf;
use windows::Win32::Foundation::HWND;

/// `egui::Context` memory key `MainWindow::ui` sets once per frame to
/// whether a blocking modal (paste-conflict, bulk-rename, checksum) is
/// currently open - checked by `itemviewer.rs`'s `modal_input_blocked` so
/// this view's own keyboard shortcuts (Ctrl+A select-all, Delete, etc.)
/// don't fire underneath a modal that's visually on top of it but doesn't
/// otherwise suppress the view's own per-frame input handling. Using
/// `egui::Context` memory (the same mechanism `global_shortcuts_disabled`'s
/// `topbar_hamburger_menu` check already uses) avoids threading a new
/// parameter through `draw_tab_content`/`draw_item_viewer` and their
/// several call sites.
pub const BLOCKING_MODAL_MEMORY_ID: &str = "blocking_modal_open";

pub fn draw_external_to_internal_drag_overlay(
    ui: &mut egui::Ui,
    i18n: &I18n,
    external_drag_to_internal_hover: bool,
) {
    if external_drag_to_internal_hover {
        let rect = ui.max_rect();

        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(6),
            ui.visuals().selection.bg_fill.linear_multiply(0.15),
        );

        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            i18n.tr("move_to_this_folder"),
            egui::TextStyle::Heading.resolve(ui.style()),
            ui.visuals().text_color(),
        );
    }
}

/// Extra space added above a column header's label/checkbox to visually
/// balance the header cell (whose child `Ui` lays out top-down, so this must
/// be added explicitly rather than relying on `header_height` alone).
const HEADER_TOP_PADDING: f32 = 4.0;

pub fn compute_layout(
    _ui: &egui::Ui,
    is_drive_view: bool,
    is_recycle_bin_view: bool,
    show_selection_checkboxes: bool,
    palette: &ThemePalette,
) -> ItemViewerLayout {
    let row_padding = 2.0;
    // Height of the actual content (icon/text).
    let content_height = palette.row_height;
    // Total height allocated to each table row.
    let row_height = content_height + row_padding * 2.0;
    // The column header (Name/Type/Size/...) gets a bit more breathing room
    // above and below its labels than a plain data row does.
    let header_height = row_height + HEADER_TOP_PADDING * 2.0;

    ItemViewerLayout {
        row_height,
        icon_size: content_height,
        header_height,
        is_drive_view,
        is_recycle_bin_view,
        show_checkboxes: !is_drive_view && show_selection_checkboxes,
    }
}

/// A context-menu row with a leading Phosphor icon glyph next to the label.
fn menu_item_button(ui: &mut egui::Ui, icon: &str, label: &str) -> egui::Response {
    menu_item_button_enabled(ui, true, icon, label)
}

/// Best-effort icon for a Windows shell menu item that didn't provide its own bitmap
/// (owner-drawn items - most third-party shell extensions use this). Matches on common
/// English verb names; falls back to a generic icon for anything else, including shell
/// items on a non-English Windows install.
fn fallback_shell_icon(label: &str) -> &'static str {
    let lower = label.to_ascii_lowercase();

    let keyword_icon: &[(&str, &str)] = &[
        ("cut", regular::SCISSORS),
        ("copy", regular::COPY),
        ("paste", regular::CLIPBOARD),
        ("delete", regular::TRASH),
        ("remove", regular::TRASH),
        ("rename", regular::PENCIL_SIMPLE),
        ("edit", regular::PENCIL_SIMPLE),
        ("share", regular::SHARE_NETWORK),
        ("send to", regular::PAPER_PLANE_TILT),
        ("print", regular::PRINTER),
        ("zip", regular::ARCHIVE),
        ("compress", regular::ARCHIVE),
        ("extract", regular::ARCHIVE),
        ("archive", regular::ARCHIVE),
        ("scan", regular::SHIELD_CHECK),
        ("pin", regular::PUSH_PIN),
        ("shortcut", regular::LINK),
        ("restore", regular::ARROW_COUNTER_CLOCKWISE),
        ("run as", regular::SHIELD),
        ("properties", regular::INFO),
        ("open", regular::FOLDER_OPEN),
    ];

    keyword_icon
        .iter()
        .find(|(keyword, _)| lower.contains(keyword))
        .map(|(_, icon)| *icon)
        .unwrap_or(regular::PUZZLE_PIECE)
}

fn menu_item_button_enabled(
    ui: &mut egui::Ui,
    enabled: bool,
    icon: &str,
    label: &str,
) -> egui::Response {
    ui.add_enabled(enabled, egui::Button::new(format!("{icon}  {label}")))
}

/// Draws the user's custom context menu entries (see
/// `core::context_menu_settings`) applicable to this right-click, as their
/// own separator-bounded group - a no-op if none apply. Shared by the
/// per-item context menu (`handle_context_menu_actions`) and the
/// folder-background menu drawn directly in `itemviewer.rs`.
#[allow(clippy::too_many_arguments)]
pub fn draw_custom_context_menu_group(
    ui: &mut egui::Ui,
    i18n: &I18n,
    icon_cache: &IconCache,
    entries: &[CustomContextMenuEntry],
    has_file: bool,
    has_folder: bool,
    is_background: bool,
    context_paths: &[PathBuf],
    action: &mut Option<ItemViewerAction>,
) {
    let applicable: Vec<&CustomContextMenuEntry> = entries
        .iter()
        .filter(|e| e.applies_to(has_file, has_folder, is_background))
        .collect();

    if applicable.is_empty() {
        return;
    }

    ui.separator();

    for entry in applicable {
        draw_custom_context_menu_entry(ui, i18n, icon_cache, entry, context_paths, action);
    }
}

fn draw_custom_context_menu_entry(
    ui: &mut egui::Ui,
    i18n: &I18n,
    icon_cache: &IconCache,
    entry: &CustomContextMenuEntry,
    context_paths: &[PathBuf],
    action: &mut Option<ItemViewerAction>,
) {
    let label = if entry.label.is_empty() {
        i18n.tr("custom_context_menu_untitled")
    } else {
        entry.label.clone()
    };

    if entry.is_submenu {
        let texture = custom_icon_texture(icon_cache, &entry.icon);
        if let Some(texture) = texture {
            let image = egui::Image::new(&texture).fit_to_exact_size(egui::vec2(16.0, 16.0));
            ui.menu_image_text_button(image, label, |ui| {
                for child in &entry.children {
                    draw_custom_context_menu_entry(
                        ui,
                        i18n,
                        icon_cache,
                        child,
                        context_paths,
                        action,
                    );
                }
            });
        } else {
            let glyph = custom_icon_glyph(&entry.icon, true);
            ui.menu_button(format!("{glyph}  {label}"), |ui| {
                for child in &entry.children {
                    draw_custom_context_menu_entry(
                        ui,
                        i18n,
                        icon_cache,
                        child,
                        context_paths,
                        action,
                    );
                }
            });
        }
        return;
    }

    let clicked = if let Some(texture) = custom_icon_texture(icon_cache, &entry.icon) {
        ui.add(egui::Button::image_and_text(
            egui::Image::new(&texture).fit_to_exact_size(egui::vec2(16.0, 16.0)),
            &label,
        ))
        .clicked()
    } else {
        menu_item_button(ui, custom_icon_glyph(&entry.icon, false), &label).clicked()
    };

    if clicked {
        *action = Some(ItemViewerAction::RunCustomCommand {
            entry_id: entry.id,
            paths: context_paths.to_vec(),
        });
        ui.close();
    }
}

/// Extensions loaded as an image's own pixel content; anything else (an
/// `.exe`/`.dll`, typically) falls back to asking the shell for that file's
/// icon instead.
const IMAGE_ICON_EXTENSIONS: &[&str] = &["ico", "png", "jpg", "jpeg", "bmp", "gif"];

fn custom_icon_texture(
    icon_cache: &IconCache,
    icon: &CustomContextMenuIcon,
) -> Option<egui::TextureHandle> {
    match icon {
        CustomContextMenuIcon::FileIcon(path) => {
            let is_image = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| IMAGE_ICON_EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e)));

            if is_image {
                icon_cache.get_custom_file_icon(path)
            } else {
                icon_cache.get(path, false)
            }
        }
        _ => None,
    }
}

fn custom_icon_glyph(icon: &CustomContextMenuIcon, is_submenu: bool) -> &str {
    match icon {
        CustomContextMenuIcon::Glyph(g) => g.as_str(),
        CustomContextMenuIcon::None => {
            if is_submenu {
                regular::LIST
            } else {
                regular::TERMINAL
            }
        }
        // A file icon that hasn't finished loading yet - fall back to a
        // generic glyph for this frame; it'll switch over once ready.
        CustomContextMenuIcon::FileIcon(_) => {
            if is_submenu {
                regular::LIST
            } else {
                regular::APP_WINDOW
            }
        }
    }
}

pub fn handle_context_menu_actions(
    ui: &mut egui::Ui,
    i18n: &I18n,
    file: &FileItem,
    is_selected: bool,
    paste_enabled: bool,
    is_drive_view: bool,
    is_recycle_bin_view: bool,
    is_cut: bool,
    action: &mut Option<ItemViewerAction>,
    _palette: &ThemePalette,
    explorer_state: &mut ExplorerState,
    tags_state: &mut TagsState,
    settings_window: &SettingsWindow,
    hwnd: Option<HWND>,
    icon_cache: &IconCache,
    is_search_view: bool,
) {
    // Apply context-menu-specific typography
    apply_eden_visual_overrides(ui, _palette);
    apply_eden_text_overrides(ui, _palette);

    // Match Explorer behavior: right-click selects if not already selected
    if !is_selected {
        *action = Some(ItemViewerAction::ReplaceSelection(file.path.clone()));
    }

    let mut context_paths: Vec<PathBuf> = if is_selected {
        explorer_state.selected_paths.iter().cloned().collect()
    } else {
        vec![file.path.clone()]
    };
    context_paths.sort();
    context_paths.dedup();

    if is_drive_view {
        if menu_item_button(ui, regular::INFO, &i18n.tr("properties")).clicked() {
            *action = Some(ItemViewerAction::Context(
                ItemViewerContextAction::Properties(context_paths.clone()),
            ));
            ui.close();
        }

        return;
    }

    if is_recycle_bin_view {
        if menu_item_button_enabled(ui, !is_cut, regular::SCISSORS, &i18n.tr("inputs_cut"))
            .clicked()
        {
            *action = Some(ItemViewerAction::Context(ItemViewerContextAction::Cut(
                context_paths.clone(),
            )));
            ui.close();
        }

        if menu_item_button(
            ui,
            regular::ARROW_COUNTER_CLOCKWISE,
            &i18n.tr("recycle_bin_restore"),
        )
        .clicked()
        {
            *action = Some(ItemViewerAction::Context(ItemViewerContextAction::Restore(
                context_paths.clone(),
            )));
            ui.close();
        }

        if menu_item_button(
            ui,
            regular::TRASH,
            &i18n.tr("recycle_bin_delete_permanently"),
        )
        .clicked()
        {
            *action = Some(ItemViewerAction::Context(ItemViewerContextAction::Delete(
                context_paths.clone(),
                true,
            )));
            ui.close();
        }

        if menu_item_button(ui, regular::INFO, &i18n.tr("properties")).clicked() {
            *action = Some(ItemViewerAction::Context(
                ItemViewerContextAction::Properties(context_paths.clone()),
            ));
            ui.close();
        }

        return;
    }

    // --- NORMAL FILE VIEW ---

    // Determine if "Open in new tab" should be enabled
    // Enable only if a single path is selected
    let enable_open_in_tab = context_paths.len() == 1;

    if menu_item_button_enabled(
        ui,
        enable_open_in_tab,
        regular::ARROW_SQUARE_OUT,
        &i18n.tr("inputs_newtab"),
    )
    .clicked()
    {
        if let Some(path) = context_paths.first() {
            *action = Some(ItemViewerAction::OpenInNewTab(path.clone()));
            ui.close();
        }
    }

    if menu_item_button_enabled(
        ui,
        enable_open_in_tab && context_paths.first().is_some_and(|p| p.is_dir()),
        regular::COLUMNS,
        &i18n.tr("inputs_opensplit"),
    )
    .clicked()
    {
        if let Some(path) = context_paths.first() {
            *action = Some(ItemViewerAction::OpenInSplitView(path.clone()));
            ui.close();
        }
    }

    // Search results span many different folders (unlike a normal listing,
    // where the containing folder is just the tab's own current directory)
    // - so "open the location this came from" is only meaningful here.
    if is_search_view
        && menu_item_button_enabled(
            ui,
            enable_open_in_tab && context_paths.first().and_then(|p| p.parent()).is_some(),
            regular::FOLDER_OPEN,
            &i18n.tr("inputs_open_location"),
        )
        .clicked()
    {
        if let Some(parent) = context_paths.first().and_then(|p| p.parent()) {
            *action = Some(ItemViewerAction::OpenInNewTab(parent.to_path_buf()));
            ui.close();
        }
    }

    let favoritable_dirs: Vec<PathBuf> = context_paths
        .iter()
        .filter(|p| p.is_dir())
        .cloned()
        .collect();

    if !favoritable_dirs.is_empty()
        && menu_item_button(ui, regular::STAR, &i18n.tr("add_favorite")).clicked()
    {
        *action = Some(ItemViewerAction::Context(
            ItemViewerContextAction::AddFavorite(favoritable_dirs),
        ));
        ui.close();
    }

    let label = if context_paths.len() == 1 {
        i18n.tr("open_default_program")
    } else {
        i18n.tr("open_files_default_program")
    };

    let has_tag = context_paths.iter().any(|path| tags_state.is_tagged(path));
    let tag_label = if has_tag {
        i18n.tr("tag_remove")
    } else {
        i18n.tr("tag_add")
    };

    if menu_item_button(ui, regular::TAG, &tag_label).clicked() {
        *action = Some(ItemViewerAction::Context(if has_tag {
            ItemViewerContextAction::RemoveTag(context_paths.clone())
        } else {
            ItemViewerContextAction::AddTag(context_paths.clone())
        }));
        ui.close();
    }

    let all_files = context_paths.iter().all(|path| !path.is_dir());

    if menu_item_button_enabled(ui, all_files, regular::PLAY, &label).clicked() {
        let paths: Vec<PathBuf> = explorer_state.selected_paths.iter().cloned().collect();
        *action = Some(ItemViewerAction::OpenWithDefault(paths));
        ui.close();
    }

    let has_file = context_paths.iter().any(|p| !p.is_dir());
    let has_folder = context_paths.iter().any(|p| p.is_dir());
    draw_custom_context_menu_group(
        ui,
        i18n,
        icon_cache,
        &settings_window.current_settings.custom_context_menu,
        has_file,
        has_folder,
        false,
        &context_paths,
        action,
    );

    ui.separator();

    if menu_item_button(ui, regular::FILE_ZIP, &i18n.tr("inputs_compress")).clicked() {
        *action = Some(ItemViewerAction::Context(ItemViewerContextAction::Compress(
            context_paths.clone(),
        )));
        ui.close();
    }

    ui.separator();

    if menu_item_button_enabled(ui, !is_cut, regular::SCISSORS, &i18n.tr("inputs_cut")).clicked() {
        *action = Some(ItemViewerAction::Context(ItemViewerContextAction::Cut(
            context_paths.clone(),
        )));
        ui.close();
    }
    if menu_item_button(ui, regular::COPY, &i18n.tr("inputs_copy")).clicked() {
        *action = Some(ItemViewerAction::Context(ItemViewerContextAction::Copy(
            context_paths.clone(),
        )));
        ui.close();
    }
    if menu_item_button(ui, regular::LINK, &i18n.tr("inputs_copy_path")).clicked() {
        *action = Some(ItemViewerAction::Context(
            ItemViewerContextAction::CopyPath(context_paths.clone()),
        ));
        ui.close();
    }
    if menu_item_button_enabled(
        ui,
        paste_enabled,
        regular::CLIPBOARD,
        &i18n.tr("inputs_paste"),
    )
    .clicked()
    {
        *action = Some(ItemViewerAction::Context(ItemViewerContextAction::Paste));
        ui.close();
    }

    if menu_item_button(ui, regular::PENCIL_SIMPLE, &i18n.tr("inputs_rename")).clicked() {
        *action = if context_paths.len() > 1 {
            Some(ItemViewerAction::Context(ItemViewerContextAction::BulkRenameRequest(
                context_paths.clone(),
            )))
        } else {
            Some(ItemViewerAction::StartEdit(file.path.clone()))
        };
        ui.close();
    }

    if menu_item_button(ui, regular::TRASH, &i18n.tr("inputs_delete")).clicked() {
        *action = Some(ItemViewerAction::Context(ItemViewerContextAction::Delete(
            context_paths.clone(),
            false,
        )));
        ui.close();
    }

    // Shift+Delete is unreliable to detect in some environments (the
    // modifier state isn't always seen at the moment the Delete key event
    // arrives), so this menu entry gives permanent delete a keyboard-free
    // path as well.
    if menu_item_button(
        ui,
        regular::TRASH,
        &i18n.tr("recycle_bin_delete_permanently"),
    )
    .clicked()
    {
        *action = Some(ItemViewerAction::Context(ItemViewerContextAction::Delete(
            context_paths.clone(),
            true,
        )));
        ui.close();
    }

    // Checksums - single real file only (no meaningful "checksum of a
    // folder"/"checksum of 3 files at once" UX).
    if context_paths.len() == 1
        && !context_paths[0].is_dir()
        && menu_item_button(ui, regular::HASH, &i18n.tr("checksum_menu_label")).clicked()
    {
        *action = Some(ItemViewerAction::Context(ItemViewerContextAction::Checksum(
            context_paths[0].clone(),
        )));
        ui.close();
    }

    // Properties (multi-select aware)
    if menu_item_button(ui, regular::INFO, &i18n.tr("properties")).clicked() {
        *action = Some(ItemViewerAction::Context(
            ItemViewerContextAction::Properties(context_paths.clone()),
        ));
        ui.close();
    }

    if settings_window
        .current_settings
        .windows_context_menu_enabled
    {
        ui.separator();
        let selected_paths = context_paths.clone();
        draw_windows_context_submenu(
            ui,
            i18n,
            _palette,
            explorer_state,
            hwnd,
            selected_paths.clone(),
            |hwnd| ShellContextMenu::for_paths(&selected_paths, hwnd),
        );
    }
}

/// Draws the "Windows menu" submenu button (toggle label + lazily-loaded,
/// scrollable item list) shared by the per-item context menu (`for_paths`,
/// via `handle_context_menu_actions`) and the folder-background context menu
/// (`for_background`, drawn directly on right-clicking empty space in the
/// item viewer). `cache_key` identifies what's currently being shown (the
/// selected paths, or `[current_dir]` for the background menu) so the cached
/// `ShellContextMenu` is only rebuilt when that changes.
pub fn draw_windows_context_submenu(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    explorer_state: &mut ExplorerState,
    hwnd: Option<HWND>,
    cache_key: Vec<PathBuf>,
    load_menu: impl FnOnce(HWND) -> windows::core::Result<ShellContextMenu>,
) {
    let toggle_label = if explorer_state.windows_context_menu_cache.is_some() {
        i18n.tr("contextmenu_hide_windows_menu_items")
    } else {
        i18n.tr("contextmenu_show_windows_menu_items")
    };

    ui.menu_button(format!("{}  {toggle_label}", regular::WINDOWS_LOGO), |ui| {
        apply_eden_visual_overrides(ui, palette);
        apply_eden_visual_color_overrides(ui, palette);
        apply_eden_text_overrides(ui, palette);

        if let Some(hwnd) = hwnd {
            let cache_miss = explorer_state
                .windows_context_menu_cache
                .as_ref()
                .map(|cache| cache.selection != cache_key)
                .unwrap_or(true);

            if cache_miss {
                explorer_state.windows_context_menu_cache = load_menu(hwnd)
                    .map(
                        |menu| crate::gui::windows::containers::structs::WindowsContextMenuCache {
                            selection: cache_key.clone(),
                            menu,
                        },
                    )
                    .map(Some)
                    .unwrap_or_else(|err| {
                        eprintln!("Windows menu load failed: {}", err);
                        None
                    });
            }

            if let Some(cache) = explorer_state.windows_context_menu_cache.as_ref() {
                if cache.menu.items().is_empty() {
                    ui.label("No Windows menu items for this selection.");
                } else {
                    let row_height = palette.text_size + 6.0;
                    let min_height = (row_height * 6.0) + (ui.spacing().item_spacing.y * 5.0);
                    let max_height = ui.ctx().viewport_rect().height() * 0.8;
                    ScrollArea::vertical()
                        .max_height(max_height)
                        .min_scrolled_height(min_height)
                        .show(ui, |ui| {
                            draw_windows_menu_items(ui, cache.menu.items(), &cache.menu, hwnd);
                        });
                }
            } else {
                ui.label(i18n.tr("contextmenu_windows_menu_unavailable"));
            }
        } else {
            ui.label(i18n.tr("contextmenu_windows_menu_available_missing"));
        }
    });
}

/// Renders one level of a `ShellContextMenu`'s items, recursing into an
/// actual nested `ui.menu_button` for each group (e.g. "7-Zip", "Send to")
/// instead of flattening the group's entries into this level - matching how
/// Explorer itself presents them.
fn draw_windows_menu_items(
    ui: &mut egui::Ui,
    items: &[ShellContextMenuItem],
    menu: &ShellContextMenu,
    hwnd: HWND,
) {
    for item in items {
        if let Some(sub_items) = &item.submenu {
            ui.add_enabled_ui(!item.disabled, |ui| {
                ui.menu_button(
                    format!("{}  {}", fallback_shell_icon(&item.label), &item.label),
                    |ui| {
                        draw_windows_menu_items(ui, sub_items, menu, hwnd);
                    },
                );
            });
            continue;
        }

        let clicked = if let Some((rgba, w, h)) = &item.icon_rgba {
            let texture = ui.ctx().load_texture(
                format!("shellmenu_icon_{}", item.id),
                egui::ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], rgba),
                egui::TextureOptions::default(),
            );

            ui.add_enabled(
                !item.disabled,
                egui::Button::image_and_text(
                    egui::Image::new(&texture).fit_to_exact_size(egui::vec2(16.0, 16.0)),
                    &item.label,
                ),
            )
            .clicked()
        } else {
            menu_item_button_enabled(
                ui,
                !item.disabled,
                fallback_shell_icon(&item.label),
                &item.label,
            )
            .clicked()
        };

        if clicked {
            if let Err(err) = menu.invoke(hwnd, item.id) {
                eprintln!("Windows menu invoke failed: {}", err);
            }
            ui.close();
        }
    }
}

pub fn handle_draw_col_name(
    ui: &mut egui::Ui,
    i18n: &I18n,
    file: &FileItem,
    layout: &ItemViewerLayout,
    icon_cache: &IconCache,
    is_selected: bool,
    is_cut: bool,
    palette: &ThemePalette,
    font_id: &egui::FontId,
    rename_state: &mut Option<RenameState>,
    show_item_viewer_icons: bool,
) -> Option<ItemViewerAction> {
    const TEXT_LEFT_PADDING: f32 = 2.0;
    const ICON_HORIZONTAL_PADDING: f32 = 2.0;

    let available_width = ui.available_width();

    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(available_width, layout.row_height),
        egui::Sense::hover(),
    );

    // Reserve a fixed area for the icon.
    let icon_size = egui::vec2(layout.icon_size, layout.icon_size);
    let icon_area_width = icon_size.x + ICON_HORIZONTAL_PADDING * 2.0;

    let text_offset_x = if show_item_viewer_icons {
        let icon_area =
            egui::Rect::from_min_size(rect.min, egui::vec2(icon_area_width, layout.row_height));

        let icon_color = if is_cut {
            palette.icon_colored_hover.linear_multiply(0.5)
        } else {
            palette.icon_colored_hover
        };

        // Use a custom Phosphor icon for recognized folder names.
        if let Some(glyph) = icon_cache.get_custom_folder_icon(&file.path, file.is_dir) {
            let font_id = egui::FontId::new(icon_size.x, egui::FontFamily::Proportional);

            ui.painter().text(
                icon_area.center(),
                egui::Align2::CENTER_CENTER,
                glyph,
                font_id,
                icon_color,
            );
        } else if let Some(icon) = icon_cache.get(&file.path, file.is_dir) {
            let icon_pos = egui::pos2(
                icon_area.center().x - icon_size.x * 0.5,
                icon_area.center().y - icon_size.y * 0.5,
            );

            ui.painter().image(
                (&icon).into(),
                egui::Rect::from_min_size(icon_pos, icon_size),
                egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1.0, 1.0)),
                icon_color,
            );
        }

        icon_area_width
    } else {
        TEXT_LEFT_PADDING
    };

    let text_rect =
        egui::Rect::from_min_max(egui::pos2(rect.min.x + text_offset_x, rect.min.y), rect.max);

    let editing_path = rename_state.as_ref().map(|rs| rs.path.clone());

    if let Some(path) = editing_path {
        if path == file.path {
            return handle_editing_file_name(
                ui,
                i18n,
                file,
                is_selected,
                palette,
                text_rect,
                rename_state,
            );
        }
    }

    let text_width = available_width - text_offset_x;
    let color = get_text_color(is_selected, is_cut, palette);

    let (display_name, _) = truncate_item_text(ui, &file.name, text_width, font_id, color);

    ui.painter().text(
        egui::pos2(rect.min.x + text_offset_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        display_name,
        font_id.clone(),
        color,
    );

    None
}

pub fn handle_draw_col_type(
    ui: &mut egui::Ui,
    file: &FileItem,
    layout: &ItemViewerLayout,
    is_selected: bool,
    is_cut: bool,
    palette: &ThemePalette,
    font_id: &egui::FontId,
    file_type_cache: &mut HashMap<String, String>,
) {
    let color = get_text_color(is_selected, is_cut, palette);

    let type_text = if file.is_dir {
        "Folder"
    } else if let Some(ext) = file.path.extension().and_then(|ext| ext.to_str()) {
        get_file_type_name(ext, file_type_cache)
    } else {
        get_file_type_name("", file_type_cache)
    };

    draw_table_text(ui, layout, type_text, font_id, color);
}

pub fn handle_draw_col_size(
    ui: &mut egui::Ui,
    file: &FileItem,
    layout: &ItemViewerLayout,
    folder_sizes: &HashMap<PathBuf, ItemViewerFolderSizeState>,
    is_selected: bool,
    is_cut: bool,
    palette: &ThemePalette,
    font_id: &egui::FontId,
    file_size_text_cache: &mut HashMap<PathBuf, (u64, String)>,
    folder_size_text_cache: &mut HashMap<PathBuf, (u64, bool, String)>,
    drive_size_text_cache: &mut HashMap<PathBuf, (u64, u64, String)>,
) {
    let text_color = get_text_color(is_selected, is_cut, palette);

    if let (Some(total), Some(free)) = (file.total_space, file.free_space) {
        let key = &file.path;
        let text = if let Some((cached_total, cached_free, cached_text)) =
            drive_size_text_cache.get(key)
        {
            if *cached_total == total && *cached_free == free {
                cached_text.as_str()
            } else {
                ""
            }
        } else {
            ""
        };

        let display_text = if text.is_empty() {
            let formatted = format!("{} / {}", format_size(free), format_size(total));
            drive_size_text_cache.insert(file.path.clone(), (total, free, formatted));
            drive_size_text_cache
                .get(key)
                .map(|(_, _, t)| t.as_str())
                .unwrap_or("")
        } else {
            text
        };

        draw_table_text(ui, layout, display_text, font_id, text_color);

        return;
    }

    if file.is_dir {
        if let Some(state) = folder_sizes.get(&file.path) {
            let cached = folder_size_text_cache.get(&file.path);
            let text = match cached {
                Some((bytes, done, value)) if *bytes == state.bytes && *done == state.done => {
                    value.as_str()
                }
                _ => {
                    let label = format_size(state.bytes);
                    let value = if state.done {
                        label
                    } else {
                        format!("⏳ {}", label)
                    };
                    folder_size_text_cache
                        .insert(file.path.clone(), (state.bytes, state.done, value));
                    folder_size_text_cache
                        .get(&file.path)
                        .map(|(_, _, v)| v.as_str())
                        .unwrap_or("")
                }
            };

            draw_table_text(ui, layout, text, font_id, text_color);
        } else {
            draw_table_text(ui, layout, "—", font_id, text_color);
        }

        return;
    }

    if let Some(size) = file.file_size {
        let cached = file_size_text_cache.get(&file.path);
        let text = match cached {
            Some((cached_size, value)) if *cached_size == size => value.as_str(),
            _ => {
                let value = format_size(size);
                file_size_text_cache.insert(file.path.clone(), (size, value));
                file_size_text_cache
                    .get(&file.path)
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("")
            }
        };
        draw_table_text(ui, layout, text, font_id, text_color);
    } else {
        draw_table_text(ui, layout, "—", font_id, text_color);
    }
}

pub fn handle_draw_col_modified(
    ui: &mut egui::Ui,
    file: &FileItem,
    layout: &ItemViewerLayout,
    is_selected: bool,
    is_cut: bool,
    palette: &ThemePalette,
    font_id: &egui::FontId,
) {
    if layout.is_drive_view {
        if let (Some(total), Some(free)) = (file.total_space, file.free_space) {
            let bar_height = layout.row_height * 0.85;
            let vertical_padding = (layout.row_height - bar_height) * 0.5;
            ui.add_space(vertical_padding);
            drive_usage_bar(ui, total, free, bar_height, palette);
        } else {
            draw_table_text(
                ui,
                layout,
                "—",
                font_id,
                get_text_color(is_selected, is_cut, palette),
            );
        }
    } else {
        let color = get_text_color(is_selected, is_cut, palette);

        if let Some(m) = &file.modified_time {
            draw_table_text(ui, layout, m, font_id, color);
        } else {
            draw_table_text(ui, layout, "—", font_id, color);
        }
    }
}

pub fn handle_draw_col_created(
    ui: &mut egui::Ui,
    file: &FileItem,
    layout: &ItemViewerLayout,
    is_selected: bool,
    is_cut: bool,
    palette: &ThemePalette,
    font_id: &egui::FontId,
) {
    let color = get_text_color(is_selected, is_cut, palette);

    draw_table_text(
        ui,
        layout,
        file.created_time.as_deref().unwrap_or("—"),
        font_id,
        color,
    );
}

pub fn handle_draw_col_deleted(
    ui: &mut egui::Ui,
    file: &FileItem,
    layout: &ItemViewerLayout,
    is_selected: bool,
    is_cut: bool,
    palette: &ThemePalette,
    font_id: &egui::FontId,
) {
    let color = get_text_color(is_selected, is_cut, palette);

    draw_table_text(
        ui,
        layout,
        file.deleted_time.as_deref().unwrap_or("—"),
        font_id,
        color,
    );
}

pub fn handle_draw_col_original_directory(
    ui: &mut egui::Ui,
    file: &FileItem,
    layout: &ItemViewerLayout,
    is_selected: bool,
    is_cut: bool,
    palette: &ThemePalette,
    font_id: &egui::FontId,
) {
    let color = get_text_color(is_selected, is_cut, palette);

    draw_table_text(
        ui,
        layout,
        file.original_directory.as_deref().unwrap_or("—"),
        font_id,
        color,
    );
}

/// Draws every tag a file belongs to as a small colored chip, in a single
/// row clipped to the column's own width, with a trailing "+N" chip for
/// whatever doesn't fit - the same overflow idiom the paste-conflict modal
/// already uses. Chip styling (fill/stroke from the tag's own color) matches
/// the toggle buttons in `draw_tag_picker_popup` for visual consistency.
pub fn handle_draw_col_tags(
    ui: &mut egui::Ui,
    file: &FileItem,
    layout: &ItemViewerLayout,
    tags_state: &TagsState,
    font_id: &egui::FontId,
) {
    let tags = tags_state.tags_for_path(&file.path);
    let (rect, _response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), layout.row_height),
        egui::Sense::hover(),
    );

    if tags.is_empty() {
        return;
    }

    // Chips use a smaller font than the row's own text - a badge/pill is
    // meant to read as a compact label, and a smaller font also means each
    // chip's own pill needs less width to fit a given tag name, which
    // reduces how often a single long tag name has to be truncated below.
    let chip_font_id = egui::FontId::new((font_id.size - 2.0).max(9.0), font_id.family.clone());
    let chip_gap = 4.0;
    let chip_pad_x = 6.0;
    let chip_height = (layout.row_height - 6.0).max(14.0);
    let mut cursor_x = rect.left();
    let available_right = rect.right();
    // A single chip is never allowed to claim more than this much width,
    // truncated with an ellipsis via `truncate_item_text` otherwise - this
    // is what actually prevents a long tag name from overflowing its own
    // pill or the column itself, rather than relying on the column always
    // being wide enough.
    const MAX_CHIP_TEXT_WIDTH: f32 = 90.0;

    for (visible_count, (_, name, color)) in tags.iter().enumerate() {
        let remaining = tags.len() - visible_count;

        // Reserve room for a trailing "+N" chip unless this is the very
        // last tag (which never needs an overflow indicator after it).
        let needs_overflow_room = remaining > 1;
        let overflow_reserve = if needs_overflow_room { 34.0 } else { 0.0 };

        let remaining_width = (available_right - overflow_reserve - cursor_x).max(0.0);
        let text_budget = MAX_CHIP_TEXT_WIDTH.min((remaining_width - chip_pad_x * 2.0).max(0.0));

        let (display_name, _truncated) =
            truncate_item_text(ui, name, text_budget, &chip_font_id, egui::Color32::WHITE);

        let painter = ui.painter_at(rect);
        let text_width = painter
            .layout_no_wrap(display_name.clone(), chip_font_id.clone(), egui::Color32::WHITE)
            .size()
            .x;
        let chip_width = text_width + chip_pad_x * 2.0;

        if (cursor_x + chip_width > available_right || text_budget <= 0.0) && visible_count > 0 {
            let overflow_label = format!("+{}", remaining);
            let overflow_text_width = painter
                .layout_no_wrap(overflow_label.clone(), chip_font_id.clone(), egui::Color32::WHITE)
                .size()
                .x;
            let overflow_width = overflow_text_width + chip_pad_x * 2.0;
            let chip_rect = egui::Rect::from_min_size(
                egui::pos2(cursor_x, rect.center().y - chip_height / 2.0),
                egui::vec2(overflow_width, chip_height),
            );
            painter.rect_filled(
                chip_rect,
                egui::CornerRadius::same(6),
                egui::Color32::GRAY.gamma_multiply(0.25),
            );
            painter.text(
                chip_rect.center(),
                egui::Align2::CENTER_CENTER,
                overflow_label,
                chip_font_id.clone(),
                egui::Color32::WHITE,
            );
            return;
        }

        let chip_rect = egui::Rect::from_min_size(
            egui::pos2(cursor_x, rect.center().y - chip_height / 2.0),
            egui::vec2(chip_width, chip_height),
        );
        painter.rect_filled(
            chip_rect,
            egui::CornerRadius::same(6),
            color.gamma_multiply(0.25),
        );
        painter.rect_stroke(
            chip_rect,
            egui::CornerRadius::same(6),
            egui::Stroke::new(1.0, color.gamma_multiply(0.6)),
            egui::StrokeKind::Inside,
        );
        painter.text(
            chip_rect.center(),
            egui::Align2::CENTER_CENTER,
            display_name,
            chip_font_id.clone(),
            egui::Color32::WHITE,
        );

        cursor_x = chip_rect.right() + chip_gap;
    }
}

pub fn get_text_color(is_selected: bool, is_cut: bool, palette: &ThemePalette) -> egui::Color32 {
    let base_color = get_row_color(is_selected, palette);
    if is_cut {
        base_color.linear_multiply(0.5)
    } else {
        base_color
    }
}

fn get_row_color(
    is_multi_selected: bool,
    palette: &crate::gui::theme::ThemePalette,
) -> egui::Color32 {
    if is_multi_selected {
        palette.item_viewer_row_text_selected
    } else {
        palette.text_normal
    }
}

pub fn handle_editing_file_name(
    ui: &mut egui::Ui,
    i18n: &I18n,
    file: &FileItem,
    is_selected: bool,
    palette: &ThemePalette,
    text_rect: egui::Rect,
    rename_state: &mut Option<RenameState>,
) -> Option<ItemViewerAction> {
    let Some(rename_state) = rename_state else {
        return None;
    };

    if rename_state.path != file.path {
        return None;
    }

    let mut action: Option<ItemViewerAction> = None;
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(text_rect));

    child_ui.scope(|ui| {
        let visuals = ui.visuals_mut();

        let bg = if is_selected {
            palette.row_selected_bg
        } else {
            palette.row_bg
        };

        visuals.widgets.inactive.bg_fill = bg;
        visuals.widgets.hovered.bg_fill = bg;
        visuals.widgets.active.bg_fill = bg;
        visuals.widgets.inactive.bg_stroke.width = 0.0;
        visuals.widgets.hovered.bg_stroke.width = 0.0;
        visuals.widgets.active.bg_stroke.width = 0.0;

        visuals.override_text_color = Some(get_row_color(is_selected, palette));

        let edit_id = ui.id().with("rename_input").with(&file.path);

        // Store original length to detect changes
        let original_len = rename_state.new_name.len();

        // Matching Windows Explorer: the base filename is pre-selected so
        // typing immediately replaces it, but the ".ext" is left unselected
        // (and out of the way) so a plain Enter doesn't clobber it. Folders
        // have no extension concept, so their whole name is selected.
        //
        // Both the cursor range AND the focus request are set *before*
        // `ui.add()` runs (rather than requesting focus afterward, which
        // would only take effect starting next frame - a visible one-frame
        // flash of the full, unselected name before the highlight caught
        // up). Setting both here means the widget picks up already focused,
        // with the right selection, on the very first frame it's drawn.
        let just_requested_focus = rename_state.should_focus;
        if just_requested_focus {
            let select_end = if file.is_dir {
                rename_state.new_name.chars().count()
            } else {
                std::path::Path::new(&rename_state.new_name)
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().chars().count())
                    .unwrap_or_else(|| rename_state.new_name.chars().count())
            };
            let mut state = egui::widgets::text_edit::TextEditState::load(ui.ctx(), edit_id)
                .unwrap_or_default();
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(select_end),
            )));
            state.store(ui.ctx(), edit_id);
            ui.memory_mut(|mem| mem.request_focus(edit_id));
        }

        let edit_response = ui.add(
            egui::TextEdit::singleline(&mut rename_state.new_name)
                .id(edit_id)
                .desired_width(f32::INFINITY)
                .font(FontId::new(palette.text_size, FontFamily::Proportional)),
        );

        // ✅ Focus once
        if rename_state.should_focus {
            edit_response.request_focus();
            if edit_response.has_focus() {
                rename_state.should_focus = false;
            }
        }

        // Real-time character validation
        if rename_state.new_name.len() != original_len {
            // Use the real-time validation function for each character typed
            if !filename_has_valid_characters_realtime(&rename_state.new_name) {
                // Remove invalid characters by keeping only valid ones
                let invalid_chars = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
                let mut cleaned_name = String::new();

                for ch in rename_state.new_name.chars() {
                    if !invalid_chars.contains(&ch) {
                        cleaned_name.push(ch);
                    }
                }

                // Check for reserved names
                let reserved_names = [
                    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6",
                    "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7",
                    "LPT8", "LPT9",
                ];

                let name_upper = cleaned_name.to_uppercase();
                for reserved in &reserved_names {
                    if name_upper == *reserved {
                        cleaned_name.clear(); // Clear the invalid reserved name
                    }
                }

                rename_state.new_name = cleaned_name;
                rename_state.validation_error_show = true; // Show error popup
            } else {
                // If valid, clear any existing error
                if rename_state.validation_error_show {
                    rename_state.validation_error_show = false;
                }
            }
        }

        // Show validation tooltip if error flag is set
        if rename_state.validation_error_show {
            let tooltip_text = format!(
                "{}{}{}{}{}{}{}",
                &i18n.tr("tooltip_rename_invalid_text1"),
                "\n",
                &i18n.tr("tooltip_rename_invalid_text2"),
                "\n",
                &i18n.tr("tooltip_rename_invalid_text3"),
                "\n",
                &i18n.tr("tooltip_rename_invalid_text4")
            );

            // Calculate position above the input field
            let popup_pos = egui::pos2(edit_response.rect.left(), edit_response.rect.top() - 60.0);

            // Show error message positioned above the input field
            egui::Area::new(ui.id().with("error_popup"))
                .pivot(egui::Align2::LEFT_BOTTOM)
                .current_pos(popup_pos)
                .show(ui.ctx(), |ui| {
                    ui.set_min_width(350.0);
                    egui::Frame::popup(ui.style())
                        .fill(egui::Color32::from_rgb(40, 40, 40))
                        .stroke(egui::Stroke::new(1.0, egui::Color32::RED))
                        .show(ui, |ui| {
                            ui.add_space(8.0);
                            ui.vertical_centered(|ui| {
                                ui.colored_label(egui::Color32::RED, tooltip_text);
                            });
                            ui.add_space(8.0);
                        });
                });

            // TODO: Add Windows alert sound when API compatibility is resolved
        }

        // ✅ Input handling (same pattern as tabs)
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));

        if enter {
            let new_name = rename_state.new_name.trim().to_string();

            // Clear validation error on successful action
            rename_state.validation_error_show = false;

            action = Some(ItemViewerAction::Context(
                ItemViewerContextAction::RenameRequest(file.path.clone(), new_name),
            ));
        } else if escape {
            // Clear validation error on cancel
            rename_state.validation_error_show = false;

            action = Some(ItemViewerAction::Context(
                ItemViewerContextAction::RenameCancel,
            ));
        } else if edit_response.lost_focus() && !just_requested_focus {
            // Clear validation error on focus loss
            rename_state.validation_error_show = false;

            // Clicking away confirms the rename with whatever's currently
            // typed (same as pressing Enter) - only Escape explicitly
            // cancels.
            //
            // `!just_requested_focus` guards the very frame the rename box
            // was created and asked for focus (e.g. right after clicking the
            // "New Folder"/"New File" toolbar icon): the click that
            // triggered creation can otherwise register as a same-frame
            // "lost focus" against this brand-new widget, which would
            // submit the un-typed placeholder name before the user ever
            // gets to type.
            let new_name = rename_state.new_name.trim().to_string();
            action = Some(ItemViewerAction::Context(
                ItemViewerContextAction::RenameRequest(file.path.clone(), new_name),
            ));
        }
    });

    action
}

pub fn handle_global_actions(
    ui: &mut egui::Ui,
    files: &[FileItem],
    palette: &ThemePalette,
    tabbar_action: &mut Option<ItemViewerNavBarAction>,
    rename_state: &mut Option<RenameState>,
    filter_state: &mut FilterState,
    drag_state: &mut DragState,
    explorer_state: &mut ExplorerState,
    is_cut_mode: bool,
    is_drive_view: bool,
    is_recycle_bin_view: bool,
    find_in_preview_active: bool,
    _settings_windows: &mut SettingsWindow,
) -> Option<ItemViewerAction> {
    let filtered_indices = &filter_state.cached_indices;
    let mut action: Option<ItemViewerAction> = None;

    let is_text_edit_active = tabbar_action
        .as_ref()
        .is_some_and(|t| t.is_breadcrumb_path_edit_active);

    // The preview pane's own Find bar (`itemviewer_preview.rs`) is drawn
    // *after* this function each frame (this one runs first, at the top of
    // `draw_item_viewer`), and it requests keyboard focus for its query
    // field the same frame it opens - but `egui::Context::memory().focused()`
    // is a snapshot from whatever last claimed it (typically the *previous*
    // frame's draw), so relying on `ctx.egui_wants_keyboard_input()` alone
    // to gate the type-to-filter capture below raced against exactly that:
    // the very first keystroke after opening the Find bar could still land
    // here instead, since this function's read of "is anything focused"
    // happens before the Find bar has had a chance to (re)claim focus this
    // frame. Checking the caller-supplied `find_in_preview_active` flag
    // directly - true for the Find bar's entire active lifetime, not just
    // a focus snapshot - closes that race outright rather than depending on
    // frame-ordering being lucky.
    if rename_state.is_some() || is_text_edit_active || find_in_preview_active {
        return None;
    }

    if is_cut_mode {
        let cancel_called = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if cancel_called {
            clear_clipboard_files();
        }
    }

    if drag_state.active && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        drag_state.active = false;
        drag_state.source_items.clear();
        drag_state.start_pos = None;
    }

    let mut set_nav_action = |nav: ItemViewerNavAction| {
        if let Some(existing) = tabbar_action.as_mut() {
            existing.nav = Some(nav);
        } else {
            *tabbar_action = Some(ItemViewerNavBarAction {
                nav: Some(nav),
                ..Default::default()
            });
        }
    };

    if filter_state.active {
        let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));

        if cancel {
            let text_edit_id = ui.id().with("filter_input");
            ui.memory_mut(|mem| {
                mem.data
                    .remove::<egui::text_edit::TextEditState>(text_edit_id)
            });
            *filter_state = FilterState::default();
            return None;
        }

        let text_edit_id = ui.id().with("filter_input");

        let response = egui::Frame::NONE
            .fill(palette.input_field_bg)
            // Full-strength, opaque `borders_active` rather than the
            // low-alpha `borders_default` blend most other bordered
            // surfaces use: this box's fill is `input_field_bg`, and a
            // translucent accent blended over its own fill color reads as
            // barely-there regardless of stroke width - confirmed by
            // pixel-sampling a live screenshot, where even a 2.5px stroke
            // at a doubled blend alpha was indistinguishable from the fill
            // except right at the anti-aliased edge. An opaque, undiluted
            // accent color plus a genuinely thicker stroke is what actually
            // reads as a real border here.
            .stroke(egui::Stroke::new(2.0, palette.borders_active))
            .corner_radius(egui::CornerRadius::same(palette.medium_radius))
            .inner_margin(egui::Margin {
                left: 10,
                right: 10,
                top: 6,
                bottom: 6,
            })
            // Space *outside* the box's own border - between the box and
            // whatever's around it (the item viewer's edge above/left, the
            // file list below) - as opposed to `inner_margin`, which is the
            // padding between the border and the text inside it. Right is
            // deliberately left at 0 - the box already reads as anchored to
            // the top-left corner of the view, and adding space on its own
            // open side would just look like an unexplained gap.
            .outer_margin(egui::Margin {
                left: 6,
                right: 0,
                top: 6,
                bottom: 8,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(regular::MAGNIFYING_GLASS)
                            .size(palette.text_size + 1.0)
                            .color(palette.icon_color),
                    );
                    ui.add_space(4.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut filter_state.query)
                            .id(text_edit_id)
                            .frame(egui::Frame::NONE)
                            .desired_width(200.0)
                            .font(FontId::new(
                                palette.text_size,
                                egui::FontFamily::Proportional,
                            )),
                    )
                })
                .inner
            })
            .inner;

        if !filter_state.focus_requested {
            response.request_focus();
            filter_state.focus_requested = true;
        }

        if response.clicked_elsewhere() {
            // Check if click is within the item viewer area (table)
            let click_pos = ui.input(|i| i.pointer.interact_pos());
            let should_clear_filter = if let Some(pos) = click_pos {
                let item_viewer_rect = ui.available_rect_before_wrap();
                // Don't clear filter if clicking within the item viewer area
                !item_viewer_rect.contains(pos)
            } else {
                // If no click position, clear filter (fallback behavior)
                true
            };

            if should_clear_filter {
                ui.memory_mut(|mem| {
                    mem.data
                        .remove::<egui::text_edit::TextEditState>(text_edit_id)
                });
                *filter_state = FilterState::default();
            }
        }

        return None;
    }

    ui.input(|i| {
        for event in &i.events {
            match event {
                egui::Event::Copy => {
                    if !is_recycle_bin_view
                        && !is_drive_view
                        && !explorer_state.selected_paths.is_empty()
                    {
                        action = Some(ItemViewerAction::Context(ItemViewerContextAction::Copy(
                            explorer_state.selected_paths.iter().cloned().collect(),
                        )));
                    }
                }
                egui::Event::Cut => {
                    if !is_drive_view && !explorer_state.selected_paths.is_empty() {
                        action = Some(ItemViewerAction::Context(ItemViewerContextAction::Cut(
                            explorer_state.selected_paths.iter().cloned().collect(),
                        )));
                    }
                }
                _ => {}
            }
        }
    });
    ui.input(|i| {
        let alt = i.modifiers.alt;

        if i.pointer.button_pressed(egui::PointerButton::Extra1)
            || (alt && i.key_pressed(egui::Key::ArrowLeft))
            || i.key_pressed(egui::Key::Backspace)
        {
            set_nav_action(ItemViewerNavAction::Back);
        }
        if i.pointer.button_pressed(egui::PointerButton::Extra2)
            || (alt && i.key_pressed(egui::Key::ArrowRight))
        {
            set_nav_action(ItemViewerNavAction::Forward);
        }
        if alt && i.key_pressed(egui::Key::ArrowUp) {
            set_nav_action(ItemViewerNavAction::Up);
        }

        if !alt && i.key_pressed(egui::Key::Enter) {
            if is_recycle_bin_view {
                return;
            }
            let selected_paths: Vec<PathBuf> = explorer_state
                .selected_paths
                .iter()
                .filter_map(|p| {
                    files
                        .iter()
                        .find(|f| &f.path == p && !f.is_dir)
                        .map(|_| p.clone())
                })
                .collect();

            if !selected_paths.is_empty() {
                action = Some(ItemViewerAction::OpenWithDefault(selected_paths));
            }

            // Optionally handle directories separately:
            for dir_path in explorer_state
                .selected_paths
                .iter()
                .filter(|p| files.iter().any(|f| &f.path == *p && f.is_dir))
            {
                action = Some(ItemViewerAction::Open(dir_path.clone()));
            }
        }
        if i.modifiers.command && i.key_pressed(egui::Key::A) {
            action = Some(ItemViewerAction::SelectAll);
        }
        if i.modifiers.command
            && i.key_released(egui::Key::V)
            && !is_recycle_bin_view
            && !is_drive_view
        {
            // Any other key functions won't work with egui v0.33.x
            action = Some(ItemViewerAction::Context(ItemViewerContextAction::Paste));
        }
        if i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::C) {
            if is_recycle_bin_view || is_drive_view {
                return;
            }
            // Copy path shortcut - only enabled when exactly one item is selected
            if explorer_state.selected_paths.len() == 1 {
                let path = explorer_state.selected_paths.iter().next().unwrap().clone();
                action = Some(ItemViewerAction::Context(
                    ItemViewerContextAction::CopyPath(vec![path]),
                ));
            }
        }
        if i.key_pressed(egui::Key::Delete) {
            if is_drive_view {
                return;
            }
            let paths: Vec<PathBuf> = if !explorer_state.selected_paths.is_empty() {
                explorer_state.selected_paths.iter().cloned().collect()
            } else if !filtered_indices.is_empty() {
                vec![files[filtered_indices[0]].path.clone()]
            } else {
                return;
            };

            // Shift+Delete skips the Recycle Bin and deletes permanently,
            // matching native Explorer; a plain Delete always goes to the
            // Recycle Bin (deleting while already inside the Recycle Bin is
            // handled separately and is already permanent either way).
            let permanent = i.modifiers.shift || is_recycle_bin_view;
            action = Some(ItemViewerAction::Context(ItemViewerContextAction::Delete(
                paths, permanent,
            )));
        }
    });

    let mut start_filter = String::new();

    // Typed-character events are broadcast to every reader of `ctx.input()`
    // for the frame, not just whichever widget actually has focus - so
    // without this check, typing into an unrelated text field elsewhere
    // (e.g. a name field in a popup menu) would *also* start/append to this
    // "type to filter" search.
    if ui.ctx().egui_wants_keyboard_input() {
        return action;
    }

    ui.input(|i| {
        if i.modifiers.command || i.modifiers.ctrl || i.modifiers.alt {
            return;
        }
        for event in &i.events {
            if let egui::Event::Text(text) = event {
                if text.chars().all(|c| !c.is_control()) {
                    start_filter.push_str(text);
                }
            }
        }
    });

    if !start_filter.is_empty() {
        filter_state.active = true;
        filter_state.query.push_str(&start_filter);
        filter_state.last_input_time = ui.input(|i| i.time);
        return None;
    }

    action
}

pub fn draw_item_viewer_header(
    i18n: &I18n,
    header: &mut egui_extras::TableRow<'_, '_>,
    is_drive_view: bool,
    is_recycle_bin_view: bool,
    show_checkboxes: bool,
    ordered_columns: &[ItemViewerHeaderColumn],
    filtered_indices: &[usize],
    files: &[FileItem],
    sort_keys: &[SortKey],
    palette: &crate::gui::theme::ThemePalette,
    explorer_state: &mut ExplorerState,
    column_state: &ItemViewerColumnState,
) -> Option<ItemViewerAction> {
    let font_id = FontId::new(palette.text_size, FontFamily::Proportional);
    let mut action: Option<ItemViewerAction> = None;

    if show_checkboxes {
        header.col(|ui| {
            // Match the top padding added to the header row's height in
            // `compute_layout` (the cell's layout is top-down, so without
            // this the extra height ends up below the content, not above).
            ui.add_space(HEADER_TOP_PADDING);
            let mut all_selected = !filtered_indices.is_empty()
                && filtered_indices
                    .iter()
                    .all(|&i| explorer_state.selected_paths.contains(&files[i].path));

            ui.scope(|ui| {
                apply_checkbox_colors(ui, palette, all_selected);
                if draw_checkbox(ui, palette, &mut all_selected, "select_all").clicked() {
                    action = Some(if all_selected {
                        ItemViewerAction::SelectAll
                    } else {
                        ItemViewerAction::DeselectAll
                    });
                }
            });
        });
    }

    for &column in ordered_columns {
        let label = match column {
            ItemViewerHeaderColumn::Name => i18n.tr("explorer_cols_name"),
            ItemViewerHeaderColumn::OriginalDirectory => {
                i18n.tr("explorer_cols_original_directory")
            }
            ItemViewerHeaderColumn::Type => i18n.tr("explorer_cols_type"),
            ItemViewerHeaderColumn::Size => i18n.tr("explorer_cols_size"),
            ItemViewerHeaderColumn::Modified => i18n.tr("explorer_cols_modified"),
            ItemViewerHeaderColumn::Created => i18n.tr("explorer_cols_created"),
            ItemViewerHeaderColumn::Usage => i18n.tr("explorer_cols_usage"),
            ItemViewerHeaderColumn::Deleted => i18n.tr("explorer_cols_deleted"),
            ItemViewerHeaderColumn::Tags => i18n.tr("explorer_cols_tags"),
        };
        draw_header_cell(
            header,
            i18n,
            palette,
            &font_id,
            sort_keys,
            column,
            label,
            &mut action,
            column_state,
            is_drive_view,
            is_recycle_bin_view,
        );
    }

    action
}

fn draw_header_cell(
    header: &mut egui_extras::TableRow<'_, '_>,
    i18n: &I18n,
    palette: &crate::gui::theme::ThemePalette,
    font_id: &FontId,
    sort_keys: &[SortKey],
    column: ItemViewerHeaderColumn,
    label: String,
    action: &mut Option<ItemViewerAction>,
    column_state: &ItemViewerColumnState,
    is_drive_view: bool,
    is_recycle_bin_view: bool,
) {
    let column_action = match column {
        ItemViewerHeaderColumn::Name => Some(SortColumn::Name),
        ItemViewerHeaderColumn::OriginalDirectory if is_recycle_bin_view => {
            Some(SortColumn::OriginalDirectory)
        }
        ItemViewerHeaderColumn::Type => Some(SortColumn::Type),
        ItemViewerHeaderColumn::Size => Some(SortColumn::Size),
        ItemViewerHeaderColumn::Modified if !is_drive_view => Some(SortColumn::Modified),
        ItemViewerHeaderColumn::Created if !is_drive_view => Some(SortColumn::Created),
        ItemViewerHeaderColumn::Deleted if is_recycle_bin_view => Some(SortColumn::Deleted),
        _ => None,
    };

    header.col(|ui| {
        // Match the top padding added to the header row's height in
        // `compute_layout` (the cell's layout is top-down, so without
        // this the extra height ends up below the text, not above).
        ui.add_space(HEADER_TOP_PADDING);
        let cell_id = ui.id().with(("itemviewer_header_cell", column));
        let cell_resp = ui.interact(ui.max_rect(), cell_id, egui::Sense::click());
        let arrow = column_action
            .and_then(|column| sort_keys.iter().find(|key| key.column == column))
            .map(|key| {
                if key.ascending {
                    regular::CARET_UP
                } else {
                    regular::CARET_DOWN
                }
            })
            .unwrap_or("");
        let sort_label = label;

        ui.add(
            egui::Label::new(
                egui::RichText::new(format!("{sort_label} {arrow}").trim_end())
                    .font(font_id.clone())
                    .size(palette.text_size)
                    .color(palette.text_header_section),
            )
            .selectable(false),
        );

        if cell_resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
        }
        if cell_resp.clicked()
            && let Some(col) = column_action
        {
            let modifiers = ui.input(|input| input.modifiers);
            *action = Some(ItemViewerAction::Sort {
                column: col,
                additive: modifiers.shift,
                remove: modifiers.ctrl,
            });
        }
        let order = column_state.order(is_drive_view, is_recycle_bin_view);
        let order_index = order.iter().position(|c| *c == column);

        Popup::context_menu(&cell_resp)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                apply_eden_text_overrides(ui, palette);
                draw_header_context_menu(
                    ui,
                    i18n,
                    column,
                    column_state,
                    action,
                    is_drive_view,
                    is_recycle_bin_view,
                    order_index,
                    order.len(),
                );
            });
    });
}

fn draw_header_context_menu(
    ui: &mut egui::Ui,
    i18n: &I18n,
    clicked_column: ItemViewerHeaderColumn,
    column_state: &ItemViewerColumnState,
    action: &mut Option<ItemViewerAction>,
    is_drive_view: bool,
    is_recycle_bin_view: bool,
    order_index: Option<usize>,
    order_len: usize,
) {
    if ui
        .button(i18n.tr("itemviewer_size_column_to_fit"))
        .clicked()
    {
        *action = Some(ItemViewerAction::FitColumn(clicked_column));
        ui.close();
    }

    if ui
        .button(i18n.tr("itemviewer_size_all_columns_to_fit"))
        .clicked()
    {
        *action = Some(ItemViewerAction::FitAllColumns);
        ui.close();
    }

    ui.separator();

    if clicked_column != ItemViewerHeaderColumn::Name {
        let can_move_left = order_index.is_some_and(|idx| idx > 0);
        let can_move_right = order_index.is_some_and(|idx| idx + 1 < order_len);

        if ui
            .add_enabled(can_move_left, egui::Button::new("Move left"))
            .clicked()
        {
            *action = Some(ItemViewerAction::MoveColumnLeft(clicked_column));
            ui.close();
        }

        if ui
            .add_enabled(can_move_right, egui::Button::new("Move right"))
            .clicked()
        {
            *action = Some(ItemViewerAction::MoveColumnRight(clicked_column));
            ui.close();
        }

        if ui
            .add_enabled(can_move_left, egui::Button::new("Move to start"))
            .clicked()
        {
            *action = Some(ItemViewerAction::MoveColumnToStart(clicked_column));
            ui.close();
        }

        if ui
            .add_enabled(can_move_right, egui::Button::new("Move to end"))
            .clicked()
        {
            *action = Some(ItemViewerAction::MoveColumnToEnd(clicked_column));
            ui.close();
        }

        ui.separator();
    }

    // Name is always visible.
    let mut name_checked = true;
    ui.add_enabled(
        false,
        egui::Checkbox::new(&mut name_checked, i18n.tr("explorer_cols_name")),
    );

    if is_recycle_bin_view {
        ui.add_enabled(
            false,
            egui::Checkbox::new(&mut true, i18n.tr("explorer_cols_original_directory")),
        );
    }

    let type_visible = column_state.is_column_visible(
        is_drive_view,
        is_recycle_bin_view,
        ItemViewerHeaderColumn::Type,
    );

    if draw_visibility_checkbox(ui, i18n.tr("explorer_cols_type"), type_visible) {
        *action = Some(ItemViewerAction::ToggleColumnVisibility(
            ItemViewerHeaderColumn::Type,
        ));
        ui.close();
    }

    let size_visible = column_state.is_column_visible(
        is_drive_view,
        is_recycle_bin_view,
        ItemViewerHeaderColumn::Size,
    );

    if draw_visibility_checkbox(ui, i18n.tr("explorer_cols_size"), size_visible) {
        *action = Some(ItemViewerAction::ToggleColumnVisibility(
            ItemViewerHeaderColumn::Size,
        ));
        ui.close();
    }

    if is_drive_view {
        // Usage is always visible in drive view.
        let mut usage_checked = true;
        ui.add_enabled(
            false,
            egui::Checkbox::new(&mut usage_checked, i18n.tr("explorer_cols_usage")),
        );
    } else if is_recycle_bin_view {
        // Deleted is always visible in recycle-bin view.
        let mut deleted_checked = true;
        ui.add_enabled(
            false,
            egui::Checkbox::new(&mut deleted_checked, i18n.tr("explorer_cols_deleted")),
        );

        let created_visible = column_state.is_column_visible(
            is_drive_view,
            is_recycle_bin_view,
            ItemViewerHeaderColumn::Created,
        );

        if draw_visibility_checkbox(ui, i18n.tr("explorer_cols_created"), created_visible) {
            *action = Some(ItemViewerAction::ToggleColumnVisibility(
                ItemViewerHeaderColumn::Created,
            ));
            ui.close();
        }
    } else {
        let modified_visible = column_state.is_column_visible(
            is_drive_view,
            is_recycle_bin_view,
            ItemViewerHeaderColumn::Modified,
        );

        if draw_visibility_checkbox(ui, i18n.tr("explorer_cols_modified"), modified_visible) {
            *action = Some(ItemViewerAction::ToggleColumnVisibility(
                ItemViewerHeaderColumn::Modified,
            ));
            ui.close();
        }

        let created_visible = column_state.is_column_visible(
            is_drive_view,
            is_recycle_bin_view,
            ItemViewerHeaderColumn::Created,
        );

        if draw_visibility_checkbox(ui, i18n.tr("explorer_cols_created"), created_visible) {
            *action = Some(ItemViewerAction::ToggleColumnVisibility(
                ItemViewerHeaderColumn::Created,
            ));
            ui.close();
        }

        let tags_visible = column_state.is_column_visible(
            is_drive_view,
            is_recycle_bin_view,
            ItemViewerHeaderColumn::Tags,
        );

        if draw_visibility_checkbox(ui, i18n.tr("explorer_cols_tags"), tags_visible) {
            *action = Some(ItemViewerAction::ToggleColumnVisibility(
                ItemViewerHeaderColumn::Tags,
            ));
            ui.close();
        }
    }
}

fn draw_visibility_checkbox(ui: &mut egui::Ui, label: String, checked: bool) -> bool {
    let mut value = checked;
    ui.checkbox(&mut value, label).clicked() && value != checked
}

pub fn handle_keyboard_navigation(
    ctx: &egui::Context,
    filtered_indices: &[usize],
    files: &Vec<FileItem>,
    is_drive_view: bool,
    explorer_state: &mut ExplorerState,
) -> Option<ItemViewerAction> {
    if filtered_indices.is_empty() {
        return None;
    }

    let is_selectable = |row_idx: usize| -> bool {
        if !is_drive_view {
            return true;
        }
        let file_idx = filtered_indices[row_idx];
        !is_raw_physical_drive_path(&files[file_idx].path)
    };

    let next_selectable = |start: usize, dir: i32| -> Option<usize> {
        let mut i = start as i32;
        loop {
            i += dir;
            if i < 0 || i >= filtered_indices.len() as i32 {
                return None;
            }
            let idx = i as usize;
            if is_selectable(idx) {
                return Some(idx);
            }
        }
    };

    let first_selectable = || (0..filtered_indices.len()).find(|&i| is_selectable(i));
    let last_selectable = || {
        (0..filtered_indices.len())
            .rev()
            .find(|&i| is_selectable(i))
    };

    let mut action: Option<ItemViewerAction> = None;
    let home_pressed = ctx.input(|i| i.key_pressed(egui::Key::Home));
    let end_pressed = ctx.input(|i| i.key_pressed(egui::Key::End));

    let current_index = explorer_state
        .selected_paths
        .iter()
        .next()
        .and_then(|selected| {
            filtered_indices
                .iter()
                .position(|&i| &files[i].path == selected)
        });

    let current_idx = match current_index {
        Some(idx) => idx,
        None => {
            if home_pressed {
                let first_idx = first_selectable()?;
                let first = files[filtered_indices[first_idx]].path.clone();

                explorer_state.selection_anchor = Some(first_idx);
                explorer_state.selection_focus = Some(first_idx);

                return Some(ItemViewerAction::ReplaceSelection(first));
            }

            if end_pressed {
                let last_idx = last_selectable()?;
                let last = files[filtered_indices[last_idx]].path.clone();

                explorer_state.selection_anchor = Some(last_idx);
                explorer_state.selection_focus = Some(last_idx);

                return Some(ItemViewerAction::ReplaceSelection(last));
            }

            if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                let first_idx = first_selectable()?;
                let first = files[filtered_indices[first_idx]].path.clone();

                explorer_state.selection_anchor = Some(first_idx);
                explorer_state.selection_focus = Some(first_idx);

                return Some(ItemViewerAction::ReplaceSelection(first));
            }

            if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                let last_idx = last_selectable()?;
                let last = files[filtered_indices[last_idx]].path.clone();

                explorer_state.selection_anchor = Some(last_idx);
                explorer_state.selection_focus = Some(last_idx);

                return Some(ItemViewerAction::ReplaceSelection(last));
            }

            return None;
        }
    };

    // SHIFT RANGE
    if ctx.input(|i| i.modifiers.shift) {
        let anchor = explorer_state.selection_anchor.unwrap_or(current_idx);
        let focus = explorer_state.selection_focus.unwrap_or(current_idx);

        // Validate that anchor and focus are within bounds
        let anchor_valid = anchor < filtered_indices.len();
        let focus_valid = focus < filtered_indices.len();

        if !anchor_valid || !focus_valid {
            // Reset to current position if indices are invalid
            explorer_state.selection_anchor = Some(current_idx);
            explorer_state.selection_focus = Some(current_idx);
            return None;
        }

        let mut new_focus = focus;

        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            if let Some(next) = next_selectable(focus, 1) {
                new_focus = next;
            }
        }

        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            if let Some(prev) = next_selectable(focus, -1) {
                new_focus = prev;
            }
        }

        if home_pressed {
            if let Some(first) = first_selectable() {
                new_focus = first;
            }
        }

        if end_pressed {
            if let Some(last) = last_selectable() {
                new_focus = last;
            }
        }

        explorer_state.selection_anchor = Some(anchor);
        explorer_state.selection_focus = Some(new_focus);

        let range_start = anchor.min(new_focus);
        let range_end = anchor.max(new_focus);

        let range_paths: Vec<PathBuf> = filtered_indices[range_start..=range_end]
            .iter()
            .filter(|&&i| {
                if !is_drive_view {
                    true
                } else {
                    !is_raw_physical_drive_path(&files[i].path)
                }
            })
            .map(|&i| files[i].path.clone())
            .collect();

        action = Some(ItemViewerAction::RangeSelect(range_paths));
    }
    // 🔹 NORMAL NAV
    else {
        let mut new_idx = current_idx;

        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            if let Some(next) = next_selectable(current_idx, 1) {
                new_idx = next;
            }
        }

        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            if let Some(prev) = next_selectable(current_idx, -1) {
                new_idx = prev;
            }
        }

        if home_pressed && let Some(first) = first_selectable() {
            new_idx = first;
        }

        if end_pressed && let Some(last) = last_selectable() {
            new_idx = last;
        }

        if new_idx != current_idx {
            let new_path = files[filtered_indices[new_idx]].path.clone();

            explorer_state.selection_anchor = Some(new_idx);
            explorer_state.selection_focus = Some(new_idx);

            action = Some(ItemViewerAction::ReplaceSelection(new_path));
        }
    }

    action
}

/// How long after a qualifying single click (see `handle_row_click`) to wait
/// before committing to rename rather than open - matches the pause Windows
/// Explorer gives you between the two clicks of its rename gesture.
pub const CLICK_TO_RENAME_DELAY: f64 = 0.35;

pub fn handle_row_click(
    row_idx: usize,
    file: &FileItem,
    modifiers: egui::Modifiers,
    filtered_indices: &[usize],
    files: &[FileItem],
    drag_state: &DragState,
    explorer_state: &mut ExplorerState,
    is_recycle_bin_view: bool,
    is_drive_view: bool,
    is_double_click: bool,
    now: f64,
) -> Option<ItemViewerAction> {
    if drag_state.active {
        return None;
    }

    // Any click other than the qualifying "second click on an
    // already-selected item" (handled below, which re-arms it itself)
    // cancels a pending rename arm - e.g. clicking a different item, or
    // shift/ctrl-extending the selection.
    explorer_state.click_to_rename_arm = None;

    if modifiers.shift {
        if let Some(anchor_idx) = explorer_state.selection_anchor {
            let current_idx = row_idx;

            // Validate that anchor_idx is still within bounds of filtered_indices
            if anchor_idx < filtered_indices.len() {
                let range_start = anchor_idx.min(current_idx);
                let range_end = anchor_idx.max(current_idx);

                let range_paths: Vec<PathBuf> = filtered_indices[range_start..=range_end]
                    .iter()
                    .map(|&i| files[i].path.clone())
                    .collect();

                explorer_state.selection_focus = Some(current_idx);
                Some(ItemViewerAction::RangeSelect(range_paths))
            } else {
                // Anchor is out of bounds, treat as simple selection
                explorer_state.selection_anchor = Some(row_idx);
                explorer_state.selection_focus = Some(row_idx);
                Some(ItemViewerAction::Select(file.path.clone()))
            }
        } else {
            explorer_state.selection_anchor = Some(row_idx);
            explorer_state.selection_focus = Some(row_idx);

            Some(ItemViewerAction::Select(file.path.clone()))
        }
    } else if modifiers.ctrl {
        if !explorer_state.selected_paths.contains(&file.path) {
            explorer_state.selected_paths.insert(file.path.clone());
        }

        explorer_state.selection_anchor = Some(row_idx);
        explorer_state.selection_focus = Some(row_idx);

        Some(ItemViewerAction::Select(file.path.clone()))
    } else {
        let is_single_selected = explorer_state.selected_paths.len() == 1
            && explorer_state.selected_paths.contains(&file.path);

        if is_single_selected {
            if is_recycle_bin_view {
                return Some(ItemViewerAction::ReplaceSelection(file.path.clone()));
            }
            if is_double_click {
                return Some(if file.is_dir {
                    ItemViewerAction::Open(file.path.clone())
                } else {
                    ItemViewerAction::OpenWithDefault(vec![file.path.clone()])
                });
            }
            if is_drive_view {
                // Drives don't support the click-to-rename gesture (renaming
                // a volume label isn't wired up here) - a plain second click
                // on an already-selected drive is just a no-op.
                return None;
            }
            // A slow second click on an already-selected item - arm the
            // rename gesture instead of opening immediately. A per-frame
            // poll (see `poll_click_to_rename`) fires the actual rename
            // once the delay elapses, unless this arm gets cancelled first
            // (a genuine double-click is handled above; anything else is
            // handled at the top of this function).
            explorer_state.click_to_rename_arm = Some((file.path.clone(), now));
            None
        } else {
            explorer_state.selection_anchor = Some(row_idx);
            explorer_state.selection_focus = Some(row_idx);

            Some(ItemViewerAction::ReplaceSelection(file.path.clone()))
        }
    }
}

/// Checks whether a rename arm from `handle_row_click` has waited out
/// `CLICK_TO_RENAME_DELAY` and, if so, starts the actual rename - matching
/// Windows Explorer's "click, pause, click again" gesture. Called once per
/// frame from each view that supports it (Details/Detail Preview, Gallery,
/// Preview - not Columns/Column Preview, which only rename via the
/// right-click menu).
pub fn poll_click_to_rename(
    files: &[FileItem],
    explorer_state: &mut ExplorerState,
    rename_state: &mut Option<RenameState>,
    now: f64,
) {
    let Some((path, armed_at)) = explorer_state.click_to_rename_arm.clone() else {
        return;
    };

    if now - armed_at < CLICK_TO_RENAME_DELAY {
        return;
    }

    explorer_state.click_to_rename_arm = None;

    // Only fire if nothing else already started editing, and the armed
    // item is still the sole selection (it could have changed in the
    // meantime via e.g. a keyboard shortcut).
    if rename_state.is_some() {
        return;
    }
    if explorer_state.selected_paths.len() != 1 || !explorer_state.selected_paths.contains(&path) {
        return;
    }
    let Some(file) = files.iter().find(|f| f.path == path) else {
        return;
    };

    *rename_state = Some(RenameState {
        path: file.path.clone(),
        new_name: file.name.clone(),
        should_focus: true,
        validation_error_show: false,
    });
}

pub fn draw_table_text(
    ui: &mut egui::Ui,
    layout: &ItemViewerLayout,
    text: &str,
    font_id: &egui::FontId,
    color: egui::Color32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), layout.row_height),
        egui::Sense::hover(),
    );

    let (display_text, _) = truncate_item_text(ui, text, rect.width(), font_id, color);

    ui.painter().text(
        egui::pos2(rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        display_text,
        font_id.clone(),
        color,
    );

    response.on_hover_cursor(egui::CursorIcon::Default)
}

#[allow(clippy::too_many_arguments)]
pub fn compute_item_viewer_column_layout(
    ui: &mut egui::Ui,
    i18n: &I18n,
    column_state: &mut ItemViewerColumnState,
    filter_state: &FilterState,
    files: &[FileItem],
    folder_sizes: &HashMap<PathBuf, ItemViewerFolderSizeState>,
    is_drive_view: bool,
    is_recycle_bin_view: bool,
    is_search_view: bool,
    show_item_viewer_icons: bool,
    palette: &ThemePalette,
    font_id: &FontId,
    file_type_cache: &mut HashMap<String, String>,
    file_size_text_cache: &mut HashMap<PathBuf, (u64, String)>,
    folder_size_text_cache: &mut HashMap<PathBuf, (u64, bool, String)>,
    drive_size_text_cache: &mut HashMap<PathBuf, (u64, u64, String)>,
    viewport_width: f32,
) -> ItemViewerColumnLayout {
    // Search view has no persisted column-width storage of its own (see
    // `visible_order`) - it reuses the *file* view's `is_drive_view=false,
    // is_recycle_bin_view=false` storage slots, but under a different
    // column set (it includes `OriginalDirectory`, which the file view's
    // storage has no slot for, while lacking `Created`, whose slot the
    // file view *does* have). Letting a "Size to Fit" request run through
    // the normal `set_all_column_widths`/`set_column_width` path for a
    // search tab was writing search-result-derived widths straight into
    // the *file* view's persisted sizes (Type/Size/Modified do have slots
    // there) - corrupting every normal folder tab's column widths the
    // next time settings were saved. Bypassing the whole fit/persisted-
    // width system for search view and computing fresh, unstored widths
    // every frame instead avoids that class of bug entirely, at the cost
    // of search columns not being manually resizable/rememberable between
    // sessions - an acceptable tradeoff for what's a supplementary,
    // non-customizable display.
    if is_search_view {
        column_state.pending_fit_request = None;
        let ordered_columns = column_state.visible_order(is_drive_view, is_recycle_bin_view, true);
        let widths = compute_item_viewer_column_widths(
            ui,
            i18n,
            files,
            &filter_state.cached_indices,
            folder_sizes,
            is_drive_view,
            show_item_viewer_icons,
            palette,
            font_id,
            file_type_cache,
            file_size_text_cache,
            folder_size_text_cache,
            drive_size_text_cache,
        );
        return ItemViewerColumnLayout {
            ordered_columns,
            name_width: widths.name,
            type_width: widths.type_width,
            size_width: widths.size_width,
            usage_width: 0.0,
            modified_width: widths.modified_width,
            created_width: 0.0,
            deleted_width: 0.0,
            original_directory_width: widths.original_directory_width,
            tags_width: widths.tags_width,
            column_sizes_changed: false,
        };
    }

    let mut fit_request = column_state.pending_fit_request.take();

    // A column state whose sizes are still sitting at the untouched generic
    // defaults gets auto-fitted to its actual content the first time it's
    // rendered with something in it - evaluated here, at render time, rather
    // than hooked into navigation/loading events, so it fires reliably no
    // matter how the view got loaded (first tab on startup, switching to an
    // already-open tab, a brand new tab, a split pane, etc). `auto_fit_checked`
    // makes sure this only happens once per navigation, not every frame,
    // so it doesn't fight a manual resize made right after.
    if fit_request.is_none()
        && !column_state.auto_fit_checked
        && !filter_state.cached_indices.is_empty()
    {
        column_state.auto_fit_checked = true;

        let sizes_are_default = if is_drive_view {
            column_state.drive_column_sizes == default_item_viewer_drive_column_size()
        } else if is_recycle_bin_view {
            column_state.recycle_bin_column_sizes == default_recycle_bin_column_size()
        } else {
            column_state.file_column_sizes == default_item_viewer_file_column_size()
        };

        if sizes_are_default {
            fit_request = Some(ItemViewerColumnFitRequest::All);
        }
    }

    let ordered_columns = column_state.visible_order(is_drive_view, is_recycle_bin_view, is_search_view);

    let mut column_sizes_changed = false;
    let current_width = viewport_width.max(1.0);

    // Handle any pending auto-fit request. The calculated widths are stored
    // back into column_state so they persist across frames and column reordering.
    if let Some(fit_request) = fit_request {
        let widths = compute_item_viewer_column_widths(
            ui,
            i18n,
            files,
            &filter_state.cached_indices,
            folder_sizes,
            is_drive_view,
            show_item_viewer_icons,
            palette,
            font_id,
            file_type_cache,
            file_size_text_cache,
            folder_size_text_cache,
            drive_size_text_cache,
        );

        match fit_request {
            ItemViewerColumnFitRequest::All => {
                column_state.set_all_column_widths(is_drive_view, is_recycle_bin_view, &widths);

                column_sizes_changed = true;
            }

            ItemViewerColumnFitRequest::Column(column) => {
                let width = match column {
                    ItemViewerHeaderColumn::Name => widths.name,
                    ItemViewerHeaderColumn::OriginalDirectory => widths.original_directory_width,
                    ItemViewerHeaderColumn::Type => widths.type_width,
                    ItemViewerHeaderColumn::Size => widths.size_width,
                    ItemViewerHeaderColumn::Modified => widths.modified_width,
                    ItemViewerHeaderColumn::Created => widths.created_width,
                    ItemViewerHeaderColumn::Deleted => widths.deleted_width,
                    ItemViewerHeaderColumn::Usage => widths.usage_width,
                    ItemViewerHeaderColumn::Tags => widths.tags_width,
                };

                column_state.set_column_width(is_drive_view, is_recycle_bin_view, column, width);

                column_sizes_changed = true;
            }
        }

        column_state.layout_generation = column_state.layout_generation.wrapping_add(1);
    }

    // Default widths are only used when a column has never been explicitly
    // sized or fitted.
    let default_name_width = (current_width * 0.35).max(180.0);
    let default_type_width = (current_width * 0.10).max(60.0);

    let default_size_width = if is_drive_view {
        (current_width * 0.14).max(120.0)
    } else {
        (current_width * 0.10).max(75.0)
    };

    let default_modified_width = (current_width * 0.20).max(100.0);
    let default_created_width = (current_width * 0.20).max(100.0);
    let default_deleted_width = (current_width * 0.20).max(100.0);
    let default_usage_width = (current_width * 0.20).max(150.0);
    let default_original_directory_width = (current_width * 0.20).max(200.0);
    let default_tags_width = (current_width * 0.20).max(140.0);

    let name_width = column_state
        .column_width(
            is_drive_view,
            is_recycle_bin_view,
            ItemViewerHeaderColumn::Name,
        )
        .unwrap_or(default_name_width);

    let original_directory_width =
        if ordered_columns.contains(&ItemViewerHeaderColumn::OriginalDirectory) {
            column_state
                .column_width(
                    is_drive_view,
                    is_recycle_bin_view,
                    ItemViewerHeaderColumn::OriginalDirectory,
                )
                .unwrap_or(default_original_directory_width)
        } else {
            0.0
        };

    let type_width = if ordered_columns.contains(&ItemViewerHeaderColumn::Type) {
        column_state
            .column_width(
                is_drive_view,
                is_recycle_bin_view,
                ItemViewerHeaderColumn::Type,
            )
            .unwrap_or(default_type_width)
    } else {
        0.0
    };

    let size_width = if ordered_columns.contains(&ItemViewerHeaderColumn::Size) {
        column_state
            .column_width(
                is_drive_view,
                is_recycle_bin_view,
                ItemViewerHeaderColumn::Size,
            )
            .unwrap_or(default_size_width)
    } else {
        0.0
    };

    let usage_width = if ordered_columns.contains(&ItemViewerHeaderColumn::Usage) {
        column_state
            .column_width(
                is_drive_view,
                is_recycle_bin_view,
                ItemViewerHeaderColumn::Usage,
            )
            .unwrap_or(default_usage_width)
    } else {
        0.0
    };

    let modified_width = if ordered_columns.contains(&ItemViewerHeaderColumn::Modified) {
        column_state
            .column_width(
                is_drive_view,
                is_recycle_bin_view,
                ItemViewerHeaderColumn::Modified,
            )
            .unwrap_or(default_modified_width)
    } else {
        0.0
    };

    let created_width = if ordered_columns.contains(&ItemViewerHeaderColumn::Created) {
        column_state
            .column_width(
                is_drive_view,
                is_recycle_bin_view,
                ItemViewerHeaderColumn::Created,
            )
            .unwrap_or(default_created_width)
    } else {
        0.0
    };

    let deleted_width = if ordered_columns.contains(&ItemViewerHeaderColumn::Deleted) {
        column_state
            .column_width(
                is_drive_view,
                is_recycle_bin_view,
                ItemViewerHeaderColumn::Deleted,
            )
            .unwrap_or(default_deleted_width)
    } else {
        0.0
    };

    let tags_width = if ordered_columns.contains(&ItemViewerHeaderColumn::Tags) {
        column_state
            .column_width(
                is_drive_view,
                is_recycle_bin_view,
                ItemViewerHeaderColumn::Tags,
            )
            .unwrap_or(default_tags_width)
    } else {
        0.0
    };

    ItemViewerColumnLayout {
        ordered_columns,
        name_width,
        type_width,
        size_width,
        usage_width,
        modified_width,
        created_width,
        deleted_width,
        original_directory_width,
        tags_width,
        column_sizes_changed,
    }
}

fn compute_item_viewer_column_widths(
    ui: &mut egui::Ui,
    i18n: &I18n,
    files: &[FileItem],
    filtered_indices: &[usize],
    folder_sizes: &HashMap<PathBuf, ItemViewerFolderSizeState>,
    is_drive_view: bool,
    show_item_viewer_icons: bool,
    palette: &ThemePalette,
    font_id: &FontId,
    file_type_cache: &mut HashMap<String, String>,
    file_size_text_cache: &mut HashMap<PathBuf, (u64, String)>,
    folder_size_text_cache: &mut HashMap<PathBuf, (u64, bool, String)>,
    drive_size_text_cache: &mut HashMap<PathBuf, (u64, u64, String)>,
) -> ItemViewerColumnWidths {
    let mut widths = ItemViewerColumnWidths {
        name: measure_text_width(
            ui,
            &i18n.tr("explorer_cols_name"),
            font_id,
            palette.text_header_section,
        ),
        original_directory_width: measure_text_width(
            ui,
            &i18n.tr("explorer_cols_original_directory"),
            font_id,
            palette.text_header_section,
        ),
        type_width: measure_text_width(
            ui,
            &i18n.tr("explorer_cols_type"),
            font_id,
            palette.text_header_section,
        ),
        size_width: measure_text_width(
            ui,
            &i18n.tr("explorer_cols_size"),
            font_id,
            palette.text_header_section,
        ),
        modified_width: measure_text_width(
            ui,
            &i18n.tr("explorer_cols_modified"),
            font_id,
            palette.text_header_section,
        ),
        created_width: measure_text_width(
            ui,
            &i18n.tr("explorer_cols_created"),
            font_id,
            palette.text_header_section,
        ),
        usage_width: measure_text_width(
            ui,
            &i18n.tr("explorer_cols_usage"),
            font_id,
            palette.text_header_section,
        ),
        deleted_width: measure_text_width(
            ui,
            &i18n.tr("explorer_cols_deleted"),
            font_id,
            palette.text_header_section,
        ),
        tags_width: measure_text_width(
            ui,
            &i18n.tr("explorer_cols_tags"),
            font_id,
            palette.text_header_section,
        )
        .max(140.0),
    };

    let icon_padding = if show_item_viewer_icons {
        palette.row_height + 6.0
    } else {
        0.0
    };

    for &idx in filtered_indices {
        let file = &files[idx];

        widths.name = widths
            .name
            .max(measure_text_width(ui, &file.name, font_id, palette.text_normal) + icon_padding);

        let type_text = if file.is_dir {
            "Folder".to_string()
        } else if let Some(ext) = file.path.extension().and_then(|ext| ext.to_str()) {
            get_file_type_name(ext, file_type_cache).to_string()
        } else {
            get_file_type_name("", file_type_cache).to_string()
        };
        widths.type_width = widths.type_width.max(measure_text_width(
            ui,
            &type_text,
            font_id,
            palette.text_normal,
        ));

        let size_text = resolve_size_text(
            file,
            folder_sizes,
            file_size_text_cache,
            folder_size_text_cache,
            drive_size_text_cache,
        );
        widths.size_width = widths.size_width.max(measure_text_width(
            ui,
            &size_text,
            font_id,
            palette.text_normal,
        ));

        if is_drive_view {
            widths.usage_width = widths.usage_width.max(
                measure_text_width(
                    ui,
                    &i18n.tr("explorer_cols_usage"),
                    font_id,
                    palette.text_normal,
                ) + 60.0,
            );
        } else {
            widths.modified_width = widths.modified_width.max(measure_text_width(
                ui,
                file.modified_time.as_deref().unwrap_or("—"),
                font_id,
                palette.text_normal,
            ));
            widths.created_width = widths.created_width.max(measure_text_width(
                ui,
                file.created_time.as_deref().unwrap_or("—"),
                font_id,
                palette.text_normal,
            ));
            widths.deleted_width = widths.deleted_width.max(measure_text_width(
                ui,
                file.deleted_time.as_deref().unwrap_or("—"),
                font_id,
                palette.text_normal,
            ));
            widths.original_directory_width =
                widths.original_directory_width.max(measure_text_width(
                    ui,
                    file.original_directory.as_deref().unwrap_or("—"),
                    font_id,
                    palette.text_normal,
                ));
        }
    }

    widths.name = widths.name.max(220.0);
    widths.type_width = widths.type_width.max(60.0);
    widths.size_width = widths.size_width.max(75.0);
    widths.modified_width = widths.modified_width.max(120.0);
    widths.created_width = widths.created_width.max(120.0);
    widths.usage_width = widths.usage_width.max(150.0);
    widths.deleted_width = widths.deleted_width.max(120.0);
    widths.original_directory_width = widths.original_directory_width.max(150.0);

    widths
}

fn measure_text_width(
    ui: &mut egui::Ui,
    text: &str,
    font_id: &FontId,
    color: egui::Color32,
) -> f32 {
    ui.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(text.to_owned(), font_id.clone(), color)
            .size()
            .x
    })
}

fn resolve_size_text(
    file: &FileItem,
    folder_sizes: &HashMap<PathBuf, ItemViewerFolderSizeState>,
    file_size_text_cache: &mut HashMap<PathBuf, (u64, String)>,
    folder_size_text_cache: &mut HashMap<PathBuf, (u64, bool, String)>,
    drive_size_text_cache: &mut HashMap<PathBuf, (u64, u64, String)>,
) -> String {
    if let (Some(total), Some(free)) = (file.total_space, file.free_space) {
        let key = &file.path;
        if let Some((cached_total, cached_free, cached_text)) = drive_size_text_cache.get(key) {
            if *cached_total == total && *cached_free == free {
                return cached_text.clone();
            }
        }

        let formatted = format!("{} / {}", format_size(free), format_size(total));
        drive_size_text_cache.insert(file.path.clone(), (total, free, formatted.clone()));
        return formatted;
    }

    if file.is_dir {
        if let Some(state) = folder_sizes.get(&file.path) {
            if let Some((bytes, done, value)) = folder_size_text_cache.get(&file.path) {
                if *bytes == state.bytes && *done == state.done {
                    return value.clone();
                }
            }

            let label = format_size(state.bytes);
            let value = if state.done {
                label
            } else {
                format!("⏳ {}", label)
            };
            folder_size_text_cache
                .insert(file.path.clone(), (state.bytes, state.done, value.clone()));
            return value;
        }

        return "—".to_string();
    }

    if let Some(size) = file.file_size {
        if let Some((cached_size, value)) = file_size_text_cache.get(&file.path) {
            if *cached_size == size {
                return value.clone();
            }
        }

        let value = format_size(size);
        file_size_text_cache.insert(file.path.clone(), (size, value.clone()));
        return value;
    }

    "—".to_string()
}

pub fn table_background_response(ui: &mut egui::Ui) -> egui::Response {
    let mut rect = ui.available_rect_before_wrap();

    rect.max.x -= ui.spacing().scroll.allocated_width();

    ui.interact(
        rect,
        ui.id().with("item_viewer_background"),
        egui::Sense::click(),
    )
}
