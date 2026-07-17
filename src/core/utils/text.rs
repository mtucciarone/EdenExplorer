use crate::gui::theme::ThemePalette;
use eframe::egui;
use egui::FontId;
use lazy_static::lazy_static;
use lru::LruCache;
use std::env;
use std::num::NonZeroUsize;
use std::sync::RwLock;

type TruncKey = (String, u32, u32); // (text, width_bucket, font_size_bucket)

lazy_static! {
    static ref TRUNCATION_CACHE: RwLock<LruCache<TruncKey, String>> =
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

/// The fast binary search truncation algorithm.
fn truncate_text_binary_search(
    ui: &mut egui::Ui,
    text: &str,
    max_width: f32,
    font_id: &egui::FontId,
    color: egui::Color32,
) -> (String, bool) {
    ui.fonts_mut(|f| {
        let full = f.layout_no_wrap(text.to_owned(), font_id.clone(), color);

        if full.size().x <= max_width {
            return (text.to_string(), false);
        }

        let ellipsis = "...";
        let ellipsis_width = f
            .layout_no_wrap(ellipsis.to_string(), font_id.clone(), color)
            .size()
            .x;
        let target_width = max_width - ellipsis_width;

        let chars: Vec<char> = text.chars().collect();
        let mut low = 0;
        let mut high = chars.len();
        let mut buffer = String::with_capacity(text.len());

        while low < high {
            let mid = (low + high) / 2;

            buffer.clear();
            for ch in &chars[..mid] {
                buffer.push(*ch);
            }

            let width = f
                .layout_no_wrap(buffer.clone(), font_id.clone(), color)
                .size()
                .x;

            if width <= target_width {
                low = mid + 1;
            } else {
                high = mid;
            }
        }

        let final_len = low.saturating_sub(1);
        buffer.clear();
        for ch in &chars[..final_len] {
            buffer.push(*ch);
        }
        buffer.push_str(ellipsis);

        (buffer, true)
    })
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
            return (cached.clone(), cached.ends_with("..."));
        }

        let (result, truncated) = truncate_text_binary_search(ui, text, max_width, font_id, color);

        cache.put(key, result.clone());

        return (result, truncated);
    }

    truncate_text_binary_search(ui, text, max_width, font_id, color)
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

pub fn apply_context_menu_typography(ui: &mut egui::Ui, palette: &ThemePalette) {
    let mut style = (*ui.ctx().style()).clone();
    style.text_styles = [
        (
            egui::TextStyle::Body,
            FontId::proportional(palette.context_menu_text_size),
        ),
        (
            egui::TextStyle::Button,
            FontId::proportional(palette.context_menu_text_size),
        ),
        (
            egui::TextStyle::Small,
            FontId::proportional(palette.context_menu_text_size),
        ),
        (
            egui::TextStyle::Heading,
            FontId::proportional(palette.context_menu_text_size + 2.0),
        ),
    ]
    .into();
    style.spacing.button_padding = egui::vec2(4.0, 2.0);
    style.spacing.item_spacing = egui::vec2(6.0, 2.0);
    style.spacing.menu_margin = egui::Margin::same(4);
    style.spacing.interact_size = egui::vec2(
        style.spacing.interact_size.x,
        palette.context_menu_text_size + 6.0,
    );
    style.visuals.widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
    style.visuals.widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
    style.visuals.widgets.hovered.bg_fill = palette.primary;
    style.visuals.widgets.hovered.weak_bg_fill = palette.primary;
    style.visuals.widgets.active.bg_fill = palette.primary;
    style.visuals.widgets.active.weak_bg_fill = palette.primary;
    ui.set_style(style);
}
