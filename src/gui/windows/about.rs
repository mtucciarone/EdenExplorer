use crate::gui::theme::ThemePalette;
use crate::gui::windows::structs::AboutWindow;
use eframe::egui;
use egui::FontId;
use egui_phosphor::regular;

pub fn draw_about_window(ctx: &egui::Context, settings: &mut AboutWindow, palette: &ThemePalette) {
    if !settings.open {
        return;
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

    egui::Window::new("About EdenExplorer")
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
            ui.set_width(ui.available_width());
            ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            let font_id = FontId::new(palette.text_size, egui::FontFamily::Proportional);

            ui.label(egui::RichText::new("EdenExplorer is a next-generation, blazing-fast fully open-source file explorer built for Windows 11+ using Rust and egui. Designed from the ground up for performance, efficiency, and modern workflows, EdenExplorer is the best FOSS alternative to the default Windows File Explorer.").font(font_id).size(palette.text_size).color(palette.text_normal));
            ui.add_space(8.0);
            ui.heading("Cargo Dependencies");
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(200.0)
                .max_width(200.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_height(ui.available_height());
                        // Dependencies table
                        egui::Grid::new("dependencies_table")
                            .num_columns(2)
                            .max_col_width(ui.available_width() / 2.0)
                            .spacing([10.0, 4.0])
                            .striped(true)
                            .show(ui, |ui| {
                                // Header
                                ui.label(egui::RichText::new("Dependency").strong().color(palette.text_normal));
                                ui.label(egui::RichText::new("Version").strong().color(palette.text_normal));
                                ui.end_row();
                                    let mut style = (*ui.ctx().style()).clone();
                                    style.text_styles = [
                                        (egui::TextStyle::Heading, egui::FontId::proportional(9.0)),
                                        (egui::TextStyle::Body, egui::FontId::proportional(9.0)),
                                        (egui::TextStyle::Button, egui::FontId::proportional(9.0)),
                                        (egui::TextStyle::Small, egui::FontId::proportional(9.0)),
                                    ].into();

                                    ui.set_style(style);
                                    // Dependencies
                                    ui.label("image");
                                    ui.label("0.25");
                                    ui.end_row();
                                    ui.label("eframe");
                                    ui.label("0.33.3");
                                    ui.end_row();
                                    ui.label("egui");
                                    ui.label("0.33.3");
                                    ui.end_row();
                                    ui.label("egui_extras");
                                    ui.label("0.33.3");
                                    ui.end_row();
                                    ui.label("ntapi");
                                    ui.label("0.4.3");
                                    ui.end_row();
                                    ui.label("windows");
                                    ui.label("0.61.3");
                                    ui.end_row();
                                    ui.label("crossbeam-channel");
                                    ui.label("0.5");
                                    ui.end_row();
                                    ui.label("dirs");
                                    ui.label("5.0");
                                    ui.end_row();
                                    ui.label("serde");
                                    ui.label("1.0");
                                    ui.end_row();
                                    ui.label("bincode");
                                    ui.label("1.3");
                                    ui.end_row();
                                    ui.label("rayon");
                                    ui.label("1.10");
                                    ui.end_row();
                                    ui.label("egui-phosphor");
                                    ui.label("0.11");
                                    ui.end_row();
                                    ui.label("serde_json");
                                    ui.label("1.0");
                                    ui.end_row();
                                    ui.label("num_cpus");
                                    ui.label("1.16");
                                    ui.end_row();
                                    ui.label("lazy_static");
                                    ui.label("1.4");
                                    ui.end_row();
                                    ui.label("raw-window-handle");
                                    ui.label("0.6");
                                    ui.end_row();
                                    ui.label("rfd");
                                    ui.label("0.14");
                                    ui.end_row();
                                    ui.label("lru");
                                    ui.label("0.16");
                                    ui.end_row();
                                });
                    });
            ui.add_space(8.0);
            ui.label("Author: Matthew Tucciarone (GitHub: mtucciarone)");
            ui.label("Repo: https://github.com/mtucciarone/EdenExplorer");
            ui.label(format!("Current Version: {}", env!("CARGO_PKG_VERSION")));
            ui.label("License: MIT");
            ui.separator();
            // Footer
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(format!("{} Close", regular::X)).clicked() {
                        should_close = true;
                    }
                });
            });
        });
    });
    // Update the open state based on should_close
    if should_close {
        settings.open = false;
    }
}
