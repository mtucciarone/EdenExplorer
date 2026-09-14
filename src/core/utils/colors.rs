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

/// Inverse of `hsl_to_color32` - `h` in degrees (0-360), `s`/`l` in 0.0-1.0.
/// Used by the color picker's HSL fields so they can show/accept a value
/// derived from whatever RGB the picker's other controls just produced.
pub fn rgb_to_hsl(color: Color32) -> (f32, f32, f32) {
    let r = color.r() as f32 / 255.0;
    let g = color.g() as f32 / 255.0;
    let b = color.b() as f32 / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let delta = max - min;

    if delta < 1e-6 {
        return (0.0, 0.0, l);
    }

    let s = if l > 0.5 {
        delta / (2.0 - max - min)
    } else {
        delta / (max + min)
    };

    let mut h = if max == r {
        60.0 * (((g - b) / delta) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    if h < 0.0 {
        h += 360.0;
    }

    (h, s, l)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(a: f32, b: f32, tol: f32) {
        assert!((a - b).abs() <= tol, "{a} not within {tol} of {b}");
    }

    #[test]
    fn rgb_to_hsl_matches_known_colors() {
        let (h, s, l) = rgb_to_hsl(Color32::from_rgb(255, 0, 0));
        assert_close(h, 0.0, 0.5);
        assert_close(s, 1.0, 0.01);
        assert_close(l, 0.5, 0.01);

        let (h, s, l) = rgb_to_hsl(Color32::from_rgb(0, 255, 0));
        assert_close(h, 120.0, 0.5);
        assert_close(s, 1.0, 0.01);
        assert_close(l, 0.5, 0.01);

        let (_, s, l) = rgb_to_hsl(Color32::from_rgb(128, 128, 128));
        assert_close(s, 0.0, 0.01);
        assert_close(l, 0.502, 0.01);
    }

    #[test]
    fn hsl_round_trips_through_rgb_for_a_range_of_hues() {
        for hue in (0..360).step_by(15) {
            let original = hsl_to_color32(hue as f32, 0.7, 0.5);
            let (h, s, l) = rgb_to_hsl(original);
            let round_tripped = hsl_to_color32(h, s, l);
            assert_close(round_tripped.r() as f32, original.r() as f32, 2.0);
            assert_close(round_tripped.g() as f32, original.g() as f32, 2.0);
            assert_close(round_tripped.b() as f32, original.b() as f32, 2.0);
        }
    }

    #[test]
    fn grayscale_has_zero_saturation_and_undefined_hue_defaults_to_zero() {
        let (h, s, _) = rgb_to_hsl(Color32::from_rgb(200, 200, 200));
        assert_eq!(h, 0.0);
        assert_eq!(s, 0.0);
    }
}
