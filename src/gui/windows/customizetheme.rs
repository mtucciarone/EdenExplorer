use crate::core::utils::fonts::get_font_list;
use crate::gui::i18n::I18n;
use crate::gui::theme::{
    ThemeMode, ThemePalette, apply_font_to_context, get_default_palette,
    regenerate_base_derived_colors,
};
use crate::gui::utils::rgba_color_edit_button;
use crate::gui::windows::enums::ThemeCustomizerAction;
use crate::gui::windows::structs::ThemeCustomizer;
use eframe::egui;
use egui::{FontFamily, FontId};

fn selectable_mode(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    current: ThemeMode,
    target: ThemeMode,
    label: &str,
) -> bool {
    let selected = current == target;

    ui.selectable_label(
        selected,
        egui::RichText::new(label).color(if selected {
            palette.text_normal
        } else {
            ui.visuals().text_color()
        }),
    )
    .clicked()
}

pub fn draw_theme_customizer(
    i18n: &I18n,
    ctx: &egui::Context,
    customizer: &mut ThemeCustomizer,
    palette: &ThemePalette,
) -> Option<ThemeCustomizerAction> {
    let mut action = None;

    if !customizer.open {
        return None;
    }

    // 🌑 Dark background overlay (modal effect)
    egui::Area::new(egui::Id::new("theme_modal_bg"))
        .order(egui::Order::Middle)
        .interactable(true)
        .show(ctx, |ui| {
            let rect = ctx.content_rect();
            ui.painter()
                .rect_filled(rect, 0.0, palette.modal_background_effect_color);
        });

    egui::Window::new(&i18n.tr("theme_title"))
        .collapsible(false)
        .resizable(false)
        .fixed_size([600.0, 500.0])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut customizer.open)
        .frame(egui::Frame::popup(&ctx.style()).corner_radius(egui::CornerRadius::same(8)))
        .show(ctx, |ui| {
            // 🎯 Match About window typography
            let mut style = (*ui.ctx().style()).clone();
            style.text_styles = [
                (egui::TextStyle::Heading, egui::FontId::proportional(14.0)),
                (
                    egui::TextStyle::Body,
                    egui::FontId::proportional(palette.text_size),
                ),
                (
                    egui::TextStyle::Button,
                    egui::FontId::proportional(palette.text_size),
                ),
                (
                    egui::TextStyle::Small,
                    egui::FontId::proportional(palette.text_size),
                ),
            ]
            .into();
            ui.set_style(style);
            ui.set_width(ui.available_width());

            let label_color = palette.text_normal;
            let font_id = FontId::new(palette.text_size, FontFamily::Proportional);

            // HEADER
            ui.horizontal(|ui| {
                ui.heading(&i18n.tr("theme_header"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(&i18n.tr("theme_reset")).clicked() {
                        let default = get_default_palette(customizer.selected_mode);
                        match customizer.selected_mode {
                            ThemeMode::Dark => customizer.dark_palette = default,
                            ThemeMode::Light => customizer.light_palette = default,
                        }
                        action = Some(ThemeCustomizerAction::ResetToDefaults(
                            customizer.selected_mode,
                        ));
                    }
                });
            });

            ui.add_space(6.0);

            // TOP SECTION: select which palette to edit
            ui.horizontal(|ui| {
                if selectable_mode(
                    ui,
                    palette,
                    customizer.selected_mode,
                    ThemeMode::Dark,
                    &i18n.tr("theme_dark"),
                ) {
                    customizer.selected_mode = ThemeMode::Dark;
                }

                if selectable_mode(
                    ui,
                    palette,
                    customizer.selected_mode,
                    ThemeMode::Light,
                    &i18n.tr("theme_light"),
                ) {
                    customizer.selected_mode = ThemeMode::Light;
                }
            });

            ui.separator();

            let editing_palette = match customizer.selected_mode {
                ThemeMode::Dark => &mut customizer.dark_palette,
                ThemeMode::Light => &mut customizer.light_palette,
            };

            let mut changed = false;

            // SCROLLABLE CONTENT
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.group(|ui| {
                        ui.label(
                            egui::RichText::new(&i18n.tr("theme_typography"))
                                .font(font_id.clone())
                                .size(palette.text_size)
                                .color(label_color),
                        );

                        ui.add_space(6.0);
                        egui::Grid::new("typography_settings")
                            .num_columns(2)
                            .spacing([12.0, 6.0])
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(&i18n.tr("theme_textsize"))
                                        .font(font_id.clone())
                                        .size(palette.text_size)
                                        .color(label_color),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        changed |= ui
                                            .add_sized(
                                                egui::vec2(90.0, 0.0),
                                                egui::DragValue::new(
                                                    &mut editing_palette.text_size,
                                                )
                                                .range(8.0..=24.0)
                                                .speed(0.2),
                                            )
                                            .changed();
                                    },
                                );
                                ui.end_row();

                                ui.label(
                                    egui::RichText::new(&i18n.tr("theme_tooltip_textsize"))
                                        .font(font_id.clone())
                                        .size(palette.text_size)
                                        .color(label_color),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        changed |= ui
                                            .add_sized(
                                                egui::vec2(90.0, 0.0),
                                                egui::DragValue::new(
                                                    &mut editing_palette.tooltip_text_size,
                                                )
                                                .range(8.0..=24.0)
                                                .speed(0.2),
                                            )
                                            .changed();
                                    },
                                );
                                ui.end_row();

                                ui.label(
                                    egui::RichText::new(&i18n.tr("theme_contextmenu_textsize"))
                                        .font(font_id.clone())
                                        .size(palette.text_size)
                                        .color(label_color),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        changed |= ui
                                            .add_sized(
                                                egui::vec2(90.0, 0.0),
                                                egui::DragValue::new(
                                                    &mut editing_palette.context_menu_text_size,
                                                )
                                                .range(8.0..=24.0)
                                                .speed(0.2),
                                            )
                                            .changed();
                                    },
                                );
                                ui.end_row();

                                ui.label(
                                    egui::RichText::new(&i18n.tr("theme_explorer_iconsize"))
                                        .font(font_id.clone())
                                        .size(palette.text_size)
                                        .color(label_color),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        changed |= ui
                                            .add_sized(
                                                egui::vec2(90.0, 0.0),
                                                egui::DragValue::new(
                                                    &mut editing_palette.explorer_icon_size,
                                                )
                                                .range(8.0..=32.0)
                                                .speed(0.2),
                                            )
                                            .changed();
                                    },
                                );
                                ui.end_row();

                                ui.label(
                                    egui::RichText::new(&i18n.tr("theme_sidebar_iconsize"))
                                        .font(font_id.clone())
                                        .size(palette.text_size)
                                        .color(label_color),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        changed |= ui
                                            .add_sized(
                                                egui::vec2(90.0, 0.0),
                                                egui::DragValue::new(
                                                    &mut editing_palette.sidebar_icon_size,
                                                )
                                                .range(8.0..=32.0)
                                                .speed(0.2),
                                            )
                                            .changed();
                                    },
                                );
                                ui.end_row();

                                ui.label(
                                    egui::RichText::new(&i18n.tr("theme_tab_iconsize"))
                                        .font(font_id.clone())
                                        .size(palette.text_size)
                                        .color(label_color),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        changed |= ui
                                            .add_sized(
                                                egui::vec2(90.0, 0.0),
                                                egui::DragValue::new(
                                                    &mut editing_palette.tab_icon_size,
                                                )
                                                .range(8.0..=32.0)
                                                .speed(0.2),
                                            )
                                            .changed();
                                    },
                                );
                                ui.end_row();
                            });

                        ui.add_space(8.0);

                        egui::Grid::new("typography_font_settings")
                            .num_columns(2)
                            .spacing([12.0, 6.0])
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(&i18n.tr("theme_font"))
                                        .font(font_id.clone())
                                        .size(palette.text_size)
                                        .color(label_color),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if let Some(new_font) = font_selector(
                                            ui,
                                            "theme_font_selector",
                                            &editing_palette.font_name,
                                        ) {
                                            editing_palette.font_name = new_font;
                                            apply_font_to_context(ctx, &editing_palette);
                                            changed = true;
                                        }
                                    },
                                );
                                ui.end_row();

                                ui.label(
                                    egui::RichText::new(&i18n.tr("theme_mono_font"))
                                        .font(font_id.clone())
                                        .size(palette.text_size)
                                        .color(label_color),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if let Some(new_font) = font_selector(
                                            ui,
                                            "theme_mono_font_selector",
                                            &editing_palette.mono_font_name,
                                        ) {
                                            editing_palette.mono_font_name = new_font;
                                            apply_font_to_context(ctx, &editing_palette);
                                            changed = true;
                                        }
                                    },
                                );
                                ui.end_row();
                            });
                    });

                    ui.add_space(8.0);

                    ui.group(|ui| {
                        ui.label(
                            egui::RichText::new(&i18n.tr("theme_core_colors"))
                                .font(font_id.clone())
                                .size(palette.text_size)
                                .color(label_color),
                        );

                        ui.add_space(6.0);

                        egui::Grid::new("theme_corecolors")
                            .num_columns(2)
                            .spacing([12.0, 6.0])
                            .show(ui, |ui| {
                                let primary_changed = color_picker(
                                    ui,
                                    &i18n.tr("theme_colors_primary"),
                                    &mut editing_palette.primary,
                                    &font_id,
                                    label_color,
                                );

                                // If primary color changed, regenerate all base-derived colors
                                if primary_changed {
                                    regenerate_base_derived_colors(
                                        editing_palette,
                                        customizer.selected_mode == ThemeMode::Dark,
                                    );
                                    changed = true;
                                }
                                ui.end_row();
                                changed |= color_picker(
                                    ui,
                                    &i18n.tr("theme_colors_primary_hover"),
                                    &mut editing_palette.primary_hover,
                                    &font_id,
                                    label_color,
                                );
                                ui.end_row();
                                changed |= color_picker(
                                    ui,
                                    &i18n.tr("theme_colors_primary_active"),
                                    &mut editing_palette.primary_active,
                                    &font_id,
                                    label_color,
                                );
                                ui.end_row();
                                changed |= color_picker(
                                    ui,
                                    &i18n.tr("theme_colors_primary_subtle"),
                                    &mut editing_palette.primary_subtle,
                                    &font_id,
                                    label_color,
                                );
                                ui.end_row();
                                changed |= color_picker(
                                    ui,
                                    &i18n.tr("theme_colors_secondary"),
                                    &mut editing_palette.secondary,
                                    &font_id,
                                    label_color,
                                );
                                ui.end_row();
                                changed |= color_picker(
                                    ui,
                                    &i18n.tr("theme_colors_application_background"),
                                    &mut editing_palette.application_bg_color,
                                    &font_id,
                                    label_color,
                                );
                                ui.end_row();
                            });
                    });
                });

            ui.separator();

            // FOOTER
            ui.horizontal(|ui| {
                if ui.button(&i18n.tr("theme_export")).clicked() {
                    action = Some(ThemeCustomizerAction::ExportTheme(customizer.selected_mode));
                }

                if ui.button(&i18n.tr("theme_import")).clicked() {
                    action = Some(ThemeCustomizerAction::ImportTheme(customizer.selected_mode));
                }
            });

            if changed && action.is_none() {
                action = Some(ThemeCustomizerAction::ThemeUpdated(
                    customizer.selected_mode,
                ));
            }
        });

    action
}

fn color_picker(
    ui: &mut egui::Ui,
    label: &str,
    color: &mut egui::Color32,
    font_id: &FontId,
    label_color: egui::Color32,
) -> bool {
    ui.label(
        egui::RichText::new(label)
            .font(font_id.clone())
            .size(font_id.size)
            .color(label_color),
    );

    rgba_color_edit_button(ui, color).changed()
}

fn font_selector(ui: &mut egui::Ui, label: &str, current_font: &str) -> Option<String> {
    let fonts = get_font_list();

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let mut selected_text = current_font.to_string();
        let mut changed = false;

        egui::ComboBox::from_id_source(egui::Id::new(label))
            .width(200.0)
            .selected_text(&selected_text)
            .show_ui(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        for font in fonts.iter() {
                            let rich_text = egui::RichText::new(font.as_str());

                            if ui
                                .selectable_label(font == current_font, rich_text)
                                .clicked()
                            {
                                selected_text = font.clone();
                                changed = true;
                                ui.close_menu();
                            }
                        }
                    });
            });

        if changed { Some(selected_text) } else { None }
    })
    .inner
}
