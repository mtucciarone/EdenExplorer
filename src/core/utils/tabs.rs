use crate::core::fs::MY_PC_PATH;
use crate::gui::i18n::I18n;
use crate::gui::windows::containers::structs::{TabInfo, TabState};
use crate::gui::windows::structs::Navigation;
use crate::gui::windows::structs::SettingsWindow;
use std::path::PathBuf;

/// Rebuilds the tab-strip display cache from the window-global tab list, if dirty.
pub fn update_tab_infos_cache(
    tabs: &[TabState],
    tab_infos_cache: &mut Vec<TabInfo>,
    tab_infos_dirty: &mut bool,
    settings_window: &SettingsWindow,
    i18n: &I18n,
) {
    if *tab_infos_dirty || tab_infos_cache.len() != tabs.len() {
        *tab_infos_cache = tabs
            .iter()
            .map(|tab| TabInfo {
                id: tab.id,
                title: tab_title_for(&tab.primary_view.nav, i18n),
                full_path: if tab.primary_view.nav.is_root() {
                    PathBuf::from(MY_PC_PATH)
                } else {
                    tab.primary_view.nav.current.clone()
                },
                is_pinned: settings_window
                    .current_settings
                    .pinned_tabs
                    .iter()
                    .any(|p| p == &tab.primary_view.nav.current),
            })
            .collect();
        *tab_infos_dirty = false;
    }
}

fn tab_title_for(nav: &Navigation, i18n: &I18n) -> String {
    if nav.is_root() {
        return i18n.tr("thispc");
    }

    if nav.is_recycle_bin() {
        return i18n.tr("recycle_bin");
    }

    nav.current
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| nav.current.display().to_string())
}
