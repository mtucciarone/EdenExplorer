use crate::core::indexer::TagsSnapshot;
use crate::gui::windows::containers::enums::{ItemViewerAction, TabbarNavAction};
use crate::gui::windows::shell_context_menu::ShellContextMenu;
use crate::gui::windows::structs::Navigation;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use rand::Rng;
use egui::Color32;
use crate::gui::utils::hsl_to_color32;

#[derive(Default)]
pub struct ExplorerState {
    pub selected_paths: HashSet<PathBuf>,
    pub selection_anchor: Option<usize>,
    pub selection_focus: Option<usize>,
    pub newly_created_path: Option<PathBuf>, // new folder or file
    pub non_ntfs_popup_path: Option<PathBuf>,
    pub windows_context_menu_expanded: bool,
    pub windows_context_menu_cache: Option<WindowsContextMenuCache>,
    pub navigation_history: HashMap<PathBuf, PathBuf>, // parent_dir -> last_visited_child
    pub navigation_selection: Option<PathBuf>,         // path to select after navigation loads
}

pub struct WindowsContextMenuCache {
    pub selection: Vec<PathBuf>,
    pub menu: ShellContextMenu,
}

#[derive(Clone)]
pub struct TabInfo {
    pub id: u64,
    pub title: String,
    pub full_path: PathBuf,
    pub is_pinned: bool,
}

#[derive(Default)]
pub struct TabsAction {
    pub activate: Option<u64>,
    pub close: Option<u64>,
    pub open_new: bool,
    pub toggle_pin: Option<PathBuf>,
    pub move_files_to_tab_dir: Option<PathBuf>,
    pub move_files_to_tab_dir_rect: Option<egui::Rect>,
}

pub struct TabState {
    pub id: u64,
    pub nav: Navigation,
    pub breadcrumb_path_editing: bool,
    pub breadcrumb_path_buffer: String,
    pub breadcrumb_just_started_editing: bool,
    pub breadcrumb_path_error: bool,
    pub breadcrumb_path_error_animation_time: f64,
}

#[derive(Default)]
pub struct TabbarAction {
    pub nav: Option<TabbarNavAction>,
    pub create_folder: bool,
    pub create_file: bool,
    pub add_favorite: bool,
    pub remove_favorite: bool,
    pub nav_to: Option<PathBuf>,
    pub refresh_current_directory: bool,
    pub is_breadcrumb_path_edit_active: bool,
    pub move_files_to_breadcrumb_dir: Option<PathBuf>,
    pub move_files_to_breadcrumb_dir_rect: Option<egui::Rect>,
}

#[derive(Clone, Copy)]
pub struct ItemViewerFolderSizeState {
    pub bytes: u64,
    pub done: bool,
}

pub struct RenameState {
    pub path: PathBuf,
    pub new_name: String,
    pub should_focus: bool,
    pub validation_error_show: bool,
}

#[derive(Clone)]
pub struct FavoriteItem {
    pub path: PathBuf,
    pub label: String,
}

#[derive(Default)]
pub struct SidebarAction {
    pub nav_to: Option<PathBuf>,
    pub open_new_tab: Option<PathBuf>,
    pub remove_favorite: Option<PathBuf>,
    pub select_favorite: Option<PathBuf>,
    pub reorder: Option<(usize, usize)>, // from_idx, to_idx
}

#[derive(Default)]
pub struct TopbarAction {
    pub toggle_theme: bool,
    pub customize_theme: bool,
    pub open_settings: bool,
    pub about: bool,
    pub exit: bool,
    pub toggle_file_explorer: bool,
}

#[derive(Default)]
pub struct ItemViewerLayout {
    pub row_height: f32,
    pub header_height: f32,
    pub header_gap: f32,
    pub available_width: f32,
    pub is_drive_view: bool,
}

#[derive(Default)]
pub struct DragState {
    pub active: bool,
    pub source_items: Vec<PathBuf>,
    pub start_pos: Option<egui::Pos2>,
}

