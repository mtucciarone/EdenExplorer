//! User-defined "tab groups": a named, ordered list of folder paths the user
//! can open all at once as tabs - either added alongside the current tabs or
//! replacing them entirely. The same path may appear more than once in a
//! group (each occurrence opens as its own separate tab, not deduplicated).
//!
//! Persisted separately from the rest of `AppSettings` (its own file, own
//! load/save functions), the same way favorites/tags/custom-context-menu
//! entries are.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct TabGroup {
    pub id: u64,
    pub name: String,
    /// Duplicates are allowed and meaningful: opening the group opens one
    /// tab per entry, even if the same path appears twice.
    #[serde(default)]
    pub paths: Vec<PathBuf>,
}

impl TabGroup {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            name: String::new(),
            paths: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
struct TabGroupsSnapshot {
    #[serde(default)]
    groups: Vec<TabGroup>,
}

fn cache_path() -> Option<PathBuf> {
    let base = dirs::data_local_dir()?;
    Some(base.join("ExplorerEden").join("tab_groups.bin"))
}

pub fn load_tab_groups() -> Vec<TabGroup> {
    let Some(path) = cache_path() else {
        return Vec::new();
    };
    let Ok(data) = std::fs::read(&path) else {
        return Vec::new();
    };
    postcard::from_bytes::<TabGroupsSnapshot>(&data)
        .map(|s| s.groups)
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
