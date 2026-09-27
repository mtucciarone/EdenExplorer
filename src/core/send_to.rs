//! User-defined "Send To" groups: a named parent (with its own icon) holding
//! an ordered list of destination folders. Selecting a file/folder and
//! choosing one of these destinations from the right-click "Send To" submenu
//! **copies** the selection there - never moves it, so the original always
//! stays put no matter which destination is picked.
//!
//! Persisted separately from the rest of `AppSettings` (its own file, own
//! load/save functions), the same way favorites/tags/tab-groups/custom-
//! context-menu entries already are. The "show Send To in the context menu"
//! toggle lives in this same file rather than as a loose `AppSettings`
//! field, since it's meaningless without the groups it controls.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How a Send To group's icon is drawn - mirrors `FavoriteItem`'s own
/// `custom_icon`/`custom_icon_file` pair (kept as one enum here instead of
/// two `Option` fields since a group's icon has no third "use the real
/// shell icon" state the way a favorite's folder does).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SendToIcon {
    /// No icon - just the label.
    None,
    /// A Phosphor icon glyph, picked from the shared searchable icon picker.
    Glyph(String),
    /// A user-browsed image file (.ico, .png, .jpg, ...), copied into the
    /// app's own data folder via `core::indexer::import_custom_icon` so it
    /// keeps working (and Export/Import Settings carries it along) even if
    /// the original file is moved or deleted.
    Custom(PathBuf),
}

impl Default for SendToIcon {
    fn default() -> Self {
        Self::None
    }
}

/// Whether a Send To group copies (originals untouched) or moves (originals
/// removed after a successful transfer) the selection into its folders.
/// Determines which of the context menu's "Copy"/"Move" branches the group
/// appears under - see `gui::windows::containers::itemviewer_helper`'s
/// `draw_send_to_menu`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SendToMode {
    #[default]
    Copy,
    Move,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct SendToGroup {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub icon: SendToIcon,
    /// Duplicates are allowed on purpose, same as everywhere else in this
    /// app that lets a user build a list of paths (Tab Groups, Favorites).
    #[serde(default)]
    pub folders: Vec<PathBuf>,
    #[serde(default)]
    pub mode: SendToMode,
}

