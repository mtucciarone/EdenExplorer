use crate::gui::theme::{ThemeMode, ThemePalette, get_default_palette};
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

    egui::Window::new("Theme Customizer")
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
                ui.heading("Theme Configuration");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Reset Theme").clicked() {
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
                    "Dark",
                ) {
                    customizer.selected_mode = ThemeMode::Dark;
                }

                if selectable_mode(
                    ui,
                    palette,
                    customizer.selected_mode,
                    ThemeMode::Light,
                    "Light",
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
                            egui::RichText::new("Typography")
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
                                    egui::RichText::new("Text Font")
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
                                    egui::RichText::new("Tooltip Text Size")
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
                                    egui::RichText::new("Context Menu Text Size")
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
                                    egui::RichText::new("Explorer Icon Size")
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
                                    egui::RichText::new("Sidebar Icon Size")
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
                                    egui::RichText::new("Tab Icon Size")
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
                    });

                    ui.add_space(8.0);

                    ui.group(|ui| {
                        ui.label(
                            egui::RichText::new("Core Colors")
                                .font(font_id.clone())
                                .size(palette.text_size)
                                .color(label_color),
                        );

                        ui.add_space(6.0);

                        egui::Grid::new("theme_core_colors")
                            .num_columns(2)
                            .spacing([12.0, 6.0])
                            .show(ui, |ui| {
                                changed |= color_picker(
                                    ui,
                                    "Primary",
                                    &mut editing_palette.primary,
                                    &font_id,
                                    label_color,
                                );
                                ui.end_row();
                                changed |= color_picker(
                                    ui,
                                    "Primary Hover",
                                    &mut editing_palette.primary_hover,
                                    &font_id,
                                    label_color,
                                );
                                ui.end_row();
                                changed |= color_picker(
                                    ui,
                                    "Primary Active",
                                    &mut editing_palette.primary_active,
                                    &font_id,
                                    label_color,
                                );
                                ui.end_row();
                                changed |= color_picker(
                                    ui,
                                    "Primary Subtle",
                                    &mut editing_palette.primary_subtle,
                                    &font_id,
                                    label_color,
                                );
                                ui.end_row();
                                changed |= color_picker(
                                    ui,
                                    "Secondary",
                                    &mut editing_palette.secondary,
                                    &font_id,
                                    label_color,
                                );
                                ui.end_row();
                                changed |= color_picker(
                                    ui,
                                    "Application Background",
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
                if ui.button("Export Theme").clicked() {
                    action = Some(ThemeCustomizerAction::ExportTheme(customizer.selected_mode));
                }

                if ui.button("Import Theme").clicked() {
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
    let mut changed = false;

    ui.label(
        egui::RichText::new(label)
            .font(font_id.clone())
            .size(font_id.size)
            .color(label_color),
    );

    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());

        let painter = ui.painter();
        let radius = egui::CornerRadius::same(4);

        // === Checkerboard background ===
        let checker_size = 4.0;
        let light = egui::Color32::from_gray(160);
        let dark = egui::Color32::from_gray(100);

        let mut y = rect.top();
        let mut row = 0;

        while y < rect.bottom() {
            let mut x = rect.left();
            let mut col = row;

            while x < rect.right() {
                let tile_color = if col % 2 == 0 { light } else { dark };

                let tile_rect = egui::Rect::from_min_size(
                    egui::pos2(x, y),
                    egui::vec2(checker_size, checker_size),
                );

                painter.rect_filled(tile_rect, 0.0, tile_color);

                x += checker_size;
                col += 1;
            }

            y += checker_size;
            row += 1;
        }

        // === Foreground color (with transparency) ===
        painter.rect_filled(rect, radius, *color);

        // === Optional border (nice polish) ===
        painter.rect_stroke(
            rect,
            radius,
            egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
            egui::StrokeKind::Inside,
        );

        // === RGBA controls ===
        let a = color.a().max(1);

        let mut rgba = [
            ((color.r() as u16 * 255) / a as u16) as u8,
            ((color.g() as u16 * 255) / a as u16) as u8,
            ((color.b() as u16 * 255) / a as u16) as u8,
            color.a(),
        ];
        let labels = ["R", "G", "B", "A"];
        for (i, c) in rgba.iter_mut().enumerate() {
            ui.label(labels[i]);
            changed |= ui.add(egui::DragValue::new(c).range(0..=255)).changed();
        }

        if changed {
            *color = egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
        }
    });

    changed
}
