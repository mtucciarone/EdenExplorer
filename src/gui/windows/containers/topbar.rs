use crate::gui::utils::clickable_icon;
use crate::gui::windows::containers::structs::TopbarAction;
use eframe::egui;
use egui::Pos2;
use egui_phosphor::regular;

pub fn draw_topbar(
    ui: &mut egui::Ui,
    is_dark: bool,
    palette: &crate::gui::theme::ThemePalette,
) -> TopbarAction {
    let mut action = TopbarAction::default();

    ui.horizontal(|ui| {
        ui.add_space(12.0);
        let menu_id = egui::Id::new("topbar_hamburger_menu");

        let menu_open = ui.memory(|mem| mem.data.get_temp::<bool>(menu_id).unwrap_or(false));

        if clickable_icon(ui, regular::LIST, palette.primary)
            .on_hover_text(
                egui::RichText::new("Menu")
                    .size(palette.tooltip_text_size)
                    .color(palette.tooltip_text_color),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            ui.memory_mut(|mem| mem.data.insert_temp(menu_id, !menu_open));
        }

        if menu_open {
            let icon_rect = ui.min_rect();
            let popup_pos = Pos2::new(icon_rect.min.x, icon_rect.max.y);

            egui::Area::new(egui::Id::new("topbar_hamburger_menu_area"))
                .fixed_pos(popup_pos)
                .show(ui.ctx(), |ui| {
                    egui::containers::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(120.0);

                        // if menu_item(ui, regular::PALETTE, "Theme", palette).clicked() {
                        //     action.customize_theme = true;
                        //     ui.memory_mut(|mem| mem.data.insert_temp(menu_id, false));
                        // }

                        if menu_item(ui, regular::SLIDERS, "Settings", palette).clicked() {
                            action.open_settings = true;
                            ui.memory_mut(|mem| mem.data.insert_temp(menu_id, false));
                        }

                        if menu_item(ui, regular::QUESTION, "About", palette).clicked() {
                            action.about = true;
                            ui.memory_mut(|mem| mem.data.insert_temp(menu_id, false));
                        }

                        if menu_item(ui, regular::X, "Exit", palette).clicked() {
                            action.exit = true;
                            ui.memory_mut(|mem| mem.data.insert_temp(menu_id, false));
                        }
                    });
                });
        }

        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            let icon = if is_dark { regular::SUN } else { regular::MOON };

            if clickable_icon(ui, icon, palette.primary)
                .on_hover_text(
                    egui::RichText::new("Toggle theme")
                        .size(palette.tooltip_text_size)
                        .color(palette.tooltip_text_color),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                action.toggle_theme = true;
            }
        });

        // Drag region: remaining empty space in the topbar row
        let drag_rect = ui.available_rect_before_wrap();
        if drag_rect.width() > 0.0 && drag_rect.height() > 0.0 {
            let resp = ui.allocate_rect(drag_rect, egui::Sense::click_and_drag());
            if resp.drag_started() || resp.dragged() {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
        }
    });

    action
}

fn menu_item(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    palette: &crate::gui::theme::ThemePalette,
) -> egui::Response {
    let text_galley = ui.fonts_mut(|fonts| {
        fonts.layout_no_wrap(
            label.to_owned(),
            egui::FontId::proportional(palette.text_size),
            ui.visuals().text_color(),
        )
    });
    let text_width = text_galley.size().x;
    let row_height = 18.0;
    let icon_width = 18.0;
    let spacing = 4.0;

    let total_size = egui::vec2(icon_width + spacing + text_width, row_height);

    let (rect, response) = ui.allocate_exact_size(total_size, egui::Sense::click());
    let mut x = rect.min.x;
    let center_y = rect.center().y;

    let icon_color = if response.hovered() {
        palette.primary
    } else {
        ui.visuals().text_color()
    };
    ui.painter().text(
        egui::pos2(x, center_y),
        egui::Align2::LEFT_CENTER,
        icon,
        egui::FontId::default(),
        icon_color,
    );
    x += icon_width + spacing;

    let text_color = if response.hovered() {
        palette.primary
    } else {
        ui.visuals().text_color()
    };
    ui.painter().text(
        egui::pos2(x, center_y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(palette.text_size),
        text_color,
    );

    response
}