impl SendToGroup {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            name: String::new(),
            icon: SendToIcon::None,
            folders: Vec::new(),
            mode: SendToMode::Copy,
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
struct SendToSnapshot {
    #[serde(default)]
    groups: Vec<SendToGroup>,
    /// Whether the "Send To" submenu shows up in the file/folder right-click
    /// menu. Meaningless with zero groups - `save_send_to` doesn't enforce
    /// that itself (the settings page does, disabling the checkbox and
    /// resetting it to `false` when the last group is removed), but this
    /// stays a plain independent field rather than something derived, so a
    /// user who removes every group and re-adds one later doesn't have to
    /// re-enable it.
    #[serde(default)]
    context_menu_enabled: bool,
}

/// Pre-`mode`-field shape of `SendToGroup`/`SendToSnapshot`, kept purely as
/// a decode fallback for a `send_to.bin` saved before the Copy/Move feature
/// existed. Postcard's binary format has no field names and no per-field
/// "missing, use the default" rescue for a struct nested inside a `Vec` the
/// way `#[serde(default)]` implies (see this codebase's own extensively
/// documented finding on this) - decoding a group with the *current* shape
/// against old bytes that don't have a trailing `mode` byte fails the whole
/// buffer, and without this fallback `load_send_to` would silently return
/// an empty list, discarding every one of the user's real saved groups.
/// Mirrors `CustomThemeEntryLegacy`'s own established pattern for the exact
/// same problem. Re-saved in the current shape automatically the next time
/// the user changes anything about their groups.
#[derive(Serialize, Deserialize, Default)]
struct SendToGroupLegacy {
    id: u64,
    name: String,
    #[serde(default)]
    icon: SendToIcon,
    #[serde(default)]
    folders: Vec<PathBuf>,
}

#[derive(Serialize, Deserialize, Default)]
struct SendToSnapshotLegacy {
    #[serde(default)]
    groups: Vec<SendToGroupLegacy>,
    #[serde(default)]
    context_menu_enabled: bool,
}

fn cache_path() -> Option<PathBuf> {
    let base = crate::core::app_data::data_dir()?;
    Some(base.join("send_to.bin"))
}

/// Returns `(groups, context_menu_enabled)`. Tries the current shape first;
/// falls back to `SendToSnapshotLegacy` (pre-`mode` field) so an existing
/// user's real saved groups survive this feature's own addition - see
/// `SendToGroupLegacy`'s doc comment.
pub fn load_send_to() -> (Vec<SendToGroup>, bool) {
    let Some(path) = cache_path() else {
        return (Vec::new(), false);
    };
    let Ok(data) = std::fs::read(&path) else {
        return (Vec::new(), false);
    };
    if let Ok(snapshot) = postcard::from_bytes::<SendToSnapshot>(&data) {
        return (snapshot.groups, snapshot.context_menu_enabled);
    }
    match postcard::from_bytes::<SendToSnapshotLegacy>(&data) {
        Ok(legacy) => {
            let groups = legacy
                .groups
                .into_iter()
                .map(|g| SendToGroup {
                    id: g.id,
                    name: g.name,
                    icon: g.icon,
                    folders: g.folders,
                    mode: SendToMode::Copy,
                })
                .collect();
            (groups, legacy.context_menu_enabled)
        }
        Err(_) => (Vec::new(), false),
    }
}

pub fn save_send_to(groups: &[SendToGroup], context_menu_enabled: bool) {
    let Some(path) = cache_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let snapshot = SendToSnapshot {
        groups: groups.to_vec(),
        context_menu_enabled,
    };
    if let Ok(data) = postcard::to_allocvec(&snapshot) {
        let _ = std::fs::write(path, data);
    }
}

/// A standalone, human-readable export of *just* the Send To groups -
/// distinct from `core::indexer::SettingsExportBundle`, which already
/// carries this same list as part of a full settings export (it clones
/// `AppSettings` wholesale, and `send_to`/`send_to_context_menu_enabled`
/// live on that struct). This one lets a user share/back up only their Send
/// To destinations, e.g. to hand a coworker a single "shared drives" file
/// without also exporting every other app setting.
#[derive(Serialize, Deserialize)]
pub struct SendToExportBundle {
    pub format_version: u32,
    pub groups: Vec<SendToGroup>,
    pub context_menu_enabled: bool,
}

pub const SEND_TO_EXPORT_FORMAT_VERSION: u32 = 1;

/// A fresh id, guaranteed higher than every group id currently in use, for a
/// newly-added group.
pub fn next_group_id(groups: &[SendToGroup]) -> u64 {
    groups.iter().map(|g| g.id).max().unwrap_or(0) + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_to_snapshot_round_trips_through_postcard() {
        let groups = vec![SendToGroup {
            id: 1,
            name: "Backups".to_string(),
            icon: SendToIcon::Glyph("\u{E000}".to_string()),
            folders: vec![PathBuf::from("D:\\Backup"), PathBuf::from("E:\\Archive")],
            mode: SendToMode::Move,
        }];
        let bytes = postcard::to_allocvec(&SendToSnapshot {
            groups: groups.clone(),
            context_menu_enabled: true,
        })
        .unwrap();
        let decoded = postcard::from_bytes::<SendToSnapshot>(&bytes).unwrap();
        assert_eq!(decoded.groups, groups);
        assert!(decoded.context_menu_enabled);
    }

    /// A `send_to.bin` saved before the Copy/Move feature existed (no
    /// `mode` field on `SendToGroup`) must still decode, defaulting every
    /// existing group to `SendToMode::Copy` - not silently return an empty
    /// list, which would discard a real user's saved groups the moment
    /// this field was added. See `SendToGroupLegacy`'s doc comment.
    #[test]
    fn legacy_send_to_bytes_without_mode_field_still_decode() {
        let legacy_groups = vec![SendToGroupLegacy {
            id: 5,
            name: "Test".to_string(),
            icon: SendToIcon::None,
            folders: vec![PathBuf::from("D:\\dest1"), PathBuf::from("D:\\dest2")],
        }];
        let bytes = postcard::to_allocvec(&SendToSnapshotLegacy {
            groups: legacy_groups,
            context_menu_enabled: true,
        })
        .unwrap();

        // The new shape must genuinely fail against old bytes for this test
        // to actually prove anything about the fallback path.
        assert!(postcard::from_bytes::<SendToSnapshot>(&bytes).is_err());

        let legacy = postcard::from_bytes::<SendToSnapshotLegacy>(&bytes).unwrap();
        assert_eq!(legacy.groups.len(), 1);
        assert_eq!(legacy.groups[0].name, "Test");
        assert!(legacy.context_menu_enabled);
    }

    #[test]
    fn next_group_id_is_higher_than_every_existing_id() {
        let groups = vec![SendToGroup::new(3), SendToGroup::new(1), SendToGroup::new(7)];
        assert_eq!(next_group_id(&groups), 8);
        assert_eq!(next_group_id(&[]), 1);
    }

    /// Exercises the exact serialize/deserialize path the Export/Import Send
    /// To buttons use (`serde_json` round-trip through `SendToExportBundle`),
    /// same rationale as
    /// `context_menu_export_bundle_round_trips_through_json` in
    /// `core::context_menu_settings`.
    #[test]
    fn send_to_export_bundle_round_trips_through_json() {
        let bundle = SendToExportBundle {
            format_version: SEND_TO_EXPORT_FORMAT_VERSION,
            groups: vec![SendToGroup {
                id: 1,
                name: "Backups".to_string(),
                icon: SendToIcon::None,
                folders: vec![PathBuf::from("D:\\Backup")],
                mode: SendToMode::Move,
            }],
            context_menu_enabled: true,
        };

        let json = serde_json::to_string_pretty(&bundle).expect("serialize");
        let round_tripped: SendToExportBundle = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(round_tripped.format_version, SEND_TO_EXPORT_FORMAT_VERSION);
        assert_eq!(round_tripped.groups.len(), 1);
        assert_eq!(round_tripped.groups[0].name, "Backups");
        assert_eq!(round_tripped.groups[0].mode, SendToMode::Move);
        assert!(round_tripped.context_menu_enabled);
    }
}
