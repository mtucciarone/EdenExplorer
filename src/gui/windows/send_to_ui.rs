//! Settings UI for user-defined "Send To" groups (see `core::send_to`) -
//! named destinations, each holding an ordered list of folders, that show up
//! in a file/folder's right-click "Send To" submenu. Picking one of a
//! group's folders there **copies** the selection to it - the original
//! never moves.
//!
//! Master-detail layout: the left column lists every group (icon + name +
//! folder count, reorderable, deletable, click to select); the right column
//! shows the selected group's own name, icon, and folder list. Selection is
//! tracked by the group's stable `id` (`SettingsWindow::selected_send_to_id`),
//! same as Tab Groups/Custom Context Menu, so reordering or deleting an
//! unrelated group never silently re-points the detail pane at the wrong one.

use crate::core::send_to::{SendToGroup, SendToIcon, SendToMode, next_group_id};
use crate::core::utils::widgets::{
    apply_eden_visual_overrides, eden_button, eden_text_label, eden_toggle_button,
};
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::enums::SettingsAction;
use crate::gui::windows::settings::{
    count_badge, empty_state_hint, info_icon, master_detail_column_size, no_selection_hint,
    reorder_buttons, setting_checkbox, setting_label, setting_row, settings_section,
};
use crate::gui::windows::structs::SettingsWindow;
use eframe::egui;
use egui::RichText;
use egui_phosphor::regular;

/// Row/header icon size - matches every other master-detail page's own
/// constant (Favorites/Tab Groups), kept local since each page already
/// duplicates it rather than sharing one across files with unrelated icon
/// needs.
const ICON_ROW_SIZE: egui::Vec2 = egui::vec2(16.0, 16.0);

