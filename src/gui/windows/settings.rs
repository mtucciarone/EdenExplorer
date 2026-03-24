use crate::core::indexer::WindowSizeMode;
use crate::gui::theme::ThemePalette;
use eframe::egui;
use egui_phosphor::regular;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppSettings {
    pub folder_scanning_enabled: bool,
    pub starting_path: Option<PathBuf>,
    pub window_size_mode: WindowSizeMode,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            folder_scanning_enabled: true,
            starting_path: None,
            window_size_mode: WindowSizeMode::default(),
        }
    }
}

#[derive(Default)]
pub struct SettingsWindow {
    pub open: bool,
    pub current_settings: AppSettings,
    pub has_unsaved_changes: bool,
    pub show_reset_favorites_confirmation: bool,
}

#[derive(Clone, Debug)]
pub enum SettingsAction {
    ResetToDefaults,
    ResetFavourites,
    ApplySettings,
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
                .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(180));
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
                        if ui.checkbox(
                            &mut settings.current_settings.folder_scanning_enabled,
                            "Enable folder size scanning"
                        ).changed() {
                            // Auto-save when setting changes
                            action = Some(SettingsAction::ApplySettings);
                        }
                        info_icon(ui, "When enabled, the application will scan folders to calculate their sizes. This may impact performance on large directories.", palette);
                    });
                    ui.add_space(12.0);
                    // Starting Path
                    ui.horizontal(|ui| {
                        ui.label("Startup Directory Path:");
                        let path_text = settings.current_settings.starting_path
                            .as_ref()
                            .map_or("Default (system)".to_string(), |p| p.to_string_lossy().to_string());
                        ui.label(path_text);
                        if ui.button(regular::FOLDER_OPEN).clicked() {
                            // TODO: Implement file dialog for path selection
                            action = Some(SettingsAction::ApplySettings);
                        }
                        if ui.button(regular::ARROW_CLOCKWISE).clicked() {
                            settings.current_settings.starting_path = None;
                            action = Some(SettingsAction::ApplySettings);
                        }
                        info_icon(ui, "Set the default directory that opens when the application starts.", palette);
                    });
                    ui.add_space(12.0);
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
                            info_icon(ui, "Configure the window size when the application launches.", palette);
                        });
                    }
                    ui.add_space(12.0);
                    // Favorites Reset
                    ui.horizontal(|ui| {
                        if ui.button(format!("{} Reset Sidebar Favorites", regular::TRASH)).clicked() {
                            settings.show_reset_favorites_confirmation = true;
                        }
                        info_icon(ui, "Clear all saved favourite locations and restore defaults.", palette);
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
