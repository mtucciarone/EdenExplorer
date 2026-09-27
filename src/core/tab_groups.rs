//! User-defined "tab groups": a named, ordered list of folder entries the
//! user can open all at once as tabs - either added alongside the current
//! tabs or replacing them entirely. The same path may appear more than once
//! in a group (each occurrence opens as its own separate tab, not
//! deduplicated).
//!
//! Each entry can also carry a `split_path` - when set, opening that entry
//! opens one tab with a Secondary split-view pane already showing that
//! second folder, mirroring how a real dual-pane tab looks today. This lets
//! a group remember "these two folders side by side" as a single unit,
//! not just a flat list of single-pane tabs.
//!
//! Persisted separately from the rest of `AppSettings` (its own file, own
//! load/save functions), the same way favorites/tags/custom-context-menu
//! entries are.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct TabGroupEntry {
    pub path: PathBuf,
    /// When set, opening this entry opens one tab with a Secondary
    /// split-view pane at this path too (a dual-pane tab), instead of a
    /// plain single-pane tab.
    #[serde(default)]
    pub split_path: Option<PathBuf>,
}

impl TabGroupEntry {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            split_path: None,
        }
    }
}

/// How a tab group's own parent icon (shown wherever the group itself is
/// listed - the "+" button's group menu, the Tab Groups settings list) is
/// drawn. Mirrors `SendToGroup`'s `SendToIcon`/`FavoriteItem`'s icon pair,
/// with one difference: `None` here doesn't mean "no icon" the way it does
/// for Send To - it means "follow the first folder's own real shell icon",
/// which is this feature's actual default look (see `resolve_tab_group_icon`
/// in `gui::windows::tab_groups_ui`) and stays that way automatically as the
/// group's first entry changes, unless the user explicitly picks something
/// else.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TabGroupIcon {
    /// Follow the group's first folder entry's own real shell icon.
    None,
    /// A Phosphor icon glyph, picked from the shared searchable icon picker.
    Glyph(String),
    /// A user-browsed image file (.ico, .png, .jpg, ...), copied into the
    /// app's own data folder via `core::indexer::import_custom_icon` so it
    /// keeps working (and Export/Import Settings carries it along) even if
    /// the original file is moved or deleted.
    Custom(PathBuf),
}

