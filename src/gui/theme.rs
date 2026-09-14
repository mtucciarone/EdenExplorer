use crate::core::utils::fonts::{apply_custom_font_definitions, load_font_data};
use eframe::egui::{Color32, CornerRadius};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::{LazyLock, RwLock};

pub const THEME_VERSION: u32 = 8;

fn default_itemviewer_row_height() -> f32 {
    16.0
}

fn default_sidebar_icon_size() -> f32 {
    20.0
}

fn default_tab_icon_size() -> f32 {
    12.0
}

fn default_text_size() -> f32 {
    11.0
}

fn default_font_name() -> String {
    "Segoe UI".to_string()
}

fn default_mono_font_name() -> String {
    "Hack".to_string()
}

// Fallbacks used only when loading a settings file saved before these fields
// existed - `regenerate_base_derived_colors` recomputes accurate values the
// next time the accent color changes, these just need to look reasonable in
// the meantime (they match the dark theme's default derivation).
fn default_icon_color_dark() -> Color32 {
    Color32::WHITE
}

fn default_sidebar_bg_color() -> Color32 {
    Color32::from_rgb(24, 27, 32)
}

fn default_tab_inactive_bg_color() -> Color32 {
    Color32::from_rgb(28, 32, 38)
}

// Fallbacks for colors that were previously hardcoded per-mode literals in
// `apply_theme` rather than real fields - matches the dark theme's own
// values (same "close enough until the user edits it, or the mode's own
// default palette re-initializes it" reasoning as the fallbacks above).
fn default_faint_bg_color() -> Color32 {
    Color32::from_rgb(26, 30, 36)
}

fn default_input_field_bg() -> Color32 {
    Color32::from_rgb(30, 34, 40)
}

fn default_widget_text_inactive() -> Color32 {
    Color32::from_rgb(220, 226, 232)
}

fn default_widget_text_hovered() -> Color32 {
    Color32::from_rgb(245, 240, 255)
}

fn default_widget_text_active() -> Color32 {
    Color32::from_rgb(255, 250, 255)
}

fn default_selection_text_color() -> Color32 {
    Color32::WHITE
}

// Matches the Default preset's own secondary (see `PALETTE_PRESETS`) so an
// old save file loading this field for the first time sees no change until
// it actually applies a preset.
fn default_secondary_accent() -> Color32 {
    Color32::from_rgb(242, 201, 76)
}

fn default_button_text_color() -> Color32 {
    Color32::from_rgb(220, 226, 232)
}

// `apply_theme` resets `style.visuals` to egui's own stock dark/light
// defaults before applying palette overrides, but never touched
// `widgets.noninteractive.bg_stroke` - the color every plain `ui.separator()`
// call actually draws. Matches egui's own stock dark-mode value (same
// "close enough for a migration fallback" reasoning as `default_icon_color_
// dark` above - a serde default fn has no way to know which mode's palette
// it's filling in for) rather than a fixed arbitrary literal.
fn default_separator_color() -> Color32 {
    egui::Visuals::dark().widgets.noninteractive.bg_stroke.color
}

// Fallbacks for a batch of surfaces/text that were previously either
// ambient `ui.visuals().text_color()` lookups or simply undrawn
// (transparent) - matches the dark theme's own equivalent value/lack of a
// fill, so these are all true no-ops for an existing user until touched.
fn default_ambient_text_color() -> Color32 {
    Color32::from_rgb(160, 170, 180)
}

fn default_transparent() -> Color32 {
    Color32::TRANSPARENT
}

// Matches the dark theme's own default `primary_hover` (the value this
// field replaces as a direct read) for the default accent color.
fn default_search_active_icon_bg() -> Color32 {
    Color32::from_rgba_unmultiplied(110, 85, 160, 128)
}

// Matches the dark theme's own `borders_default`/`input_field_bg`/
// `text_normal` defaults - the values these fields replace as direct reads
// in the notification panel/toast.
fn default_notification_border_color() -> Color32 {
    Color32::from_rgba_unmultiplied(110, 85, 160, 60)
}

fn default_notification_bg_color() -> Color32 {
    Color32::from_rgb(30, 34, 40)
}

fn default_notification_header_text_color() -> Color32 {
    Color32::from_rgb(160, 170, 180)
}

// Standard semantic colors - unlike the fields above, these introduce a
// genuinely new look (the panel previously used the accent color for every
// status) rather than preserving an old value, since that's the whole
// point of this field: completed/in-progress/failed should read as
// green/yellow/red regardless of the theme's own accent.
fn default_notification_status_success() -> Color32 {
    Color32::from_rgb(60, 190, 110)
}

fn default_notification_status_warning() -> Color32 {
    Color32::from_rgb(235, 155, 60)
}

fn default_notification_status_error() -> Color32 {
    Color32::from_rgb(220, 60, 60)
}

fn default_notification_status_info() -> Color32 {
    Color32::from_rgb(0, 120, 215)
}

// `primary_dialog_button` previously hardcoded its text to plain white
// regardless of theme/mode - matches that exact literal.
fn default_primary_button_text_color() -> Color32 {
    Color32::WHITE
}

