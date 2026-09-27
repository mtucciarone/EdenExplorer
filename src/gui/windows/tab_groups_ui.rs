//! Settings UI for user-defined tab groups (see `core::tab_groups`) - named
//! sets of folder paths that can be opened all at once as tabs. The same
//! path may be added to a group more than once on purpose (opening the
//! group then opens that many separate tabs for it).
//!
//! Master-detail layout: the left column lists every group (name + folder
//! count, reorderable, deletable, click to select); the right column shows
//! the selected group's own name field and its folder list. Selection is
//! tracked by the group's stable `id` (`SettingsWindow::selected_tab_group_id`)
//! rather than a positional index, so reordering or deleting an unrelated
//! group never silently re-points the detail pane at the wrong one.

use crate::core::tab_groups::{TabGroup, TabGroupIcon, next_group_id};
use crate::core::utils::widgets::{apply_eden_visual_overrides, eden_button, eden_text_label};
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::enums::SettingsAction;
use crate::gui::windows::settings::{
    count_badge, empty_state_hint, master_detail_column_size, no_selection_hint, reorder_buttons,
    setting_label, setting_row, settings_section,
};
use crate::gui::windows::structs::SettingsWindow;
use eframe::egui;
use egui_phosphor::regular;

/// Row/header icon size for a folder's real shell icon - small enough to sit
/// inline with text like any other icon in this app's menus/lists.
const FOLDER_ICON_SIZE: egui::Vec2 = egui::vec2(16.0, 16.0);

/// Resolves what a tab group's own parent icon should actually show, for
/// every place a group is listed (the "+" button's group menu in `tabs.rs`,
/// and this page's own master-detail list below): a user-picked image file's
/// real texture, a user-picked glyph, or - the default, `TabGroupIcon::None`
/// - the group's first folder entry's own real shell icon, falling back to
/// the plain folder glyph if there's no first entry or its icon hasn't
/// loaded yet. Centralized here (rather than duplicated per call site) so
/// both places agree on the same fallback chain, including what happens if a
/// user-picked custom image file fails to load (falls through to the
/// first-folder default rather than showing nothing).
pub fn resolve_tab_group_icon<'a>(
    icon_cache: &'a IconCache,
    group: &'a TabGroup,
) -> (Option<egui::TextureHandle>, Option<&'a str>) {
    match &group.icon {
        TabGroupIcon::Custom(path) => {
            if let Some(texture) = icon_cache.get_custom_file_icon(path) {
                return (Some(texture), None);
            }
        }
        TabGroupIcon::Glyph(glyph) => return (None, Some(glyph.as_str())),
        TabGroupIcon::None => {}
    }
    let texture = group
        .entries
        .first()
        .and_then(|e| icon_cache.get(&e.path, true));
    (texture, None)
}

