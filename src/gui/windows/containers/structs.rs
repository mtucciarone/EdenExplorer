use crate::core::fs::FileItem;
use crate::core::indexer::TagsSnapshot;
use crate::core::indexer::{
    default_item_viewer_drive_column_size, default_item_viewer_file_column_size,
    default_recycle_bin_column_size,
};
use crate::core::utils::thumbnails::ThumbnailService;
use crate::gui::utils::hsl_to_color32;
use crate::gui::windows::containers::enums::{
    ItemViewerAction, ItemViewerHeaderColumn, ItemViewerNavAction,
};
use crate::gui::windows::shell_context_menu::ShellContextMenu;
use crate::gui::windows::structs::Navigation;
use crossbeam_channel::{Receiver, Sender};
use egui::Color32;
use rand::Rng;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct ExplorerState {
    pub selected_paths: HashSet<PathBuf>,
    pub selection_anchor: Option<usize>,
    pub selection_focus: Option<usize>,
    pub pending_selection_paths: Option<Vec<PathBuf>>, // select after refresh
    pub non_ntfs_popup_path: Option<PathBuf>,
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
    pub scroll_left: bool,
    pub scroll_right: bool,
    pub max_scroll_offset: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SplitSide {
    Primary,
    Secondary,
}

/// One directory view: its own navigation, sort/selection/filter, listing, and
/// async scan state. A tab always has a `primary_view`; if `split_view` is
/// `Some`, the tab shows both side by side.
pub struct TabView {
    pub nav: Navigation,
    pub breadcrumb_path_editing: bool,
    pub breadcrumb_path_buffer: String,
    pub breadcrumb_just_started_editing: bool,
    pub breadcrumb_select_all_on_focus: bool,
    pub breadcrumb_path_error: bool,
    pub breadcrumb_path_error_animation_time: f64,
    pub sort_column: crate::gui::utils::SortColumn,
    pub sort_ascending: bool,
    pub explorer_state: ExplorerState,
    pub item_viewer_filter_state: FilterState,
    pub column_state: ItemViewerColumnState,
    pub display_mode: ItemViewerDisplayMode,
    pub gallery_state: GalleryState,
    pub thumbnail_service: ThumbnailService,
    pub files: Vec<FileItem>,
    pub drag_state: DragState,
    pub is_loading: bool,
    pub rx: Option<Receiver<FileItem>>,
    pub size_req_tx: Option<Sender<PathBuf>>,
    pub size_rx: Option<Receiver<(PathBuf, u64, bool)>>,
    pub pending_size_queue: VecDeque<PathBuf>,
    pub pending_size_set: HashSet<PathBuf>,
    pub size_threads: Vec<std::thread::JoinHandle<()>>,
}

impl TabView {
    pub fn new(
        nav: Navigation,
        default_sort_column: crate::gui::utils::SortColumn,
        default_sort_ascending: bool,
    ) -> Self {
        Self {
            nav,
            breadcrumb_path_editing: false,
            breadcrumb_path_buffer: String::new(),
            breadcrumb_just_started_editing: false,
            breadcrumb_select_all_on_focus: false,
            breadcrumb_path_error: false,
            breadcrumb_path_error_animation_time: 0.0,
            sort_column: default_sort_column,
            sort_ascending: default_sort_ascending,
            explorer_state: ExplorerState::default(),
            item_viewer_filter_state: FilterState::default(),
            column_state: ItemViewerColumnState::default(),
            display_mode: ItemViewerDisplayMode::Details,
            gallery_state: GalleryState::default(),
            thumbnail_service: ThumbnailService::default(),
            files: Vec::new(),
            drag_state: DragState::default(),
            is_loading: false,
            rx: None,
            size_req_tx: None,
            size_rx: None,
            pending_size_queue: VecDeque::new(),
            pending_size_set: HashSet::new(),
            size_threads: Vec::new(),
        }
    }

    /// Used when opening a split: a fresh view pointed at the same directory
    /// and sort as `self`, but with its own (empty) selection/filter/listing.
    pub fn duplicate_as_new(&self) -> Self {
        let mut view = Self::new(self.nav.clone(), self.sort_column, self.sort_ascending);
        view.column_state = self.column_state.clone();
        view.display_mode = self.display_mode;
        view.gallery_state = self.gallery_state;
        view
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemViewerDisplayMode {
    Details,
    Gallery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GalleryThumbnailSize {
    ExtraSmall,
    Small,
    Medium,
    Large,
    ExtraLarge,
}

impl GalleryThumbnailSize {
    pub fn pixel_size(self) -> f32 {
        match self {
            Self::ExtraSmall => 24.0,
            Self::Small => 48.0,
            Self::Medium => 96.0,
            Self::Large => 128.0,
            Self::ExtraLarge => 256.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ExtraSmall => "Extra Small",
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
            Self::ExtraLarge => "Extra Large",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GalleryState {
    pub thumbnail_size: GalleryThumbnailSize,
    pub thumbnail_gap: f32,
    pub thumbnail_padding: f32,
}

impl Default for GalleryState {
    fn default() -> Self {
        Self {
            thumbnail_size: GalleryThumbnailSize::Medium,
            thumbnail_gap: 12.0,
            thumbnail_padding: 6.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemViewerColumnFitRequest {
    Column(ItemViewerHeaderColumn),
    All,
}

#[derive(Clone, Debug)]
pub struct ItemViewerColumnState {
    pub file_column_order: Vec<ItemViewerHeaderColumn>,
    pub drive_column_order: Vec<ItemViewerHeaderColumn>,
    pub recycle_bin_column_order: Vec<ItemViewerHeaderColumn>,

    pub file_column_sizes: Vec<f32>,
    pub drive_column_sizes: Vec<f32>,
    pub recycle_bin_column_sizes: Vec<f32>,

    pub layout_generation: u64,
    pub pending_fit_request: Option<ItemViewerColumnFitRequest>,
}

impl Default for ItemViewerColumnState {
    fn default() -> Self {
        Self {
            file_column_order: default_file_column_order(),
            drive_column_order: default_drive_column_order(),
            recycle_bin_column_order: default_recycle_bin_column_order(),
            file_column_sizes: default_item_viewer_file_column_size(),
            drive_column_sizes: default_item_viewer_drive_column_size(),
            recycle_bin_column_sizes: default_recycle_bin_column_size(),
            layout_generation: 0,
            pending_fit_request: None,
        }
    }
}

impl ItemViewerColumnState {
    pub fn toggle_column_visibility(
        &mut self,
        is_drive_view: bool,
        is_recycle_bin_view: bool,
        column: ItemViewerHeaderColumn,
    ) -> bool {
        if column == ItemViewerHeaderColumn::Name {
            return false;
        }

        let order = self.order_mut(is_drive_view, is_recycle_bin_view);

        if let Some(index) = order.iter().position(|c| *c == column) {
            order.remove(index);
            return true;
        }

        let default_order = if is_drive_view {
            default_drive_column_order()
        } else if is_recycle_bin_view {
            default_recycle_bin_column_order()
        } else {
            default_file_column_order()
        };

        let default_index = default_order
            .iter()
            .position(|c| *c == column)
            .unwrap_or(order.len());

        let insert_index = order
            .iter()
            .filter_map(|existing| default_order.iter().position(|c| c == existing))
            .filter(|&index| index < default_index)
            .count();

        order.insert(insert_index, column);

        true
    }
    pub fn is_column_visible(
        &self,
        is_drive_view: bool,
        is_recycle_bin_view: bool,
        column: ItemViewerHeaderColumn,
    ) -> bool {
        if column == ItemViewerHeaderColumn::Name {
            return true;
        }

        self.order(is_drive_view, is_recycle_bin_view)
            .contains(&column)
    }

    pub fn column_width(
        &self,
        is_drive_view: bool,
        is_recycle_bin_view: bool,
        column: ItemViewerHeaderColumn,
    ) -> Option<f32> {
        let sizes = if is_drive_view {
            &self.drive_column_sizes
        } else if is_recycle_bin_view {
            &self.recycle_bin_column_sizes
        } else {
            &self.file_column_sizes
        };

        let index = if is_drive_view {
            match column {
                ItemViewerHeaderColumn::Name => 0,
                ItemViewerHeaderColumn::Size => 1,
                ItemViewerHeaderColumn::Usage => 2,
                _ => return None,
            }
        } else if is_recycle_bin_view {
            match column {
                ItemViewerHeaderColumn::Name => 0,
                ItemViewerHeaderColumn::OriginalDirectory => 1,
                ItemViewerHeaderColumn::Type => 2,
                ItemViewerHeaderColumn::Size => 3,
                ItemViewerHeaderColumn::Deleted => 4,
                ItemViewerHeaderColumn::Created => 5,
                _ => return None,
            }
        } else {
            match column {
                ItemViewerHeaderColumn::Name => 0,
                ItemViewerHeaderColumn::Type => 1,
                ItemViewerHeaderColumn::Size => 2,
                ItemViewerHeaderColumn::Modified => 3,
                ItemViewerHeaderColumn::Created => 4,
                _ => return None,
            }
        };

        sizes.get(index).copied()
    }

    pub fn set_column_width(
        &mut self,
        is_drive_view: bool,
        is_recycle_bin_view: bool,
        column: ItemViewerHeaderColumn,
        width: f32,
    ) {
        let sizes = if is_drive_view {
            &mut self.drive_column_sizes
        } else if is_recycle_bin_view {
            &mut self.recycle_bin_column_sizes
        } else {
            &mut self.file_column_sizes
        };

        let index = if is_drive_view {
            match column {
                ItemViewerHeaderColumn::Name => 0,
                ItemViewerHeaderColumn::Size => 1,
                ItemViewerHeaderColumn::Usage => 2,
                _ => return,
            }
        } else if is_recycle_bin_view {
            match column {
                ItemViewerHeaderColumn::Name => 0,
                ItemViewerHeaderColumn::OriginalDirectory => 1,
                ItemViewerHeaderColumn::Type => 2,
                ItemViewerHeaderColumn::Size => 3,
                ItemViewerHeaderColumn::Deleted => 4,
                ItemViewerHeaderColumn::Created => 5,
                _ => return,
            }
        } else {
            match column {
                ItemViewerHeaderColumn::Name => 0,
                ItemViewerHeaderColumn::Type => 1,
                ItemViewerHeaderColumn::Size => 2,
                ItemViewerHeaderColumn::Modified => 3,
                ItemViewerHeaderColumn::Created => 4,
                _ => return,
            }
        };

        if let Some(slot) = sizes.get_mut(index) {
            *slot = width;
        }
    }

    pub fn set_all_column_widths(
        &mut self,
        is_drive_view: bool,
        is_recycle_bin_view: bool,
        widths: &ItemViewerColumnWidths,
    ) {
        self.set_column_width(
            is_drive_view,
            is_recycle_bin_view,
            ItemViewerHeaderColumn::Name,
            widths.name,
        );

        self.set_column_width(
            is_drive_view,
            is_recycle_bin_view,
            ItemViewerHeaderColumn::OriginalDirectory,
            widths.original_directory_width,
        );

        self.set_column_width(
            is_drive_view,
            is_recycle_bin_view,
            ItemViewerHeaderColumn::Type,
            widths.type_width,
        );

        self.set_column_width(
            is_drive_view,
            is_recycle_bin_view,
            ItemViewerHeaderColumn::Size,
            widths.size_width,
        );

        self.set_column_width(
            is_drive_view,
            is_recycle_bin_view,
            ItemViewerHeaderColumn::Modified,
            widths.modified_width,
        );

        self.set_column_width(
            is_drive_view,
            is_recycle_bin_view,
            ItemViewerHeaderColumn::Created,
            widths.created_width,
        );

        self.set_column_width(
            is_drive_view,
            is_recycle_bin_view,
            ItemViewerHeaderColumn::Deleted,
            widths.deleted_width,
        );

        self.set_column_width(
            is_drive_view,
            is_recycle_bin_view,
            ItemViewerHeaderColumn::Usage,
            widths.usage_width,
        );
    }
    pub fn column_sizes(&self, is_drive_view: bool, is_recycle_bin_view: bool) -> &[f32] {
        if is_drive_view {
            &self.drive_column_sizes
        } else if is_recycle_bin_view {
            &self.recycle_bin_column_sizes
        } else {
            &self.file_column_sizes
        }
    }

    pub fn from_orders(
        file_column_order: Vec<ItemViewerHeaderColumn>,
        drive_column_order: Vec<ItemViewerHeaderColumn>,
        recycle_bin_column_order: Vec<ItemViewerHeaderColumn>,
        file_column_sizes: Vec<f32>,
        drive_column_sizes: Vec<f32>,
        recycle_bin_column_sizes: Vec<f32>,
    ) -> Self {
        let mut state = Self::default();
        state.file_column_order =
            sanitize_column_order(file_column_order, &default_file_column_order());
        state.drive_column_order =
            sanitize_column_order(drive_column_order, &default_drive_column_order());
        state.recycle_bin_column_order = sanitize_column_order(
            recycle_bin_column_order,
            &default_recycle_bin_column_order(),
        );
        state.file_column_sizes =
            sanitize_column_sizes(file_column_sizes, &default_item_viewer_file_column_size());
        state.drive_column_sizes =
            sanitize_column_sizes(drive_column_sizes, &default_item_viewer_drive_column_size());
        state.recycle_bin_column_sizes =
            sanitize_column_sizes(recycle_bin_column_sizes, &default_recycle_bin_column_size());
        state
    }

    pub fn order_mut(
        &mut self,
        is_drive_view: bool,
        is_recycle_bin_view: bool,
    ) -> &mut Vec<ItemViewerHeaderColumn> {
        if is_drive_view {
            &mut self.drive_column_order
        } else if is_recycle_bin_view {
            &mut self.recycle_bin_column_order
        } else {
            &mut self.file_column_order
        }
    }

    pub fn order(
        &self,
        is_drive_view: bool,
        is_recycle_bin_view: bool,
    ) -> &[ItemViewerHeaderColumn] {
        if is_drive_view {
            &self.drive_column_order
        } else if is_recycle_bin_view {
            &self.recycle_bin_column_order
        } else {
            &self.file_column_order
        }
    }

    pub fn move_column(
        &mut self,
        is_drive_view: bool,
        is_recycle_bin_view: bool,
        column: ItemViewerHeaderColumn,
        offset: isize,
    ) -> bool {
        if column == ItemViewerHeaderColumn::Name || offset == 0 {
            return false;
        }

        let order = self.order_mut(is_drive_view, is_recycle_bin_view);
        let Some(index) = order.iter().position(|c| *c == column) else {
            return false;
        };

        let len = order.len() as isize;
        let mut new_index = index as isize + offset;
        if new_index < 0 {
            new_index = 0;
        } else if new_index >= len {
            new_index = len - 1;
        }

        if new_index as usize == index {
            return false;
        }

        let item = order.remove(index);
        order.insert(new_index as usize, item);
        true
    }

    pub fn move_column_to_edge(
        &mut self,
        is_drive_view: bool,
        is_recycle_bin_view: bool,
        column: ItemViewerHeaderColumn,
        to_start: bool,
    ) -> bool {
        if column == ItemViewerHeaderColumn::Name {
            return false;
        }

        let order = self.order_mut(is_drive_view, is_recycle_bin_view);
        let Some(index) = order.iter().position(|c| *c == column) else {
            return false;
        };

        let edge_index = if to_start {
            0
        } else {
            order.len().saturating_sub(1)
        };
        if index == edge_index {
            return false;
        }

        let item = order.remove(index);
        order.insert(edge_index, item);
        true
    }

    pub fn visible_order(
        &self,
        is_drive_view: bool,
        is_recycle_bin_view: bool,
    ) -> Vec<ItemViewerHeaderColumn> {
        let allowed: &[ItemViewerHeaderColumn] = if is_drive_view {
            &[
                ItemViewerHeaderColumn::Name,
                ItemViewerHeaderColumn::Size,
                ItemViewerHeaderColumn::Usage,
            ]
        } else if is_recycle_bin_view {
            &[
                ItemViewerHeaderColumn::Name,
                ItemViewerHeaderColumn::OriginalDirectory,
                ItemViewerHeaderColumn::Type,
                ItemViewerHeaderColumn::Size,
                ItemViewerHeaderColumn::Deleted,
                ItemViewerHeaderColumn::Created,
            ]
        } else {
            &[
                ItemViewerHeaderColumn::Name,
                ItemViewerHeaderColumn::Type,
                ItemViewerHeaderColumn::Size,
                ItemViewerHeaderColumn::Modified,
                ItemViewerHeaderColumn::Created,
            ]
        };

        let mut visible = Vec::with_capacity(allowed.len());

        // Name is always visible and always first.
        visible.push(ItemViewerHeaderColumn::Name);

        for column in self.order(is_drive_view, is_recycle_bin_view) {
            if allowed.contains(column) && *column != ItemViewerHeaderColumn::Name {
                visible.push(*column);
            }
        }

        visible
    }
}

fn sanitize_column_sizes(sizes: Vec<f32>, defaults: &[f32]) -> Vec<f32> {
    if sizes.len() != defaults.len() {
        return defaults.to_vec();
    }

    sizes
        .into_iter()
        .zip(defaults.iter().copied())
        .map(|(size, default)| {
            if size.is_finite() && size > 0.0 {
                size
            } else {
                default
            }
        })
        .collect()
}

fn default_file_column_order() -> Vec<ItemViewerHeaderColumn> {
    vec![
        ItemViewerHeaderColumn::Name,
        ItemViewerHeaderColumn::Type,
        ItemViewerHeaderColumn::Size,
        ItemViewerHeaderColumn::Modified,
        ItemViewerHeaderColumn::Created,
    ]
}

fn default_drive_column_order() -> Vec<ItemViewerHeaderColumn> {
    vec![
        ItemViewerHeaderColumn::Name,
        ItemViewerHeaderColumn::Type,
        ItemViewerHeaderColumn::Size,
        ItemViewerHeaderColumn::Usage,
    ]
}

fn default_recycle_bin_column_order() -> Vec<ItemViewerHeaderColumn> {
    vec![
        ItemViewerHeaderColumn::Name,
        ItemViewerHeaderColumn::OriginalDirectory,
        ItemViewerHeaderColumn::Type,
        ItemViewerHeaderColumn::Size,
        ItemViewerHeaderColumn::Deleted,
        ItemViewerHeaderColumn::Created,
    ]
}

fn sanitize_column_order(
    order: Vec<ItemViewerHeaderColumn>,
    default_order: &[ItemViewerHeaderColumn],
) -> Vec<ItemViewerHeaderColumn> {
    let mut sanitized = Vec::with_capacity(default_order.len());

    // Keep all valid columns from the user's saved order.
    for column in order {
        if default_order.contains(&column) && !sanitized.contains(&column) {
            sanitized.push(column);
        }
    }

    // Add any newly introduced columns according to their position
    // in the default order, while preserving the user's existing order.
    for (default_index, &column) in default_order.iter().enumerate() {
        if sanitized.contains(&column) {
            continue;
        }

        let insert_index = sanitized
            .iter()
            .filter_map(|existing| default_order.iter().position(|default| default == existing))
            .filter(|&index| index < default_index)
            .count();

        sanitized.insert(insert_index, column);
    }

    sanitized
}

pub struct TabState {
    pub id: u64,
    pub primary_view: TabView,
    pub split_view: Option<TabView>,
}

impl TabState {
    pub fn new(
        id: u64,
        nav: Navigation,
        default_sort_column: crate::gui::utils::SortColumn,
        default_sort_ascending: bool,
    ) -> Self {
        Self {
            id,
            primary_view: TabView::new(nav, default_sort_column, default_sort_ascending),
            split_view: None,
        }
    }

    pub fn view(&self, side: SplitSide) -> &TabView {
        match side {
            SplitSide::Primary => &self.primary_view,
            SplitSide::Secondary => self.split_view.as_ref().unwrap_or(&self.primary_view),
        }
    }

    pub fn view_mut(&mut self, side: SplitSide) -> &mut TabView {
        match side {
            SplitSide::Primary => &mut self.primary_view,
            SplitSide::Secondary => self.split_view.as_mut().unwrap_or(&mut self.primary_view),
        }
    }
}

#[derive(Default)]
pub struct ItemViewerNavBarAction {
    pub nav: Option<ItemViewerNavAction>,
    pub create_folder: bool,
    pub create_file: bool,
    pub add_favorite: bool,
    pub remove_favorite: bool,
    pub nav_to: Option<PathBuf>,
    pub refresh_current_directory: bool,
    pub toggle_gallery: bool,
    pub is_breadcrumb_path_edit_active: bool,
    pub move_files_to_breadcrumb_dir: Option<PathBuf>,
    pub move_files_to_breadcrumb_dir_rect: Option<egui::Rect>,
}

#[derive(Clone, Copy)]
pub struct ItemViewerFolderSizeState {
    pub bytes: u64,
    pub done: bool,
}

#[derive(Clone, Debug)]
pub struct Breadcrumb {
    pub label: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct RenderedBreadcrumb {
    pub label: String,
    pub full_label: String,
    pub path: PathBuf,
    pub truncated: bool,
    pub is_ellipsis: bool,
    pub width: f32,
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
    pub move_files_to_sidebar_dir: Option<PathBuf>,
}

#[derive(Default)]
pub struct TopbarAction {
    pub toggle_theme: bool,
    pub customize_theme: bool,
    pub open_settings: bool,
    pub about: bool,
    pub exit: bool,
    pub toggle_file_explorer: bool,
    pub toggle_sidebar: bool,
    pub toggle_active_tab_split: bool,
}

#[derive(Default)]
pub struct ItemViewerLayout {
    pub row_height: f32, // total row height
    pub icon_size: f32,
    pub header_height: f32,
    pub is_drive_view: bool,
    pub is_recycle_bin_view: bool,
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
    pub last_show_hidden_files_folders: bool,
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
            last_show_hidden_files_folders: false,
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

    pub fn is_tagged(&self, path: &Path) -> bool {
        self.groups
            .iter()
            .any(|group| group.items.iter().any(|item| item == path))
    }

    pub fn tag_color_for_path(&self, path: &Path) -> Option<egui::Color32> {
        self.groups
            .iter()
            .find(|group| group.items.iter().any(|item| item == path))
            .map(|group| group.color)
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

        for path in &paths {
            self.remove_path(path);
        }

        for path in paths {
            if !group.items.contains(&path) {
                group.items.push(path);
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
}

pub fn default_tag_color() -> Color32 {
    let hue = rand::rng().random_range(0.0..360.0);
    hsl_to_color32(hue, 0.55, 0.88)
}

pub struct ItemViewerColumnLayout {
    pub layout_generation: u64,
    pub ordered_columns: Vec<ItemViewerHeaderColumn>,
    pub name_width: f32,
    pub type_width: f32,
    pub size_width: f32,
    pub usage_width: f32,
    pub modified_width: f32,
    pub created_width: f32,
    pub deleted_width: f32,
    pub original_directory_width: f32,
    pub column_sizes_changed: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ItemViewerColumnWidths {
    pub name: f32,
    pub type_width: f32,
    pub size_width: f32,
    pub modified_width: f32,
    pub created_width: f32,
    pub usage_width: f32,
    pub deleted_width: f32,
    pub original_directory_width: f32,
}
