//! Settings UI for managing sidebar Favorites: rename, delete, change icon,
//! change target folder, and reorder - mirroring how Tags, Tab Groups, and
//! Custom Context Menu entries are managed elsewhere in Settings.

use crate::core::utils::widgets::{apply_eden_visual_overrides, eden_button, eden_text_label};
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::containers::structs::FavoriteItem;
use crate::gui::windows::settings::{
    empty_state_hint, entry_card, reorder_buttons, setting_label, setting_row, settings_section,
};
use eframe::egui;
use egui::RichText;
use egui_phosphor::regular;

const ICON_ROW_SIZE: egui::Vec2 = egui::vec2(16.0, 16.0);

pub fn draw_favorites_settings(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    icon_cache: &IconCache,
    favorites: &mut Vec<FavoriteItem>,
    icon_picker_search: &mut String,
) -> bool {
    let mut changed = false;

    settings_section(ui, palette, |ui| {
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
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
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
            return;
        }

        let mut remove_index: Option<usize> = None;
        let mut move_indices: Option<(usize, usize)> = None;
        let total_len = favorites.len();

        for index in 0..total_len {
            let header_label = favorites[index].label.clone();
            let header_icon_glyph = favorites[index].custom_icon.clone();
            let header_icon_file = favorites[index].custom_icon_file.clone();
            let path_for_icon = favorites[index].path.clone();

            entry_card(ui, palette, |ui| {
                let header_id =
                    ui.make_persistent_id(("favorite_entry", index, path_for_icon.clone()));
                let header_state = egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    header_id,
                    false,
                );

                let header_response = header_state.show_header(ui, |ui| {
                    if let Some(texture) = header_icon_file
                        .as_deref()
                        .and_then(|f| icon_cache.get_custom_file_icon(f))
                    {
                        ui.add(egui::Image::new(&texture).fit_to_exact_size(ICON_ROW_SIZE));
                    } else if let Some(glyph) = &header_icon_glyph {
                        ui.label(RichText::new(glyph.as_str()).size(16.0));
                    } else if let Some(texture) = icon_cache.get(&path_for_icon, true) {
                        ui.add(egui::Image::new(&texture).fit_to_exact_size(ICON_ROW_SIZE));
                    } else {
                        ui.label(regular::FOLDER);
                    }
                    ui.label(RichText::new(&header_label).strong());

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

                let _ = header_response.body(|ui| {
                    ui.add_space(4.0);
                    let fav = &mut favorites[index];

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
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
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
                        let is_default = fav.custom_icon.is_none() && fav.custom_icon_file.is_none();
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
                    });
                    ui.add_space(4.0);
                    if let Some(glyph) = crate::gui::windows::icon_picker_ui::draw_icon_picker(
                        ui,
                        i18n,
                        palette,
                        icon_picker_search,
                        ("favorite_icon_picker", index),
                        |glyph| {
                            fav.custom_icon_file.is_none() && fav.custom_icon.as_deref() == Some(glyph)
                        },
                    ) {
                        fav.custom_icon = Some(glyph);
                        fav.custom_icon_file = None;
                        changed = true;
                    }

                    // A user-browsed image file's own icon, on its own row
                    // below the glyph choices - same layout as the Custom
                    // Context Menu icon picker.
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if eden_button(ui, palette, &i18n.tr("favorite_icon_browse")).clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter(
                                    "Icon/Image",
                                    &["ico", "png", "jpg", "jpeg", "bmp", "gif"],
                                )
                                .add_filter("All files", &["*"])
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
                        if let Some(file) = &fav.custom_icon_file {
                            if let Some(texture) = icon_cache.get_custom_file_icon(file) {
                                ui.add(
                                    egui::Image::new(&texture)
                                        .fit_to_exact_size(egui::vec2(20.0, 20.0)),
                                );
                            }
                            ui.label(file.display().to_string())
                                .on_hover_text(file.display().to_string());
                        }
                    });
                });
            });
        }

        if let Some((from, to)) = move_indices {
            favorites.swap(from, to);
            changed = true;
        }
        if let Some(i) = remove_index {
            favorites.remove(i);
            changed = true;
        }
    });

    changed
}
