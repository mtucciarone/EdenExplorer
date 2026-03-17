use eframe::egui;
use egui_phosphor::regular;

#[derive(Default)]
pub struct TopbarAction {
    pub toggle_theme: bool,
    pub hamburger: bool,
}

pub fn draw_topbar(
    ui: &mut egui::Ui,
    is_dark: bool,
) -> TopbarAction {
    let mut action = TopbarAction::default();

    ui.horizontal(|ui| {
        if ui.button(regular::LIST).clicked() {
            action.hamburger = true;
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let icon = if is_dark { regular::SUN } else { regular::MOON };
            if ui.button(icon).clicked() {
                action.toggle_theme = true;
            }
        });
    });

    action
}
