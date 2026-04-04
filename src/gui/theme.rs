use eframe::egui::{Color32, CornerRadius};
use serde::{Deserialize, Serialize};
use std::sync::{LazyLock, RwLock};

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
    // 🔤 Typography
    pub text_size: f32,
    pub tooltip_text_size: f32,

    // 🎯 Brand / primary colors
    pub primary: Color32,
    pub primary_hover: Color32,
    pub primary_active: Color32,
    pub primary_subtle: Color32,
    pub secondary: Color32,
    pub text_normal: Color32,
    pub text_header_section: Color32,

    // 🧱 Application surfaces
    pub application_bg_color: Color32,
    pub modal_background_effect_color: Color32,

    // 🎯 Icons & text
    pub icon_color: Color32,
    pub icon_windows: Color32,
    pub icon_colored_hover: Color32,
    pub item_viewer_row_text_selected: Color32,
    pub tooltip_text_color: Color32,
    pub tab_text_selected: Color32,

    // 🎯 Rows / list items
    pub row_selected_bg: Color32,
    pub row_bg: Color32,

    // 🎯 Handles / borders
    pub resize_handle: Color32,
    pub tab_border_active: Color32,
    pub tab_border_default: Color32,

    // 🎯 Tab button colors
    pub tab_close_hover: Color32,
    pub tab_close_active: Color32,
    pub tab_close_normal: Color32,
    pub tab_add_hover: Color32,
    pub pinned_tab_color: Color32,

    // 🎯 Drive usage colors
    pub drive_usage_critical: Color32,
    pub drive_usage_warning: Color32,
    pub drive_usage_normal: Color32,
    pub drive_usage_background: Color32,
    pub drive_usage_text: Color32,

    // ☑️ Checkbox
    pub checkbox_bg_default: Color32,
    pub checkbox_checkmark_color: Color32,
    pub checkbox_bg_hover: Color32,
    pub checkbox_bg_active: Color32,

    // 🔘 Buttons
    pub button_background: Color32,
    pub button_stroke: Color32,
    pub button_favorite_fill: Color32,
    pub button_seperator_handle_fill: Color32,

    // 🎯 Corner radius values
    pub small_radius: u8,
    pub medium_radius: u8,
    pub large_radius: u8,
    pub tab_active_radius: CornerRadius,
    pub tab_inactive_radius: CornerRadius,
    pub tab_button_radius: CornerRadius,
}

// 🎯 Single base color (your purple)
fn base_color() -> Color32 {
    Color32::from_rgb(110, 85, 160)
}

// 🌙 Dark theme palette (defaults)
pub static DEFAULT_PALETTE_DARK: LazyLock<ThemePalette> = LazyLock::new(|| {
    let base = base_color();
    ThemePalette {
        // 🔤 Typography
        text_size: 12.0,
        tooltip_text_size: 13.0,

        // 🎯 Brand / primary colors
        primary: base,
        primary_hover: Color32::from_rgba_unmultiplied(95, 75, 135, 128),
        primary_active: Color32::from_rgb(70, 55, 110),
        primary_subtle: Color32::from_rgba_unmultiplied(95, 75, 135, 60),
        secondary: Color32::from_rgb(255, 255, 255),
        text_normal: Color32::from_rgb(160, 170, 180),
        text_header_section: Color32::WHITE,

        // 🧱 Application surfaces
        application_bg_color: Color32::from_rgb(20, 22, 26),
        modal_background_effect_color: Color32::from_black_alpha(180),

        // 🎯 Icons & text
        icon_color: Color32::WHITE,
        icon_windows: Color32::WHITE,
        icon_colored_hover: Color32::WHITE,
        item_viewer_row_text_selected: Color32::WHITE,
        tooltip_text_color: Color32::from_rgb(160, 170, 180),
        tab_text_selected: Color32::WHITE,

        // 🎯 Rows / list items
        row_selected_bg: Color32::from_rgb(70, 78, 86),
        row_bg: Color32::from_rgb(40, 45, 50),

        // 🎯 Handles / borders
        resize_handle: Color32::from_rgb(160, 170, 180),
        tab_border_active: base,
        tab_border_default: Color32::from_rgba_unmultiplied(95, 75, 135, 60),

        // 🎯 Tab button colors
        tab_close_hover: Color32::from_rgb(200, 52, 52),
        tab_close_active: Color32::WHITE,
        tab_close_normal: Color32::from_rgb(160, 170, 180),
        tab_add_hover: Color32::from_rgb(54, 168, 82),
        pinned_tab_color: Color32::from_rgb(242, 201, 76),

        // 🎯 Drive usage colors
        drive_usage_critical: Color32::from_rgb(220, 60, 60),
        drive_usage_warning: Color32::from_rgb(245, 170, 60),
        drive_usage_normal: Color32::from_rgb(60, 190, 110),
        drive_usage_background: Color32::from_rgba_unmultiplied(160, 170, 180, 20),
        drive_usage_text: Color32::WHITE,

        // ☑️ Checkbox
        checkbox_bg_default: Color32::from_rgba_unmultiplied(160, 170, 180, 20),
        checkbox_checkmark_color: Color32::WHITE,
        checkbox_bg_hover: base,
        checkbox_bg_active: base,

        // 🔘 Buttons
        button_background: Color32::from_rgba_unmultiplied(160, 170, 180, 20),
        button_stroke: Color32::from_rgba_unmultiplied(160, 170, 180, 60),
        button_favorite_fill: Color32::from_rgb(242, 201, 76),
        button_seperator_handle_fill: Color32::from_gray(120),

        // 🎯 Corner radius values
        small_radius: 2,
        medium_radius: 4,
        large_radius: 6,
        tab_active_radius: CornerRadius {
            nw: 8,
            ne: 8,
            sw: 0,
            se: 0,
        },
        tab_inactive_radius: CornerRadius {
            nw: 6,
            ne: 6,
            sw: 0,
            se: 0,
        },
        tab_button_radius: CornerRadius::same(4),
    }
});