pub struct FilterState {
    pub active: bool,
    pub query: String,
    pub last_input_time: f64,
    pub focus_requested: bool,
    pub last_query: String,
    pub last_files_len: usize,
    pub cached_indices: Vec<usize>,
    pub dirty: bool,
}

impl Default for FilterState {
    fn default() -> Self {
        Self {
            active: false,
            query: String::new(),
            last_input_time: 0.0,
            focus_requested: false,
            last_query: String::new(),
            last_files_len: 0,
            cached_indices: Vec::new(),
            dirty: true,
        }
    }
}

#[derive(Clone)]
pub struct TagGroup {
    pub id: u64,
    pub name: String,
    pub color: egui::Color32,
    pub items: Vec<PathBuf>,
}

pub struct TagPickerState {
    pub paths: Vec<PathBuf>,
    pub new_group_name: String,
    pub new_group_color: egui::Color32,
    pub focus_requested: bool,
}

pub struct TagRenameState {
    pub group_id: u64,
    pub buffer: String,
    pub should_focus: bool,
}

#[derive(Clone, Copy)]
pub struct TagDragState {
    pub group_id: u64,
    pub source_index: usize,
    pub active: bool,
}

pub struct TagsState {
    pub groups: Vec<TagGroup>,
    pub next_group_id: u64,
    pub picker: Option<TagPickerState>,
    pub rename_state: Option<TagRenameState>,
    pub drag_state: Option<TagDragState>,
    pub delete_confirmation: Option<u64>,
    pub pending_action: Option<ItemViewerAction>,
}

impl Default for TagsState {
    fn default() -> Self {
        Self {
            groups: Vec::new(),
            next_group_id: 1,
            picker: None,
            rename_state: None,
            drag_state: None,
            delete_confirmation: None,
            pending_action: None,
        }
    }
}

impl TagPickerState {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self {
            paths,
            new_group_name: String::new(),
            new_group_color: default_tag_color(),
            focus_requested: true,
        }
    }
}

impl TagsState {
    pub fn from_snapshot(snapshot: TagsSnapshot) -> Self {
        Self {
            groups: snapshot
                .groups
                .into_iter()
                .map(|group| TagGroup {
                    id: group.id,
                    name: group.name,
                    color: egui::Color32::from_rgba_unmultiplied(
                        group.color[0],
                        group.color[1],
                        group.color[2],
                        group.color[3],
                    ),
                    items: group.items,
                })
                .collect(),
            next_group_id: snapshot.next_group_id.max(1),
            picker: None,
            rename_state: None,
            drag_state: None,
            delete_confirmation: None,
            pending_action: None,
        }
    }

    pub fn to_snapshot(&self) -> TagsSnapshot {
        TagsSnapshot {
            version: 1,
            next_group_id: self.next_group_id.max(1),
            groups: self
                .groups
                .iter()
                .map(|group| crate::core::indexer::TagGroupSnapshot {
                    id: group.id,
                    name: group.name.clone(),
                    color: group.color.to_array(),
                    items: group.items.clone(),
                })
                .collect(),
        }
    }

    pub fn open_picker(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }

