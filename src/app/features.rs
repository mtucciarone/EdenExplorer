use eframe::egui;

#[derive(Clone, Copy, PartialEq)]
pub enum ThemeMode {
    Light,
    Dark,
}

pub struct ThemePalette {
    pub sidebar_bg: egui::Color32,
    pub sidebar_hover: egui::Color32,
    pub sidebar_active: egui::Color32,
}

pub fn palette(mode: ThemeMode) -> ThemePalette {
    match mode {
        ThemeMode::Dark => ThemePalette {
            sidebar_bg: egui::Color32::from_rgb(28, 32, 37),
            sidebar_hover: egui::Color32::from_rgb(38, 44, 52),
            sidebar_active: egui::Color32::from_rgb(46, 54, 64),
        },
        ThemeMode::Light => ThemePalette {
            sidebar_bg: egui::Color32::from_rgb(235, 239, 245),
            sidebar_hover: egui::Color32::from_rgb(224, 232, 242),
            sidebar_active: egui::Color32::from_rgb(214, 224, 236),
        },
    }
}

pub fn apply_theme(ctx: &egui::Context, mode: ThemeMode) {
    let mut style = (*ctx.style()).clone();
    style.visuals = match mode {
        ThemeMode::Dark => egui::Visuals::dark(),
        ThemeMode::Light => egui::Visuals::light(),
    };

    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(10);
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(18.0));
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
    style.visuals.window_corner_radius = egui::CornerRadius::same(10);
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(6);
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(6);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(6);

    match mode {
        ThemeMode::Dark => {
            style.visuals.panel_fill = egui::Color32::from_rgb(20, 22, 26);
            style.visuals.faint_bg_color = egui::Color32::from_rgb(26, 30, 36);
            style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(30, 34, 40);
            style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(38, 44, 52);
            style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(46, 54, 64);
            style.visuals.selection.bg_fill = egui::Color32::from_rgb(60, 90, 130);
            style.visuals.selection.stroke.color = egui::Color32::from_rgb(120, 160, 210);
            style.visuals.widgets.inactive.fg_stroke.color = egui::Color32::from_rgb(220, 226, 232);
            style.visuals.widgets.hovered.fg_stroke.color = egui::Color32::from_rgb(235, 240, 246);
            style.visuals.widgets.active.fg_stroke.color = egui::Color32::from_rgb(245, 248, 252);
            style.visuals.widgets.noninteractive.fg_stroke.color =
                egui::Color32::from_rgb(160, 170, 180);
        }
        ThemeMode::Light => {
            style.visuals.panel_fill = egui::Color32::from_rgb(250, 251, 253);
            style.visuals.faint_bg_color = egui::Color32::from_rgb(244, 246, 249);
            style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(247, 248, 250);
            style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(236, 240, 246);
            style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(224, 231, 242);
            style.visuals.selection.bg_fill = egui::Color32::from_rgb(210, 225, 245);
            style.visuals.selection.stroke.color = egui::Color32::from_rgb(60, 90, 130);
            style.visuals.widgets.inactive.fg_stroke.color = egui::Color32::from_rgb(35, 41, 47);
            style.visuals.widgets.hovered.fg_stroke.color = egui::Color32::from_rgb(25, 29, 33);
            style.visuals.widgets.active.fg_stroke.color = egui::Color32::from_rgb(15, 18, 22);
            style.visuals.widgets.noninteractive.fg_stroke.color =
                egui::Color32::from_rgb(70, 78, 86);
        }
    }

    ctx.set_style(style);
}