// ☀️ Light theme palette (defaults)
pub static DEFAULT_PALETTE_LIGHT: LazyLock<ThemePalette> = LazyLock::new(|| {
    let base = base_color();
    ThemePalette {
        // 🔤 Typography
        text_size: 12.0,
        tooltip_text_size: 13.0,

        // 🎯 Brand / primary colors
        primary: base,
        primary_hover: Color32::from_rgba_unmultiplied(110, 85, 160, 90),
        primary_active: Color32::from_rgb(140, 120, 200),
        primary_subtle: Color32::from_rgba_unmultiplied(110, 85, 160, 40),
        secondary: Color32::BLACK,
        text_normal: Color32::from_rgb(70, 78, 86),
        text_header_section: Color32::BLACK,

        // 🧱 Application surfaces
        application_bg_color: Color32::from_rgb(245, 245, 245),
        modal_background_effect_color: Color32::from_black_alpha(180),

        // 🎯 Icons & text
        icon_color: Color32::from_rgb(40, 40, 40),
        icon_windows: Color32::WHITE,
        icon_colored_hover: Color32::WHITE,
        item_viewer_row_text_selected: Color32::BLACK,
        tooltip_text_color: Color32::from_rgb(40, 40, 40),
        tab_text_selected: Color32::WHITE,

        // 🎯 Rows / list items
        row_selected_bg: Color32::from_rgb(70, 78, 86),
        row_bg: Color32::from_rgb(240, 245, 250),

        // 🎯 Handles / borders
        resize_handle: Color32::from_rgb(160, 170, 180),
        tab_border_active: base,
        tab_border_default: Color32::from_rgba_unmultiplied(110, 85, 160, 40),

        // 🎯 Tab button colors
        tab_close_hover: Color32::from_rgb(200, 52, 52),
        tab_close_active: Color32::WHITE,
        tab_close_normal: Color32::from_rgb(40, 40, 40),
        tab_add_hover: Color32::from_rgb(54, 168, 82),
        pinned_tab_color: Color32::from_rgb(242, 201, 76),

        // 🎯 Drive usage colors
        drive_usage_critical: Color32::from_rgb(200, 52, 52),
        drive_usage_warning: Color32::from_rgb(235, 155, 60),
        drive_usage_normal: Color32::from_rgb(54, 168, 82),
        drive_usage_background: Color32::from_rgba_unmultiplied(200, 210, 220, 120),
        drive_usage_text: Color32::from_rgb(70, 78, 86),

        // ☑️ Checkbox
        checkbox_bg_default: Color32::from_rgba_unmultiplied(160, 170, 180, 95),
        checkbox_checkmark_color: Color32::WHITE,
        checkbox_bg_hover: base,
        checkbox_bg_active: base,

        // 🔘 Buttons
        button_background: Color32::from_rgba_unmultiplied(160, 170, 180, 95),
        button_stroke: Color32::from_rgba_unmultiplied(160, 170, 180, 60),
        button_favorite_fill: Color32::from_rgb(242, 201, 76),
        button_seperator_handle_fill: Color32::from_gray(180),

        // 🎯 Corner radius values
        small_radius: 2,
        medium_radius: 4,
        large_radius: 6,
        tab_active_radius: CornerRadius {
            nw: 8,
            ne: 8,
            sw: 0,
            se: 0,
        },
        tab_inactive_radius: CornerRadius {
            nw: 6,
            ne: 6,
            sw: 0,
            se: 0,
        },
        tab_button_radius: CornerRadius::same(4),
    }
});

