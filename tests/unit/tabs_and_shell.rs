#[path = "../common/mod.rs"]
mod common;

use super::{move_items_via_shell, rename_item_via_shell, tab_index_for_id};

#[test]
fn tab_index_for_id_finds_known_and_ignores_stale_ids() {
    let ids = [3u64, 7, 9];
    assert_eq!(tab_index_for_id(ids.into_iter(), 7), Some(1));
    assert_eq!(tab_index_for_id(ids.into_iter(), 8), None);
    assert_eq!(tab_index_for_id(std::iter::empty(), 1), None);
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
