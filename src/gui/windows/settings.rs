use crate::core::{fs::MY_PC_PATH, indexer::WindowSizeMode};
use crate::gui::theme::{ThemePalette, apply_checkbox_colors};
use crate::gui::windows::enums::SettingsAction;
use crate::gui::windows::structs::{AppSettings, SettingsWindow};
use eframe::egui;
use egui::RichText;
use egui_phosphor::regular;
use std::path::PathBuf;

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            folder_scanning_enabled: true,
            windows_context_menu_enabled: false,
            start_path: Some(PathBuf::from(MY_PC_PATH)),
            window_size_mode: WindowSizeMode::default(),
            pinned_tabs: Vec::new(),
            time_format_24h: true,
        }
    }
}

// Helper function for info icon with hover text (non-clickable)
fn info_icon(ui: &mut egui::Ui, hover_text: &str, palette: &ThemePalette) -> egui::Response {
    let resp = ui.add(egui::Label::new(regular::QUESTION).sense(egui::Sense::hover()));

    if resp.hovered() {
        ui.painter().text(
            resp.rect.center(),
            egui::Align2::CENTER_CENTER,
            regular::QUESTION,
            egui::FontId::default(),
            palette.primary,
        );
    }

    if resp.hovered() {
        ui.ctx()
            .output_mut(|o| o.cursor_icon = egui::CursorIcon::Default);
        egui::containers::Area::new(ui.next_auto_id())
            .current_pos(resp.rect.right_top())
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(hover_text)
                                .size(palette.text_size)
                                .color(ui.visuals().text_color()),
                        );
                    });
            });
    }

    resp
}