pub fn draw_send_to_settings(
    ui: &mut egui::Ui,
    i18n: &I18n,
    settings: &mut SettingsWindow,
    palette: &ThemePalette,
    icon_cache: &IconCache,
) -> Option<SettingsAction> {
    let mut action = None;
    let mut changed = false;

    setting_label(
        ui,
        &i18n.tr("settings_send_to"),
        Some((&i18n.tr("tooltip_settings_send_to"), palette)),
        palette,
    );
    ui.add_space(8.0);

    settings_section(ui, palette, |ui| {
        ui.horizontal(|ui| {
            let enabled_toggle = !settings.current_settings.send_to.is_empty();
            ui.add_enabled_ui(enabled_toggle, |ui| {
                if setting_checkbox(
                    ui,
                    palette,
                    &mut settings.current_settings.send_to_context_menu_enabled,
                    RichText::new(i18n.tr("send_to_show_in_context_menu")),
                    "send_to_context_menu_toggle",
                ) {
                    changed = true;
                }
            });
            info_icon(
                ui,
                &i18n.tr(if enabled_toggle {
                    "tooltip_send_to_show_in_context_menu"
                } else {
                    "tooltip_send_to_show_in_context_menu_disabled"
                }),
                palette,
            );
        });
    });

    ui.add_space(10.0);

    if eden_button(
        ui,
        palette,
        &format!("{} {}", regular::PLUS, i18n.tr("send_to_add")),
    )
    .clicked()
    {
        let id = next_group_id(&settings.current_settings.send_to);
        settings.current_settings.send_to.push(SendToGroup::new(id));
        settings.selected_send_to_id = Some(id);
        changed = true;
    }

    ui.add_space(6.0);

    ui.horizontal(|ui| {
        if eden_button(ui, palette, &i18n.tr("send_to_export")).clicked() {
            action = Some(SettingsAction::ExportSendTo);
        }
        if eden_button(ui, palette, &i18n.tr("send_to_import")).clicked() {
            action = Some(SettingsAction::ImportSendTo);
        }
    });

    ui.add_space(10.0);

    if settings.current_settings.send_to.is_empty() {
        empty_state_hint(
            ui,
            palette,
            regular::PAPER_PLANE_TILT,
            &i18n.tr("send_to_empty_state"),
        );
        if changed {
            action = Some(SettingsAction::ApplySettings);
        }
        return action;
    }

    // Keep a valid selection: default to the first group the first time
    // there's anything to select, and drop a selection that pointed at a
    // group which no longer exists.
    if settings.selected_send_to_id.is_none()
        || !settings
            .current_settings
            .send_to
            .iter()
            .any(|g| Some(g.id) == settings.selected_send_to_id)
    {
        settings.selected_send_to_id = settings.current_settings.send_to.first().map(|g| g.id);
    }

    let mut remove_index: Option<usize> = None;
    let mut move_indices: Option<(usize, usize)> = None;
    let total_len = settings.current_settings.send_to.len();

    let (col_w, col_h) = master_detail_column_size(ui);
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(col_w, col_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("send_to_list_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (index, group) in
                            settings.current_settings.send_to.iter().enumerate()
                        {
                            let label = if group.name.is_empty() {
                                i18n.tr("send_to_untitled")
                            } else {
                                group.name.clone()
                            };
                            let is_selected = Some(group.id) == settings.selected_send_to_id;

                            let row = list_row(ui, palette, is_selected, |ui| {
                                draw_group_icon(ui, icon_cache, &group.icon, ICON_ROW_SIZE);
                                ui.add(
                                    egui::Label::new(egui::RichText::new(&label).strong())
                                        .selectable(false),
                                );

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if let Some(swap) =
                                            reorder_buttons(ui, palette, index, total_len)
                                        {
                                            move_indices = Some(swap);
                                        }
                                        if eden_button(ui, palette, regular::TRASH)
                                            .on_hover_text(i18n.tr("send_to_remove"))
                                            .clicked()
                                        {
                                            remove_index = Some(index);
                                        }
                                        count_badge(ui, palette, group.folders.len());
                                    },
                                );
                            });

                            if row.clicked() {
                                settings.selected_send_to_id = Some(group.id);
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
                    .id_salt("send_to_detail_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let Some(selected_id) = settings.selected_send_to_id else {
                            no_selection_hint(
                                ui,
                                palette,
                                regular::PAPER_PLANE_TILT,
                                &i18n.tr("send_to_select_hint"),
                            );
                            return;
                        };
                        let Some(group) = settings
                            .current_settings
                            .send_to
                            .iter_mut()
                            .find(|g| g.id == selected_id)
                        else {
                            return;
                        };

                        settings_section(ui, palette, |ui| {
                            setting_row(
                                ui,
                                |ui| {
                                    setting_label(ui, &i18n.tr("send_to_name"), None, palette);
                                },
                                |ui| {
                                    apply_eden_visual_overrides(ui, palette);
                                    changed |= ui
                                        .add_sized(
                                            [260.0, ui.spacing().interact_size.y],
                                            egui::TextEdit::singleline(&mut group.name),
                                        )
                                        .changed();
                                },
                            );

                            ui.add_space(8.0);
                            eden_text_label(ui, palette, &i18n.tr("send_to_icon"));
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                let is_default = matches!(group.icon, SendToIcon::None);
                                let default_color = if is_default {
                                    palette.primary
                                } else {
                                    ui.visuals().text_color()
                                };
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(regular::PROHIBIT)
                                                .size(16.0)
                                                .color(default_color),
                                        )
                                        .min_size(egui::vec2(24.0, 24.0)),
                                    )
                                    .on_hover_text(i18n.tr("send_to_icon_default"))
                                    .clicked()
                                {
                                    group.icon = SendToIcon::None;
                                    changed = true;
                                }

                                ui.add_space(4.0);

                                let current_glyph = match &group.icon {
                                    SendToIcon::Glyph(g) => Some(g.as_str()),
                                    _ => None,
                                };
                                if let Some(glyph) =
                                    crate::gui::windows::icon_picker_ui::draw_icon_picker_button(
                                        ui,
                                        i18n,
                                        palette,
                                        &mut settings.icon_picker_search,
                                        ("send_to_icon_picker", selected_id),
                                        current_glyph,
                                        regular::SHAPES,
                                    )
                                {
                                    group.icon = SendToIcon::Glyph(glyph);
                                    changed = true;
                                }
                            });

                            // A user-browsed image file's own icon, on its own
                            // row below the glyph choices - same layout as
                            // Favorites' own icon picker.
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                if eden_button(ui, palette, &i18n.tr("send_to_icon_browse"))
                                    .clicked()
                                {
                                    if let Some(path) =
                                        crate::gui::windows::windowsoverrides::dialog()
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
                                        group.icon = SendToIcon::Custom(stored_path);
                                        changed = true;
                                    }
                                }
                                if eden_button(ui, palette, &i18n.tr("send_to_icon_clear"))
                                    .clicked()
                                {
                                    group.icon = SendToIcon::None;
                                    changed = true;
                                }
                            });
                            if let SendToIcon::Custom(file) = &group.icon {
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
                                    ui.add(
                                        egui::Label::new(&path_text).truncate().selectable(false),
                                    )
                                    .on_hover_text(&path_text);
                                });
                            }

                            ui.add_space(10.0);
                            eden_text_label(ui, palette, &i18n.tr("send_to_operation"));
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if eden_toggle_button(
                                    ui,
                                    palette,
                                    group.mode == SendToMode::Copy,
                                    &i18n.tr("send_to_mode_copy"),
                                )
                                .on_hover_text(i18n.tr("tooltip_send_to_mode_copy"))
                                .clicked()
                                {
                                    group.mode = SendToMode::Copy;
                                    changed = true;
                                }
                                ui.add_space(6.0);
                                if eden_toggle_button(
                                    ui,
                                    palette,
                                    group.mode == SendToMode::Move,
                                    &i18n.tr("send_to_mode_move"),
                                )
                                .on_hover_text(i18n.tr("tooltip_send_to_mode_move"))
                                .clicked()
                                {
                                    group.mode = SendToMode::Move;
                                    changed = true;
                                }
                            });

                            ui.add_space(10.0);
                            ui.separator();
                            ui.add_space(8.0);

                            if eden_button(
                                ui,
                                palette,
                                &format!(
                                    "{} {}",
                                    regular::FOLDER_OPEN,
                                    i18n.tr("send_to_add_folder")
                                ),
                            )
                            .clicked()
                            {
                                if let Some(path) =
                                    crate::gui::windows::windowsoverrides::dialog().pick_folder()
                                {
                                    // Duplicates are allowed on purpose - same
                                    // convention as Tab Groups.
                                    group.folders.push(path);
                                    changed = true;
                                }
                            }

                            ui.add_space(8.0);
                            if group.folders.is_empty() {
                                ui.weak(i18n.tr("send_to_no_folders"));
                            } else {
                                let mut remove_folder: Option<usize> = None;
                                let mut move_folder: Option<(usize, usize)> = None;
                                let folder_total = group.folders.len();
                                for (folder_index, path) in group.folders.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        if let Some(swap) = reorder_buttons(
                                            ui,
                                            palette,
                                            folder_index,
                                            folder_total,
                                        ) {
                                            move_folder = Some(swap);
                                        }
                                        if eden_button(ui, palette, regular::TRASH).clicked() {
                                            remove_folder = Some(folder_index);
                                        }
                                        if let Some(texture) = icon_cache.get(path, true) {
                                            ui.add(
                                                egui::Image::new(&texture)
                                                    .fit_to_exact_size(ICON_ROW_SIZE),
                                            );
                                        }
                                        ui.label(path.display().to_string())
                                            .on_hover_text(path.display().to_string());
                                    });
                                    ui.add_space(6.0);
                                }
                                if let Some((from, to)) = move_folder {
                                    group.folders.swap(from, to);
                                    changed = true;
                                }
                                if let Some(i) = remove_folder {
                                    group.folders.remove(i);
                                    changed = true;
                                }
                            }
                        });
                    });
            },
        );
    });

    if let Some((from, to)) = move_indices {
        settings.current_settings.send_to.swap(from, to);
        changed = true;
    }
    if let Some(i) = remove_index {
        let removed_id = settings.current_settings.send_to[i].id;
        settings.current_settings.send_to.remove(i);
        if settings.selected_send_to_id == Some(removed_id) {
            settings.selected_send_to_id =
                settings.current_settings.send_to.first().map(|g| g.id);
        }
        // The context-menu toggle is meaningless with zero groups - reset it
        // rather than leaving a dangling "enabled" flag with nothing to show,
        // per this feature's own design (see `core::send_to`'s doc comment).
        if settings.current_settings.send_to.is_empty() {
            settings.current_settings.send_to_context_menu_enabled = false;
        }
        changed = true;
    }

    if changed {
        action = Some(SettingsAction::ApplySettings);
    }

    action
}

