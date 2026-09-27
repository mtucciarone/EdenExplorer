//! Settings UI for managing sidebar Favorites: rename, delete, change icon,
//! change target folder, and reorder - mirroring how Tags, Tab Groups, and
//! Custom Context Menu entries are managed elsewhere in Settings.
//!
//! Master-detail layout: the left column lists every favorite (icon + name,
//! reorderable, deletable, click to select); the right column shows the
//! selected favorite's own name/location/icon fields. Selection is tracked
//! by index (`SettingsWindow::selected_favorite_index`) rather than a
//! stable id, since `FavoriteItem` has none - reorder/remove below adjust
//! it explicitly so it keeps pointing at the same entry.

use crate::core::utils::widgets::{apply_eden_visual_overrides, eden_button, eden_text_label};
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::containers::structs::FavoriteItem;
use crate::gui::windows::settings::{
    empty_state_hint, master_detail_column_size, no_selection_hint, reorder_buttons,
    setting_label, setting_row, settings_section,
};
use crate::gui::windows::structs::SettingsWindow;
use eframe::egui;
use egui::RichText;
use egui_phosphor::regular;

const ICON_ROW_SIZE: egui::Vec2 = egui::vec2(16.0, 16.0);

pub fn draw_favorites_settings(
    ui: &mut egui::Ui,
    i18n: &I18n,
    settings: &mut SettingsWindow,
    palette: &ThemePalette,
    icon_cache: &IconCache,
    favorites: &mut Vec<FavoriteItem>,
) -> bool {
    let mut changed = false;

    setting_label(
        ui,
        &i18n.tr("settings_favorites"),
        Some((&i18n.tr("tooltip_settings_favorites"), palette)),
        palette,
    );
    ui.add_space(8.0);

    if eden_button(
        ui,
        palette,
        &format!("{} {}", regular::PLUS, i18n.tr("favorite_add")),
    )
    .clicked()
    {
        if let Some(path) = crate::gui::windows::windowsoverrides::dialog().pick_folder() {
            let label = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string());
            favorites.push(FavoriteItem {
                path,
                label,
                custom_icon: None,
                custom_icon_file: None,
            });
            settings.selected_favorite_index = Some(favorites.len() - 1);
            changed = true;
        }
    }

    ui.add_space(10.0);

    if favorites.is_empty() {
        empty_state_hint(
            ui,
            palette,
            regular::STAR,
            &i18n.tr("favorite_empty_state"),
        );
        return changed;
    }

    // Keep a valid selection - default to the first favorite once there's
    // anything to select.
    match settings.selected_favorite_index {
        Some(i) if i < favorites.len() => {}
        _ => settings.selected_favorite_index = Some(0),
    }

    let mut remove_index: Option<usize> = None;
    let mut move_indices: Option<(usize, usize)> = None;
    let total_len = favorites.len();

    let (col_w, col_h) = master_detail_column_size(ui);
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(col_w, col_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
            egui::ScrollArea::vertical()
                .id_salt("favorites_list_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for index in 0..total_len {
                        let label = favorites[index].label.clone();
                        let icon_glyph = favorites[index].custom_icon.clone();
                        let icon_file = favorites[index].custom_icon_file.clone();
                        let path_for_icon = favorites[index].path.clone();
                        let is_selected = settings.selected_favorite_index == Some(index);

                        let row = list_row(ui, palette, is_selected, |ui| {
                            if let Some(texture) = icon_file
                                .as_deref()
                                .and_then(|f| icon_cache.get_custom_file_icon(f))
                            {
                                ui.add(egui::Image::new(&texture).fit_to_exact_size(ICON_ROW_SIZE));
                            } else if let Some(glyph) = &icon_glyph {
                                ui.add(
                                    egui::Label::new(RichText::new(glyph.as_str()).size(16.0))
                                        .selectable(false),
                                );
                            } else if let Some(texture) = icon_cache.get(&path_for_icon, true) {
                                ui.add(egui::Image::new(&texture).fit_to_exact_size(ICON_ROW_SIZE));
                            } else {
                                // `.selectable(false)` - a plain `ui.label` is
                                // selectable text by default in this app's
                                // style, which registers its own click sense
                                // *after* the row's own background sense and
                                // would otherwise steal a click meant to
                                // select the row (see `list_row`'s own doc
                                // comment on background-sensed-first).
                                ui.add(egui::Label::new(regular::FOLDER).selectable(false));
                            }
                            ui.add(egui::Label::new(RichText::new(&label).strong()).selectable(false));

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if let Some(swap) = reorder_buttons(ui, palette, index, total_len) {
                                    move_indices = Some(swap);
                                }
                                if eden_button(ui, palette, regular::TRASH)
                                    .on_hover_text(i18n.tr("favorite_remove"))
                                    .clicked()
                                {
                                    remove_index = Some(index);
                                }
                            });
                        });

                        if row.clicked() {
                            settings.selected_favorite_index = Some(index);
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
                .id_salt("favorites_detail_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let Some(index) = settings.selected_favorite_index else {
                        no_selection_hint(
                            ui,
                            palette,
                            regular::STAR,
                            &i18n.tr("favorite_select_hint"),
                        );
                        return;
                    };
                    let Some(fav) = favorites.get_mut(index) else {
                        return;
                    };

                    settings_section(ui, palette, |ui| {
                        setting_row(
                            ui,
                            |ui| {
                                setting_label(ui, &i18n.tr("favorite_name"), None, palette);
                            },
                            |ui| {
                                apply_eden_visual_overrides(ui, palette);
                                changed |= ui
                                    .add_sized(
                                        [260.0, ui.spacing().interact_size.y],
                                        egui::TextEdit::singleline(&mut fav.label),
                                    )
                                    .changed();
                            },
                        );

                        ui.add_space(8.0);
                        setting_label(ui, &i18n.tr("favorite_location"), None, palette);
                        ui.horizontal(|ui| {
                            if eden_button(ui, palette, regular::FOLDER_OPEN)
                                .on_hover_text(i18n.tr("favorite_location_browse"))
                                .clicked()
                            {
                                if let Some(path) = crate::gui::windows::windowsoverrides::dialog().pick_folder() {
                                    fav.path = path;
                                    changed = true;
                                }
                            }
                            ui.label(fav.path.display().to_string())
                                .on_hover_text(fav.path.display().to_string());
                        });

                        ui.add_space(8.0);
                        eden_text_label(ui, palette, &i18n.tr("favorite_icon"));
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let is_default =
                                fav.custom_icon.is_none() && fav.custom_icon_file.is_none();
                            let default_color = if is_default {
                                palette.primary
                            } else {
                                ui.visuals().text_color()
                            };
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(regular::IMAGE)
                                            .size(16.0)
                                            .color(default_color),
                                    )
                                    .min_size(egui::vec2(24.0, 24.0)),
                                )
                                .on_hover_text(i18n.tr("favorite_icon_default"))
                                .clicked()
                            {
                                fav.custom_icon = None;
                                fav.custom_icon_file = None;
                                changed = true;
                            }

                            ui.add_space(4.0);

                            let current_glyph = if fav.custom_icon_file.is_none() {
                                fav.custom_icon.as_deref()
                            } else {
                                None
                            };
                            if let Some(glyph) =
                                crate::gui::windows::icon_picker_ui::draw_icon_picker_button(
                                    ui,
                                    i18n,
                                    palette,
                                    &mut settings.icon_picker_search,
                                    ("favorite_icon_picker", index),
                                    current_glyph,
                                    regular::SHAPES,
                                )
                            {
                                fav.custom_icon = Some(glyph);
                                fav.custom_icon_file = None;
                                changed = true;
                            }
                        });

                        // A user-browsed image file's own icon, on its own row
                        // below the glyph choices - same layout as the Custom
                        // Context Menu icon picker. The icon preview + file
                        // path go on a *separate* row below the Browse/Clear
                        // buttons rather than sharing one - a real path is
                        // often long enough to run past the column's own
                        // right edge when everything shares one row.
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if eden_button(ui, palette, &i18n.tr("favorite_icon_browse")).clicked() {
                                if let Some(path) = crate::gui::windows::windowsoverrides::dialog()
                                    .add_filter(
                                        "Icon/Image",
                                        &["ico", "png", "jpg", "jpeg", "bmp", "gif"],
                                    )
                                    .add_filter("All Files", &["*"])
                                    .pick_file()
                                {
                                    // Copy into the app's own data folder so the
                                    // icon keeps working (and settings export/
                                    // import points somewhere stable) even if
                                    // the original file is moved or deleted.
                                    let stored_path =
                                        crate::core::indexer::import_custom_icon(&path)
                                            .unwrap_or(path);
                                    fav.custom_icon_file = Some(stored_path);
                                    fav.custom_icon = None;
                                    changed = true;
                                }
                            }
                            if eden_button(ui, palette, &i18n.tr("favorite_icon_clear")).clicked() {
                                fav.custom_icon_file = None;
                                changed = true;
                            }
                        });
                        if let Some(file) = fav.custom_icon_file.clone() {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if let Some(texture) = icon_cache.get_custom_file_icon(&file) {
                                    ui.add(
                                        egui::Image::new(&texture)
                                            .fit_to_exact_size(egui::vec2(20.0, 20.0)),
                                    );
                                }
                                let path_text = file.display().to_string();
                                ui.add(
                                    egui::Label::new(&path_text)
                                        .truncate()
                                        .selectable(false),
                                )
                                .on_hover_text(&path_text);
                            });
                        }
                    });
                });
            },
        );
    });

    if let Some((from, to)) = move_indices {
        favorites.swap(from, to);
        // Keep tracking the same entry through the swap.
        settings.selected_favorite_index = match settings.selected_favorite_index {
            Some(i) if i == from => Some(to),
            Some(i) if i == to => Some(from),
            other => other,
        };
        changed = true;
    }
    if let Some(i) = remove_index {
        favorites.remove(i);
        settings.selected_favorite_index = match settings.selected_favorite_index {
            Some(sel) if sel == i => {
                if favorites.is_empty() {
                    None
                } else {
                    Some(sel.min(favorites.len() - 1))
                }
            }
            Some(sel) if sel > i => Some(sel - 1),
            other => other,
        };
        changed = true;
    }

    changed
}

/// One selectable row in a master-detail page's left-column list - a
/// smaller nested card, like `entry_card`, but click-sensitive and with a
/// distinct fill/border when selected.
///
/// Senses the row's own background click *before* drawing `add_contents`
/// (its buttons included), rather than after - egui gives click priority to
/// whichever overlapping sense was registered later, so sensing the
/// background first and drawing (and thus sensing) the buttons afterward is
/// what lets a reorder/delete button inside the row still work, instead of
/// the row-select swallowing every click in its rect including the ones
/// meant for a button on top of it.
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
