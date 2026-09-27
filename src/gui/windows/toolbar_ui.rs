//! Settings > Toolbar: choose which buttons the file-view toolbar shows,
//! their order, and where separators go (see `core::toolbar`). Changes apply
//! immediately; Reset To Default restores the original layout.

use crate::core::toolbar::{ToolbarItem, available_buttons, effective_layout, normalize};
use crate::core::utils::widgets::eden_button;
use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::containers::itemviewer_navbar::toolbar_item_icon_and_key;
use crate::gui::windows::enums::SettingsAction;
use crate::gui::windows::settings::{reorder_buttons, setting_label, settings_section};
use crate::gui::windows::structs::SettingsWindow;
use eframe::egui;
use egui_phosphor::regular;

const ROW_ICON_SIZE: f32 = 16.0;

fn item_label(i18n: &I18n, item: ToolbarItem) -> String {
    i18n.tr(toolbar_item_icon_and_key(item).1)
}

pub fn draw_toolbar_settings(
    ui: &mut egui::Ui,
    i18n: &I18n,
    settings: &mut SettingsWindow,
    palette: &ThemePalette,
) -> Option<SettingsAction> {
    let prefs = &mut settings.current_settings.ui_prefs;
    let mut layout = effective_layout(prefs.toolbar.as_deref());
    let mut changed = false;
    let mut reset = false;

    setting_label(
        ui,
        &i18n.tr("settings_category_toolbar"),
        Some((&i18n.tr("tooltip_settings_toolbar"), palette)),
        palette,
    );
    ui.add_space(8.0);

    // Live preview of the toolbar as it will look.
    settings_section(ui, palette, |ui| {
        ui.label(
            egui::RichText::new(i18n.tr("toolbar_preview"))
                .strong()
                .size(palette.text_size)
                .color(palette.text_header_section),
        );
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            for item in normalize(&layout) {
                if item == ToolbarItem::Separator {
                    ui.separator();
                    continue;
                }
                let (icon, key) = toolbar_item_icon_and_key(item);
                ui.label(
                    egui::RichText::new(icon)
                        .size(20.0)
                        .color(palette.toolbar_icon_color),
                )
                .on_hover_text(i18n.tr(key));
            }
        });
    });

    ui.horizontal(|ui| {
        if eden_button(
            ui,
            palette,
            &format!("{} {}", regular::MINUS, i18n.tr("context_menu_order_add_separator")),
        )
        .clicked()
        {
            layout.push(ToolbarItem::Separator);
            changed = true;
        }
        if prefs.toolbar.is_some()
            && eden_button(ui, palette, &i18n.tr("toolbar_reset")).clicked()
        {
            reset = true;
        }
    });
    ui.add_space(10.0);

    // Current toolbar, in order.
    ui.label(
        egui::RichText::new(i18n.tr("toolbar_current"))
            .strong()
            .size(palette.text_size)
            .color(palette.text_header_section),
    );
    ui.add_space(4.0);
    let mut remove_index = None;
    let mut swap = None;
    let total = layout.len();
    for (index, &item) in layout.iter().enumerate() {
        list_row(ui, palette, |ui| {
            if item == ToolbarItem::Separator {
                row_icon(ui, palette, regular::MINUS, palette.tooltip_text_color);
                ui.label(
                    egui::RichText::new(i18n.tr("toolbar_separator"))
                        .italics()
                        .color(palette.tooltip_text_color),
                );
            } else {
                row_icon(ui, palette, toolbar_item_icon_and_key(item).0, palette.icon_color);
                ui.label(egui::RichText::new(item_label(i18n, item)).strong());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if eden_button(ui, palette, regular::TRASH)
                    .on_hover_text(i18n.tr("toolbar_remove"))
                    .clicked()
                {
                    remove_index = Some(index);
                }
                if let Some(s) = reorder_buttons(ui, palette, index, total) {
                    swap = Some(s);
                }
            });
        });
    }
    if let Some((from, to)) = swap {
        layout.swap(from, to);
        changed = true;
    }
    if let Some(index) = remove_index {
        layout.remove(index);
        changed = true;
    }

    // Buttons not on the toolbar.
    let available = available_buttons(&layout);
    if !available.is_empty() {
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(i18n.tr("toolbar_available"))
                .strong()
                .size(palette.text_size)
                .color(palette.text_header_section),
        );
        ui.add_space(4.0);
        for item in available {
            list_row(ui, palette, |ui| {
                row_icon(ui, palette, toolbar_item_icon_and_key(item).0, palette.icon_color);
                ui.label(item_label(i18n, item));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if eden_button(ui, palette, &format!("{} {}", regular::PLUS, i18n.tr("toolbar_add")))
                        .clicked()
                    {
                        layout.push(item);
                        changed = true;
                    }
                });
            });
        }
    }

    if reset {
        prefs.toolbar = None;
        return Some(SettingsAction::ApplySettings);
    }
    if changed {
        prefs.toolbar = Some(layout);
        return Some(SettingsAction::ApplySettings);
    }
    None
}

fn list_row(ui: &mut egui::Ui, palette: &ThemePalette, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(palette.row_bg)
        .stroke(egui::Stroke::new(1.0, palette.borders_default))
        .corner_radius(palette.small_radius)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.horizontal(add_contents);
        });
    ui.add_space(4.0);
}

fn row_icon(ui: &mut egui::Ui, _palette: &ThemePalette, icon: &str, color: egui::Color32) {
    ui.add(
        egui::Label::new(egui::RichText::new(icon).size(ROW_ICON_SIZE).color(color))
            .selectable(false),
    );
}
