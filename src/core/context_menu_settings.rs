//! User-defined custom context menu entries: commands the user configures in
//! Settings that then show up in the right-click menu for files, folders,
//! and/or the folder background - similar in spirit to Files Community's
//! custom context menu feature.
//!
//! Persisted separately from the rest of `AppSettings` (its own file, own
//! load/save functions), the same way favorites/tags/session-tabs are - this
//! keeps it out of the large positional tuple `core::indexer::load_app_settings`
//! uses for the rest of the settings, which would otherwise need updating at
//! every call site for a feature that's logically independent.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::HSTRING;

/// How a custom entry's icon is drawn in the menu.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CustomContextMenuIcon {
    /// No icon - just the label.
    None,
    /// A Phosphor icon glyph (one of the curated set offered in the picker).
    Glyph(String),
    /// Extract the real shell icon of this file (typically the same `.exe`
    /// the command runs), the same mechanism used for file/folder icons
    /// elsewhere in the app.
    FileIcon(PathBuf),
}

impl Default for CustomContextMenuIcon {
    fn default() -> Self {
        Self::None
    }
}

/// One custom context menu entry - either a leaf command (`is_submenu ==
/// false`, runs `executable` with `arguments` when clicked) or a submenu
/// group (`is_submenu == true`, `children` are its leaf entries; a submenu's
/// own `executable`/`arguments`/run options are unused).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct CustomContextMenuEntry {
    pub id: u64,
    pub label: String,
    #[serde(default)]
    pub icon: CustomContextMenuIcon,
    pub applies_to_files: bool,
    pub applies_to_folders: bool,
    pub applies_to_background: bool,
    #[serde(default)]
    pub is_submenu: bool,
    #[serde(default)]
    pub children: Vec<CustomContextMenuEntry>,
    /// The program to run. Supports `%1` (first selected path), `%V` (an
    /// alias for `%1`, matching the classic Windows shell verb token used
    /// for a working-directory-ish argument), and `%*` (every selected path,
    /// each individually quoted) inside `arguments`.
    #[serde(default)]
    pub executable: String,
    #[serde(default)]
    pub arguments: String,
    #[serde(default)]
    pub run_as_admin: bool,
    /// When multiple items are selected: run the command once per item
    /// (substituting `%1`/`%V` with each path in turn) instead of once with
    /// `%*` expanded to the whole selection.
    #[serde(default)]
    pub run_once_per_selection: bool,
}

impl CustomContextMenuEntry {
    pub fn new_leaf(id: u64) -> Self {
        Self {
            id,
            label: String::new(),
            applies_to_files: true,
            applies_to_folders: true,
            ..Default::default()
        }
    }

    pub fn new_submenu(id: u64) -> Self {
        Self {
            id,
            label: String::new(),
            applies_to_files: true,
            applies_to_folders: true,
            is_submenu: true,
            ..Default::default()
        }
    }

    /// Whether this entry should be shown for a right-click where the
    /// selection contains at least one file (`has_file`) and/or at least one
    /// folder (`has_folder`) - or, for the background menu, neither.
    pub fn applies_to(&self, has_file: bool, has_folder: bool, is_background: bool) -> bool {
        if is_background {
            return self.applies_to_background;
        }
        (has_file && self.applies_to_files) || (has_folder && self.applies_to_folders)
    }
}

#[derive(Serialize, Deserialize, Default)]
struct CustomContextMenuSnapshot {
    #[serde(default)]
    entries: Vec<CustomContextMenuEntry>,
}

/// A standalone, human-readable export of *just* the custom context menu -
/// distinct from `core::indexer::SettingsExportBundle`, which already
/// carries this same list as part of a full settings export (it clones
/// `AppSettings` wholesale, and `custom_context_menu` lives on that struct).
/// This one lets a user share/back up only their custom commands, e.g. to
/// hand a coworker a single "right-click tools" file without also exporting
/// every other app setting.
#[derive(Serialize, Deserialize)]
pub struct ContextMenuExportBundle {
    pub format_version: u32,
    pub entries: Vec<CustomContextMenuEntry>,
}

pub const CONTEXT_MENU_EXPORT_FORMAT_VERSION: u32 = 1;

fn cache_path() -> Option<PathBuf> {
    let base = dirs::data_local_dir()?;
    Some(base.join("ExplorerEden").join("context_menu.bin"))
}

pub fn load_custom_context_menu() -> Vec<CustomContextMenuEntry> {
    let Some(path) = cache_path() else {
        return Vec::new();
    };
    let Ok(data) = std::fs::read(&path) else {
        return Vec::new();
    };
    postcard::from_bytes::<CustomContextMenuSnapshot>(&data)
        .map(|s| s.entries)
        .unwrap_or_default()
}

pub fn save_custom_context_menu(entries: &[CustomContextMenuEntry]) {
    let Some(path) = cache_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let snapshot = CustomContextMenuSnapshot {
        entries: entries.to_vec(),
    };
    if let Ok(data) = postcard::to_allocvec(&snapshot) {
        let _ = std::fs::write(path, data);
    }
}

/// A fresh id, guaranteed higher than every id currently in use (including
/// inside submenus), for a newly-added entry.
pub fn next_entry_id(entries: &[CustomContextMenuEntry]) -> u64 {
    fn max_id(entries: &[CustomContextMenuEntry]) -> u64 {
        entries
            .iter()
            .map(|e| e.id.max(max_id(&e.children)))
            .max()
            .unwrap_or(0)
    }
    max_id(entries) + 1
}