// 🎯 Runtime-editable palettes
static PALETTE_DARK: LazyLock<RwLock<ThemePalette>> =
    LazyLock::new(|| RwLock::new(DEFAULT_PALETTE_DARK.clone()));
static PALETTE_LIGHT: LazyLock<RwLock<ThemePalette>> =
    LazyLock::new(|| RwLock::new(DEFAULT_PALETTE_LIGHT.clone()));

// Usage:
pub fn get_palette(mode: ThemeMode) -> ThemePalette {
    match mode {
        ThemeMode::Dark => PALETTE_DARK
            .read()
            .map(|p| p.clone())
            .unwrap_or_else(|_| DEFAULT_PALETTE_DARK.clone()),
        ThemeMode::Light => PALETTE_LIGHT
            .read()
            .map(|p| p.clone())
            .unwrap_or_else(|_| DEFAULT_PALETTE_LIGHT.clone()),
    }
}

pub fn get_default_palette(mode: ThemeMode) -> ThemePalette {
    match mode {
        ThemeMode::Dark => DEFAULT_PALETTE_DARK.clone(),
        ThemeMode::Light => DEFAULT_PALETTE_LIGHT.clone(),
    }
}

pub fn set_palette(mode: ThemeMode, palette: ThemePalette) {
    let target = match mode {
        ThemeMode::Dark => &PALETTE_DARK,
        ThemeMode::Light => &PALETTE_LIGHT,
    };

    if let Ok(mut guard) = target.write() {
        *guard = palette;
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
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::proportional(palette.text_size + 4.0),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::proportional(palette.text_size),
    );

    // 🔲 Shape
    style.visuals.window_corner_radius = CornerRadius::same(10);
    style.visuals.widgets.inactive.corner_radius = CornerRadius::same(6);
    style.visuals.widgets.hovered.corner_radius = CornerRadius::same(6);
    style.visuals.widgets.active.corner_radius = CornerRadius::same(6);

    match mode {
        ThemeMode::Dark => {
            style.visuals.panel_fill = palette.application_bg_color;
            style.visuals.faint_bg_color = Color32::from_rgb(26, 30, 36);

            style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(30, 34, 40);

            // 🎯 PRIMARY SYSTEM
            style.visuals.widgets.hovered.bg_fill = palette.primary_hover;
            style.visuals.widgets.hovered.weak_bg_fill = palette.primary_hover;
            style.visuals.widgets.hovered.bg_stroke.color = palette.primary_hover;

            style.visuals.widgets.active.bg_fill = palette.primary_active;

            style.visuals.selection.bg_fill = palette.primary_hover;
            style.visuals.selection.stroke.color = palette.primary_active;

            // 🔤 Text
            style.visuals.widgets.inactive.fg_stroke.color = Color32::from_rgb(220, 226, 232);
            style.visuals.widgets.hovered.fg_stroke.color = Color32::from_rgb(245, 240, 255);
            style.visuals.widgets.active.fg_stroke.color = Color32::from_rgb(255, 250, 255);

            style.visuals.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(160, 170, 180);
        }

        ThemeMode::Light => {
            style.visuals.panel_fill = palette.application_bg_color;
            style.visuals.faint_bg_color = Color32::from_rgb(244, 246, 249);

            style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(247, 248, 250);

            // 🎯 SAME SYSTEM (just lighter)
            style.visuals.widgets.hovered.bg_fill = palette.primary_hover;
            style.visuals.widgets.hovered.weak_bg_fill = palette.primary_hover;
            style.visuals.widgets.hovered.bg_stroke.color = palette.primary_hover;

            style.visuals.widgets.active.bg_fill = palette.primary_active;

            style.visuals.selection.bg_fill = palette.primary_subtle;
            style.visuals.selection.stroke.color = palette.primary;

            // 🔤 Text
            style.visuals.widgets.inactive.fg_stroke.color = Color32::from_rgb(35, 41, 47);
            style.visuals.widgets.hovered.fg_stroke.color = Color32::from_rgb(25, 29, 33);
            style.visuals.widgets.active.fg_stroke.color = Color32::from_rgb(15, 18, 22);

            style.visuals.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(70, 78, 86);
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