pub fn draw_tab_groups_settings(
    ui: &mut egui::Ui,
    i18n: &I18n,
    settings: &mut SettingsWindow,
    palette: &ThemePalette,
    icon_cache: &IconCache,
) -> Option<SettingsAction> {
    let mut action = None;

    setting_label(
        ui,
        &i18n.tr("settings_tab_groups"),
        Some((&i18n.tr("tooltip_settings_tab_groups"), palette)),
        palette,
    );
    ui.add_space(8.0);

    if eden_button(
        ui,
        palette,
        &format!("{} {}", regular::PLUS, i18n.tr("tab_group_add")),
    )
    .clicked()
    {
        let id = next_group_id(&settings.current_settings.tab_groups);
        settings.current_settings.tab_groups.push(TabGroup::new(id));
        settings.selected_tab_group_id = Some(id);
        action = Some(SettingsAction::ApplySettings);
    }

    ui.add_space(6.0);

    ui.horizontal(|ui| {
        if eden_button(ui, palette, &i18n.tr("tab_group_export")).clicked() {
            action = Some(SettingsAction::ExportTabGroups);
        }
        if eden_button(ui, palette, &i18n.tr("tab_group_import")).clicked() {
            action = Some(SettingsAction::ImportTabGroups);
        }
    });

    ui.add_space(10.0);

    if settings.current_settings.tab_groups.is_empty() {
        empty_state_hint(
            ui,
            palette,
            regular::FOLDERS,
            &i18n.tr("tab_group_empty_state"),
        );
        return action;
    }

    // Keep a valid selection: default to the first group the first time
    // there's anything to select, and drop a selection that pointed at a
    // group which no longer exists (deleted from another session's import,
    // or just deleted this frame below).
    if settings.selected_tab_group_id.is_none()
        || !settings
            .current_settings
            .tab_groups
            .iter()
            .any(|g| Some(g.id) == settings.selected_tab_group_id)
    {
        settings.selected_tab_group_id = settings.current_settings.tab_groups.first().map(|g| g.id);
    }

    let mut remove_index: Option<usize> = None;
    let mut move_indices: Option<(usize, usize)> = None;
    let mut changed = false;
    let total_len = settings.current_settings.tab_groups.len();

    let (col_w, col_h) = master_detail_column_size(ui);
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(col_w, col_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
            egui::ScrollArea::vertical()
                .id_salt("tab_groups_list_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (index, group) in
                        settings.current_settings.tab_groups.iter().enumerate()
                    {
                        let label = if group.name.is_empty() {
                            i18n.tr("tab_group_untitled")
                        } else {
                            group.name.clone()
                        };
                        let (icon_texture, icon_glyph) = resolve_tab_group_icon(icon_cache, group);
                        let is_selected = Some(group.id) == settings.selected_tab_group_id;

                        let row = list_row(ui, palette, is_selected, |ui| {
                            if let Some(texture) = &icon_texture {
                                ui.add(egui::Image::new(texture).fit_to_exact_size(FOLDER_ICON_SIZE));
                            } else {
                                // `.selectable(false)` - a plain `ui.label` is
                                // selectable text by default in this app's
                                // style, which registers its own click sense
                                // *after* the row's own background sense and
                                // would otherwise steal a click meant to
                                // select the row (see `list_row`'s own doc
                                // comment on background-sensed-first).
                                ui.add(
                                    egui::Label::new(icon_glyph.unwrap_or(regular::FOLDERS))
                                        .selectable(false),
                                );
                            }
                            ui.add(
                                egui::Label::new(egui::RichText::new(&label).strong())
                                    .selectable(false),
                            );

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if let Some(swap) = reorder_buttons(ui, palette, index, total_len) {
                                    move_indices = Some(swap);
                                }
                                if eden_button(ui, palette, regular::TRASH)
                                    .on_hover_text(i18n.tr("tab_group_remove"))
                                    .clicked()
                                {
                                    remove_index = Some(index);
                                }
                                count_badge(ui, palette, group.entries.len());
                            });
                        });

                        if row.clicked() {
                            settings.selected_tab_group_id = Some(group.id);
                        }
                    }
                });
            },
        );
        ui.allocate_ui_with_layout(
            egui::vec2(col_w, col_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
            egui::ScrollArea::vertical()
                .id_salt("tab_groups_detail_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let Some(selected_id) = settings.selected_tab_group_id else {
                        no_selection_hint(
                            ui,
                            palette,
                            regular::FOLDERS,
                            &i18n.tr("tab_group_select_hint"),
                        );
                        return;
                    };
                    let Some(group) = settings
                        .current_settings
                        .tab_groups
                        .iter_mut()
                        .find(|g| g.id == selected_id)
                    else {
                        return;
                    };

                    settings_section(ui, palette, |ui| {
                        setting_row(
                            ui,
                            |ui| {
                                setting_label(ui, &i18n.tr("tab_group_name"), None, palette);
                            },
                            |ui| {
                                apply_eden_visual_overrides(ui, palette);
                                changed |= ui
                                    .add_sized(
                                        [280.0, ui.spacing().interact_size.y],
                                        egui::TextEdit::singleline(&mut group.name),
                                    )
                                    .changed();
                            },
                        );

                        ui.add_space(8.0);
                        eden_text_label(ui, palette, &i18n.tr("tab_group_icon"));
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let is_default = matches!(group.icon, TabGroupIcon::None);
                            let default_color = if is_default {
                                palette.primary
                            } else {
                                ui.visuals().text_color()
                            };
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new(regular::PROHIBIT)
                                            .size(16.0)
                                            .color(default_color),
                                    )
                                    .min_size(egui::vec2(24.0, 24.0)),
                                )
                                .on_hover_text(i18n.tr("tab_group_icon_default"))
                                .clicked()
                            {
                                group.icon = TabGroupIcon::None;
                                changed = true;
                            }

                            ui.add_space(4.0);

                            let current_glyph = match &group.icon {
                                TabGroupIcon::Glyph(g) => Some(g.as_str()),
                                _ => None,
                            };
                            if let Some(glyph) =
                                crate::gui::windows::icon_picker_ui::draw_icon_picker_button(
                                    ui,
                                    i18n,
                                    palette,
                                    &mut settings.icon_picker_search,
                                    ("tab_group_icon_picker", selected_id),
                                    current_glyph,
                                    regular::FOLDERS,
                                )
                            {
                                group.icon = TabGroupIcon::Glyph(glyph);
                                changed = true;
                            }
                        });

                        // A user-browsed image file's own icon, on its own
                        // row below the glyph choices - same layout as Send
                        // To/Favorites' own icon pickers.
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if eden_button(ui, palette, &i18n.tr("tab_group_icon_browse")).clicked()
                            {
                                if let Some(path) = crate::gui::windows::windowsoverrides::dialog()
                                    .add_filter(
                                        "Icon/Image",
                                        &["ico", "png", "jpg", "jpeg", "bmp", "gif"],
                                    )
                                    .add_filter("All Files", &["*"])
                                    .pick_file()
                                {
                                    let stored_path =
                                        crate::core::indexer::import_custom_icon(&path)
                                            .unwrap_or(path);
                                    group.icon = TabGroupIcon::Custom(stored_path);
                                    changed = true;
                                }
                            }
                            if eden_button(ui, palette, &i18n.tr("tab_group_icon_clear")).clicked()
                            {
                                group.icon = TabGroupIcon::None;
                                changed = true;
                            }
                        });
                        if let TabGroupIcon::Custom(file) = &group.icon {
                            let file = file.clone();
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if let Some(texture) = icon_cache.get_custom_file_icon(&file) {
                                    ui.add(
                                        egui::Image::new(&texture)
                                            .fit_to_exact_size(egui::vec2(20.0, 20.0)),
                                    );
                                }
                                let path_text = file.display().to_string();
                                ui.add(egui::Label::new(&path_text).truncate().selectable(false))
                                    .on_hover_text(&path_text);
                            });
                        }

                        ui.add_space(8.0);
                        if eden_button(
                            ui,
                            palette,
                            &format!(
                                "{} {}",
                                regular::FOLDER_OPEN,
                                i18n.tr("tab_group_add_folder")
                            ),
                        )
                        .clicked()
                        {
                            if let Some(path) = crate::gui::windows::windowsoverrides::dialog().pick_folder() {
                                // Duplicates are allowed on purpose - the same
                                // folder can be added more than once so opening
                                // the group opens it as multiple separate tabs.
                                group
                                    .entries
                                    .push(crate::core::tab_groups::TabGroupEntry::new(path));
                                changed = true;
                            }
                        }

                        ui.add_space(8.0);
                        if group.entries.is_empty() {
                            ui.weak(i18n.tr("tab_group_empty"));
                        } else {
                            let mut remove_entry: Option<usize> = None;
                            let mut move_entry: Option<(usize, usize)> = None;
                            let mut clear_split: Option<usize> = None;
                            let mut set_split: Option<(usize, std::path::PathBuf)> = None;
                            let entry_total = group.entries.len();
                            for (entry_index, entry) in group.entries.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    if let Some(swap) =
                                        reorder_buttons(ui, palette, entry_index, entry_total)
                                    {
                                        move_entry = Some(swap);
                                    }
                                    if eden_button(ui, palette, regular::TRASH).clicked() {
                                        remove_entry = Some(entry_index);
                                    }
                                    if let Some(texture) = icon_cache.get(&entry.path, true) {
                                        ui.add(
                                            egui::Image::new(&texture)
                                                .fit_to_exact_size(FOLDER_ICON_SIZE),
                                        );
                                    }
                                    ui.label(entry.path.display().to_string())
                                        .on_hover_text(entry.path.display().to_string());
                                });

                                // The dual-pane half of this entry - either
                                // shows the split folder (with its own
                                // remove button) or a small link to set one,
                                // indented under the primary folder so the
                                // pairing reads as "this entry opens as one
                                // dual-pane tab", not two separate entries.
                                ui.horizontal(|ui| {
                                    ui.add_space(24.0);
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(regular::COLUMNS)
                                                .color(palette.text_normal.gamma_multiply(0.7)),
                                        )
                                        .selectable(false),
                                    );
                                    if let Some(split_path) = &entry.split_path {
                                        if eden_button(ui, palette, regular::X)
                                            .on_hover_text(i18n.tr("tab_group_remove_split"))
                                            .clicked()
                                        {
                                            clear_split = Some(entry_index);
                                        }
                                        if let Some(texture) = icon_cache.get(split_path, true) {
                                            ui.add(
                                                egui::Image::new(&texture)
                                                    .fit_to_exact_size(FOLDER_ICON_SIZE),
                                            );
                                        }
                                        ui.label(split_path.display().to_string())
                                            .on_hover_text(split_path.display().to_string());
                                    } else if eden_button(
                                        ui,
                                        palette,
                                        &format!(
                                            "{} {}",
                                            regular::FOLDER_OPEN,
                                            i18n.tr("tab_group_add_split_folder")
                                        ),
                                    )
                                    .on_hover_text(i18n.tr("tooltip_tab_group_add_split_folder"))
                                    .clicked()
                                    {
                                        if let Some(path) =
                                            crate::gui::windows::windowsoverrides::dialog()
                                                .pick_folder()
                                        {
                                            set_split = Some((entry_index, path));
                                        }
                                    }
                                });
                                ui.add_space(6.0);
                            }
                            if let Some((from, to)) = move_entry {
                                group.entries.swap(from, to);
                                changed = true;
                            }
                            if let Some(i) = clear_split {
                                group.entries[i].split_path = None;
                                changed = true;
                            }
                            if let Some((i, path)) = set_split {
                                group.entries[i].split_path = Some(path);
                                changed = true;
                            }
                            if let Some(i) = remove_entry {
                                group.entries.remove(i);
                                changed = true;
                            }
                        }
                    });
                });
            },
        );
    });

    if let Some((from, to)) = move_indices {
        settings.current_settings.tab_groups.swap(from, to);
        changed = true;
    }
    if let Some(i) = remove_index {
        let removed_id = settings.current_settings.tab_groups[i].id;
        settings.current_settings.tab_groups.remove(i);
        if settings.selected_tab_group_id == Some(removed_id) {
            settings.selected_tab_group_id =
                settings.current_settings.tab_groups.first().map(|g| g.id);
        }
        changed = true;
    }

    if changed {
        action = Some(SettingsAction::ApplySettings);
    }

    action
}