// Matches `base_color()` - the accent shade `palette.primary` starts as
// before any preset/custom accent is applied, since these fields replace
// what used to be direct `palette.primary` reads.
fn default_accent_derived_color() -> Color32 {
    Color32::from_rgb(110, 85, 160)
}

// `eden_button` previously had no disabled/enabled distinction at all - it
// always drew with `button_background`/`button_text_color`, and relied
// purely on egui's automatic disabled-opacity fade to look "disabled".
// Defaulting these to the dark theme's own `button_background`/
// `button_text_color` values preserves that exact look until a user
// actually picks a distinct disabled color.
fn default_button_disabled_bg() -> Color32 {
    Color32::from_rgba_unmultiplied(160, 170, 180, 20)
}

fn default_button_disabled_text() -> Color32 {
    Color32::from_rgb(220, 226, 232)
}

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
    #[serde(default = "default_text_size")]
    pub text_size: f32,
    pub tooltip_text_size: f32,
    #[serde(default = "default_sidebar_icon_size")]
    pub sidebar_icon_size: f32,
    #[serde(default = "default_tab_icon_size")]
    pub tab_icon_size: f32,
    #[serde(default = "default_font_name")]
    pub font_name: String,
    #[serde(default = "default_mono_font_name")]
    pub mono_font_name: String,

    // 🎯 Brand / primary colors
    pub primary: Color32,
    pub primary_hover: Color32,
    pub primary_active: Color32,
    pub text_normal: Color32,
    pub text_header_section: Color32,

    // 🧱 Application surfaces
    pub application_bg_color: Color32,
    pub modal_background_effect_color: Color32,

    // 🎯 Icons & text
    #[serde(default = "default_icon_color_dark")]
    pub icon_color: Color32,
    pub icon_windows: Color32,
    pub icon_colored_hover: Color32,
    #[serde(default = "default_icon_color_dark")]
    pub toolbar_icon_color: Color32,
    pub item_viewer_row_text_selected: Color32,
    pub tooltip_text_color: Color32,
    pub tab_text_selected: Color32,

    // 🎯 Rows / list items
    pub row_selected_bg: Color32,
    pub row_bg: Color32,
    #[serde(default = "default_itemviewer_row_height")]
    pub row_height: f32,
    pub sidebar_item_spacing_y: f32,

    // 🧱 Panel surfaces (sidebar/tabs get their own accent-tinted surface,
    // distinct from the main explorer background)
    #[serde(default = "default_sidebar_bg_color")]
    pub sidebar_bg_color: Color32,
    #[serde(default = "default_tab_inactive_bg_color")]
    pub tab_inactive_bg_color: Color32,

    // 🎯 Handles / borders
    pub resize_handle: Color32,
    pub borders_default: Color32,
    pub borders_active: Color32,

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

    // 🎯 Corner radius values
    pub small_radius: u8,
    pub medium_radius: u8,
    pub large_radius: u8,
    pub tab_active_radius: CornerRadius,
    pub tab_inactive_radius: CornerRadius,
    pub tab_button_radius: CornerRadius,

    // 🎯 Widget surfaces/text that used to be hardcoded per-mode literals in
    // `apply_theme` - promoted to real fields so a full re-theme can reach
    // them too. Defaults below match each mode's previous hardcoded value,
    // so existing users see zero visual change until they touch a picker.
    #[serde(default = "default_faint_bg_color")]
    pub faint_bg_color: Color32,
    /// Shared by a `TextEdit`'s own background (`extreme_bg_color`) and
    /// every other input widget's background (`widgets.inactive.bg_fill`) -
    /// these were already kept equal to each other in `apply_theme`, so one
    /// field covers both rather than risking them drifting apart.
    #[serde(default = "default_input_field_bg")]
    pub input_field_bg: Color32,
    #[serde(default = "default_widget_text_inactive")]
    pub widget_text_inactive: Color32,
    #[serde(default = "default_widget_text_hovered")]
    pub widget_text_hovered: Color32,
    #[serde(default = "default_widget_text_active")]
    pub widget_text_active: Color32,
    /// Text color repainted over a text selection highlight - kept separate
    /// from `text_normal` since it needs to contrast against
    /// `style.visuals.selection.bg_fill`, not the page background.
    #[serde(default = "default_selection_text_color")]
    pub selection_text_color: Color32,

    /// The theme's "second" hue, deliberately distinct from `primary` in hue
    /// and value (a `ThemePresetDef` picks it via complementary/temperature
    /// contrast against its own accent - see `PALETTE_PRESETS`'s doc
    /// comment). `regenerate_base_derived_colors` tints `toolbar_icon_color`
    /// from this instead of `primary`, so the toolbar reads as a distinct
    /// "band" from the primary-tinted surfaces around it. Not exposed as its
    /// own Core Colors picker - `pinned_tab_color`/`button_favorite_fill`/
    /// `toolbar_icon_color` already have their own rows for manual tuning,
    /// this field only exists so a preset click can set all of them
    /// consistently in one step.
    #[serde(default = "default_secondary_accent")]
    pub secondary_accent: Color32,

    /// Text color for `eden_button` (the app's general-purpose button, used
    /// throughout Settings, dialogs, and confirmation popups) - a light
    /// secondary-accent tint, mirroring how `icon_color` is a light
    /// primary-accent tint, so button text picks up the theme's second
    /// color too without sacrificing legibility.
    #[serde(default = "default_button_text_color")]
    pub button_text_color: Color32,

    /// Color of every plain `ui.separator()` line app-wide (Settings page
    /// section dividers, the notification panel divider, toolbar dividers,
    /// etc.) - previously not palette-driven at all (`apply_theme` never
    /// touched `widgets.noninteractive.bg_stroke`, so every separator used
    /// egui's own stock gray regardless of theme).
    #[serde(default = "default_separator_color")]
    pub separator_color: Color32,

    // Sidebar / status bar / address bar / search / settings-nav / toolbar /
    // preview-pane - surfaces that were either ambient `ui.visuals().
    // text_color()` lookups or simply never filled at all, promoted to real
    // fields so the whole app is genuinely palette-driven.
    #[serde(default = "default_ambient_text_color")]
    pub sidebar_text_color: Color32,
    #[serde(default = "default_transparent")]
    pub status_bar_bg_color: Color32,
    #[serde(default = "default_ambient_text_color")]
    pub status_bar_text_color: Color32,
    #[serde(default = "default_ambient_text_color")]
    pub status_bar_icon_color: Color32,
    #[serde(default = "default_transparent")]
    pub address_bar_bg_color: Color32,
    /// Background behind an active/hovered `clickable_active_icon` (the
    /// search box's This-folder/Everywhere scope toggle, the topbar's
    /// active-state icons) - previously just read `primary_hover` directly
    /// with no dedicated field of its own. Still re-derived from the accent
    /// by `regenerate_base_derived_colors` (matching `primary_hover`'s own
    /// default), so a preset/accent change still updates it automatically,
    /// exactly like before - it's just independently editable now too.
    #[serde(default = "default_search_active_icon_bg")]
    pub search_active_icon_bg: Color32,
    #[serde(default = "default_ambient_text_color")]
    pub settings_nav_inactive_color: Color32,
    #[serde(default = "default_transparent")]
    pub toolbar_bg_color: Color32,
    #[serde(default = "default_ambient_text_color")]
    pub toolbar_icon_disabled_color: Color32,
    #[serde(default = "default_transparent")]
    pub preview_pane_bg_color: Color32,

    // Notification panel/toast - previously a mix of ambient/shared-field
    // reads (border/background/header text) and a single accent color used
    // for every status (success/warning/error all read as `primary`).
    #[serde(default = "default_notification_border_color")]
    pub notification_border_color: Color32,
    #[serde(default = "default_notification_bg_color")]
    pub notification_bg_color: Color32,
    #[serde(default = "default_notification_header_text_color")]
    pub notification_header_text_color: Color32,
    /// Standard semantic colors for `FileOpStatus` - completed (green),
    /// in-progress/paused (yellow/orange), failed/cancelled (red). `_info`
    /// (blue) has no current call site (there's no plain "info" toast in
    /// this codebase, only `FileOpStatus`-backed ones) but is still a real,
    /// user-editable field for future use.
    #[serde(default = "default_notification_status_success")]
    pub notification_status_success: Color32,
    #[serde(default = "default_notification_status_warning")]
    pub notification_status_warning: Color32,
    #[serde(default = "default_notification_status_error")]
    pub notification_status_error: Color32,
    #[serde(default = "default_notification_status_info")]
    pub notification_status_info: Color32,

    // Button color completeness - a dedicated text color for the dialog
    // "primary" button (previously hardcoded to plain white regardless of
    // theme) and a real disabled-state pair for `eden_button` (previously
    // no distinction at all beyond egui's own automatic opacity fade).
    #[serde(default = "default_primary_button_text_color")]
    pub primary_button_text_color: Color32,
    #[serde(default = "default_button_disabled_bg")]
    pub button_disabled_bg: Color32,
    #[serde(default = "default_button_disabled_text")]
    pub button_disabled_text: Color32,

    // Settings nav selected-item text/icon, the navigation toast's own
    // border/background (previously sharing the notification-panel fields
    // rather than having its own), and the toolbar/address-bar active-icon
    // color - all previously hardcoded to `palette.primary` or the shared
    // notification fields, not independently themeable.
    #[serde(default = "default_accent_derived_color")]
    pub settings_nav_selected_text_color: Color32,
    #[serde(default = "default_accent_derived_color")]
    pub settings_nav_selected_icon_color: Color32,
    #[serde(default = "default_notification_border_color")]
    pub navigation_toast_border_color: Color32,
    #[serde(default = "default_notification_bg_color")]
    pub navigation_toast_bg_color: Color32,
    #[serde(default = "default_accent_derived_color")]
    pub toolbar_icon_active_color: Color32,

    // Hover state for every plain toolbar icon button (back/forward/up/
    // refresh/new-folder/favorite-star/display-mode) - previously each
    // button's hover color was hardcoded to `palette.primary` with no
    // background at all, *except* Refresh, which alone used a different
    // shared helper that also painted a hardcoded `primary_hover`-derived
    // background - an inconsistency between otherwise-identical toolbar
    // buttons. Refresh now goes through the same `nav_icon_button` helper
    // as the rest instead of its own, so these two fields cover the whole
    // toolbar row uniformly. Background default is transparent (matching
    // every button *except* Refresh's prior look) rather than Refresh's old
    // background, since that was the minority behavior.
    #[serde(default = "default_accent_derived_color")]
    pub toolbar_icon_hover_color: Color32,
    #[serde(default = "default_transparent")]
    pub toolbar_icon_hover_bg_color: Color32,
}

