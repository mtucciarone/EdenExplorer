//! Newer interface preferences, saved as JSON in `ui_prefs.json`.
//!
//! Older settings live in postcard files where adding a field breaks
//! decoding of existing files, which is why several features got their own
//! small `.bin` file. This one is JSON with `#[serde(default)]`, so new
//! options can be added here later without breaking saved files: a missing
//! field simply takes its default.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPrefs {
    /// Each folder remembers its own view mode, sort, and columns
    /// (`AppSettings::directory_settings`). Off = every folder uses the
    /// default view.
    pub remember_folder_views: bool,
    /// Keep calculated folder sizes on disk so they show instantly on the
    /// next visit (see `core::folder_size_cache`).
    pub persist_folder_sizes: bool,
    /// Show a thumbnail tooltip when hovering an image or video file.
    pub hover_previews: bool,
    /// Space opens Quick Look (a large preview of the selected item); off =
    /// Space starts type-to-filter like any other character.
    pub quick_look: bool,
    /// Double-clicking a zip/7z/tar archive opens it like a folder instead
    /// of in its default program (see `core::archive_view`).
    pub browse_archives: bool,
    /// Copies and moves that share a disk run one after another instead of
    /// at the same time (see the transfer queue in `notifications`).
    pub queue_transfers: bool,
    /// After a copy finishes, every copied file is compared with its
    /// source by checksum (see `core::verify`).
    pub verify_copies: bool,
    /// Put the address bar on its own row above the toolbar (split panes
    /// always do); off = the toolbar and address bar share one row.
    pub address_bar_own_row: bool,
    /// Analyzing a whole NTFS drive asks Windows for administrator
    /// permission so the fast MFT scan can run (see `core::mft_helper`);
    /// off = the fast scan only runs when the app already is elevated.
    pub disk_usage_ask_admin: bool,
    /// Extra folder listed under New (besides the built-in templates) whose
    /// files are offered as templates. `None` = the default
    /// `Templates` folder inside the data folder.
    pub templates_folder: Option<PathBuf>,
    /// The file-view toolbar's buttons and separators, in order
    /// (Settings > Toolbar). `None` = the default layout.
    #[serde(deserialize_with = "lenient")]
    pub toolbar: Option<Vec<crate::core::toolbar::ToolbarItem>>,
    /// Keyboard shortcuts changed from their defaults (Settings >
    /// Shortcuts) - see `core::keymap`.
    #[serde(deserialize_with = "lenient")]
    pub shortcuts: crate::core::keymap::ShortcutOverrides,
    /// The border around previews in the preview pane and Quick Look
    /// (Settings > Appearance > Preview Frame).
    #[serde(deserialize_with = "lenient")]
    pub preview_frame: PreviewFrame,
    /// The `>` arrows between address bar segments open a menu of that
    /// folder's subfolders (like Explorer); off = plain separators.
    pub breadcrumb_dropdowns: bool,
    /// The terminal pane (`gui::windows::terminal_panel`).
    #[serde(deserialize_with = "lenient")]
    pub terminal: TerminalPrefs,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalPrefs {
    /// Shell new terminals start with (`ShellProfile::id`); `None` = the
    /// first of PowerShell 7, Windows PowerShell, Command Prompt.
    pub default_shell: Option<String>,
    pub font_size: f32,
    /// Height of the pane in points.
    pub height: f32,
}

impl Default for TerminalPrefs {
    fn default() -> Self {
        Self { default_shell: None, font_size: 13.0, height: 260.0 }
    }
}

/// The rounded border drawn around every preview.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PreviewFrame {
    pub enabled: bool,
    /// Border width in points (0 = no line, the background still shows).
    pub thickness: f32,
    /// RGBA; `None` = the theme's border color.
    pub color: Option<[u8; 4]>,
    /// Corner radius in points.
    pub radius: f32,
}

impl Default for PreviewFrame {
    fn default() -> Self {
        Self { enabled: true, thickness: 1.0, color: None, radius: 8.0 }
    }
}

impl PreviewFrame {
    pub const MAX_THICKNESS: f32 = 8.0;
    pub const MAX_RADIUS: f32 = 32.0;

    pub fn thickness(&self) -> f32 {
        if self.enabled { self.thickness.clamp(0.0, Self::MAX_THICKNESS) } else { 0.0 }
    }

