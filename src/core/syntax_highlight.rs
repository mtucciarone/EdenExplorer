//! Syntax highlighting for the text preview pane, via `syntect` (configured
//! with its pure-Rust `fancy-regex` backend, not the default C/Oniguruma
//! `onig` one, to avoid pulling a C toolchain dependency into this codebase -
//! see `preview.rs`'s existing AVIF/`dav1d` avoidance note for the same
//! policy elsewhere).
//!
//! Exposes a single `highlighted_layout_job` entry point that turns a file's
//! text into an `egui::text::LayoutJob` with per-token colors. Both the
//! `Text` payload's `TextEdit` layouter and the Markdown renderer's fenced
//! code blocks go through this one function, so there's exactly one place
//! that knows how to map a syntect `Style` onto an egui `TextFormat`.

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style as SynStyle, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: std::sync::OnceLock<SyntaxSet> = std::sync::OnceLock::new();
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme_set() -> &'static ThemeSet {
    static THEME_SET: std::sync::OnceLock<ThemeSet> = std::sync::OnceLock::new();
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

fn theme_for_mode(dark_mode: bool) -> &'static Theme {
    let themes = &theme_set().themes;
    let name = if dark_mode {
        "base16-ocean.dark"
    } else {
        "InspiredGitHub"
    };
    themes
        .get(name)
        .unwrap_or_else(|| themes.values().next().expect("syntect ships default themes"))
}

/// Whether `token` (a file extension like `"rs"`, or a Markdown fenced-code-
/// block language tag like `"rust"`/`"python"`) has a syntect syntax
/// definition - used to decide whether a file/code-block gets highlighted
/// at all versus falling back to plain monospace text (every extension
/// `is_known_text_extension` already recognizes still previews fine either
/// way; this only controls whether it's colored). Checks extension first,
/// then syntax name case-insensitively - `find_syntax_by_token` is syntect's
/// own purpose-built lookup for exactly this "short token from the user"
/// case (its own docs cite Markdown code-block highlighting by name).
pub fn has_syntax_for_extension(token: &str) -> bool {
    syntax_set().find_syntax_by_token(token).is_some()
}

/// Builds a highlighted `LayoutJob` for `text`, assuming it's the language
/// named by `token` (extension or fence-language-tag - see
/// `has_syntax_for_extension`). Never fails - an unrecognized token (or any
/// internal syntect error) falls back to a plain, uncolored monospace job
/// identical in shape to what the caller would've built without
/// highlighting at all, so callers don't need their own fallback path.
pub fn highlighted_layout_job(text: &str, token: &str, font_id: FontId, dark_mode: bool) -> LayoutJob {
    let ss = syntax_set();
    let Some(syntax) = ss.find_syntax_by_token(token) else {
        return plain_layout_job(text, font_id);
    };

    let theme = theme_for_mode(dark_mode);
    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut job = LayoutJob::default();

    for line in LinesWithEndings::from(text) {
        let Ok(ranges) = highlighter.highlight_line(line, ss) else {
            job.append(line, 0.0, TextFormat::simple(font_id.clone(), Color32::GRAY));
            continue;
        };

        for (style, piece) in ranges {
            job.append(piece, 0.0, TextFormat::simple(font_id.clone(), syn_color(style)));
        }
    }

    job
}

fn plain_layout_job(text: &str, font_id: FontId) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(text, 0.0, TextFormat::simple(font_id, Color32::GRAY));
    job
}

fn syn_color(style: SynStyle) -> Color32 {
    let c = style.foreground;
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_common_code_extensions() {
        assert!(has_syntax_for_extension("rs"));
        assert!(has_syntax_for_extension("py"));
        assert!(has_syntax_for_extension("js"));
        assert!(has_syntax_for_extension("json"));
    }

    #[test]
    fn highlighting_json_produces_more_than_one_styled_run() {
        let job = highlighted_layout_job(
            "{\"name\": \"eden\", \"count\": 3}",
            "json",
            FontId::monospace(12.0),
            true,
        );
        assert!(job.sections.len() > 1);
    }

    #[test]
    fn unrecognized_extension_has_no_syntax() {
        assert!(!has_syntax_for_extension("not-a-real-extension"));
    }

    #[test]
    fn highlighting_an_unrecognized_extension_falls_back_to_one_plain_run() {
        let job = highlighted_layout_job("hello world", "zzz", FontId::monospace(12.0), true);
        assert_eq!(job.text, "hello world");
        assert_eq!(job.sections.len(), 1);
    }

    #[test]
    fn highlighting_rust_produces_more_than_one_styled_run() {
        let job = highlighted_layout_job(
            "fn main() { println!(\"hi\"); }",
            "rs",
            FontId::monospace(12.0),
            true,
        );
        assert_eq!(job.text, "fn main() { println!(\"hi\"); }");
        assert!(job.sections.len() > 1);
    }
}
