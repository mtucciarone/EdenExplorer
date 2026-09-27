//! Settings UI for the user-configurable right-click context menu order (see
//! `core::context_menu_order`). A single reorderable list - each fixed
//! section (Send To, Custom Context Menu, the Cut/Copy/Paste block, etc.)
//! always appears exactly once and can only be moved, never deleted;
//! `Separator` rows are the one thing the user can freely add and remove,
//! anywhere in the list.

use crate::core::context_menu_order::ContextMenuSection;
use crate::core::utils::widgets::eden_button;
use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::enums::SettingsAction;
use crate::gui::windows::settings::{reorder_buttons, setting_label};
use crate::gui::windows::structs::SettingsWindow;
use eframe::egui;
use egui_phosphor::regular;

/// Row height/icon sizing - matches this app's other settings-page lists
/// (Tab Groups/Send To/Custom Context Menu's own `list_row` helpers), kept
/// local since this page's rows don't need the icon/count-badge machinery
/// those bigger master-detail pages do.
const ROW_ICON_SIZE: f32 = 16.0;

pub fn draw_context_menu_order_settings(
    ui: &mut egui::Ui,
    i18n: &I18n,
    settings: &mut SettingsWindow,
    palette: &ThemePalette,
) -> Option<SettingsAction> {
    let mut action = None;
    let mut changed = false;

    setting_label(
        ui,
        &i18n.tr("settings_category_context_menu_order"),
        Some((&i18n.tr("tooltip_settings_context_menu_order"), palette)),
        palette,
    );
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        if eden_button(
            ui,
            palette,
            &format!(
                "{} {}",
                regular::MINUS,
                i18n.tr("context_menu_order_add_separator")
            ),
        )
        .clicked()
        {
            settings
                .current_settings
                .context_menu_order
                .push(ContextMenuSection::Separator);
            changed = true;
        }
        if eden_button(ui, palette, &i18n.tr("context_menu_order_reset")).clicked() {
            settings.current_settings.context_menu_order =
                crate::core::context_menu_order::default_order();
            changed = true;
        }
    });

    ui.add_space(10.0);

    let mut remove_index: Option<usize> = None;
    let mut move_indices: Option<(usize, usize)> = None;
    let total_len = settings.current_settings.context_menu_order.len();

    // No `ScrollArea` of its own here - this page already renders inside the
    // Settings tab's own shared `settings_content_scroll` (see `settings.rs`),
    // and nesting a second independently-scrolling layout inside that one is
    // a known source of squashed/collapsed-height bugs in this codebase (see
    // `CLAUDE.md`'s entry on the Appearance page's own redesign) - the list
    // here is short enough (13 fixed sections plus however many separators
    // the user adds) that the outer scroll area alone is enough.
    for (index, section) in settings
        .current_settings
        .context_menu_order
        .iter()
        .enumerate()
    {
        let is_separator = section.is_separator();

        egui::Frame::new()
            .fill(palette.row_bg)
            .stroke(egui::Stroke::new(1.0, palette.borders_default))
            .corner_radius(palette.small_radius)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if is_separator {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(regular::MINUS)
                                    .size(ROW_ICON_SIZE)
                                    .color(palette.tooltip_text_color),
                            )
                            .selectable(false),
                        );
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(i18n.tr(section.i18n_key()))
                                    .italics()
                                    .color(palette.tooltip_text_color),
                            )
                            .selectable(false),
                        );
                    } else {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(regular::DOTS_SIX_VERTICAL)
                                    .size(ROW_ICON_SIZE)
                                    .color(palette.icon_color),
                            )
                            .selectable(false),
                        );
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(i18n.tr(section.i18n_key())).strong(),
                            )
                            .selectable(false),
                        );
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if is_separator
                            && eden_button(ui, palette, regular::TRASH)
                                .on_hover_text(i18n.tr("context_menu_order_remove_separator"))
                                .clicked()
                        {
                            remove_index = Some(index);
                        }
                        if let Some(swap) = reorder_buttons(ui, palette, index, total_len) {
                            move_indices = Some(swap);
                        }
                    });
                });
            });

        ui.add_space(4.0);
    }

    if let Some((from, to)) = move_indices {
        settings
            .current_settings
            .context_menu_order
            .swap(from, to);
        changed = true;
    }

    if let Some(i) = remove_index {
        settings.current_settings.context_menu_order.remove(i);
        changed = true;
    }

    if changed {
        crate::core::context_menu_order::save_context_menu_order(
            &settings.current_settings.context_menu_order,
        );
        action = Some(SettingsAction::ApplySettings);
    }

    action
}