impl Default for TabGroupIcon {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct TabGroup {
    pub id: u64,
    pub name: String,
    /// Duplicates are allowed and meaningful: opening the group opens one
    /// tab per entry, even if the same path appears twice.
    #[serde(default)]
    pub entries: Vec<TabGroupEntry>,
    #[serde(default)]
    pub icon: TabGroupIcon,
}

impl TabGroup {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            name: String::new(),
            entries: Vec::new(),
            icon: TabGroupIcon::None,
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
struct TabGroupsSnapshot {
    #[serde(default)]
    groups: Vec<TabGroup>,
}

// --- Legacy shape, from after dual-pane entries existed but before this
// group-icon field did (`entries: Vec<TabGroupEntry>`, no `icon`) - kept as
// a decode fallback for the same reason `TabGroupsSnapshotLegacy` below is:
// postcard's whole-struct decode has no per-field rescue for a trailing
// field (see `CLAUDE.md`'s notes on this), so an existing user's saved tab
// groups would otherwise silently reset to empty the first time they launch
// a build with this field.
#[derive(Serialize, Deserialize)]
struct TabGroupLegacyV2 {
    id: u64,
    name: String,
    #[serde(default)]
    entries: Vec<TabGroupEntry>,
}

#[derive(Serialize, Deserialize, Default)]
struct TabGroupsSnapshotLegacyV2 {
    #[serde(default)]
    groups: Vec<TabGroupLegacyV2>,
}

impl From<TabGroupsSnapshotLegacyV2> for TabGroupsSnapshot {
    fn from(legacy: TabGroupsSnapshotLegacyV2) -> Self {
        Self {
            groups: legacy
                .groups
                .into_iter()
                .map(|g| TabGroup {
                    id: g.id,
                    name: g.name,
                    entries: g.entries,
                    icon: TabGroupIcon::None,
                })
                .collect(),
        }
    }
}

// --- Older legacy shape still, from before dual-pane entries existed
// (`paths: Vec<PathBuf>` instead of `entries: Vec<TabGroupEntry>`) - same
// reasoning as `TabGroupLegacyV2` above, one generation further back.
#[derive(Serialize, Deserialize)]
struct TabGroupLegacy {
    id: u64,
    name: String,
    #[serde(default)]
    paths: Vec<PathBuf>,
}

#[derive(Serialize, Deserialize, Default)]
struct TabGroupsSnapshotLegacy {
    #[serde(default)]
    groups: Vec<TabGroupLegacy>,
}

impl From<TabGroupsSnapshotLegacy> for TabGroupsSnapshot {
    fn from(legacy: TabGroupsSnapshotLegacy) -> Self {
        Self {
            groups: legacy
                .groups
                .into_iter()
                .map(|g| TabGroup {
                    id: g.id,
                    name: g.name,
                    entries: g.paths.into_iter().map(TabGroupEntry::new).collect(),
                    icon: TabGroupIcon::None,
                })
                .collect(),
        }
    }
}

fn cache_path() -> Option<PathBuf> {
    let base = crate::core::app_data::data_dir()?;
    Some(base.join("tab_groups.bin"))
}

pub fn load_tab_groups() -> Vec<TabGroup> {
    let Some(path) = cache_path() else {
        return Vec::new();
    };
    let Ok(data) = std::fs::read(&path) else {
        return Vec::new();
    };
    if let Ok(snapshot) = postcard::from_bytes::<TabGroupsSnapshot>(&data) {
        return snapshot.groups;
    }
    if let Ok(legacy) = postcard::from_bytes::<TabGroupsSnapshotLegacyV2>(&data) {
        return TabGroupsSnapshot::from(legacy).groups;
    }
    postcard::from_bytes::<TabGroupsSnapshotLegacy>(&data)
        .map(|legacy| TabGroupsSnapshot::from(legacy).groups)
        .unwrap_or_default()
}

pub fn save_tab_groups(groups: &[TabGroup]) {
    let Some(path) = cache_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let snapshot = TabGroupsSnapshot {
        groups: groups.to_vec(),
    };
    if let Ok(data) = postcard::to_allocvec(&snapshot) {
        let _ = std::fs::write(path, data);
    }
}

/// A fresh id, guaranteed higher than every group id currently in use, for a
/// newly-added group.
pub fn next_group_id(groups: &[TabGroup]) -> u64 {
    groups.iter().map(|g| g.id).max().unwrap_or(0) + 1
}

/// A standalone, human-readable export of *just* the Tab Groups list -
/// distinct from `core::indexer::SettingsExportBundle`, which already
/// carries this same list as part of a full settings export (it clones
/// `AppSettings` wholesale, and `tab_groups` lives on that struct). This one
/// lets a user share/back up only their tab groups, e.g. to hand a coworker
/// a single "project folders" file without also exporting every other app
/// setting.
#[derive(Serialize, Deserialize)]
pub struct TabGroupsExportBundle {
    pub format_version: u32,
    pub groups: Vec<TabGroup>,
}

pub const TAB_GROUPS_EXPORT_FORMAT_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_tab_group_bytes_without_split_path_still_decode() {
        let legacy = TabGroupsSnapshotLegacy {
            groups: vec![TabGroupLegacy {
                id: 1,
                name: "Dev".to_string(),
                paths: vec![PathBuf::from("C:\\a"), PathBuf::from("C:\\b")],
            }],
        };
        let bytes = postcard::to_allocvec(&legacy).unwrap();

        // The new shape must fail to decode legacy bytes (otherwise the
        // fallback below would never be reached in practice).
        assert!(postcard::from_bytes::<TabGroupsSnapshot>(&bytes).is_err());

        let decoded_legacy = postcard::from_bytes::<TabGroupsSnapshotLegacy>(&bytes).unwrap();
        let migrated: TabGroupsSnapshot = decoded_legacy.into();
        assert_eq!(migrated.groups.len(), 1);
        assert_eq!(migrated.groups[0].name, "Dev");
        assert_eq!(
            migrated.groups[0].entries,
            vec![
                TabGroupEntry::new(PathBuf::from("C:\\a")),
                TabGroupEntry::new(PathBuf::from("C:\\b")),
            ]
        );
    }

