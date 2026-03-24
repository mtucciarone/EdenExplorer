use crate::core::state::Navigation;
use crate::gui::windows::containers::enums::TabbarNavAction;
use std::path::PathBuf;

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
    pub is_editing_path: bool,
    pub path_buffer: String,
}

#[derive(Default)]
pub struct TabbarAction {
    pub nav: Option<TabbarNavAction>,
    pub create_folder: bool,
    pub create_file: bool,
    pub add_favorite: bool,
    pub nav_to: Option<PathBuf>,
    pub refresh_current_directory: bool,
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
}
