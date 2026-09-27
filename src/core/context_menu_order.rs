//! User-configurable ordering of the file/folder right-click context menu's
//! sections (Send To, Custom Context Menu, the Cut/Copy/Paste block, etc.),
//! plus freely add/removable separator lines between them - lets a user put,
//! say, Send To at the very top, or drop a separator between Rename and
//! Delete without touching anything else. Doesn't control *whether* a
//! section shows (each section already has its own enable condition - e.g.
//! `SendTo` only appears if `send_to_context_menu_enabled` is set and at
//! least one group exists, same as before this feature existed), only the
//! *order* they're tried in and where separators sit among them.
//!
//! Persisted separately from the rest of `AppSettings` (its own file, own
//! load/save functions), the same way favorites/tags/tab-groups/send-to/
//! custom-context-menu entries already are.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// One entry in the configured context-menu order: either a fixed section
/// (every one of these appears in `default_order()` and stays present
/// forever - a user can move it around but not delete it) or `Separator`,
/// which is the only variant a user can freely add/remove/duplicate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContextMenuSection {
    AddFavorite,
    Tags,
    OpenDefaultProgram,
    Compress,
    SendTo,
    CustomContextMenu,
    FileOperations,
    Rename,
    Delete,
    CreateShortcut,
    Checksum,
    Properties,
    WindowsMenu,
    /// A plain divider line. Unlike every other variant, more than one of
    /// these can appear, and the user can add/remove them freely - see
    /// `ContextMenuSection::is_removable`.
    Separator,
}

impl ContextMenuSection {
    /// Every fixed (non-`Separator`) section, in this app's original
    /// hardcoded order - used both as `default_order()`'s backbone and to
    /// detect/repair a saved order that's missing one (see
    /// `normalize_order`).
    const FIXED_SECTIONS: &'static [ContextMenuSection] = &[
        ContextMenuSection::AddFavorite,
        ContextMenuSection::Tags,
        ContextMenuSection::OpenDefaultProgram,
        ContextMenuSection::SendTo,
        ContextMenuSection::Compress,
        ContextMenuSection::CustomContextMenu,
        ContextMenuSection::FileOperations,
        ContextMenuSection::Rename,
        ContextMenuSection::Delete,
        ContextMenuSection::CreateShortcut,
        ContextMenuSection::Checksum,
        ContextMenuSection::Properties,
        ContextMenuSection::WindowsMenu,
    ];

    pub fn is_separator(self) -> bool {
        matches!(self, ContextMenuSection::Separator)
    }

    pub fn i18n_key(self) -> &'static str {
        match self {
            ContextMenuSection::AddFavorite => "context_menu_order_add_favorite",
            ContextMenuSection::Tags => "context_menu_order_tags",
            ContextMenuSection::OpenDefaultProgram => "context_menu_order_open_default_program",
            ContextMenuSection::Compress => "context_menu_order_compress",
            ContextMenuSection::SendTo => "context_menu_order_send_to",
            ContextMenuSection::CustomContextMenu => "context_menu_order_custom_context_menu",
            ContextMenuSection::FileOperations => "context_menu_order_file_operations",
            ContextMenuSection::Rename => "context_menu_order_rename",
            ContextMenuSection::Delete => "context_menu_order_delete",
            ContextMenuSection::CreateShortcut => "context_menu_order_create_shortcut",
            ContextMenuSection::Checksum => "context_menu_order_checksum",
            ContextMenuSection::Properties => "context_menu_order_properties",
            ContextMenuSection::WindowsMenu => "context_menu_order_windows_menu",
            ContextMenuSection::Separator => "context_menu_order_separator",
        }
    }
}

/// This app's original, hardcoded menu order (before this feature existed),
/// expressed as data - the default for every user who hasn't customized
/// anything, so nothing visually changes until they actually open this
/// settings page and rearrange something.
pub fn default_order() -> Vec<ContextMenuSection> {
    use ContextMenuSection::*;
    vec![
        AddFavorite,
        Tags,
        OpenDefaultProgram,
        Separator,
        SendTo,
        Separator,
        Compress,
        CustomContextMenu,
        Separator,
        FileOperations,
        Rename,
        Delete,
        Separator,
        CreateShortcut,
        Checksum,
        Properties,
        Separator,
        WindowsMenu,
    ]
}

