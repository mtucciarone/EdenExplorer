//! The file-view toolbar's layout: which buttons it shows, in what order,
//! and where the separators go (Settings > Toolbar). Stored in
//! `UiPrefs::toolbar`; `None` there means the default layout.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolbarItem {
    Back,
    Forward,
    Up,
    Refresh,
    NewFolder,
    NewFile,
    Favorite,
    Search,
    ViewDetails,
    ViewGallery,
    ViewColumns,
    ViewColumnPreview,
    ViewPreview,
    ViewDetailPreview,
    Terminal,
    /// Shows or hides the docked terminal pane.
    TerminalPane,
    SelectAll,
    InvertSelection,
    SelectByPattern,
    PerformancePanel,
    Settings,
    /// A thin divider; may appear any number of times.
    Separator,
}

impl ToolbarItem {
    /// The layout the app has always had.
    pub const DEFAULT: [ToolbarItem; 18] = [
        ToolbarItem::Back,
        ToolbarItem::Forward,
        ToolbarItem::Up,
        ToolbarItem::Refresh,
        ToolbarItem::NewFolder,
        ToolbarItem::NewFile,
        ToolbarItem::Favorite,
        ToolbarItem::Search,
        ToolbarItem::Separator,
        ToolbarItem::ViewDetails,
        ToolbarItem::ViewGallery,
        ToolbarItem::ViewColumns,
        ToolbarItem::ViewColumnPreview,
        ToolbarItem::ViewPreview,
        ToolbarItem::ViewDetailPreview,
        ToolbarItem::Separator,
        ToolbarItem::Terminal,
        ToolbarItem::TerminalPane,
    ];

    /// Every button that can be placed (separators aside), in the order
    /// the Settings page lists them.
    pub const BUTTONS: [ToolbarItem; 21] = [
        ToolbarItem::Back,
        ToolbarItem::Forward,
        ToolbarItem::Up,
        ToolbarItem::Refresh,
        ToolbarItem::NewFolder,
        ToolbarItem::NewFile,
        ToolbarItem::Favorite,
        ToolbarItem::Search,
        ToolbarItem::ViewDetails,
        ToolbarItem::ViewGallery,
        ToolbarItem::ViewColumns,
        ToolbarItem::ViewColumnPreview,
        ToolbarItem::ViewPreview,
        ToolbarItem::ViewDetailPreview,
        ToolbarItem::Terminal,
        ToolbarItem::TerminalPane,
        ToolbarItem::SelectAll,
        ToolbarItem::InvertSelection,
        ToolbarItem::SelectByPattern,
        ToolbarItem::PerformancePanel,
        ToolbarItem::Settings,
    ];
}

/// The layout to draw: the saved one, or the default.
pub fn effective_layout(saved: Option<&[ToolbarItem]>) -> Vec<ToolbarItem> {
    match saved {
        Some(items) => items.to_vec(),
        None => ToolbarItem::DEFAULT.to_vec(),
    }
}

/// What the toolbar actually draws: the layout, tidied (see `normalize`).
pub fn drawn_layout(saved: Option<&[ToolbarItem]>) -> Vec<ToolbarItem> {
    normalize(&effective_layout(saved))
}

/// Buttons not currently in `layout` (each button appears at most once).
pub fn available_buttons(layout: &[ToolbarItem]) -> Vec<ToolbarItem> {
    ToolbarItem::BUTTONS
        .iter()
        .copied()
        .filter(|button| !layout.contains(button))
        .collect()
}

/// Drops duplicate buttons (keeping the first) and separators that would
/// render back-to-back or at either end, so any edit leaves a tidy toolbar.
pub fn normalize(layout: &[ToolbarItem]) -> Vec<ToolbarItem> {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<ToolbarItem> = Vec::with_capacity(layout.len());
    for &item in layout {
        if item == ToolbarItem::Separator {
            if out.last().is_some_and(|last| *last != ToolbarItem::Separator) {
                out.push(item);
            }
        } else if seen.insert(item) {
            out.push(item);
        }
    }
    while out.last() == Some(&ToolbarItem::Separator) {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_used_when_nothing_is_saved() {
        assert_eq!(effective_layout(None), ToolbarItem::DEFAULT.to_vec());
        assert_eq!(effective_layout(Some(&[ToolbarItem::Up])), vec![ToolbarItem::Up]);
    }

    #[test]
    fn default_has_every_original_button_once() {
        assert_eq!(normalize(&ToolbarItem::DEFAULT), ToolbarItem::DEFAULT.to_vec());
        assert_eq!(available_buttons(&ToolbarItem::DEFAULT).len(), 5);
    }

    #[test]
    fn normalize_tidies_separators_and_duplicates() {
        use ToolbarItem::*;
        let messy = [Separator, Back, Separator, Separator, Up, Back, Separator];
        assert_eq!(normalize(&messy), vec![Back, Separator, Up]);
    }

    #[test]
    fn serializes_by_name() {
        let json = serde_json::to_string(&vec![ToolbarItem::Back, ToolbarItem::Separator]).unwrap();
        assert_eq!(json, r#"["Back","Separator"]"#);
    }
}
