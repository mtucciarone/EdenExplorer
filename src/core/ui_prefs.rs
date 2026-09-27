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
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            remember_folder_views: true,
            persist_folder_sizes: true,
            hover_previews: true,
            address_bar_own_row: true,
            disk_usage_ask_admin: true,
            templates_folder: None,
            toolbar: None,
            shortcuts: Default::default(),
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