// 🎯 Single base color (your purple)
fn base_color() -> Color32 {
    Color32::from_rgb(110, 85, 160)
}

// 🌙 Dark theme palette (defaults)
pub static DEFAULT_PALETTE_DARK: LazyLock<ThemePalette> = LazyLock::new(|| {
    let base = base_color();
    let mut palette = ThemePalette {
        // 🔤 Typography
        text_size: 12.0,
        tooltip_text_size: 13.0,
        sidebar_icon_size: 20.0,
        tab_icon_size: 12.0,
        font_name: default_font_name(),
        mono_font_name: default_mono_font_name(),

        // 🎯 Brand / primary colors
        primary: base,
        primary_hover: Color32::from_rgba_unmultiplied(95, 75, 135, 128),
        primary_active: Color32::from_rgb(70, 55, 110),
        text_normal: Color32::from_rgb(160, 170, 180),
        text_header_section: Color32::WHITE,

        // 🧱 Application surfaces
        application_bg_color: Color32::from_rgb(20, 22, 26),
        modal_background_effect_color: Color32::from_black_alpha(180),

        // 🎯 Icons & text
        icon_color: Color32::WHITE,
        icon_windows: Color32::WHITE,
        icon_colored_hover: Color32::WHITE,
        toolbar_icon_color: Color32::WHITE,
        item_viewer_row_text_selected: Color32::WHITE,
        tooltip_text_color: Color32::from_rgb(160, 170, 180),
        tab_text_selected: Color32::WHITE,

        // 🎯 Rows / list items
        row_selected_bg: Color32::from_rgb(70, 78, 86),
        row_bg: Color32::from_rgb(40, 45, 50),
        row_height: default_itemviewer_row_height(),
        sidebar_item_spacing_y: 0.5,

        // 🧱 Panel surfaces
        sidebar_bg_color: Color32::from_rgb(24, 27, 32),
        tab_inactive_bg_color: Color32::from_rgb(28, 32, 38),

        // 🎯 Handles / borders
        resize_handle: Color32::from_rgb(160, 170, 180),
        borders_default: Color32::from_rgba_unmultiplied(95, 75, 135, 60),
        borders_active: base,

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

        // 🎯 Widget surfaces/text (formerly hardcoded in `apply_theme`)
        faint_bg_color: Color32::from_rgb(26, 30, 36),
        input_field_bg: Color32::from_rgb(30, 34, 40),
        widget_text_inactive: Color32::from_rgb(220, 226, 232),
        widget_text_hovered: Color32::from_rgb(245, 240, 255),
        widget_text_active: Color32::from_rgb(255, 250, 255),
        selection_text_color: Color32::from_rgb(255, 255, 255),
        secondary_accent: default_secondary_accent(),
        button_text_color: default_button_text_color(),
        separator_color: egui::Visuals::dark().widgets.noninteractive.bg_stroke.color,
        sidebar_text_color: Color32::from_rgb(160, 170, 180),
        status_bar_bg_color: Color32::TRANSPARENT,
        status_bar_text_color: Color32::from_rgb(160, 170, 180),
        status_bar_icon_color: Color32::from_rgb(160, 170, 180),
        address_bar_bg_color: Color32::TRANSPARENT,
        search_active_icon_bg: default_search_active_icon_bg(),
        settings_nav_inactive_color: Color32::from_rgb(160, 170, 180),
        toolbar_bg_color: Color32::TRANSPARENT,
        toolbar_icon_disabled_color: Color32::from_rgb(160, 170, 180),
        preview_pane_bg_color: Color32::TRANSPARENT,
        notification_border_color: default_notification_border_color(),
        notification_bg_color: default_notification_bg_color(),
        notification_header_text_color: Color32::from_rgb(160, 170, 180),
        notification_status_success: default_notification_status_success(),
        notification_status_warning: default_notification_status_warning(),
        notification_status_error: default_notification_status_error(),
        notification_status_info: default_notification_status_info(),
        primary_button_text_color: default_primary_button_text_color(),
        // Overwritten below by `regenerate_base_derived_colors` (tracks
        // `button_background`/`button_text_color`) - placeholder value here
        // is never actually observed.
        button_disabled_bg: Color32::TRANSPARENT,
        button_disabled_text: Color32::TRANSPARENT,
        // Overwritten below by `regenerate_base_derived_colors` (tracks
        // `primary`, matching what these fields replaced).
        settings_nav_selected_text_color: base,
        settings_nav_selected_icon_color: base,
        navigation_toast_border_color: default_notification_border_color(),
        navigation_toast_bg_color: default_notification_bg_color(),
        toolbar_icon_active_color: base,
        toolbar_icon_hover_color: base,
        toolbar_icon_hover_bg_color: Color32::TRANSPARENT,
    };
    regenerate_base_derived_colors(&mut palette, true);
    palette
});