/// Repairs a loaded order against `FIXED_SECTIONS`: appends any fixed
/// section that's missing (e.g. a future build adds a new section type a
/// saved file predates - the same "an old save can't reference a variant
/// that didn't exist yet" gap `postcard`'s whole-struct decode already
/// forces this module to guard against everywhere else) and drops any
/// duplicate of a fixed section beyond its first occurrence (defensive only
/// - nothing in this module's own UI can produce one, but an oddly hand-
/// edited exported file could). `Separator` entries are left exactly as the
/// user arranged them, including duplicates - those are meaningful, not a
/// corruption to fix.
fn normalize_order(mut order: Vec<ContextMenuSection>) -> Vec<ContextMenuSection> {
    let mut seen = std::collections::HashSet::new();
    order.retain(|section| section.is_separator() || seen.insert(*section));

    for &section in ContextMenuSection::FIXED_SECTIONS {
        if !order.contains(&section) {
            order.push(section);
        }
    }

    order
}

#[derive(Serialize, Deserialize)]
struct ContextMenuOrderSnapshot {
    #[serde(default = "default_order")]
    order: Vec<ContextMenuSection>,
}

impl Default for ContextMenuOrderSnapshot {
    fn default() -> Self {
        Self {
            order: default_order(),
        }
    }
}

fn cache_path() -> Option<PathBuf> {
    let base = crate::core::app_data::data_dir()?;
    Some(base.join("context_menu_order.bin"))
}

pub fn load_context_menu_order() -> Vec<ContextMenuSection> {
    let Some(path) = cache_path() else {
        return default_order();
    };
    let Ok(data) = std::fs::read(&path) else {
        return default_order();
    };
    let order = postcard::take_from_bytes::<ContextMenuOrderSnapshot>(&data)
        .ok()
        .filter(|(_, rest)| rest.is_empty())
        .map(|(v, _)| v.order)
        .unwrap_or_else(default_order);
    normalize_order(order)
}

pub fn save_context_menu_order(order: &[ContextMenuSection]) {
    let Some(path) = cache_path() else {
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    let _ = std::fs::create_dir_all(parent);
    let snapshot = ContextMenuOrderSnapshot {
        order: order.to_vec(),
    };
    if let Ok(data) = postcard::to_allocvec(&snapshot) {
        let _ = std::fs::write(path, data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_order_contains_every_fixed_section_exactly_once() {
        let order = default_order();
        for &section in ContextMenuSection::FIXED_SECTIONS {
            assert_eq!(
                order.iter().filter(|s| **s == section).count(),
                1,
                "{section:?} should appear exactly once in the default order"
            );
        }
    }

    #[test]
    fn context_menu_order_snapshot_round_trips_through_postcard() {
        let order = vec![
            ContextMenuSection::SendTo,
            ContextMenuSection::Separator,
            ContextMenuSection::AddFavorite,
        ];
        let bytes = postcard::to_allocvec(&ContextMenuOrderSnapshot {
            order: order.clone(),
        })
        .unwrap();
        let decoded = postcard::take_from_bytes::<ContextMenuOrderSnapshot>(&bytes).unwrap();
        assert_eq!(decoded.0.order, order);
        assert!(decoded.1.is_empty());
    }

    #[test]
    fn normalize_order_appends_a_missing_fixed_section() {
        let mut order = default_order();
        order.retain(|s| *s != ContextMenuSection::Checksum);
        assert!(!order.contains(&ContextMenuSection::Checksum));

        let normalized = normalize_order(order);
        assert!(normalized.contains(&ContextMenuSection::Checksum));
        assert_eq!(
            normalized
                .iter()
                .filter(|s| **s == ContextMenuSection::Checksum)
                .count(),
            1
        );
    }

    #[test]
    fn normalize_order_drops_a_duplicated_fixed_section_but_keeps_duplicate_separators() {
        let mut order = default_order();
        order.push(ContextMenuSection::Checksum);
        order.push(ContextMenuSection::Separator);

        let normalized = normalize_order(order);
        assert_eq!(
            normalized
                .iter()
                .filter(|s| **s == ContextMenuSection::Checksum)
                .count(),
            1
        );
        assert_eq!(
            normalized
                .iter()
                .filter(|s| **s == ContextMenuSection::Separator)
                .count(),
            default_order()
                .iter()
                .filter(|s| **s == ContextMenuSection::Separator)
                .count()
                + 1
        );
    }
}
