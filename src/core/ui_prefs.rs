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
    /// Show a checkbox on every row/tile; clicking it adds or removes that
    /// item from the selection without Ctrl.
    pub checkbox_selection: bool,
    /// Extra folder listed under New (besides the built-in templates) whose
    /// files are offered as templates. `None` = the default
    /// `Templates` folder inside the data folder.
    pub templates_folder: Option<PathBuf>,
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            remember_folder_views: true,
            persist_folder_sizes: true,
            hover_previews: true,
            checkbox_selection: false,
            templates_folder: None,
        }
    }
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
        assert!(!prefs.checkbox_selection);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let prefs: UiPrefs = serde_json::from_str(r#"{"from_the_future": 1}"#).unwrap();
        assert_eq!(prefs, UiPrefs::default());
    }
}