fn quote(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

/// Substitutes the classic shell verb tokens `%1`/`%V` (the "primary" path -
/// for a single invocation this is the first selected item; for a
/// once-per-item invocation it's that item, both quoted) and `%*` (every
/// selected path, individually quoted) into an arguments template. `%L`
/// mirrors the real Windows shell verb convention (the registry's own
/// distinction between `%1`, always quoted, and `%L`, the same "long" path
/// with no quotes added) - it exists specifically for commands that want to
/// do their own quoting, or none at all (e.g. `echo %L| clip` to copy a bare
/// path with no surrounding quotes in the result).
fn substitute(template: &str, primary: &Path, all: &[PathBuf]) -> String {
    let all_quoted = all.iter().map(|p| quote(p)).collect::<Vec<_>>().join(" ");
    template
        .replace("%1", &quote(primary))
        .replace("%V", &quote(primary))
        .replace("%L", &primary.display().to_string())
        .replace("%*", &all_quoted)
}

fn launch(executable: &str, arguments: &str, run_as_admin: bool) {
    let verb = if run_as_admin { "runas" } else { "open" };
    unsafe {
        ShellExecuteW(
            None,
            &HSTRING::from(verb),
            &HSTRING::from(executable),
            &HSTRING::from(arguments),
            None,
            SW_SHOWNORMAL,
        );
    }
}

/// Runs a leaf entry's command against `selected_paths` (the file/folder
/// selection it was invoked on) - or, for the background menu, against
/// `context_dir` (the folder being browsed) when `selected_paths` is empty.
pub fn run(entry: &CustomContextMenuEntry, selected_paths: &[PathBuf], context_dir: &Path) {
    if entry.is_submenu || entry.executable.trim().is_empty() {
        return;
    }

    let targets: Vec<PathBuf> = if selected_paths.is_empty() {
        vec![context_dir.to_path_buf()]
    } else {
        selected_paths.to_vec()
    };

    if entry.run_once_per_selection {
        for path in &targets {
            let args = substitute(&entry.arguments, path, std::slice::from_ref(path));
            launch(&entry.executable, &args, entry.run_as_admin);
        }
    } else {
        let primary = targets
            .first()
            .cloned()
            .unwrap_or_else(|| context_dir.to_path_buf());
        let args = substitute(&entry.arguments, &primary, &targets);
        launch(&entry.executable, &args, entry.run_as_admin);
    }
}

#[cfg(test)]
mod substitute_tests {
    use super::*;

    #[test]
    fn percent_1_and_percent_v_are_quoted_but_percent_l_is_not() {
        let primary = PathBuf::from(r"C:\Some Folder\file.txt");
        let all = [primary.clone()];

        assert_eq!(
            substitute("%1", &primary, &all),
            r#""C:\Some Folder\file.txt""#
        );
        assert_eq!(
            substitute("%V", &primary, &all),
            r#""C:\Some Folder\file.txt""#
        );
        assert_eq!(
            substitute("echo %L| clip", &primary, &all),
            r"echo C:\Some Folder\file.txt| clip"
        );
    }

    #[test]
    fn percent_star_quotes_every_selected_path_individually() {
        let primary = PathBuf::from(r"C:\a.txt");
        let all = [PathBuf::from(r"C:\a.txt"), PathBuf::from(r"C:\b.txt")];

        assert_eq!(substitute("%*", &primary, &all), r#""C:\a.txt" "C:\b.txt""#);
    }

    /// Exercises the exact serialize/deserialize path the Export/Import
    /// Custom Context Menu buttons use (`serde_json` round-trip through
    /// `ContextMenuExportBundle`), independent of the file-dialog/disk-write
    /// plumbing around it - a deterministic check for the part that's
    /// actually new, since the dialog + `std::fs::write`/`read_to_string`
    /// calls are the same already-proven pattern `ExportSettings`/
    /// `ImportSettings` use elsewhere.
    #[test]
    fn context_menu_export_bundle_round_trips_through_json() {
        let mut entry = CustomContextMenuEntry::new_leaf(1);
        entry.label = "Edit With Notepad".to_string();
        entry.executable = "notepad.exe".to_string();
        entry.arguments = "%L".to_string();

        let mut submenu = CustomContextMenuEntry::new_submenu(2);
        submenu.label = "Tools".to_string();
        submenu.children.push({
            let mut child = CustomContextMenuEntry::new_leaf(3);
            child.label = "Child Command".to_string();
            child
        });

        let bundle = ContextMenuExportBundle {
            format_version: CONTEXT_MENU_EXPORT_FORMAT_VERSION,
            entries: vec![entry, submenu],
        };

        let json = serde_json::to_string_pretty(&bundle).expect("serialize");
        let round_tripped: ContextMenuExportBundle =
            serde_json::from_str(&json).expect("deserialize");

        assert_eq!(round_tripped.format_version, CONTEXT_MENU_EXPORT_FORMAT_VERSION);
        assert_eq!(round_tripped.entries.len(), 2);
        assert_eq!(round_tripped.entries[0].label, "Edit With Notepad");
        assert_eq!(round_tripped.entries[0].executable, "notepad.exe");
        assert_eq!(round_tripped.entries[0].arguments, "%L");
        assert_eq!(round_tripped.entries[1].children.len(), 1);
        assert_eq!(round_tripped.entries[1].children[0].label, "Child Command");
    }
}
