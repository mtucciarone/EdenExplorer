use eframe::egui;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::features::{ThemeMode, ThemePalette};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustomTheme {
    pub name: String,
    pub mode: ThemeMode,
    pub palette: ThemePalette,
}

impl Default for CustomTheme {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            mode: ThemeMode::Dark,
            palette: crate::app::features::get_palette(ThemeMode::Dark).clone(),
        }
    }
}

#[derive(Default)]
pub struct ThemeCustomizer {
    pub open: bool,
    pub current_theme: CustomTheme,
    pub custom_themes: Vec<CustomTheme>,
    pub selected_mode: ThemeMode,
    pub has_unsaved_changes: bool,
}

#[derive(Clone, Debug)]
pub enum ThemeCustomizerAction {
    ApplyTheme,
    SaveTheme,
    LoadTheme,
    ResetToDefaults,
    ExportTheme,
    ImportTheme,
}

pub fn draw_theme_customizer(
    ctx: &egui::Context,
    customizer: &mut ThemeCustomizer,
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
                .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(180));
        });

    egui::Window::new("Theme Customizer")
        .collapsible(false)
        .resizable(false)
        .fixed_size([600.0, 500.0])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut customizer.open)
        .frame(egui::Frame::popup(&ctx.style()).corner_radius(egui::CornerRadius::same(8)))
        .show(ctx, |ui| {
            // 🎯 Smaller font override (fix giant UI)
            let mut style = (*ui.ctx().style()).clone();
            style.text_styles = [
                (egui::TextStyle::Heading, egui::FontId::proportional(18.0)),
                (egui::TextStyle::Body, egui::FontId::proportional(14.0)),
                (egui::TextStyle::Button, egui::FontId::proportional(14.0)),
                (egui::TextStyle::Small, egui::FontId::proportional(12.0)),
            ]
            .into();
            ui.set_style(style);

            // HEADER
            ui.horizontal(|ui| {
                ui.heading("Theme Configuration");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if customizer.has_unsaved_changes {
                        if ui.button("💾 Save").clicked() {
                            action = Some(ThemeCustomizerAction::SaveTheme);
                        }
                    }

                    if ui.button("📁 Load").clicked() {
                        action = Some(ThemeCustomizerAction::LoadTheme);
                    }

                    if ui.button("🔄 Reset").clicked() {
                        action = Some(ThemeCustomizerAction::ResetToDefaults);
                    }
                });
            });

            ui.separator();

            // MODE SWITCH
            ui.horizontal(|ui| {
                ui.label("Theme Mode:");
                ui.radio_value(&mut customizer.selected_mode, ThemeMode::Dark, "🌙 Dark");
                ui.radio_value(&mut customizer.selected_mode, ThemeMode::Light, "☀️ Light");
            });

            ui.separator();

            // SCROLLABLE CONTENT
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    draw_color_section(
                        ui,
                        "🎯 Core Colors",
                        &mut customizer.current_theme.palette,
                        |palette, ui| {
                            ui.horizontal(|ui| {
                                color_picker(ui, "Primary", &mut palette.primary);
                                color_picker(ui, "Primary Hover", &mut palette.primary_hover);
                                color_picker(ui, "Primary Active", &mut palette.primary_active);
                            });
                            ui.horizontal(|ui| {
                                color_picker(ui, "Primary Subtle", &mut palette.primary_subtle);
                                color_picker(ui, "Secondary", &mut palette.secondary);
                                color_picker(ui, "Row Selected", &mut palette.row_label_selected);
                            });
                        },
                    );

                    draw_color_section(
                        ui,
                        "🎨 UI Elements",
                        &mut customizer.current_theme.palette,
                        |palette, ui| {
                            ui.horizontal(|ui| {
                                color_picker(
                                    ui,
                                    "Box Selection Stroke",
                                    &mut palette.box_selection_stroke,
                                );
                                color_picker(
                                    ui,
                                    "Box Selection Fill",
                                    &mut palette.box_selection_fill,
                                );
                                color_picker(ui, "Icon Color", &mut palette.icon_color);
                            });
                        },
                    );

                    draw_color_section(
                        ui,
                        "📊 Drive Usage",
                        &mut customizer.current_theme.palette,
                        |palette, ui| {
                            ui.horizontal(|ui| {
                                color_picker(ui, "Critical", &mut palette.drive_usage_critical);
                                color_picker(ui, "Warning", &mut palette.drive_usage_warning);
                                color_picker(ui, "Normal", &mut palette.drive_usage_normal);
                            });
                        },
                    );

                    draw_color_section(
                        ui,
                        "🎯 Tab Buttons",
                        &mut customizer.current_theme.palette,
                        |palette, ui| {
                            ui.horizontal(|ui| {
                                color_picker(ui, "Close Hover", &mut palette.tab_close_hover);
                                color_picker(ui, "Add Hover", &mut palette.tab_add_hover);
                            });
                        },
                    );

                    draw_radius_section(ui, &mut customizer.current_theme.palette);
                });

            ui.separator();

            // PREVIEW
            ui.group(|ui| {
                ui.heading("🔍 Preview");
                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("Selected row:");
                    ui.colored_label(
                        customizer.current_theme.palette.row_label_selected,
                        "Sample Selected Text",
                    );
                    ui.label("|");
                    ui.label("Normal text:");
                    ui.label("Sample Normal Text");
                });

                ui.horizontal(|ui| {
                    ui.button("📁 Folder");
                    ui.button("📄 File");
                    ui.add(egui::TextEdit::singleline(&mut "Sample input".to_string()));
                });
            });

            ui.separator();

            // FOOTER
            ui.horizontal(|ui| {
                if ui.button("📤 Export Theme").clicked() {
                    action = Some(ThemeCustomizerAction::ExportTheme);
                }

                if ui.button("📥 Import Theme").clicked() {
                    action = Some(ThemeCustomizerAction::ImportTheme);
                }

                if customizer.has_unsaved_changes {
                    ui.colored_label(egui::Color32::YELLOW, "⚠️ Unsaved changes");
                }
            });
        });

    action
}