// ☀️ Light theme palette (defaults)
pub static DEFAULT_PALETTE_LIGHT: LazyLock<ThemePalette> = LazyLock::new(|| {
    let base = base_color();
    let mut palette = ThemePalette {
        // 🔤 Typography
        text_size: 12.0,
        tooltip_text_size: 13.0,
        sidebar_icon_size: 20.0,
        tab_icon_size: 12.0,
        font_name: default_font_name(),
        mono_font_name: default_mono_font_name(),

        // 🎯 Brand / primary colors
        primary: base,
        primary_hover: Color32::from_rgba_unmultiplied(110, 85, 160, 90),
        primary_active: Color32::from_rgb(140, 120, 200),
        text_normal: Color32::from_rgb(70, 78, 86),
        text_header_section: Color32::BLACK,

        // 🧱 Application surfaces
        application_bg_color: Color32::from_rgb(245, 245, 245),
        modal_background_effect_color: Color32::from_black_alpha(180),

        // 🎯 Icons & text
        icon_color: Color32::from_rgb(40, 40, 40),
        icon_windows: Color32::WHITE,
        icon_colored_hover: Color32::WHITE,
        toolbar_icon_color: Color32::from_rgb(40, 40, 40),
        item_viewer_row_text_selected: Color32::BLACK,
        tooltip_text_color: Color32::from_rgb(40, 40, 40),
        tab_text_selected: Color32::WHITE,

        // 🎯 Rows / list items
        row_selected_bg: Color32::from_rgb(70, 78, 86),
        row_bg: Color32::from_rgb(240, 245, 250),
        row_height: default_itemviewer_row_height(),
        sidebar_item_spacing_y: 0.5,

        // 🧱 Panel surfaces
        sidebar_bg_color: Color32::from_rgb(238, 239, 242),
        tab_inactive_bg_color: Color32::from_rgb(233, 235, 239),

        // 🎯 Handles / borders
        resize_handle: Color32::from_rgb(160, 170, 180),
        borders_default: Color32::from_rgba_unmultiplied(110, 85, 160, 40),
        borders_active: base,

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

        // 🎯 Widget surfaces/text (formerly hardcoded in `apply_theme`)
        faint_bg_color: Color32::from_rgb(244, 246, 249),
        input_field_bg: Color32::from_rgb(247, 248, 250),
        widget_text_inactive: Color32::from_rgb(35, 41, 47),
        widget_text_hovered: Color32::from_rgb(25, 29, 33),
        widget_text_active: Color32::from_rgb(15, 18, 22),
        selection_text_color: Color32::from_rgb(15, 18, 22),
        secondary_accent: default_secondary_accent(),
        button_text_color: Color32::from_rgb(35, 41, 47),
        separator_color: egui::Visuals::light().widgets.noninteractive.bg_stroke.color,
        sidebar_text_color: Color32::from_rgb(70, 78, 86),
        status_bar_bg_color: Color32::TRANSPARENT,
        status_bar_text_color: Color32::from_rgb(70, 78, 86),
        status_bar_icon_color: Color32::from_rgb(70, 78, 86),
        address_bar_bg_color: Color32::TRANSPARENT,
        search_active_icon_bg: Color32::from_rgba_unmultiplied(110, 85, 160, 90),
        settings_nav_inactive_color: Color32::from_rgb(70, 78, 86),
        toolbar_bg_color: Color32::TRANSPARENT,
        toolbar_icon_disabled_color: Color32::from_rgb(70, 78, 86),
        preview_pane_bg_color: Color32::TRANSPARENT,
        notification_border_color: Color32::from_rgba_unmultiplied(110, 85, 160, 40),
        notification_bg_color: Color32::from_rgb(247, 248, 250),
        notification_header_text_color: Color32::from_rgb(70, 78, 86),
        notification_status_success: Color32::from_rgb(54, 168, 82),
        notification_status_warning: Color32::from_rgb(235, 155, 60),
        notification_status_error: Color32::from_rgb(200, 52, 52),
        notification_status_info: Color32::from_rgb(0, 120, 215),
        primary_button_text_color: Color32::WHITE,
        button_disabled_bg: Color32::TRANSPARENT,
        button_disabled_text: Color32::TRANSPARENT,
        settings_nav_selected_text_color: base,
        settings_nav_selected_icon_color: base,
        navigation_toast_border_color: Color32::from_rgba_unmultiplied(110, 85, 160, 40),
        navigation_toast_bg_color: Color32::from_rgb(247, 248, 250),
        toolbar_icon_active_color: base,
        toolbar_icon_hover_color: base,
        toolbar_icon_hover_bg_color: Color32::TRANSPARENT,
    };
    regenerate_base_derived_colors(&mut palette, false);
    palette
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
    let theme = match mode {
        ThemeMode::Dark => egui::Theme::Dark,
        ThemeMode::Light => egui::Theme::Light,
    };

    let mut style = (*ctx.style_of(theme)).clone();
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

    style.visuals.panel_fill = palette.application_bg_color;
    style.visuals.faint_bg_color = palette.faint_bg_color;

    style.visuals.widgets.inactive.bg_fill = palette.input_field_bg;
    // `extreme_bg_color` is what a `TextEdit`'s own interior uses (as
    // opposed to `widgets.inactive.bg_fill`, which a `DragValue` or button
    // uses) - kept equal to `input_field_bg` so a text field doesn't read as
    // a completely different color than every other input in the same form.
    style.visuals.extreme_bg_color = palette.input_field_bg;

    // 🎯 PRIMARY SYSTEM
    style.visuals.widgets.hovered.bg_fill = palette.primary_hover;
    style.visuals.widgets.hovered.weak_bg_fill = palette.primary_hover;
    style.visuals.widgets.hovered.bg_stroke.color = palette.primary_hover;

    style.visuals.widgets.active.bg_fill = palette.primary_active;

    style.visuals.selection.bg_fill = match mode {
        ThemeMode::Dark => palette.primary_hover,
        ThemeMode::Light => palette.borders_default,
    };
    // Selected text is repainted in this color (not the field's normal text
    // color) - the highlight itself is a mid-tone accent shade, so this
    // needs to be a separate, deliberately high-contrast color rather than
    // reusing `primary_active`/the accent directly.
    style.visuals.selection.stroke.color = palette.selection_text_color;

    // 🔤 Text
    style.visuals.widgets.inactive.fg_stroke.color = palette.widget_text_inactive;
    style.visuals.widgets.hovered.fg_stroke.color = palette.widget_text_hovered;
    style.visuals.widgets.active.fg_stroke.color = palette.widget_text_active;
    style.visuals.widgets.noninteractive.fg_stroke.color = palette.text_normal;
    style.visuals.widgets.noninteractive.bg_stroke.color = palette.separator_color;

    ctx.set_style_of(theme, style);
    ctx.set_theme(theme);
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

/// Like `tint`, but for a translucent neutral (e.g. `button_background`'s
/// low-alpha gray) rather than an opaque surface color - tints the RGB the
/// same way, then reapplies `alpha` rather than the fully-opaque result
/// `tint` itself would produce.
fn tint_alpha(base_rgb: Color32, accent: Color32, t: f32, alpha: u8) -> Color32 {
    let tinted = tint(base_rgb, accent, t);
    Color32::from_rgba_unmultiplied(tinted.r(), tinted.g(), tinted.b(), alpha)
}

/// Blend `base` toward `accent` by `t` (0 = pure `base`, 1 = pure `accent`).
/// Used to give neutral surfaces/icons a subtle hint of the accent color
/// without losing contrast, rather than painting them the raw accent.
fn tint(base: Color32, accent: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |b: u8, a: u8| (b as f32 * (1.0 - t) + a as f32 * t).round() as u8;
    Color32::from_rgb(
        mix(base.r(), accent.r()),
        mix(base.g(), accent.g()),
        mix(base.b(), accent.b()),
    )
}

// 🎯 Regenerate all base-derived colors when primary (accent) color changes.
// Backgrounds get a subtle accent tint (`tint`); text/icons that need to
// stay legible against any accent are left as-is by callers - only the
// surfaces below, plus the pre-existing interactive-highlight colors, are
// accent-driven.
pub fn regenerate_base_derived_colors(palette: &mut ThemePalette, is_dark: bool) {
    let base = palette.primary;

    if is_dark {
        // Dark theme calculations
        palette.primary_hover = Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 128);
        palette.primary_active = Color32::from_rgb(
            ((base.r() as u16 * 7) / 10) as u8, // 70% of red
            ((base.g() as u16 * 7) / 10) as u8, // 70% of green
            ((base.b() as u16 * 7) / 10) as u8, // 70% of blue
        );
        palette.borders_default = Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 60);

        // Surfaces: a faint accent tint over each area's neutral dark tone,
        // kept distinct from one another so panels still read as separate.
        palette.application_bg_color = tint(Color32::from_rgb(20, 22, 26), base, 0.05);
        palette.sidebar_bg_color = tint(Color32::from_rgb(24, 27, 32), base, 0.08);
        palette.row_bg = tint(Color32::from_rgb(40, 45, 50), base, 0.05);
        palette.tab_inactive_bg_color = tint(Color32::from_rgb(28, 32, 38), base, 0.10);

        // Icons: white base, only slightly warmed toward the accent so they
        // stay legible against any accent hue - this is the "60%" dominant
        // tier (every ordinary file/folder icon shares this tint, cohering
        // with the accent-tinted surfaces around it). Toolbar icons instead
        // tint toward `secondary_accent`: a full, visually-bounded row of
        // icons is exactly the "30%" tier a genuinely distinct second hue
        // needs to actually read as present, rather than a token accent
        // touch nobody notices.
        palette.icon_color = tint(Color32::WHITE, base, 0.15);
        palette.toolbar_icon_color = tint(Color32::WHITE, palette.secondary_accent, 0.55);

        // Buttons tint from `secondary_accent`, not the primary accent -
        // this is the same "30% tier" as the toolbar icons, so a theme's
        // second color has real, constant presence (every plain button in
        // the app) rather than showing up only in a couple of rare spots.
        let neutral = Color32::from_rgb(160, 170, 180);
        palette.button_background = tint_alpha(neutral, palette.secondary_accent, 0.16, 20);
        palette.button_stroke = tint_alpha(neutral, palette.secondary_accent, 0.30, 90);
        palette.button_text_color = tint(Color32::WHITE, palette.secondary_accent, 0.12);
    } else {
        // Light theme calculations
        palette.primary_hover = Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 90);
        palette.primary_active = Color32::from_rgb(
            ((base.r() as u16 * 14) / 10).min(255) as u8, // 140% of red
            ((base.g() as u16 * 14) / 10).min(255) as u8, // 140% of green
            ((base.b() as u16 * 14) / 10).min(255) as u8, // 140% of blue
        );
        palette.borders_default = Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 40);

        palette.application_bg_color = tint(Color32::from_rgb(245, 245, 245), base, 0.05);
        palette.sidebar_bg_color = tint(Color32::from_rgb(238, 239, 242), base, 0.08);
        palette.row_bg = tint(Color32::from_rgb(240, 245, 250), base, 0.05);
        palette.tab_inactive_bg_color = tint(Color32::from_rgb(233, 235, 239), base, 0.10);

        palette.icon_color = tint(Color32::from_rgb(40, 40, 40), base, 0.15);
        palette.toolbar_icon_color =
            tint(Color32::from_rgb(40, 40, 40), palette.secondary_accent, 0.55);

        let neutral = Color32::from_rgb(160, 170, 180);
        palette.button_background = tint_alpha(neutral, palette.secondary_accent, 0.16, 95);
        palette.button_stroke = tint_alpha(neutral, palette.secondary_accent, 0.30, 90);
        palette.button_text_color = tint(Color32::from_rgb(40, 40, 40), palette.secondary_accent, 0.12);
    }

    // Common to both themes
    palette.borders_active = base;
    palette.checkbox_bg_hover = base;
    palette.checkbox_bg_active = base;
    // Kept in sync with `primary_hover` (the field this one now stands in
    // for, so an accent/preset change still updates it automatically) while
    // still being directly editable on its own, like every other field here.
    palette.search_active_icon_bg = palette.primary_hover;
    // Same reasoning: previously a disabled `eden_button` looked identical
    // to an enabled one (just faded by egui's own opacity multiply), so
    // these default to matching `button_background`/`button_text_color`
    // exactly, tracking the accent/preset the same way those two do.
    palette.button_disabled_bg = palette.button_background;
    palette.button_disabled_text = palette.button_text_color;
    // These three previously just read `palette.primary` directly at their
    // call sites, so default to tracking it the same way, staying in sync
    // with an accent/preset change while still being directly editable.
    palette.settings_nav_selected_text_color = base;
    palette.settings_nav_selected_icon_color = base;
    palette.toolbar_icon_active_color = base;
    palette.toolbar_icon_hover_color = base;
}

