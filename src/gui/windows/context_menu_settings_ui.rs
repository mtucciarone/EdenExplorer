//! Settings UI for user-defined custom context menu entries (see
//! `core::context_menu_settings`). One top level of submenu nesting is
//! supported (a submenu's children are always leaf commands), matching how
//! this is actually used in practice.

use crate::core::context_menu_settings::{
    CustomContextMenuEntry, CustomContextMenuIcon, next_entry_id,
};
use crate::core::utils::widgets::{apply_eden_visual_overrides, eden_button, eden_text_label};
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::enums::SettingsAction;
use crate::gui::windows::settings::{
    empty_state_hint, entry_card, reorder_buttons, setting_checkbox, setting_label, setting_row,
    settings_section,
};
use crate::gui::windows::structs::SettingsWindow;
use eframe::egui;
use egui::RichText;
use egui_phosphor::regular;

/// Extensions previewed as an image's own pixel content; anything else (an
/// `.exe`/`.dll`, typically) falls back to the shell icon for that file -
/// mirroring the same dispatch used when the command actually runs.
const IMAGE_ICON_EXTENSIONS: &[&str] = &["ico", "png", "jpg", "jpeg", "bmp", "gif"];

pub fn draw_custom_context_menu_settings(
    ui: &mut egui::Ui,
    i18n: &I18n,
    settings: &mut SettingsWindow,
    palette: &ThemePalette,
    icon_cache: &IconCache,
) -> Option<SettingsAction> {
    let mut action = None;
    let mut icon_picker_search = std::mem::take(&mut settings.icon_picker_search);

    settings_section(ui, palette, |ui| {
        setting_label(
            ui,
            &i18n.tr("settings_custom_context_menu"),
            Some((&i18n.tr("tooltip_settings_custom_context_menu"), palette)),
            palette,
        );
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            if eden_button(
                ui,
                palette,
                &format!(
                    "{} {}",
                    regular::PLUS,
                    i18n.tr("custom_context_menu_add_command")
                ),
            )
            .clicked()
            {
                let id = next_entry_id(&settings.current_settings.custom_context_menu);
                settings
                    .current_settings
                    .custom_context_menu
                    .push(CustomContextMenuEntry::new_leaf(id));
                action = Some(SettingsAction::ApplySettings);
            }
            if eden_button(
                ui,
                palette,
                &format!(
                    "{} {}",
                    regular::PLUS,
                    i18n.tr("custom_context_menu_add_submenu")
                ),
            )
            .clicked()
            {
                let id = next_entry_id(&settings.current_settings.custom_context_menu);
                settings
                    .current_settings
                    .custom_context_menu
                    .push(CustomContextMenuEntry::new_submenu(id));
                action = Some(SettingsAction::ApplySettings);
            }
        });

        ui.add_space(6.0);

        ui.horizontal(|ui| {
            if eden_button(ui, palette, &i18n.tr("custom_context_menu_export")).clicked() {
                action = Some(SettingsAction::ExportContextMenu);
            }
            if eden_button(ui, palette, &i18n.tr("custom_context_menu_import")).clicked() {
                action = Some(SettingsAction::ImportContextMenu);
            }
        });

        ui.add_space(10.0);

        if settings.current_settings.custom_context_menu.is_empty() {
            empty_state_hint(
                ui,
                palette,
                regular::LIST,
                &i18n.tr("custom_context_menu_empty_state"),
            );
            return;
        }

        let mut remove_index: Option<usize> = None;
        let mut move_indices: Option<(usize, usize)> = None;
        let mut changed = false;
        let total_len = settings.current_settings.custom_context_menu.len();

        for (index, entry) in settings
            .current_settings
            .custom_context_menu
            .iter_mut()
            .enumerate()
        {
            let header_label = if entry.label.is_empty() {
                i18n.tr("custom_context_menu_untitled")
            } else {
                entry.label.clone()
            };
            let header_icon = match &entry.icon {
                CustomContextMenuIcon::Glyph(g) => g.clone(),
                _ => (if entry.is_submenu {
                    regular::LIST
                } else {
                    regular::TERMINAL
                })
                .to_string(),
            };

            entry_card(ui, palette, |ui| {
                let header_id = ui.make_persistent_id(("custom_ctx_menu_entry", entry.id));
                let header_state = egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    header_id,
                    false,
                );

                let header_response = header_state.show_header(ui, |ui| {
                    ui.label(&header_icon);
                    ui.label(egui::RichText::new(&header_label).strong());

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(swap) = reorder_buttons(ui, palette, index, total_len) {
                            move_indices = Some(swap);
                        }
                        if eden_button(ui, palette, regular::TRASH)
                            .on_hover_text(i18n.tr("custom_context_menu_remove"))
                            .clicked()
                        {
                            remove_index = Some(index);
                        }
                        if entry.is_submenu {
                            ui.label(
                                egui::RichText::new(format!("({})", entry.children.len()))
                                    .color(palette.tooltip_text_color),
                            );
                        }
                    });
                });

                let _ = header_response.body(|ui| {
                    ui.add_space(4.0);
                    if draw_entry_fields(
                        ui,
                        i18n,
                        palette,
                        icon_cache,
                        &mut icon_picker_search,
                        entry,
                        false,
                    ) {
                        changed = true;
                    }

                    if entry.is_submenu {
                        ui.add_space(8.0);
                        eden_text_label(ui, palette, &i18n.tr("custom_context_menu_children"));
                        ui.add_space(4.0);
                        ui.indent(("custom_ctx_menu_children", entry.id), |ui| {
                            let mut remove_child: Option<usize> = None;
                            let mut move_child: Option<(usize, usize)> = None;
                            let child_total = entry.children.len();
                            for (child_index, child) in entry.children.iter_mut().enumerate() {
                                let child_label = if child.label.is_empty() {
                                    i18n.tr("custom_context_menu_untitled")
                                } else {
                                    child.label.clone()
                                };
                                let child_icon = match &child.icon {
                                    CustomContextMenuIcon::Glyph(g) => g.clone(),
                                    _ => regular::TERMINAL.to_string(),
                                };

                                entry_card(ui, palette, |ui| {
                                    let child_header_id = ui
                                        .make_persistent_id(("custom_ctx_menu_child", entry.id, child.id));
                                    let child_header_state =
                                        egui::collapsing_header::CollapsingState::load_with_default_open(
                                            ui.ctx(),
                                            child_header_id,
                                            false,
                                        );

                                    let child_header_response =
                                        child_header_state.show_header(ui, |ui| {
                                            ui.label(&child_icon);
                                            ui.label(egui::RichText::new(&child_label).strong());

                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if let Some(swap) = reorder_buttons(
                                                        ui,
                                                        palette,
                                                        child_index,
                                                        child_total,
                                                    ) {
                                                        move_child = Some(swap);
                                                    }
                                                    if eden_button(ui, palette, regular::TRASH)
                                                        .on_hover_text(
                                                            i18n.tr("custom_context_menu_remove"),
                                                        )
                                                        .clicked()
                                                    {
                                                        remove_child = Some(child_index);
                                                    }
                                                },
                                            );
                                        });

                                    let _ = child_header_response.body(|ui| {
                                        ui.add_space(4.0);
                                        if draw_entry_fields(
                                            ui,
                                            i18n,
                                            palette,
                                            icon_cache,
                                            &mut icon_picker_search,
                                            child,
                                            true,
                                        ) {
                                            changed = true;
                                        }
                                    });
                                });
                            }
                            if let Some((from, to)) = move_child {
                                entry.children.swap(from, to);
                                changed = true;
                            }
                            if let Some(i) = remove_child {
                                entry.children.remove(i);
                                changed = true;
                            }

                            if eden_button(
                                ui,
                                palette,
                                &format!(
                                    "{} {}",
                                    regular::PLUS,
                                    i18n.tr("custom_context_menu_add_command")
                                ),
                            )
                            .clicked()
                            {
                                let id = entry
                                    .children
                                    .iter()
                                    .map(|c| c.id)
                                    .max()
                                    .unwrap_or(entry.id)
                                    + 1;
                                entry.children.push(CustomContextMenuEntry::new_leaf(id));
                                changed = true;
                            }
                        });
                    }
                });
            });
        }

        if let Some((from, to)) = move_indices {
            settings.current_settings.custom_context_menu.swap(from, to);
            changed = true;
        }

        if let Some(i) = remove_index {
            settings.current_settings.custom_context_menu.remove(i);
            changed = true;
        }

        if changed {
            action = Some(SettingsAction::ApplySettings);
        }
    });

    settings.icon_picker_search = icon_picker_search;

    action
}