pub fn draw_settings_window(
    ctx: &egui::Context,
    settings: &mut SettingsWindow,
    palette: &ThemePalette,
) -> Option<SettingsAction> {
    let mut action = None;

    if !settings.open {
        return None;
    }

    let mut should_close = false;

    // 🌑 Dark background overlay (modal effect)
    egui::Area::new(egui::Id::new("settings_modal_bg"))
        .order(egui::Order::Middle)
        .interactable(true)
        .show(ctx, |ui| {
            let rect = ctx.content_rect();
            ui.painter()
                .rect_filled(rect, 0.0, palette.modal_background_effect_color);
        });

    egui::Window::new("EdenExplorer Settings")
        .collapsible(false)
        .resizable(false)
        .fixed_size([400.0, 400.0])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(egui::Frame::popup(&ctx.style()).corner_radius(egui::CornerRadius::same(8)))
        .show(ctx, |ui| {
            // 🎯 Smaller font override (fix giant UI)
            let mut style = (*ui.ctx().style()).clone();
            style.text_styles = [
                (egui::TextStyle::Heading, egui::FontId::proportional(14.0)),
                (egui::TextStyle::Body, egui::FontId::proportional(palette.text_size)),
                (egui::TextStyle::Button, egui::FontId::proportional(palette.text_size)),
                (egui::TextStyle::Small, egui::FontId::proportional(palette.text_size)),
            ]
            .into();
            ui.set_style(style);

            // SCROLLABLE CONTENT
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button(format!("{} Reset", regular::ARROW_CLOCKWISE)).clicked() {
                            action = Some(SettingsAction::ResetToDefaults);
                        }
                    });
                    // Folder Scanning
                    ui.horizontal(|ui| {
                        ui.scope(|ui| {
                            apply_checkbox_colors(ui, palette, false);
                            if ui.checkbox(
                                &mut settings.current_settings.folder_scanning_enabled,
                                RichText::new("Enable folder size scanning").color(palette.text_normal)
                            ).changed() {
                                // Auto-save when setting changes
                                action = Some(SettingsAction::ApplySettings);
                            }
                        });
                    info_icon(ui, "When enabled, the application will scan folders to calculate their sizes. This may impact performance on large directories.", palette);
                    });
                    ui.add_space(8.0);
                    // Starting Path
                    ui.label("Startup Directory:");
                    ui.horizontal(|ui| {
                        let path_text = settings.current_settings.start_path
                            .as_ref()
                            .map(|p| {
                                if p.as_os_str() == MY_PC_PATH {
                                    return "Default (My PC)".to_string();
                                }

                                let s = p.to_string_lossy();

                                if s.len() > 40 {
                                    format!("...{}", &s[s.len() - 40..])
                                } else {
                                    s.to_string()
                                }
                            })
                            .unwrap_or_else(|| "Default (My PC)".to_string());

                        ui.add_sized(
                            [200.0, 18.0],
                            egui::Label::new(path_text),
                        ).on_hover_text(
                            settings.current_settings.start_path
                                .as_ref()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_default()
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(regular::ARROW_COUNTER_CLOCKWISE)
                                .on_hover_text("Reset to default")
                                .clicked()
                            {
                                settings.current_settings.start_path = Some(PathBuf::from(MY_PC_PATH));
                                action = Some(SettingsAction::ApplySettings);
                            }

                            if ui.button(regular::FOLDER_OPEN)
                                .on_hover_text("Choose a folder")
                                .clicked()
                            {
                                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                    settings.current_settings.start_path = Some(path);
                                    action = Some(SettingsAction::ApplySettings);
                                }
                            }
                        });
                    });
                    ui.add_space(8.0);
                    // Display Settings Section
                    ui.heading(format!("{} Display Settings", regular::MONITOR));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label("Launch mode:");
                        let mut full_screen = matches!(settings.current_settings.window_size_mode, WindowSizeMode::FullScreen);
                        let mut half_screen = matches!(settings.current_settings.window_size_mode, WindowSizeMode::HalfScreen);
                        let mut custom = matches!(settings.current_settings.window_size_mode, WindowSizeMode::Custom { .. });
                        if ui.checkbox(&mut full_screen, "Fullscreen").changed() & half_screen {
                            settings.current_settings.window_size_mode = WindowSizeMode::FullScreen;
                            action = Some(SettingsAction::ApplySettings);
                        }
                        if ui.checkbox(&mut half_screen, "Half Screen").changed() & half_screen {
                            settings.current_settings.window_size_mode = WindowSizeMode::HalfScreen;
                            action = Some(SettingsAction::ApplySettings);
                        }
                        if ui.checkbox(&mut custom, "Custom").changed() & custom {
                            settings.current_settings.window_size_mode = WindowSizeMode::Custom { width: 1200.0, height: 800.0 };
                            action = Some(SettingsAction::ApplySettings);
                        }
                    });
                    // Custom size inputs
                    if let WindowSizeMode::Custom { width, height } = &mut settings.current_settings.window_size_mode {
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.label("Width:");
                            if ui.add(egui::DragValue::new(width).range(400.0..=3840.0)).changed() {
                                action = Some(SettingsAction::ApplySettings);
                            }
                            ui.label("Height:");
                            if ui.add(egui::DragValue::new(height).range(300.0..=2160.0)).changed() {
                                action = Some(SettingsAction::ApplySettings);
                            }
                            info_icon(ui, "Configure the window size when the application launches. Changes are applied after restart", palette);
                        });
                    }
                    ui.add_space(8.0);
                    // Time Format Section
                    ui.horizontal(|ui| {
                        ui.scope(|ui| {
                            apply_checkbox_colors(ui, palette, false);
                            if ui.checkbox(
                                &mut settings.current_settings.time_format_24h,
                                RichText::new("Use 24-hour time format")
                                    .color(palette.text_normal),
                            )
                            .changed()
                            {
                                action = Some(SettingsAction::ApplySettings);
                            }
                        });
                        info_icon(
                            ui,
                            "When enabled, times will be displayed in 24-hour format (e.g., 14:30). When disabled, 12-hour format will be used (e.g., 2:30 PM).",
                            palette,
                        );
                    });
                    ui.add_space(8.0);
                    // Context Menu Section
                    ui.heading("Context Menu");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.scope(|ui| {
                            apply_checkbox_colors(ui, palette, false);
                            if ui.checkbox(
                                &mut settings.current_settings.windows_context_menu_enabled,
                                RichText::new("Enable Windows context menu items")
                                    .color(palette.text_normal),
                            )
                            .changed()
                            {
                                action = Some(SettingsAction::ApplySettings);
                            }
                        });
                        info_icon(
                            ui,
                            "Adds a Windows section to the right-click menu with shell actions.",
                            palette,
                        );
                    });
                    ui.add_space(8.0);
                    // Favorites Reset
                    ui.horizontal(|ui| {
                        if ui.button(format!("{} Reset Sidebar Favorites", regular::TRASH))
                            .on_hover_text( egui::RichText::new("Reset favorites to default")
                                    .size(palette.tooltip_text_size)
                                    .color(palette.tooltip_text_color))
                            .clicked() {
                            settings.show_reset_favorites_confirmation = true;
                        }
                    });
                });
            // Reset Favorites Confirmation Dialog
            if settings.show_reset_favorites_confirmation {
                let mut should_close = false;
                egui::Window::new("Reset Favorites")
                    .collapsible(false)
                    .resizable(false)
                    .fixed_size([400.0, 150.0])
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .frame(egui::Frame::popup(&ctx.style()).corner_radius(egui::CornerRadius::same(8)))
                    .show(ctx, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("This will clear all your saved favorite locations")
                                    .size(palette.text_size)
                            );
                            ui.label(
                                egui::RichText::new("and restore the default favorites.")
                                    .size(palette.text_size)
                            );
                            ui.add_space(20.0);
                            ui.horizontal(|ui| {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("Close").clicked() {
                                        should_close = true;
                                    }
                                    if ui.button("Reset").clicked() {
                                        action = Some(SettingsAction::ResetFavourites);
                                        should_close = true;
                                    }
                                });
                            });
                        });
                    });
                if should_close {
                    settings.show_reset_favorites_confirmation = false;
                }
            }
            ui.separator();
            // Footer
            ui.horizontal(|ui| {
                ui.label("Changes are automatically saved when modified.");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(format!("{} Close", regular::X)).clicked() {
                        should_close = true;
                    }
                });
            });
        });
    // Update the open state based on should_close
    if should_close {
        settings.open = false;
    }
    action
}