/// A named, color-only theme preset - the "bigger-grained" alternative to
/// hand-picking an accent + core colors one at a time.
///
/// Each preset is a deliberate two-color system, not just an accent with a
/// decorative afterthought - `accent` and `secondary` are chosen together
/// against a checklist of real color-design principles, so the second color
/// actually reads as intentional rather than as a random extra swatch:
///
/// - **Complementary / Temperature Contrast**: `secondary` sits on the
///   opposite side of the color wheel from `accent` *and* the opposite
///   temperature (a cool accent gets a warm secondary, and vice versa) -
///   e.g. Midnight's cool blue accent pairs with a warm amber secondary,
///   Sunset's warm orange accent pairs with a cool sky-blue secondary. A
///   secondary that's just a paler/darker tint of the *same* hue as accent
///   (an early mistake in this list, since fixed) isn't a real secondary at
///   all - it's an analogous tint, no contrast to offer.
/// - **Value Contrast**: every pair keeps a meaningfully different
///   perceptual brightness (roughly a 25+ point gap in 0-255 luma), so the
///   two halves of a preset's preview swatch - and the two roles they play
///   in the UI - stay visually distinguishable rather than reading as one
///   same-brightness blob.
/// - **Analogous Ties**: contrast alone would make every preset a jarring
///   photonegative of itself - each secondary's *saturation/value* is tuned
///   to feel like it belongs to the same "family" as its accent (e.g. both
///   fairly saturated and mid-toned, or both muted), which is what actually
///   reads as "these two colors were chosen together" rather than "two
///   colors picked at random."
/// - **60-30-10 Rule / Visual Weight / Asymmetrical Placement**: `accent`
///   is the dominant 60% - it tints the large surfaces (backgrounds,
///   sidebar, row backgrounds) and the app's ordinary file/folder icons via
///   `regenerate_base_derived_colors`. `secondary` is the 30% mid-tier -
///   `toolbar_icon_color` (a whole, visually-bounded row of icons) tints
///   from it instead of accent, so it has real coverage, not a token touch.
///   `pinned_tab_color`/`button_favorite_fill` are the rare 10% pop - tiny,
///   high-saturation accents on elements a user only occasionally sees. The
///   two colors deliberately never share the same *kind* of element
///   (asymmetrical placement) - accent owns backgrounds, secondary owns the
///   toolbar band plus a couple of small pops - so they read as a designed
///   hierarchy instead of two competing "this is the accent" signals.
/// - **Echoing**: `secondary` reappears in three distinct places (toolbar
///   icons, the pinned-tab star, the favorite-folder star) rather than
///   once, so it registers as a real part of the theme's identity instead
///   of a one-off accent nobody notices.
///
/// Deliberately excludes any semantic color (drive-usage warning/critical,
/// tab-close hover, etc.) - a preset can't change what a color *means*
/// elsewhere in the app, only the brand identity built from `accent`/
/// `secondary`.
pub struct ThemePresetDef {
    pub name_key: &'static str,
    pub accent: Color32,
    pub secondary: Color32,
}

