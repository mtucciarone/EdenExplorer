//! Settings UI for user-defined custom context menu entries (see
//! `core::context_menu_settings`). One top level of submenu nesting is
//! supported (a submenu's children are always leaf commands), matching how
//! this is actually used in practice.
//!
//! Master-detail layout: the left column lists every top-level entry (icon +
//! label, reorderable, deletable, click to select); the right column shows
//! the selected entry's own fields, and - if it's a submenu - its children
//! below as always-visible cards (no collapse/expand), each individually
//! reorderable/deletable. Selection is tracked by the entry's stable `id`
//! (`SettingsWindow::selected_context_menu_id`).

use crate::core::context_menu_settings::{
    CustomContextMenuEntry, CustomContextMenuIcon, next_entry_id,
};
use crate::core::utils::widgets::{apply_eden_visual_overrides, eden_button, eden_text_label};
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::enums::SettingsAction;
use crate::gui::windows::settings::{
    count_badge, empty_state_hint, entry_card, info_icon, master_detail_column_size,
    no_selection_hint, reorder_buttons, setting_checkbox, setting_label, setting_row,
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

    setting_label(
        ui,
        &i18n.tr("settings_custom_context_menu"),
        Some((&i18n.tr("tooltip_settings_custom_context_menu"), palette)),
        palette,
    );
    ui.add_space(8.0);

    settings_section(ui, palette, |ui| {
        ui.horizontal(|ui| {
            let enabled_toggle = !settings.current_settings.custom_context_menu.is_empty();
            ui.add_enabled_ui(enabled_toggle, |ui| {
                if setting_checkbox(
                    ui,
                    palette,
                    &mut settings.current_settings.custom_context_menu_enabled,
                    RichText::new(i18n.tr("custom_context_menu_show_in_context_menu")),
                    "custom_context_menu_toggle",
                ) {
                    action = Some(SettingsAction::ApplySettings);
                }
            });
            info_icon(
                ui,
                &i18n.tr(if enabled_toggle {
                    "tooltip_custom_context_menu_show_in_context_menu"
                } else {
                    "tooltip_custom_context_menu_show_in_context_menu_disabled"
                }),
                palette,
            );
        });
    });

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
            settings.selected_context_menu_id = Some(id);
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
            settings.selected_context_menu_id = Some(id);
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
        settings.icon_picker_search = icon_picker_search;
        return action;
    }

    if settings.selected_context_menu_id.is_none()
        || !settings
            .current_settings
            .custom_context_menu
            .iter()
            .any(|e| Some(e.id) == settings.selected_context_menu_id)
    {
        settings.selected_context_menu_id = settings
            .current_settings
            .custom_context_menu
            .first()
            .map(|e| e.id);
    }

    let mut remove_index: Option<usize> = None;
    let mut move_indices: Option<(usize, usize)> = None;
    let mut changed = false;
    let total_len = settings.current_settings.custom_context_menu.len();

    let (col_w, col_h) = master_detail_column_size(ui);
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(col_w, col_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
            egui::ScrollArea::vertical()
                .id_salt("ccm_list_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (index, entry) in settings
                        .current_settings
                        .custom_context_menu
                        .iter()
                        .enumerate()
                    {
                        let label = if entry.label.is_empty() {
                            i18n.tr("custom_context_menu_untitled")
                        } else {
                            entry.label.clone()
                        };
                        let icon = match &entry.icon {
                            CustomContextMenuIcon::Glyph(g) => g.clone(),
                            _ => (if entry.is_submenu {
                                regular::LIST
                            } else {
                                regular::TERMINAL
                            })
                            .to_string(),
                        };
                        let is_selected = Some(entry.id) == settings.selected_context_menu_id;

                        let row = list_row(ui, palette, is_selected, |ui| {
                            // `.selectable(false)` on both - a plain
                            // `ui.label` is selectable text by default in
                            // this app's style, which registers its own
                            // click sense *after* the row's own background
                            // sense and would otherwise steal a click meant
                            // to select the row (see `list_row`'s own doc
                            // comment on background-sensed-first).
                            ui.add(egui::Label::new(&icon).selectable(false));
                            ui.add(
                                egui::Label::new(egui::RichText::new(&label).strong())
                                    .selectable(false),
                            );

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
                                    count_badge(ui, palette, entry.children.len());
                                }
                            });
                        });

                        if row.clicked() {
                            settings.selected_context_menu_id = Some(entry.id);
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
                .id_salt("ccm_detail_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let Some(selected_id) = settings.selected_context_menu_id else {
                        no_selection_hint(
                            ui,
                            palette,
                            regular::LIST,
                            &i18n.tr("custom_context_menu_select_hint"),
                        );
                        return;
                    };
                    let Some(entry) = settings
                        .current_settings
                        .custom_context_menu
                        .iter_mut()
                        .find(|e| e.id == selected_id)
                    else {
                        return;
                    };

                    settings_section(ui, palette, |ui| {
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
                    });

                    if entry.is_submenu {
                        ui.add_space(6.0);
                        settings_section(ui, palette, |ui| {
                            eden_text_label(ui, palette, &i18n.tr("custom_context_menu_children"));
                            ui.add_space(8.0);

                            if entry.children.is_empty() {
                                ui.weak(i18n.tr("custom_context_menu_no_children"));
                            }

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
                                    ui.horizontal(|ui| {
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
                                    ui.add_space(6.0);
                                    ui.separator();
                                    ui.add_space(6.0);

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
                            }
                            if let Some((from, to)) = move_child {
                                entry.children.swap(from, to);
                                changed = true;
                            }
                            if let Some(i) = remove_child {
                                entry.children.remove(i);
                                changed = true;
                            }

                            ui.add_space(4.0);
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
            },
        );
    });

    if let Some((from, to)) = move_indices {
        settings.current_settings.custom_context_menu.swap(from, to);
        changed = true;
    }

    if let Some(i) = remove_index {
        let removed_id = settings.current_settings.custom_context_menu[i].id;
        settings.current_settings.custom_context_menu.remove(i);
        if settings.selected_context_menu_id == Some(removed_id) {
            settings.selected_context_menu_id = settings
                .current_settings
                .custom_context_menu
                .first()
                .map(|e| e.id);
        }
        // The context-menu toggle is meaningless with zero entries - reset
        // it rather than leaving a dangling "enabled" flag with nothing to
        // show, same as `core::send_to`'s own design.
        if settings.current_settings.custom_context_menu.is_empty() {
            settings.current_settings.custom_context_menu_enabled = false;
        }
        changed = true;
    }

    if changed {
        action = Some(SettingsAction::ApplySettings);
    }

    settings.icon_picker_search = icon_picker_search;

    action
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
    // Pin this row's own automatic between-widget spacing rather than
    // trusting whatever this page's earlier buttons happened to leave it at
    // - `eden_button`'s style override only survives on whichever `Ui` it
    // was actually called on, so a button wrapped in its own nested
    // `ui.horizontal(|ui| ...)` (as this page's Add/Export/Import buttons
    // are) never leaks that override back out to the list's own `ui`, while
    // another page calling a button directly on its outer `ui` does - two
    // pages sharing this exact `list_row` source can otherwise render a
    // visibly different gap between rows for that reason alone. The
    // trailing `add_space` below is what actually controls the gap now.
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
    let current_glyph = match &entry.icon {
        CustomContextMenuIcon::Glyph(g) => Some(g.as_str()),
        _ => None,
    };
    if let Some(glyph) = crate::gui::windows::icon_picker_ui::draw_icon_picker_button(
        ui,
        i18n,
        palette,
        icon_picker_search,
        ("ccm_icon_picker", entry.id),
        current_glyph,
        regular::SHAPES,
    ) {
        entry.icon = CustomContextMenuIcon::Glyph(glyph);
        changed = true;
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if eden_button(ui, palette, &i18n.tr("custom_context_menu_icon_browse")).clicked() {
            if let Some(path) = crate::gui::windows::windowsoverrides::dialog()
                .add_filter("Icon/Image", &["ico", "png", "jpg", "jpeg", "bmp", "gif"])
                .add_filter("All Files", &["*"])
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
    });
    // Icon preview + file path go on their own row below the Browse/Clear
    // buttons - a real path is often long enough to run past the column's
    // own right edge when everything shares one row (same fix as Favorites'
    // identical icon-file picker).
    if let CustomContextMenuIcon::FileIcon(path) = entry.icon.clone() {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let is_image = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| IMAGE_ICON_EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e)));
            let texture = if is_image {
                icon_cache.get_custom_file_icon(&path)
            } else {
                icon_cache.get(&path, false)
            };
            if let Some(texture) = texture {
                ui.add(egui::Image::new(&texture).fit_to_exact_size(egui::vec2(20.0, 20.0)));
            }
            let path_text = path.display().to_string();
            ui.add(egui::Label::new(&path_text).truncate().selectable(false))
                .on_hover_text(&path_text);
        });
    }

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
                    if let Some(path) = crate::gui::windows::windowsoverrides::dialog().pick_file() {
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
