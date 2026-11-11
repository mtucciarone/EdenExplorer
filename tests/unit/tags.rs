use super::*;
use crate::core::indexer::TagGroupSnapshot;

fn state_with_two_tags() -> TagsState {
    let mut state = TagsState::default();
    state.create_group_and_add(
        "Alpha".to_string(),
        Color32::RED,
        &[PathBuf::from("C:/data/one.txt")],
    );
    state.create_group_and_add(
        "Beta".to_string(),
        Color32::BLUE,
        &[
            PathBuf::from("C:/data/two.txt"),
            PathBuf::from("C:/data/three.txt"),
        ],
    );
    state
}

#[test]
fn removing_last_object_deletes_the_tag() {
    let mut state = state_with_two_tags();
    let id = state.groups[0].id;
    assert!(state.remove_paths_from_group(id, &[PathBuf::from("C:/data/one.txt")]));
    assert_eq!(state.groups.len(), 1);
}

#[test]
fn saved_column_orders_preserve_hidden_columns() {
    let state = ItemViewerColumnState::from_saved_orders(
        vec![
            ItemViewerHeaderColumn::Name,
            ItemViewerHeaderColumn::Type,
            ItemViewerHeaderColumn::Modified,
            ItemViewerHeaderColumn::Created,
        ],
        default_drive_column_order(),
        default_recycle_bin_column_order(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    assert!(
        !state
            .file_column_order
            .contains(&ItemViewerHeaderColumn::Size)
    );
    assert!(
        !state
            .file_column_order
            .contains(&ItemViewerHeaderColumn::Tags)
    );
    assert_eq!(state.file_column_order[0], ItemViewerHeaderColumn::Name);
}

#[test]
fn removing_some_objects_keeps_a_non_empty_tag() {
    let mut state = state_with_two_tags();
    let id = state.groups[1].id;
    assert!(state.remove_paths_from_group(id, &[PathBuf::from("C:/data/two.txt")]));
    assert_eq!(state.groups[1].items.len(), 1);
}

#[test]
fn deleting_objects_by_prefix_deletes_emptied_tags() {
    let mut state = state_with_two_tags();
    assert!(state.remove_path_prefix(Path::new("C:/data")));
    assert!(state.groups.is_empty());
}

#[test]
fn loading_a_snapshot_drops_empty_tags() {
    let snapshot = TagsSnapshot {
        version: 1,
        next_group_id: 3,
        groups: vec![
            TagGroupSnapshot {
                id: 1,
                name: "Kept".into(),
                color: [255, 0, 0, 255],
                items: vec![PathBuf::from("C:/data/one.txt")],
            },
            TagGroupSnapshot {
                id: 2,
                name: "Empty".into(),
                color: [0, 0, 255, 255],
                items: vec![],
            },
        ],
    };
    let state = TagsState::from_snapshot(snapshot);
    assert_eq!(state.groups.len(), 1);
    assert_eq!(state.groups[0].name, "Kept");
}