fn draw_color_section(
    ui: &mut egui::Ui,
    title: &str,
    palette: &mut ThemePalette,
    mut content: impl FnMut(&mut ThemePalette, &mut egui::Ui),
) {
    ui.group(|ui| {
        ui.heading(title);
        content(palette, ui);
    });
}

fn color_picker(ui: &mut egui::Ui, label: &str, color: &mut egui::Color32) {
    ui.vertical(|ui| {
        ui.label(label);

        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());

            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(4), *color);

            let mut rgb = [color.r(), color.g(), color.b()];

            ui.vertical(|ui| {
                ui.label("RGB:");
                ui.horizontal(|ui| {
                    for c in &mut rgb {
                        ui.add(egui::DragValue::new(c).range(0..=255));
                    }
                });
            });

            *color = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
        });
    });
}

fn draw_radius_section(ui: &mut egui::Ui, palette: &mut ThemePalette) {
    ui.group(|ui| {
        ui.heading("🔲 Corner Radius");

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label("Small:");
                ui.add(egui::Slider::new(&mut palette.small_radius, 0..=20));
            });

            ui.vertical(|ui| {
                ui.label("Medium:");
                ui.add(egui::Slider::new(&mut palette.medium_radius, 0..=20));
            });

            ui.vertical(|ui| {
                ui.label("Large:");
                ui.add(egui::Slider::new(&mut palette.large_radius, 0..=20));
            });
        });
    });
}

pub fn save_theme_to_file(
    theme: &CustomTheme,
    path: &PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(theme)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn load_theme_from_file(path: &PathBuf) -> Result<CustomTheme, Box<dyn std::error::Error>> {
    let json = std::fs::read_to_string(path)?;
    let theme: CustomTheme = serde_json::from_str(&json)?;
    Ok(theme)
}