    #[test]
    fn tab_group_with_split_path_round_trips_through_postcard() {
        let groups = vec![TabGroup {
            id: 1,
            name: "Dev".to_string(),
            entries: vec![TabGroupEntry {
                path: PathBuf::from("C:\\a"),
                split_path: Some(PathBuf::from("C:\\b")),
            }],
            icon: TabGroupIcon::Glyph("star".to_string()),
        }];
        let bytes = postcard::to_allocvec(&TabGroupsSnapshot {
            groups: groups.clone(),
        })
        .unwrap();
        let decoded = postcard::from_bytes::<TabGroupsSnapshot>(&bytes).unwrap();
        assert_eq!(decoded.groups, groups);
    }

    #[test]
    fn legacy_tab_group_bytes_without_icon_field_still_decode() {
        let legacy = TabGroupsSnapshotLegacyV2 {
            groups: vec![TabGroupLegacyV2 {
                id: 1,
                name: "Dev".to_string(),
                entries: vec![TabGroupEntry::new(PathBuf::from("C:\\a"))],
            }],
        };
        let bytes = postcard::to_allocvec(&legacy).unwrap();

        // The current shape must fail to decode legacy (pre-icon) bytes -
        // otherwise the fallback below would never be reached in practice.
        assert!(postcard::from_bytes::<TabGroupsSnapshot>(&bytes).is_err());

        let decoded_legacy = postcard::from_bytes::<TabGroupsSnapshotLegacyV2>(&bytes).unwrap();
        let migrated: TabGroupsSnapshot = decoded_legacy.into();
        assert_eq!(migrated.groups.len(), 1);
        assert_eq!(migrated.groups[0].name, "Dev");
        assert_eq!(migrated.groups[0].icon, TabGroupIcon::None);
    }

    /// Exercises the exact serialize/deserialize path the Export/Import Tab
    /// Groups buttons use (`serde_json` round-trip through
    /// `TabGroupsExportBundle`), same rationale as
    /// `context_menu_export_bundle_round_trips_through_json` in
    /// `core::context_menu_settings`.
    #[test]
    fn tab_groups_export_bundle_round_trips_through_json() {
        let bundle = TabGroupsExportBundle {
            format_version: TAB_GROUPS_EXPORT_FORMAT_VERSION,
            groups: vec![TabGroup {
                id: 1,
                name: "Dev".to_string(),
                entries: vec![TabGroupEntry::new(PathBuf::from("C:\\a"))],
                icon: TabGroupIcon::Custom(PathBuf::from("C:\\icon.png")),
            }],
        };

        let json = serde_json::to_string_pretty(&bundle).expect("serialize");
        let round_tripped: TabGroupsExportBundle =
            serde_json::from_str(&json).expect("deserialize");

        assert_eq!(round_tripped.format_version, TAB_GROUPS_EXPORT_FORMAT_VERSION);
        assert_eq!(round_tripped.groups.len(), 1);
        assert_eq!(round_tripped.groups[0].name, "Dev");
        assert_eq!(
            round_tripped.groups[0].icon,
            TabGroupIcon::Custom(PathBuf::from("C:\\icon.png"))
        );
    }
}