    pub fn radius(&self) -> f32 {
        if self.enabled { self.radius.clamp(0.0, Self::MAX_RADIUS) } else { 0.0 }
    }

    /// Space between the border and the content. A rounded corner curves
    /// in by `radius × (1 − 1/√2)` (≈ 0.29 × radius) at its middle, so at
    /// least that much keeps square content (an image, a list row) from
    /// poking out past the curve; never less than 4.
    pub fn padding(&self) -> f32 {
        if !self.enabled {
            return 0.0;
        }
        (self.radius() * (1.0 - std::f32::consts::FRAC_1_SQRT_2)).ceil().max(4.0)
    }

    /// Corner radius for rounded things inside the frame (images, the
    /// archive header), so both curves share a center and the gap stays
    /// even: radius − border − padding, never below 0.
    pub fn inner_radius(&self) -> f32 {
        (self.radius() - self.thickness() - self.padding()).max(0.0)
    }
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            remember_folder_views: true,
            persist_folder_sizes: true,
            hover_previews: true,
            quick_look: true,
            browse_archives: true,
            queue_transfers: true,
            verify_copies: false,
            address_bar_own_row: true,
            disk_usage_ask_admin: true,
            templates_folder: None,
            toolbar: None,
            shortcuts: Default::default(),
            preview_frame: PreviewFrame::default(),
            breadcrumb_dropdowns: true,
            terminal: TerminalPrefs::default(),
        }
    }
}

/// Reads a field that may hold values this version doesn't understand (a
/// button or key from a newer version, a hand-edit): anything unreadable
/// falls back to the default instead of discarding the whole file.
fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

fn prefs_path() -> Option<PathBuf> {
    Some(crate::core::app_data::data_dir()?.join("ui_prefs.json"))
}

pub fn load_ui_prefs() -> UiPrefs {
    prefs_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_ui_prefs(prefs: &UiPrefs) {
    let Some(path) = prefs_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(prefs) {
        let _ = std::fs::write(path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_frame_corner_math() {
        let frame = PreviewFrame { enabled: true, thickness: 1.0, color: None, radius: 8.0 };
        assert_eq!(frame.padding(), 4.0); // 8 × 0.29 = 2.3, at least 4
        assert_eq!(frame.inner_radius(), 3.0); // 8 − 1 − 4
        let big = PreviewFrame { radius: 24.0, thickness: 2.0, ..frame };
        assert_eq!(big.padding(), 8.0); // ceil(24 × 0.29) = 8
        assert_eq!(big.inner_radius(), 14.0);
        let off = PreviewFrame { enabled: false, ..big };
        assert_eq!((off.thickness(), off.radius(), off.padding(), off.inner_radius()), (0.0, 0.0, 0.0, 0.0));
        let wild = PreviewFrame { thickness: 99.0, radius: -3.0, ..frame };
        assert_eq!((wild.thickness(), wild.radius(), wild.inner_radius()), (8.0, 0.0, 0.0));
    }

    #[test]
    fn old_files_get_the_default_preview_frame() {
        let prefs: UiPrefs = serde_json::from_str(r#"{"quick_look": false}"#).unwrap();
        assert_eq!(prefs.preview_frame, PreviewFrame::default());
        let prefs: UiPrefs = serde_json::from_str(r#"{"preview_frame": {"radius": 12}}"#).unwrap();
        assert_eq!(prefs.preview_frame.radius, 12.0);
        assert!(prefs.preview_frame.enabled);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let prefs: UiPrefs = serde_json::from_str(r#"{"hover_previews": false}"#).unwrap();
        assert!(!prefs.hover_previews);
        assert!(prefs.remember_folder_views);
        assert!(prefs.persist_folder_sizes);
    }

    #[test]
    fn one_unreadable_field_does_not_lose_the_others() {
        let prefs: UiPrefs = serde_json::from_str(
            r#"{"hover_previews": false, "toolbar": ["Back", "FutureButton"], "shortcuts": {"Nope": 1}}"#,
        )
        .unwrap();
        assert!(!prefs.hover_previews);
        assert_eq!(prefs.toolbar, None);
        assert!(prefs.shortcuts.is_empty());
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let prefs: UiPrefs = serde_json::from_str(r#"{"from_the_future": 1}"#).unwrap();
        assert_eq!(prefs, UiPrefs::default());
    }
}