/// One selectable row in a master-detail page's left-column list - a
/// smaller nested card, like `entry_card`, but click-sensitive and with a
/// distinct fill/border when selected. Returns the row's own click response
/// so the caller decides what selecting it means.
///
/// Senses the row's own background click *before* drawing `add_contents`
/// (its buttons included), rather than after - egui gives click priority to
/// whichever overlapping sense was registered later, so sensing the
/// background first and drawing (and thus sensing) the buttons afterward is
/// what lets a reorder/delete button inside the row still work, instead of
/// the row-select swallowing every click in its rect including the ones
/// meant for a button on top of it. Mirrors the same background-sensed-
/// first-then-content-drawn-on-top pattern the tab strip's own drag
/// background already uses (`mainwindow.rs`).
fn list_row(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    selected: bool,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    // Pin this row's own automatic between-widget spacing so the gap between
    // rows is fully controlled by the trailing `add_space` below, regardless
    // of whatever ambient item_spacing this page's earlier buttons left the
    // `ui` in - see the identical comment in `context_menu_settings_ui.rs`'s
    // `list_row` for why that ambient state isn't reliable across pages that
    // otherwise share this exact helper.
    ui.spacing_mut().item_spacing.y = 0.0;

    let row_height = ui.spacing().interact_size.y + 20.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_height),
        egui::Sense::click(),
    );

    if ui.is_rect_visible(rect) {
        let fill = if selected {
            palette.primary_hover
        } else {
            palette.row_bg
        };
        let stroke_color = if selected {
            palette.borders_active
        } else {
            palette.borders_default
        };
        let corner = egui::CornerRadius::same(palette.small_radius);
        ui.painter().rect_filled(rect, corner, fill);
        ui.painter()
            .rect_stroke(rect, corner, egui::Stroke::new(1.5, stroke_color), egui::StrokeKind::Inside);
    }

    let content_rect = rect.shrink2(egui::vec2(10.0, 6.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(content_rect), |ui| {
        ui.horizontal_centered(|ui| add_contents(ui));
    });

    ui.add_space(8.0);

    response
}
