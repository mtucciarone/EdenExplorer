use eframe::egui;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub enum ThemeMode {
    Light,
    Dark,
}

impl Default for ThemeMode {
    fn default() -> Self {
        ThemeMode::Dark
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThemePalette {
    pub text_size: f32,
    // 🎯 Core brand color
    pub primary: egui::Color32,
    pub primary_hover: egui::Color32,
    pub primary_active: egui::Color32,
    pub primary_subtle: egui::Color32,
    pub secondary: egui::Color32,

    // 🎯 UI element colors
    pub box_selection_stroke: egui::Color32,
    pub box_selection_fill: egui::Color32,
    pub icon_color: egui::Color32,
    pub row_label_selected: egui::Color32,
    pub row_label_default: egui::Color32,
    pub row_selected_bg: egui::Color32,
    pub row_bg: egui::Color32,
    pub itemviewer_header_color: egui::Color32,

    // 🎯 Corner radius values
    pub small_radius: u8,
    pub medium_radius: u8,
    pub large_radius: u8,
    pub tab_active_radius: egui::CornerRadius,
    pub tab_inactive_radius: egui::CornerRadius,
    pub tab_button_radius: egui::CornerRadius,

    // 🎯 Drive usage colors
    pub drive_usage_critical: egui::Color32,
    pub drive_usage_warning: egui::Color32,
    pub drive_usage_normal: egui::Color32,

    // 🎯 Tab button colors
    pub tab_close_hover: egui::Color32,
    pub tab_add_hover: egui::Color32,

    // checkbox
    pub checkbox_bg_default: egui::Color32,
    pub checkbox_checkmark_color: egui::Color32,
    pub checkbox_bg_hover: egui::Color32,
    pub checkbox_bg_active: egui::Color32,
}

// 🎯 Single base color (your purple)
fn base_color() -> egui::Color32 {
    egui::Color32::from_rgb(110, 85, 160)
}

// 🌙 Dark theme palette (lazy)
pub static PALETTE_DARK: LazyLock<ThemePalette> = LazyLock::new(|| {
    let base = base_color();
    ThemePalette {
        text_size: 12.0,
        primary: base,
        primary_hover: egui::Color32::from_rgba_unmultiplied(95, 75, 135, 128),
        primary_active: egui::Color32::from_rgb(70, 55, 110),
        primary_subtle: egui::Color32::from_rgba_unmultiplied(95, 75, 135, 60),
        secondary: egui::Color32::from_rgb(255, 255, 255),
        box_selection_stroke: egui::Color32::from_rgba_unmultiplied(95, 75, 135, 60),
        box_selection_fill: base,
        icon_color: egui::Color32::WHITE,
        row_label_selected: egui::Color32::WHITE,
        row_label_default: egui::Color32::from_rgb(160, 170, 180),
        row_selected_bg: egui::Color32::from_rgb(70, 78, 86),
        row_bg: egui::Color32::from_rgb(40, 45, 50),
        itemviewer_header_color: egui::Color32::WHITE,
        small_radius: 2,
        medium_radius: 4,
        large_radius: 6,
        tab_active_radius: egui::CornerRadius {
            nw: 8,
            ne: 8,
            sw: 0,
            se: 0,
        },
        tab_inactive_radius: egui::CornerRadius {
            nw: 6,
            ne: 6,
            sw: 0,
            se: 0,
        },
        tab_button_radius: egui::CornerRadius::same(4),
        drive_usage_critical: egui::Color32::from_rgb(200, 72, 72),
        drive_usage_warning: egui::Color32::from_rgb(214, 170, 76),
        drive_usage_normal: egui::Color32::from_rgb(88, 170, 120),
        tab_close_hover: egui::Color32::from_rgb(200, 52, 52),
        tab_add_hover: egui::Color32::from_rgb(54, 168, 82),
        checkbox_bg_default: egui::Color32::from_rgba_unmultiplied(160, 170, 180, 20),
        checkbox_checkmark_color: egui::Color32::WHITE,
        checkbox_bg_hover: base,
        checkbox_bg_active: base,
    }
});

// ☀️ Light theme palette (lazy)
pub static PALETTE_LIGHT: LazyLock<ThemePalette> = LazyLock::new(|| {
    let base = base_color();
    ThemePalette {
        text_size: 12.0,
        primary: base,
        primary_hover: egui::Color32::from_rgba_unmultiplied(110, 85, 160, 90),
        primary_active: egui::Color32::from_rgb(140, 120, 200),
        primary_subtle: egui::Color32::from_rgba_unmultiplied(110, 85, 160, 40),
        secondary: egui::Color32::from_rgb(0, 0, 0),
        box_selection_stroke: egui::Color32::from_rgba_unmultiplied(110, 85, 160, 40),
        box_selection_fill: base,
        icon_color: egui::Color32::WHITE,
        row_label_selected: egui::Color32::from_rgb(0, 0, 0),
        row_label_default: egui::Color32::from_rgb(70, 78, 86),
        row_selected_bg: egui::Color32::from_rgb(70, 78, 86),
        row_bg: egui::Color32::from_rgb(240, 245, 250),
        itemviewer_header_color: egui::Color32::from_rgb(0, 0, 0),
        small_radius: 2,
        medium_radius: 4,
        large_radius: 6,
        tab_active_radius: egui::CornerRadius {
            nw: 8,
            ne: 8,
            sw: 0,
            se: 0,
        },
        tab_inactive_radius: egui::CornerRadius {
            nw: 6,
            ne: 6,
            sw: 0,
            se: 0,
        },
        tab_button_radius: egui::CornerRadius::same(4),
        drive_usage_critical: egui::Color32::from_rgb(200, 72, 72),
        drive_usage_warning: egui::Color32::from_rgb(214, 170, 76),
        drive_usage_normal: egui::Color32::from_rgb(88, 170, 120),
        tab_close_hover: egui::Color32::from_rgb(200, 52, 52),
        tab_add_hover: egui::Color32::from_rgb(54, 168, 82),
        checkbox_bg_default: egui::Color32::from_rgba_unmultiplied(160, 170, 180, 95),
        checkbox_checkmark_color: egui::Color32::WHITE,
        checkbox_bg_hover: base,
        checkbox_bg_active: base,
    }
});

// Usage:
pub fn get_palette(mode: ThemeMode) -> &'static ThemePalette {
    match mode {
        ThemeMode::Dark => &PALETTE_DARK,
        ThemeMode::Light => &PALETTE_LIGHT,
    }
}

pub fn apply_theme(ctx: &egui::Context, mode: ThemeMode) {
    let mut style = (*ctx.style()).clone();
    let palette = get_palette(mode);

    style.visuals = match mode {
        ThemeMode::Dark => egui::Visuals::dark(),
        ThemeMode::Light => egui::Visuals::light(),
    };

    // 📐 Layout / spacing
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(10);

    // 🔤 Typography
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(18.0));
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));

    // 🔲 Shape
    style.visuals.window_corner_radius = egui::CornerRadius::same(10);
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(6);
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(6);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(6);

    match mode {
        ThemeMode::Dark => {
            style.visuals.panel_fill = egui::Color32::from_rgb(20, 22, 26);
            style.visuals.faint_bg_color = egui::Color32::from_rgb(26, 30, 36);

            style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(30, 34, 40);

            // 🎯 PRIMARY SYSTEM
            style.visuals.widgets.hovered.bg_fill = palette.primary_hover;
            style.visuals.widgets.hovered.weak_bg_fill = palette.primary_hover;
            style.visuals.widgets.hovered.bg_stroke.color = palette.primary_hover;

            style.visuals.widgets.active.bg_fill = palette.primary_active;

            style.visuals.selection.bg_fill = palette.primary_hover;
            style.visuals.selection.stroke.color = palette.primary_active;

            // 🔤 Text
            style.visuals.widgets.inactive.fg_stroke.color = egui::Color32::from_rgb(220, 226, 232);
            style.visuals.widgets.hovered.fg_stroke.color = egui::Color32::from_rgb(245, 240, 255);
            style.visuals.widgets.active.fg_stroke.color = egui::Color32::from_rgb(255, 250, 255);

            style.visuals.widgets.noninteractive.fg_stroke.color =
                egui::Color32::from_rgb(160, 170, 180);
        }

        ThemeMode::Light => {
            style.visuals.panel_fill = egui::Color32::from_rgb(250, 251, 253);
            style.visuals.faint_bg_color = egui::Color32::from_rgb(244, 246, 249);

            style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(247, 248, 250);

            // 🎯 SAME SYSTEM (just lighter)
            style.visuals.widgets.hovered.bg_fill = palette.primary_hover;
            style.visuals.widgets.hovered.weak_bg_fill = palette.primary_hover;
            style.visuals.widgets.hovered.bg_stroke.color = palette.primary_hover;

            style.visuals.widgets.active.bg_fill = palette.primary_active;

            style.visuals.selection.bg_fill = palette.primary_subtle;
            style.visuals.selection.stroke.color = palette.primary;

            // 🔤 Text
            style.visuals.widgets.inactive.fg_stroke.color = egui::Color32::from_rgb(35, 41, 47);
            style.visuals.widgets.hovered.fg_stroke.color = egui::Color32::from_rgb(25, 29, 33);
            style.visuals.widgets.active.fg_stroke.color = egui::Color32::from_rgb(15, 18, 22);

            style.visuals.widgets.noninteractive.fg_stroke.color =
                egui::Color32::from_rgb(70, 78, 86);
        }
    }

    ctx.set_style(style);
}

pub fn apply_checkbox_colors(ui: &mut egui::Ui, palette: &ThemePalette, checked: bool) {
    let visuals = &mut ui.visuals_mut().widgets;

    // Determine the background color depending on checked state
    let bg_fill = if checked {
        palette.checkbox_bg_active // "base" color when checked
    } else {
        palette.checkbox_bg_default
    };

    // Background fill
    visuals.inactive.bg_fill = bg_fill;
    visuals.hovered.bg_fill = if checked {
        palette.checkbox_bg_active
    } else {
        palette.checkbox_bg_hover
    };
    visuals.active.bg_fill = palette.checkbox_bg_active;

    // Border / stroke
    visuals.inactive.bg_stroke.color = bg_fill;
    visuals.hovered.bg_stroke.color = if checked {
        palette.checkbox_bg_active
    } else {
        palette.checkbox_bg_hover
    };
    visuals.active.bg_stroke.color = palette.checkbox_bg_active;

    // Checkmark color
    visuals.inactive.fg_stroke.color = palette.checkbox_checkmark_color;
    visuals.hovered.fg_stroke.color = palette.checkbox_checkmark_color;
    visuals.active.fg_stroke.color = palette.checkbox_checkmark_color;
}