        let mut paths = paths;
        paths.sort();
        paths.dedup();
        self.picker = Some(TagPickerState::new(paths));
    }

    pub fn begin_rename_group(&mut self, group_id: u64) -> bool {
        let Some(group) = self.groups.iter().find(|group| group.id == group_id) else {
            return false;
        };

        self.rename_state = Some(TagRenameState {
            group_id,
            buffer: group.name.clone(),
            should_focus: true,
        });
        true
    }

    pub fn cancel_rename_group(&mut self) {
        self.rename_state = None;
    }

    pub fn rename_group(&mut self, group_id: u64, new_name: String) -> bool {
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return false;
        }

        let Some(group) = self.groups.iter_mut().find(|group| group.id == group_id) else {
            return false;
        };

        if group.name == new_name {
            return false;
        }

        group.name = new_name.to_string();
        true
    }

    pub fn is_tagged(&self, path: &Path) -> bool {
        self.groups
            .iter()
            .any(|group| group.items.iter().any(|item| item == path))
    }

    pub fn add_paths_to_group(&mut self, group_id: u64, paths: &[PathBuf]) -> bool {
        let Some(target_index) = self.groups.iter().position(|group| group.id == group_id) else {
            return false;
        };

        let mut paths: Vec<PathBuf> = paths.iter().cloned().collect();
        paths.sort();
        paths.dedup();

        let mut changed = false;
        for path in &paths {
            changed |= self.remove_path(path);
        }

        let target_group = &mut self.groups[target_index];
        for path in paths {
            if !target_group.items.contains(&path) {
                target_group.items.push(path);
                changed = true;
            }
        }

        changed
    }

    pub fn create_group_and_add(
        &mut self,
        name: String,
        color: egui::Color32,
        paths: &[PathBuf],
    ) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }

        let group_id = self.next_group_id.max(1);
        self.next_group_id = group_id.saturating_add(1);

        let mut group = TagGroup {
            id: group_id,
            name: name.to_string(),
            color,
            items: Vec::new(),
        };

        let mut paths: Vec<PathBuf> = paths.iter().cloned().collect();
        paths.sort();
        paths.dedup();

        let mut changed = false;
        for path in &paths {
            changed |= self.remove_path(path);
        }

        for path in paths {
            if !group.items.contains(&path) {
                group.items.push(path);
                changed = true;
            }
        }

        self.groups.push(group);
        true
    }

    pub fn remove_path(&mut self, path: &Path) -> bool {
        let mut changed = false;

        for group in &mut self.groups {
            let before = group.items.len();
            group.items.retain(|item| item != path);
            changed |= group.items.len() != before;
        }

        changed
    }

    pub fn remove_paths(&mut self, paths: &[PathBuf]) -> bool {
        let paths: HashSet<PathBuf> = paths.iter().cloned().collect();
        let mut changed = false;

        for group in &mut self.groups {
            let before = group.items.len();
            group.items.retain(|item| !paths.contains(item));
            changed |= group.items.len() != before;
        }

        changed
    }

    pub fn remap_path_prefix(&mut self, source_root: &Path, target_root: &Path) -> bool {
        let mut changed = false;

        for group in &mut self.groups {
            for item in &mut group.items {
                if let Ok(relative) = item.strip_prefix(source_root) {
                    let new_path = if relative.as_os_str().is_empty() {
                        target_root.to_path_buf()
                    } else {
                        target_root.join(relative)
                    };

                    if *item != new_path {
                        *item = new_path;
                        changed = true;
                    }
                }
            }
        }

        changed
    }

    pub fn remove_path_prefix(&mut self, source_root: &Path) -> bool {
        let mut changed = false;

        for group in &mut self.groups {
            let before = group.items.len();
            group
                .items
                .retain(|item| item != source_root && item.strip_prefix(source_root).is_err());
            changed |= group.items.len() != before;
        }

        changed
    }

    pub fn replace_path(&mut self, old_path: &Path, new_path: PathBuf) -> bool {
        let mut changed = false;

        for group in &mut self.groups {
            if let Some(item_index) = group.items.iter().position(|item| item == old_path) {
                if group.items.iter().any(|item| item == &new_path) {
                    group.items.remove(item_index);
                } else {
                    group.items[item_index] = new_path.clone();
                }
                changed = true;
            }
        }

        changed
    }

    pub fn reorder_within_group(
        &mut self,
        group_id: u64,
        from_index: usize,
        to_index: usize,
    ) -> bool {
        let Some(group) = self.groups.iter_mut().find(|group| group.id == group_id) else {
            return false;
        };

        if from_index >= group.items.len() || from_index == to_index {
            return false;
        }

        let item = group.items.remove(from_index);
        let mut target = to_index;

        if to_index > from_index {
            target -= 1;
        }

        target = target.min(group.items.len());
        group.items.insert(target, item);
        true
    }
}

pub fn default_tag_color() -> Color32 {
    let hue = rand::rng().random_range(0.0..360.0);
    hsl_to_color32(hue, 0.55, 0.88)
}