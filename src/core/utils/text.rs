use crate::gui::theme::ThemePalette;
use eframe::egui;
use egui::FontId;
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use lazy_static::lazy_static;
use lru::LruCache;
use std::env;
use std::num::NonZeroUsize;
use std::sync::RwLock;

type TruncKey = (String, u32, u32); // (text, width_bucket, font_size_bucket)

lazy_static! {
    static ref TRUNCATION_CACHE: RwLock<LruCache<TruncKey, (String, bool)>> =
        RwLock::new(LruCache::new(NonZeroUsize::new(1024).unwrap()));
}

pub fn fuzzy_match(name: &str, query: &str) -> bool {
    let mut query_chars = query.chars().map(|c| c.to_ascii_lowercase());
    let mut current = query_chars.next();

    for c in name.chars().map(|c| c.to_ascii_lowercase()) {
        if let Some(q) = current {
            if c == q {
                current = query_chars.next();
            }
        } else {
            return true;
        }
    }

    current.is_none()
}

/// Expands Windows environment variables in a path (e.g., %appdata% -> C:\Users\...\AppData\Roaming)
pub fn expand_environment_variables(path: &str) -> String {
    let mut result = String::new();
    let mut chars = path.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            let mut var_name = String::new();

            while let Some(&next_ch) = chars.peek() {
                if next_ch == '%' {
                    chars.next();
                    break;
                }
                var_name.push(chars.next().unwrap());
            }

            if !var_name.is_empty() {
                if let Ok(value) = env::var(&var_name) {
                    result.push_str(&value);
                } else {
                    result.push('%');
                    result.push_str(&var_name);
                    result.push('%');
                }
            } else {
                result.push('%');
            }
        } else {
            result.push(ch);
        }
    }

    result
}

fn width_bucket(width: f32) -> u32 {
    (width / 8.0).round() as u32
}

/// Truncate text using egui's native single-pass text wrapping.
fn truncate_text_native(
    ui: &mut egui::Ui,
    text: &str,
    max_width: f32,
    font_id: &egui::FontId,
    color: egui::Color32,
) -> (String, bool) {
    ui.fonts_mut(|f| {
        let mut job =
            LayoutJob::single_section(text.to_owned(), TextFormat::simple(font_id.clone(), color));
        job.wrap = TextWrapping::truncate_at_width(max_width.max(0.0));
        job.break_on_newline = false;

        let galley = f.layout_job(job);
        if !galley.elided {
            return (text.to_string(), false);
        }

        let display_text = galley
            .rows
            .first()
            .map(|row| row.text())
            .unwrap_or_default();

        (display_text, true)
    })
}

pub fn paint_truncated_text(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    text: &str,
    font_id: &egui::FontId,
    color: egui::Color32,
    truncate: bool,
) -> bool {
    let galley = ui.fonts_mut(|f| {
        if truncate {
            let mut job = LayoutJob::single_section(
                text.to_owned(),
                TextFormat::simple(font_id.clone(), color),
            );
            job.wrap = TextWrapping::truncate_at_width(rect.width().max(0.0));
            job.break_on_newline = false;
            f.layout_job(job)
        } else {
            f.layout_no_wrap(text.to_owned(), font_id.clone(), color)
        }
    });

    let truncated = truncate && galley.elided;
    let pos = egui::pos2(rect.left(), rect.center().y - galley.size().y * 0.5);
    ui.painter().with_clip_rect(rect).galley(pos, galley, color);

    truncated
}

/// Truncates text to fit within `max_width`, caching results for performance.
pub fn truncate_item_text(
    ui: &mut egui::Ui,
    text: &str,
    max_width: f32,
    font_id: &egui::FontId,
    color: egui::Color32,
) -> (String, bool) {
    let width_bucket = width_bucket(max_width);
    let font_bucket = font_id.size.round() as u32;
    let key = (text.to_string(), width_bucket, font_bucket);

    if let Ok(mut cache) = TRUNCATION_CACHE.write() {
        if let Some(cached) = cache.get(&key) {
            return cached.clone();
        }

        let (result, truncated) = truncate_text_native(ui, text, max_width, font_id, color);

        cache.put(key, (result.clone(), truncated));

        return (result, truncated);
    }

    truncate_text_native(ui, text, max_width, font_id, color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_environment_variables() {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let expanded = expand_environment_variables("%appdata%");
            assert_eq!(expanded, appdata);
        }

        if let Ok(windir) = std::env::var("WINDIR") {
            let expanded = expand_environment_variables("%WiNdIr%");
            assert_eq!(expanded, windir);
        }

        let expanded = expand_environment_variables("%nonexistent%");
        assert_eq!(expanded, "%nonexistent%");

        if let Ok(appdata) = std::env::var("APPDATA") {
            let expanded = expand_environment_variables("C:\\test\\%appdata%\\subfolder");
            assert_eq!(expanded, format!("C:\\test\\{}\\subfolder", appdata));
        }

        let expanded = expand_environment_variables("C:\\Windows\\System32");
        assert_eq!(expanded, "C:\\Windows\\System32");

        let expanded = expand_environment_variables("");
        assert_eq!(expanded, "");

        if let (Ok(appdata), Ok(windir)) = (std::env::var("APPDATA"), std::env::var("WINDIR")) {
            let expanded = expand_environment_variables("%appdata%\\%windir%");
            assert_eq!(expanded, format!("{}\\{}", appdata, windir));
        }
    }
}

pub fn apply_eden_text_overrides(ui: &mut egui::Ui, palette: &ThemePalette) {
    let text_styles = &mut ui.style_mut().text_styles;

    text_styles.insert(
        egui::TextStyle::Body,
        FontId::proportional(palette.text_size),
    );

    text_styles.insert(
        egui::TextStyle::Button,
        FontId::proportional(palette.text_size),
    );

    text_styles.insert(
        egui::TextStyle::Small,
        FontId::proportional(palette.text_size),
    );

    text_styles.insert(
        egui::TextStyle::Heading,
        FontId::proportional(palette.text_size + 2.0),
    );
}
