use crate::gui::theme::ThemePalette;
use egui::Color32;

pub fn drive_usage_color(ratio: f32, palette: &ThemePalette) -> Color32 {
    let base = if ratio > 0.95 {
        palette.drive_usage_critical
    } else if ratio >= 0.85 {
        palette.drive_usage_warning
    } else {
        palette.drive_usage_normal
    };

    base.gamma_multiply(0.6)
}

#[allow(dead_code)]
pub fn tag_color(tag: &str) -> Color32 {
    let mut h: i32 = 0;

    for c in tag.chars() {
        h = 31i32.wrapping_mul(h).wrapping_add(c as i32);
    }

    let hue = h.unsigned_abs() % 360;

    hsl_to_color32(hue as f32, 0.55, 0.88)
}

pub fn hsl_to_color32(h: f32, s: f32, l: f32) -> Color32 {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
    let m = l - c / 2.0;

    let (r, g, b) = match h {
        h if h < 60.0 => (c, x, 0.0),
        h if h < 120.0 => (x, c, 0.0),
        h if h < 180.0 => (0.0, c, x),
        h if h < 240.0 => (0.0, x, c),
        h if h < 300.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    Color32::from_rgb(
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}