/// Draws a Send To group's icon in a list row/menu at `size` - the user's
/// own glyph/custom image if set, otherwise a generic "send" glyph as the
/// fallback (a group has no real folder of its own to fall back to a shell
/// icon for, unlike Favorites).
pub fn draw_group_icon(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    icon: &SendToIcon,
    size: egui::Vec2,
) {
    match icon {
        SendToIcon::Custom(file) => {
            if let Some(texture) = icon_cache.get_custom_file_icon(file) {
                ui.add(egui::Image::new(&texture).fit_to_exact_size(size));
                return;
            }
        }
        SendToIcon::Glyph(glyph) => {
            ui.add(
                egui::Label::new(RichText::new(glyph.as_str()).size(size.y)).selectable(false),
            );
            return;
        }
        SendToIcon::None => {}
    }
    // `.selectable(false)` - see `list_row`'s own doc comment on why a plain
    // label would otherwise steal the row's click.
    ui.add(egui::Label::new(regular::PAPER_PLANE_TILT).selectable(false));
}

/// One selectable row in a master-detail page's left-column list - identical
/// shape to `favorites_ui.rs`/`tab_groups_ui.rs`'s own `list_row` (each page
/// keeps its own copy rather than sharing one, matching this codebase's
/// existing convention for this exact helper).
fn list_row(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    selected: bool,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
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
        ui.painter().rect_stroke(
            rect,
            corner,
            egui::Stroke::new(1.5, stroke_color),
            egui::StrokeKind::Inside,
        );
    }

    let content_rect = rect.shrink2(egui::vec2(10.0, 6.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(content_rect), |ui| {
        ui.horizontal_centered(|ui| add_contents(ui));
    });

    ui.add_space(8.0);

    response
}
