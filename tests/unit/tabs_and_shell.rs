#[path = "../common/mod.rs"]
mod common;

use super::{
    MainWindow, move_items_via_shell, prepare_file_location_navigation, rename_item_via_shell,
    tab_index_for_id,
};
use crate::core::utils::tabs::update_tab_infos_cache;
use crate::gui::windows::containers::structs::SplitSide;
use std::path::PathBuf;

#[test]
fn tab_index_for_id_finds_known_and_ignores_stale_ids() {
    let ids = [3u64, 7, 9];
    assert_eq!(tab_index_for_id(ids.into_iter(), 7), Some(1));
    assert_eq!(tab_index_for_id(ids.into_iter(), 8), None);
    assert_eq!(tab_index_for_id(std::iter::empty(), 1), None);
}

#[test]
fn file_location_navigation_marks_tab_metadata_dirty() {
    let mut explorer = MainWindow::default();
    explorer.tab_infos_dirty = false;
    update_tab_infos_cache(
        &explorer.tabs,
        &mut explorer.tab_infos_cache,
        &mut explorer.tab_infos_dirty,
        &explorer.settings_window,
        &explorer.i18n,
    );

    let folder = PathBuf::from(r"C:\eden-test-location");
    let target = folder.join("target.txt");
    prepare_file_location_navigation(&mut explorer, folder.clone(), vec![target.clone()]);

    assert!(explorer.tab_infos_dirty);
    assert_eq!(explorer.current_nav().current, folder);
    assert_eq!(
        explorer
            .active_tab()
            .view(SplitSide::Primary)
            .explorer_state
            .pending_selection_paths,
        Some(vec![target])
    );

    update_tab_infos_cache(
        &explorer.tabs,
        &mut explorer.tab_infos_cache,
        &mut explorer.tab_infos_dirty,
        &explorer.settings_window,
        &explorer.i18n,
    );
    assert_eq!(explorer.tab_infos_cache[0].full_path, folder);
}

#[test]
fn rename_missing_source_returns_error() {
    common::init_com();
    assert!(rename_item_via_shell(&common::missing_path("rename.txt"), "renamed.txt").is_err());
}

#[test]
fn move_skips_vanished_source() {
    common::init_com();
    let sources = [common::missing_path("move.txt")];
    assert!(
        move_items_via_shell(&sources, &std::env::temp_dir())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn move_to_missing_target_returns_error() {
    common::init_com();
    let sources = [common::missing_path("source.txt")];
    assert!(move_items_via_shell(&sources, &common::missing_path("target")).is_err());
}
