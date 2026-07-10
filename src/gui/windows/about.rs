use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::structs::AboutWindow;
use eframe::egui;
use egui::FontId;
use egui_phosphor::regular;

pub fn draw_about_window(
    i18n: &I18n,
    ctx: &egui::Context,
    settings: &mut AboutWindow,
    palette: &ThemePalette,
) {
    if !settings.open {
        return;
    }

    let mut should_close = false;

    // 🌑 Dark background overlay (modal effect); clicking it dismisses the window
    let modal_bg_clicked = egui::Area::new(egui::Id::new("about_modal_bg"))
        .order(egui::Order::Middle)
        .interactable(true)
        .show(ctx, |ui| {
            let rect = ctx.content_rect();
            ui.painter()
                .rect_filled(rect, 0.0, palette.modal_background_effect_color);
            ui.interact(rect, ui.id().with("about_modal_bg_click"), egui::Sense::click())
                .clicked()
        })
        .inner;

    if modal_bg_clicked {
        should_close = true;
    }

    egui::Window::new(format!("{} {}", &i18n.tr("about"), regular::INFO))
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
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                let font_id = FontId::new(palette.text_size, egui::FontFamily::Proportional);

                ui.label(
                    egui::RichText::new(&i18n.tr("about_description"))
                        .font(font_id.clone())
                        .size(palette.text_size)
                        .color(palette.text_normal),
                );
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{}: Matthew Tucciarone (GitHub: mtucciarone)",
                        &i18n.tr("about_author")
                    ))
                    .font(font_id.clone())
                    .size(palette.text_size)
                    .color(palette.text_normal),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "{}: https://github.com/mtucciarone/EdenExplorer",
                        &i18n.tr("about_repo")
                    ))
                    .font(font_id.clone())
                    .size(palette.text_size)
                    .color(palette.text_normal),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "{}: {}",
                        &i18n.tr("about_current_version"),
                        env!("CARGO_PKG_VERSION")
                    ))
                    .font(font_id.clone())
                    .size(palette.text_size)
                    .color(palette.text_normal),
                );
                ui.label(
                    egui::RichText::new(format!("{}: MIT", &i18n.tr("about_license")))
                        .font(font_id.clone())
                        .size(palette.text_size)
                        .color(palette.text_normal),
                );
                ui.separator();
                // Footer
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(format!("{} {}", regular::X, &i18n.tr("close")))
                            .clicked()
                        {
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