pub static PALETTE_PRESETS: &[ThemePresetDef] = &[
    ThemePresetDef {
        name_key: "theme_preset_default",
        accent: Color32::from_rgb(110, 85, 160),
        secondary: Color32::from_rgb(242, 201, 76),
    },
    ThemePresetDef {
        name_key: "theme_preset_midnight",
        accent: Color32::from_rgb(54, 90, 196),
        secondary: Color32::from_rgb(224, 150, 64),
    },
    ThemePresetDef {
        name_key: "theme_preset_ocean",
        accent: Color32::from_rgb(0, 150, 160),
        secondary: Color32::from_rgb(235, 120, 95),
    },
    ThemePresetDef {
        name_key: "theme_preset_forest",
        accent: Color32::from_rgb(46, 133, 64),
        secondary: Color32::from_rgb(214, 170, 64),
    },
    ThemePresetDef {
        name_key: "theme_preset_sunset",
        accent: Color32::from_rgb(214, 94, 54),
        secondary: Color32::from_rgb(110, 180, 230),
    },
    ThemePresetDef {
        name_key: "theme_preset_slate",
        accent: Color32::from_rgb(90, 98, 110),
        secondary: Color32::from_rgb(224, 158, 64),
    },
    ThemePresetDef {
        name_key: "theme_preset_crimson",
        accent: Color32::from_rgb(196, 43, 60),
        secondary: Color32::from_rgb(60, 190, 168),
    },
    ThemePresetDef {
        name_key: "theme_preset_magenta",
        accent: Color32::from_rgb(191, 0, 119),
        secondary: Color32::from_rgb(60, 196, 140),
    },
    ThemePresetDef {
        name_key: "theme_preset_violet",
        accent: Color32::from_rgb(139, 58, 201),
        secondary: Color32::from_rgb(224, 188, 72),
    },
    ThemePresetDef {
        name_key: "theme_preset_indigo",
        accent: Color32::from_rgb(63, 81, 181),
        secondary: Color32::from_rgb(232, 168, 64),
    },
    ThemePresetDef {
        name_key: "theme_preset_skyline",
        accent: Color32::from_rgb(0, 120, 215),
        secondary: Color32::from_rgb(255, 214, 64),
    },
    ThemePresetDef {
        name_key: "theme_preset_horizon",
        accent: Color32::from_rgb(55, 120, 185),
        secondary: Color32::from_rgb(235, 150, 60),
    },
];