/// Draws label/icon/(scope + submenu toggle)/(executable/arguments/run
/// options) fields for one entry. `is_child` suppresses the scope checkboxes
/// and "is submenu" toggle - children are always leaves and are shown
/// whenever their parent submenu is (the parent's own scope decides
/// applicability).
fn draw_entry_fields(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    icon_cache: &IconCache,
    icon_picker_search: &mut String,
    entry: &mut CustomContextMenuEntry,
    is_child: bool,
) -> bool {
    let mut changed = false;

    setting_row(
        ui,
        |ui| {
            setting_label(ui, &i18n.tr("custom_context_menu_label"), None, palette);
        },
        |ui| {
            apply_eden_visual_overrides(ui, palette);
            changed |= ui
                .add_sized(
                    [260.0, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut entry.label),
                )
                .changed();
        },
    );

    ui.add_space(10.0);
    eden_text_label(ui, palette, &i18n.tr("custom_context_menu_icon"));
    ui.add_space(4.0);
    if let Some(glyph) = crate::gui::windows::icon_picker_ui::draw_icon_picker(
        ui,
        i18n,
        palette,
        icon_picker_search,
        ("ccm_icon_picker", entry.id),
        |glyph| matches!(&entry.icon, CustomContextMenuIcon::Glyph(g) if g == glyph),
    ) {
        entry.icon = CustomContextMenuIcon::Glyph(glyph);
        changed = true;
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if eden_button(ui, palette, &i18n.tr("custom_context_menu_icon_browse")).clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Icon/Image", &["ico", "png", "jpg", "jpeg", "bmp", "gif"])
                .add_filter("All files", &["*"])
                .pick_file()
            {
                // Copy into the app's own data folder so the icon keeps
                // working (and settings export/import points somewhere
                // stable) even if the original file is moved or deleted.
                let stored_path =
                    crate::core::indexer::import_custom_icon(&path).unwrap_or(path);
                entry.icon = CustomContextMenuIcon::FileIcon(stored_path);
                changed = true;
            }
        }
        if eden_button(ui, palette, &i18n.tr("custom_context_menu_icon_clear")).clicked() {
            entry.icon = CustomContextMenuIcon::None;
            changed = true;
        }
        if let CustomContextMenuIcon::FileIcon(path) = &entry.icon {
            let is_image = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| IMAGE_ICON_EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e)));
            let texture = if is_image {
                icon_cache.get_custom_file_icon(path)
            } else {
                icon_cache.get(path, false)
            };
            if let Some(texture) = texture {
                ui.add(egui::Image::new(&texture).fit_to_exact_size(egui::vec2(20.0, 20.0)));
            }
            ui.label(path.display().to_string())
                .on_hover_text(path.display().to_string());
        }
    });

    if !is_child {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            changed |= setting_checkbox(
                ui,
                palette,
                &mut entry.applies_to_files,
                RichText::new(i18n.tr("custom_context_menu_scope_files")),
                ("ccm_files", entry.id),
            );
            ui.add_space(12.0);
            changed |= setting_checkbox(
                ui,
                palette,
                &mut entry.applies_to_folders,
                RichText::new(i18n.tr("custom_context_menu_scope_folders")),
                ("ccm_folders", entry.id),
            );
            ui.add_space(12.0);
            changed |= setting_checkbox(
                ui,
                palette,
                &mut entry.applies_to_background,
                RichText::new(i18n.tr("custom_context_menu_scope_background")),
                ("ccm_background", entry.id),
            );
        });

        ui.add_space(10.0);
        let mut is_submenu = entry.is_submenu;
        if setting_checkbox(
            ui,
            palette,
            &mut is_submenu,
            RichText::new(i18n.tr("custom_context_menu_is_submenu")),
            ("ccm_submenu", entry.id),
        ) {
            entry.is_submenu = is_submenu;
            changed = true;
        }
    }

    if !entry.is_submenu {
        ui.add_space(10.0);
        setting_row(
            ui,
            |ui| {
                setting_label(
                    ui,
                    &i18n.tr("custom_context_menu_executable"),
                    None,
                    palette,
                );
            },
            |ui| {
                if eden_button(ui, palette, regular::FOLDER_OPEN).clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                        entry.executable = path.display().to_string();
                        changed = true;
                    }
                }
                apply_eden_visual_overrides(ui, palette);
                changed |= ui
                    .add_sized(
                        [260.0, ui.spacing().interact_size.y],
                        egui::TextEdit::singleline(&mut entry.executable),
                    )
                    .changed();
            },
        );

        ui.add_space(8.0);
        setting_row(
            ui,
            |ui| {
                setting_label(
                    ui,
                    &i18n.tr("custom_context_menu_arguments"),
                    Some((&i18n.tr("tooltip_custom_context_menu_arguments"), palette)),
                    palette,
                );
            },
            |ui| {
                apply_eden_visual_overrides(ui, palette);
                changed |= ui
                    .add_sized(
                        [260.0, ui.spacing().interact_size.y],
                        egui::TextEdit::singleline(&mut entry.arguments),
                    )
                    .changed();
            },
        );

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            changed |= setting_checkbox(
                ui,
                palette,
                &mut entry.run_as_admin,
                RichText::new(i18n.tr("custom_context_menu_run_as_admin")),
                ("ccm_admin", entry.id),
            );
            ui.add_space(12.0);
            changed |= setting_checkbox(
                ui,
                palette,
                &mut entry.run_once_per_selection,
                RichText::new(i18n.tr("custom_context_menu_run_once_per_selection")),
                ("ccm_once", entry.id),
            );
        });
    }

    changed
}
