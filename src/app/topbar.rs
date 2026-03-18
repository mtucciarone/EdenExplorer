use eframe::egui;
use egui_phosphor::regular;

#[derive(Default)]
pub struct TopbarAction {
    pub toggle_theme: bool,
    pub customize_theme: bool,
}

pub fn draw_topbar(ui: &mut egui::Ui, is_dark: bool) -> TopbarAction {
    let mut action = TopbarAction::default();

    ui.horizontal(|ui| {
        // 🍔 Hamburger menu (proper egui menu)
        ui.menu_button(regular::LIST, |ui| {
            ui.set_min_width(180.0);

            if ui.button("🎨 Customize Theme").clicked() {
                action.customize_theme = true;
                ui.close();
            }
        });

        // Right side (theme toggle)
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let icon = if is_dark { regular::SUN } else { regular::MOON };

            if ui.button(icon).clicked() {
                action.toggle_theme = true;
            }
        });
    });

    action
}