/// Applies a named preset's accent + secondary to `palette`, then re-derives
/// every accent/secondary-driven surface/icon color exactly like manually
/// editing the two swatches would - a preset is just a bundle of manual
/// edits applied in one click, not a separate code path.
pub fn apply_theme_preset(palette: &mut ThemePalette, preset: &ThemePresetDef, is_dark: bool) {
    palette.primary = preset.accent;
    palette.secondary_accent = preset.secondary;
    palette.pinned_tab_color = preset.secondary;
    palette.button_favorite_fill = preset.secondary;
    regenerate_base_derived_colors(palette, is_dark);
}

pub fn apply_font_to_context(ctx: &egui::Context, palette: &ThemePalette) {
    let mut fonts = egui::FontDefinitions::default();

    // Load proportional font
    if let Some(font_data) = load_font_data(&palette.font_name) {
        fonts.font_data.insert(
            "custom_proportional".to_string(),
            Arc::new(egui::FontData::from_owned(font_data)),
        );

        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "custom_proportional".to_string());
    }

    // Load monospace font
    if let Some(font_data) = load_font_data(&palette.mono_font_name) {
        fonts.font_data.insert(
            "custom_monospace".to_string(),
            Arc::new(egui::FontData::from_owned(font_data)),
        );

        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "custom_monospace".to_string());
    }

    apply_custom_font_definitions(&mut fonts);

    ctx.set_fonts(fonts);
    ctx.request_repaint();
}
