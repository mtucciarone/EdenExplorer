use std::collections::HashSet;
use std::time::Instant;

use crate::gui::windows::containers::enums::TabbarNavAction;
use crate::gui::windows::structs::Navigation;
use std::path::PathBuf;

#[derive(Default)]
pub struct ExplorerState {
    pub selected_paths: HashSet<PathBuf>,
    pub selection_anchor: Option<usize>,
    pub selection_focus: Option<usize>,
    pub newly_created_path: Option<PathBuf>, // new folder or file
}

#[derive(Clone)]
pub struct TabInfo {
    pub id: u64,
    pub title: String,
    pub full_path: PathBuf,
}

#[derive(Default)]
pub struct TabsAction {
    pub activate: Option<u64>,
    pub close: Option<u64>,
    pub open_new: bool,
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
    pub nav_to: Option<PathBuf>,
    pub refresh_current_directory: bool,
    pub is_breadcrumb_path_edit_active: bool,
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

pub struct ClipboardState {
    paths: HashSet<PathBuf>,
    is_cut: bool,
    last_update: Instant,
}
