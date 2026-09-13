//! Settings UI for user-defined tab groups (see `core::tab_groups`) - named
//! sets of folder paths that can be opened all at once as tabs. The same
//! path may be added to a group more than once on purpose (opening the
//! group then opens that many separate tabs for it).

use crate::core::tab_groups::{TabGroup, next_group_id};
use crate::core::utils::widgets::{apply_eden_visual_overrides, eden_button};
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::enums::SettingsAction;
use crate::gui::windows::settings::{
    empty_state_hint, entry_card, reorder_buttons, setting_label, setting_row, settings_section,
};
use crate::gui::windows::structs::SettingsWindow;
use eframe::egui;
use egui_phosphor::regular;

/// Row/header icon size for a folder's real shell icon - small enough to sit
/// inline with text like any other icon in this app's menus/lists.
const FOLDER_ICON_SIZE: egui::Vec2 = egui::vec2(16.0, 16.0);

pub fn draw_tab_groups_settings(
    ui: &mut egui::Ui,
    i18n: &I18n,
    settings: &mut SettingsWindow,
    palette: &ThemePalette,
    icon_cache: &IconCache,
) -> Option<SettingsAction> {
    let mut action = None;

    settings_section(ui, palette, |ui| {
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
            action = Some(SettingsAction::ApplySettings);
        }

        ui.add_space(10.0);

        if settings.current_settings.tab_groups.is_empty() {
            empty_state_hint(
                ui,
                palette,
                regular::FOLDERS,
                &i18n.tr("tab_group_empty_state"),
            );
            return;
        }

        let mut remove_index: Option<usize> = None;
        let mut move_indices: Option<(usize, usize)> = None;
        let mut changed = false;
        let total_len = settings.current_settings.tab_groups.len();

        for (index, group) in settings.current_settings.tab_groups.iter_mut().enumerate() {
            let header_label = if group.name.is_empty() {
                i18n.tr("tab_group_untitled")
            } else {
                group.name.clone()
            };
            // The group's own representative icon is its first folder's real
            // shell icon (once loaded).
            let header_icon = group.paths.first().and_then(|p| icon_cache.get(p, true));

            entry_card(ui, palette, |ui| {
                let header_id = ui.make_persistent_id(("tab_group_entry", group.id));
                let header_state = egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    header_id,
                    false,
                );

                let header_response = header_state.show_header(ui, |ui| {
                    if let Some(texture) = &header_icon {
                        ui.add(egui::Image::new(texture).fit_to_exact_size(FOLDER_ICON_SIZE));
                    } else {
                        ui.label(regular::FOLDERS);
                    }
                    ui.label(egui::RichText::new(&header_label).strong());

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
                        ui.label(
                            egui::RichText::new(format!("({})", group.paths.len()))
                                .color(palette.tooltip_text_color),
                        );
                    });
                });

                let _ = header_response.body(|ui| {
                    ui.add_space(4.0);
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
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            // Duplicates are allowed on purpose - the same
                            // folder can be added more than once so opening
                            // the group opens it as multiple separate tabs.
                            group.paths.push(path);
                            changed = true;
                        }
                    }

                    ui.add_space(8.0);
                    if group.paths.is_empty() {
                        ui.weak(i18n.tr("tab_group_empty"));
                    } else {
                        let mut remove_path: Option<usize> = None;
                        let mut move_path: Option<(usize, usize)> = None;
                        let path_total = group.paths.len();
                        for (path_index, path) in group.paths.iter().enumerate() {
                            ui.horizontal(|ui| {
                                if let Some(swap) =
                                    reorder_buttons(ui, palette, path_index, path_total)
                                {
                                    move_path = Some(swap);
                                }
                                if eden_button(ui, palette, regular::TRASH).clicked() {
                                    remove_path = Some(path_index);
                                }
                                if let Some(texture) = icon_cache.get(path, true) {
                                    ui.add(
                                        egui::Image::new(&texture)
                                            .fit_to_exact_size(FOLDER_ICON_SIZE),
                                    );
                                }
                                ui.label(path.display().to_string())
                                    .on_hover_text(path.display().to_string());
                            });
                        }
                        if let Some((from, to)) = move_path {
                            group.paths.swap(from, to);
                            changed = true;
                        }
                        if let Some(i) = remove_path {
                            group.paths.remove(i);
                            changed = true;
                        }
                    }
                });
            });
        }

        if let Some((from, to)) = move_indices {
            settings.current_settings.tab_groups.swap(from, to);
            changed = true;
        }
        if let Some(i) = remove_index {
            settings.current_settings.tab_groups.remove(i);
            changed = true;
        }

        if changed {
            action = Some(SettingsAction::ApplySettings);
        }
    });

    action
}
