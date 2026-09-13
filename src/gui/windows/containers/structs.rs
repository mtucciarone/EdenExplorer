use crate::core::fs::FileItem;
use crate::core::indexer::TagsSnapshot;
use crate::core::indexer::{
    default_item_viewer_drive_column_size, default_item_viewer_file_column_size,
    default_recycle_bin_column_size,
};
use crate::core::utils::thumbnails::ThumbnailService;
use crate::gui::utils::{SortKey, hsl_to_color32};
use crate::gui::windows::containers::enums::{
    ItemViewerAction, ItemViewerHeaderColumn, ItemViewerNavAction,
};
use crate::gui::windows::shell_context_menu::ShellContextMenu;
use crate::gui::windows::structs::Navigation;
use crossbeam_channel::{Receiver, Sender};
use egui::Color32;
use rand::Rng;
use serde::{Deserialize, Serialize};
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
    /// Armed when a single click lands on an item that was already the sole
    /// selection - matching Windows Explorer's "click, pause, click again"
    /// rename gesture. If a genuine double-click follows quickly, the arm is
    /// cancelled and the item opens as usual; otherwise, once the pause
    /// (`CLICK_TO_RENAME_DELAY`) elapses with the selection unchanged, rename
    /// mode starts. Not armed in the Columns/Column Preview views, which
    /// only rename via the right-click menu.
    pub click_to_rename_arm: Option<(PathBuf, f64)>,
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
    /// Close every other tab, keeping this one (and any pinned tabs).
    pub close_others: Option<u64>,
    /// Close every tab to the right of this one (pinned tabs kept).
    pub close_to_right: Option<u64>,
    /// Close every tab to the left of this one (pinned tabs kept).
    pub close_to_left: Option<u64>,
    pub open_new: bool,
    pub duplicate: Option<PathBuf>,
    pub toggle_pin: Option<PathBuf>,
    pub move_files_to_tab_dir: Option<PathBuf>,
    /// (from_index, to_index) in the pre-move tab list, from dragging a tab to
    /// reorder it.
    pub reorder: Option<(usize, usize)>,
    /// Open every path in a saved tab group as a new tab, alongside whatever
    /// tabs are already open. Duplicated paths in the group intentionally
    /// open as separate tabs.
    pub open_group: Option<Vec<PathBuf>>,
    /// Close every current tab and replace them with a saved tab group's
    /// paths (one tab per entry, duplicates included).
    pub replace_with_group: Option<Vec<PathBuf>>,
    /// Create a brand-new tab group with this name, containing just this one
    /// path (from right-clicking a tab and choosing "Add to New Group").
    pub add_tab_to_new_group: Option<(String, PathBuf)>,
    /// Append this path to an existing group (by id) - allowed to duplicate
    /// a path already in that group.
    pub add_tab_to_existing_group: Option<(u64, PathBuf)>,
    /// Add or remove this tab's folder from the sidebar Favorites list
    /// (from right-clicking a tab and choosing "Add to Favorites"/"Remove
    /// from Favorites").
    pub toggle_favorite: Option<PathBuf>,
    /// "Save Search" chosen from a search-results tab's own right-click menu
    /// - same `(query, scope)` shape as the navbar search box's own "save
    /// this search" button, so both funnel into the same handler.
    pub save_search: Option<(String, crate::core::everything::SearchScope)>,
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
    pub sort_keys: Vec<SortKey>,
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
    /// State for `ItemViewerDisplayMode::Columns` (Finder-style column browser).
    pub columns_view_state: ColumnsViewState,
    /// Background loader/cache for `ItemViewerDisplayMode::Preview`'s preview pane.
    pub preview_service: crate::core::preview::PreviewService,
    /// Currently previewed file, for `ItemViewerDisplayMode::Preview`.
    pub preview_selection: Option<PathBuf>,
    /// Live video playback (Media Foundation) backing the preview pane when
    /// the selected file is a video - kept separate from `preview_service`
    /// since it's a continuously-updated resource, not a one-shot payload.
    pub video_service: crate::core::video::VideoPreviewService,
    /// Live audio playback + waveform, mirroring `video_service` for the
    /// preview pane's audio-file case.
    pub audio_service: crate::core::audio::AudioPreviewService,
    /// Find-in-preview state for the Text/Markdown preview content pane.
    pub find_in_preview: FindInPreviewState,
    /// Whether the navbar's inline search box is currently being edited on
    /// this pane (replaces the breadcrumb the same way address-bar editing
    /// does).
    pub search_box_editing: bool,
    pub search_box_buffer: String,
    pub search_box_scope: crate::core::everything::SearchScope,
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
            sort_keys: vec![SortKey {
                column: default_sort_column,
                ascending: default_sort_ascending,
            }],
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
            columns_view_state: ColumnsViewState::default(),
            preview_service: crate::core::preview::PreviewService::default(),
            preview_selection: None,
            video_service: crate::core::video::VideoPreviewService::default(),
            audio_service: crate::core::audio::AudioPreviewService::default(),
            find_in_preview: FindInPreviewState::default(),
            search_box_editing: false,
            search_box_buffer: String::new(),
            search_box_scope: crate::core::everything::SearchScope::Everywhere,
        }
    }

    /// Used when opening a split: a fresh view pointed at the same directory
    /// and sort as `self`, but with its own (empty) selection/filter/listing.
    pub fn duplicate_as_new(&self) -> Self {
        let mut view = Self::new(self.nav.clone(), self.sort_column, self.sort_ascending);
        view.sort_keys = self.sort_keys.clone();
        view.column_state = self.column_state.clone();
        view.display_mode = self.display_mode;
        view.gallery_state = self.gallery_state;
        view
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemViewerDisplayMode {
    Details,
    Gallery,
    /// macOS Finder-style column browser: each folder you click opens another
    /// column to its right showing that folder's contents.
    Columns,
    /// The column browser, plus a preview pane on the right showing the content
    /// of the selected file when it's previewable.
    ColumnPreview,
    /// A side-by-side layout: a file list next to a preview pane for the
    /// selected file.
    Preview,
    /// The regular Details table, plus a preview pane on the right (equal
    /// width) showing the content of the file selected with a single click.
    DetailPreview,
}

impl Default for ItemViewerDisplayMode {
    fn default() -> Self {
        Self::Details
    }
}

#[derive(Default)]
pub struct ColumnsViewState {
    pub columns: Vec<ColumnEntry>,
    /// The currently-selected file (not folder) in the deepest column, used by
    /// `ItemViewerDisplayMode::ColumnPreview` to know what to preview.
    pub selected_file: Option<PathBuf>,
    /// Set whenever the view navigates/refreshes (see `load_view`) so the
    /// column browser re-reads every currently-open column's contents from
    /// disk instead of reusing its cache - otherwise Columns/ColumnPreview
    /// never picks up files created, renamed, or deleted (by this app or
    /// externally) since the chain of open columns doesn't change on a
    /// same-directory refresh, which is the only thing that normally
    /// triggers a reload.
    pub needs_reload: bool,
}

pub struct ColumnEntry {
    pub path: PathBuf,
    pub items: Vec<ColumnItem>,
}

#[derive(Clone)]
pub struct ColumnItem {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_hidden: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GalleryThumbnailSize {
    ExtraSmall,
    Small,
    Medium,
    Large,
    ExtraLarge,
}

impl Default for GalleryThumbnailSize {
    fn default() -> Self {
        Self::Medium
    }
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

impl GalleryState {
    pub fn set_thumbnail_size(&mut self, size: GalleryThumbnailSize) {
        self.thumbnail_size = size;
        self.thumbnail_gap = match size {
            GalleryThumbnailSize::ExtraSmall => 0.0,
            GalleryThumbnailSize::Small => 4.0,
            GalleryThumbnailSize::Medium => 8.0,
            GalleryThumbnailSize::Large => 12.0,
            GalleryThumbnailSize::ExtraLarge => 16.0,
        };
        self.thumbnail_padding = match size {
            GalleryThumbnailSize::ExtraSmall => 0.0,
            GalleryThumbnailSize::Small => 2.0,
            GalleryThumbnailSize::Medium => 6.0,
            GalleryThumbnailSize::Large => 8.0,
            GalleryThumbnailSize::ExtraLarge => 10.0,
        };
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

    /// Whether `compute_item_viewer_column_layout` has already checked this
    /// (freshly loaded/navigated-to) column state for auto-fit. Render-time
    /// driven instead of an event/navigation hook, so it fires reliably no
    /// matter how the view ended up loaded (first tab on startup, switching
    /// to an already-open tab, a new tab, a split pane, etc).
    pub auto_fit_checked: bool,
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
            auto_fit_checked: false,
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
                ItemViewerHeaderColumn::Tags => 5,
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
                ItemViewerHeaderColumn::Tags => 5,
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

        self.set_column_width(
            is_drive_view,
            is_recycle_bin_view,
            ItemViewerHeaderColumn::Tags,
            widths.tags_width,
        );
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
        is_search_view: bool,
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
        } else if is_search_view {
            // Search results come from many different folders (unlike a
            // normal listing, where every row shares the tab's own current
            // directory) - reusing `OriginalDirectory` (the same column the
            // Recycle Bin already shows "where this came from" in) surfaces
            // each match's actual location instead of leaving that
            // information only visible one file at a time via Properties.
            &[
                ItemViewerHeaderColumn::Name,
                ItemViewerHeaderColumn::OriginalDirectory,
                ItemViewerHeaderColumn::Type,
                ItemViewerHeaderColumn::Size,
                ItemViewerHeaderColumn::Modified,
                ItemViewerHeaderColumn::Tags,
            ]
        } else {
            &[
                ItemViewerHeaderColumn::Name,
                ItemViewerHeaderColumn::Type,
                ItemViewerHeaderColumn::Size,
                ItemViewerHeaderColumn::Modified,
                ItemViewerHeaderColumn::Created,
                ItemViewerHeaderColumn::Tags,
            ]
        };

        // Search view's column set is fixed (not reordered/toggled by the
        // user) - there's no dedicated persisted order for it, and its
        // `allowed` set (above) includes `OriginalDirectory`, which the
        // normal file view's own persisted order never contains, so
        // intersecting with `self.order(...)` below would silently drop it.
        if is_search_view {
            return allowed.to_vec();
        }

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
        ItemViewerHeaderColumn::Tags,
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
    pub set_display_mode: Option<ItemViewerDisplayMode>,
    pub is_breadcrumb_path_edit_active: bool,
    pub move_files_to_breadcrumb_dir: Option<PathBuf>,
    pub move_files_to_breadcrumb_dir_rect: Option<egui::Rect>,
    /// Set when the user submits a query from the navbar's inline search
    /// box (Enter key) - opens a new search-results tab for it.
    pub open_search: Option<(String, crate::core::everything::SearchScope)>,
    /// Set when the navbar's search icon (or its keyboard shortcut) is
    /// pressed - the caller picks a default scope (from settings + whether
    /// this tab is showing a real folder) and switches the breadcrumb row
    /// into the inline search box.
    pub activate_search_box: bool,
    /// Set when the "save this search" button next to the search box is
    /// clicked - the caller appends a `SavedSearch` (or shows the
    /// limit-reached message if already at `MAX_SAVED_SEARCHES`).
    pub save_search: Option<(String, crate::core::everything::SearchScope)>,
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

/// Find-in-preview state for `ItemViewerDisplayMode::Preview`'s content pane
/// (Text and Markdown payloads only - see `itemviewer_preview.rs`). Matches
/// are byte ranges into the payload's own source string (the plain text, or
/// the raw Markdown before rendering), computed by a case-insensitive
/// substring search - kept here rather than in `itemviewer_preview.rs` since
/// it's plain per-tab UI state, matching where `RenameState`/`GalleryState`
/// and friends already live.
#[derive(Default)]
pub struct FindInPreviewState {
    pub active: bool,
    pub query: String,
    current_path: Option<PathBuf>,
    matches: Vec<(usize, usize)>,
    current: usize,
    /// Set for exactly one frame after the current match changes (a fresh
    /// search, or Next/Prev) - `take_jump_target` clears it, so the
    /// text/markdown renderer only scrolls once per change instead of
    /// fighting the user's own manual scrolling every frame.
    jump_requested: bool,
}

impl FindInPreviewState {
    /// Resets all find state when the previewed file changes - a match
    /// range from the previous file is meaningless for a new one. Call once
    /// per frame before anything else touches this state.
    pub fn ensure_current_path(&mut self, path: &Path) {
        if self.current_path.as_deref() != Some(path) {
            self.current_path = Some(path.to_path_buf());
            self.active = false;
            self.query.clear();
            self.matches.clear();
            self.current = 0;
            self.jump_requested = false;
        }
    }

    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    pub fn current_match_number(&self) -> usize {
        self.current + 1
    }

    /// The current match's byte range, if there is one - unlike
    /// `take_jump_target`, this doesn't consume anything, so the renderer
    /// can call it every frame to keep the match highlighted (scrolling
    /// only needs to happen once per change, but the highlight itself
    /// should stay visible for as long as it's the active match).
    pub fn current_match_range(&self) -> Option<(usize, usize)> {
        self.matches.get(self.current).copied()
    }

    /// Re-runs the search against `haystack` (the previewed text, or the
    /// raw Markdown source) and jumps to the first match.
    pub fn recompute(&mut self, haystack: &str) {
        self.matches.clear();
        if !self.query.is_empty() {
            let query_lower = self.query.to_lowercase();
            let haystack_lower = haystack.to_lowercase();
            let mut start = 0;
            while let Some(found) = haystack_lower[start..].find(&query_lower) {
                let match_start = start + found;
                let match_end = match_start + query_lower.len();
                self.matches.push((match_start, match_end));
                start = match_end.max(match_start + 1);
            }
        }
        self.current = 0;
        self.jump_requested = !self.matches.is_empty();
    }

    pub fn next_match(&mut self) {
        if !self.matches.is_empty() {
            self.current = (self.current + 1) % self.matches.len();
            self.jump_requested = true;
        }
    }

    pub fn prev_match(&mut self) {
        if !self.matches.is_empty() {
            self.current = (self.current + self.matches.len() - 1) % self.matches.len();
            self.jump_requested = true;
        }
    }

    /// The current match's byte range, if a jump was requested this frame -
    /// clears the request so it only fires once.
    pub fn take_jump_target(&mut self) -> Option<(usize, usize)> {
        if self.jump_requested {
            self.jump_requested = false;
            self.matches.get(self.current).copied()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod find_in_preview_tests {
    use super::FindInPreviewState;
    use std::path::Path;

    #[test]
    fn recompute_finds_every_case_insensitive_occurrence() {
        let mut state = FindInPreviewState::default();
        state.query = "cat".to_string();
        state.recompute("The cat sat on THE mat, next to another cat.");
        assert_eq!(state.match_count(), 2);
    }

    #[test]
    fn recompute_with_empty_query_clears_matches() {
        let mut state = FindInPreviewState::default();
        state.query = String::new();
        state.recompute("anything at all");
        assert_eq!(state.match_count(), 0);
    }

    #[test]
    fn next_and_prev_wrap_around() {
        let mut state = FindInPreviewState::default();
        state.query = "a".to_string();
        state.recompute("a b a b a");
        assert_eq!(state.match_count(), 3);
        assert_eq!(state.current_match_number(), 1);

        state.next_match();
        assert_eq!(state.current_match_number(), 2);
        state.next_match();
        state.next_match();
        assert_eq!(state.current_match_number(), 1); // wrapped forward

        state.prev_match();
        assert_eq!(state.current_match_number(), 3); // wrapped backward
    }

    #[test]
    fn take_jump_target_fires_once_per_change() {
        let mut state = FindInPreviewState::default();
        state.query = "b".to_string();
        state.recompute("a b c");
        assert_eq!(state.take_jump_target(), Some((2, 3)));
        // Same match, no new change - shouldn't jump again on its own.
        assert_eq!(state.take_jump_target(), None);

        state.next_match();
        assert!(state.take_jump_target().is_some());
    }

    #[test]
    fn ensure_current_path_resets_state_on_file_change() {
        let mut state = FindInPreviewState::default();
        state.ensure_current_path(Path::new("C:/a.txt")); // first selection
        state.query = "x".to_string();
        state.recompute("x marks the spot");
        state.active = true;

        state.ensure_current_path(Path::new("C:/a.txt")); // re-selecting the same file
        assert_eq!(state.match_count(), 1); // untouched

        state.ensure_current_path(Path::new("C:/b.txt")); // switched to a different file
        assert!(!state.active);
        assert_eq!(state.query, "");
        assert_eq!(state.match_count(), 0);
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct FavoriteItem {
    pub path: PathBuf,
    pub label: String,
    /// A Phosphor glyph name overriding the real shell icon this favorite
    /// would otherwise show (see `FAVORITE_ICON_CHOICES`). Ignored when
    /// `custom_icon_file` is set. `None` (with `custom_icon_file` also
    /// `None`) means use the folder's real icon, same as before this field
    /// existed.
    #[serde(default)]
    pub custom_icon: Option<String>,
    /// A user-browsed image file (.ico, .png, .jpg, ...) whose own pixel
    /// content overrides the icon shown for this favorite, taking priority
    /// over both `custom_icon` and the folder's real icon.
    #[serde(default)]
    pub custom_icon_file: Option<PathBuf>,
}

#[derive(Default)]
pub struct SidebarAction {
    pub nav_to: Option<PathBuf>,
    pub open_new_tab: Option<PathBuf>,
    pub remove_favorite: Option<PathBuf>,
    pub select_favorite: Option<PathBuf>,
    pub reorder: Option<(usize, usize)>, // from_idx, to_idx
    pub move_files_to_sidebar_dir: Option<PathBuf>,
    pub open_network_browser: bool,
    pub open_settings: bool,
    /// A sidebar tag was clicked: open (or focus, if already open) the
    /// dedicated tab showing that tag group's tagged items.
    pub open_tag_view: Option<u64>,
    /// A saved search was clicked (or "Open in new tab" chosen): reopen it
    /// as a normal search tab, same as re-typing that query.
    pub open_saved_search: Option<u64>,
    pub remove_saved_search: Option<u64>,
    /// "Remove from Recent" was chosen from a recent location's context
    /// menu - navigating to one doesn't need its own action, it already
    /// reuses `nav_to` (a recent location is just a real path).
    pub remove_recent_location: Option<PathBuf>,
    /// "Clear All" was chosen from the Recent Locations section header's
    /// context menu.
    pub clear_recent_locations: bool,
}

#[derive(Default)]
pub struct TopbarAction {
    pub toggle_theme: bool,
    pub open_settings: bool,
    pub about: bool,
    pub exit: bool,
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
    pub show_checkboxes: bool,
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
    pub column_state: TagColumnState,
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
            column_state: TagColumnState::default(),
        }
    }
}

/// A user-saved search: a name plus the exact query/scope needed to reopen
/// it via `search_view_path`/`open_or_focus_search_tab` - the same sentinel-
/// path mechanism a live search already uses, so reopening one is just
/// re-running that search, not a separate code path.
#[derive(Clone)]
pub struct SavedSearch {
    pub id: u64,
    pub name: String,
    pub query: String,
    pub scope_folder: Option<PathBuf>,
}

pub struct SavedSearchRenameState {
    pub id: u64,
    pub buffer: String,
    pub should_focus: bool,
}

/// Saved searches accumulate one click at a time (unlike Favorites, which
/// requires deliberately dragging a folder in) - capped so the sidebar list
/// can't grow unbounded from casual use.
pub const MAX_SAVED_SEARCHES: usize = 50;

pub struct SavedSearchesState {
    pub items: Vec<SavedSearch>,
    pub next_id: u64,
    pub rename_state: Option<SavedSearchRenameState>,
}

impl Default for SavedSearchesState {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            next_id: 1,
            rename_state: None,
        }
    }
}

impl SavedSearchesState {
    pub fn from_snapshot(snapshot: crate::core::indexer::SavedSearchesSnapshot) -> Self {
        Self {
            items: snapshot
                .items
                .into_iter()
                .map(|item| SavedSearch {
                    id: item.id,
                    name: item.name,
                    query: item.query,
                    scope_folder: item.scope_folder,
                })
                .collect(),
            next_id: snapshot.next_id.max(1),
            rename_state: None,
        }
    }

    pub fn to_snapshot(&self) -> crate::core::indexer::SavedSearchesSnapshot {
        crate::core::indexer::SavedSearchesSnapshot {
            version: 1,
            next_id: self.next_id.max(1),
            items: self
                .items
                .iter()
                .map(|item| crate::core::indexer::SavedSearchSnapshot {
                    id: item.id,
                    name: item.name.clone(),
                    query: item.query.clone(),
                    scope_folder: item.scope_folder.clone(),
                })
                .collect(),
        }
    }

    /// Adds a new saved search using the query text itself as the default
    /// name (renamable afterward via the sidebar's context menu) - matching
    /// Favorites' pattern of no name-prompt-on-add, just a sensible default.
    /// Returns `false` (without adding anything) once at `MAX_SAVED_SEARCHES`.
    pub fn add(&mut self, query: String, scope_folder: Option<PathBuf>) -> bool {
        if self.items.len() >= MAX_SAVED_SEARCHES {
            return false;
        }
        let id = self.next_id.max(1);
        self.next_id = id.saturating_add(1);
        self.items.push(SavedSearch {
            id,
            name: query.clone(),
            query,
            scope_folder,
        });
        true
    }

    pub fn remove(&mut self, id: u64) {
        self.items.retain(|item| item.id != id);
        if self.rename_state.as_ref().is_some_and(|r| r.id == id) {
            self.rename_state = None;
        }
    }
}

/// Recent locations accumulate on every real-folder navigation (see
/// `MainWindow::load_view_with_fallback`'s normal-folder branch in
/// `mainwindow_imp.rs`) - capped so casual browsing can't grow the list
/// unbounded. Unlike `SavedSearch`, an entry has no user-editable name or
/// id: its identity and display label are both just its path.
pub const MAX_RECENT_LOCATIONS: usize = 20;

pub struct RecentLocationsState {
    /// Most-recently-visited first. Re-visiting a path already in the list
    /// moves it to the front instead of duplicating it.
    pub items: Vec<PathBuf>,
}

impl Default for RecentLocationsState {
    fn default() -> Self {
        Self { items: Vec::new() }
    }
}

impl RecentLocationsState {
    pub fn from_snapshot(snapshot: crate::core::indexer::RecentLocationsSnapshot) -> Self {
        Self {
            items: snapshot.items,
        }
    }

    pub fn to_snapshot(&self) -> crate::core::indexer::RecentLocationsSnapshot {
        crate::core::indexer::RecentLocationsSnapshot {
            version: 1,
            items: self.items.clone(),
        }
    }

    pub fn record_visit(&mut self, path: PathBuf) {
        self.items.retain(|p| p != &path);
        self.items.insert(0, path);
        self.items.truncate(MAX_RECENT_LOCATIONS);
    }

    pub fn remove(&mut self, path: &Path) {
        self.items.retain(|p| p != path);
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

/// Columns shown in the "tagged items" table for one tag group (see
/// `draw_tag_view`). Kept separate from `ItemViewerHeaderColumn` since this
/// table is a virtual list (not a real directory listing) with its own,
/// smaller column set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TagColumn {
    Name,
    Type,
    Location,
    Size,
    Modified,
}

impl TagColumn {
    pub fn index(self) -> usize {
        match self {
            TagColumn::Name => 0,
            TagColumn::Type => 1,
            TagColumn::Location => 2,
            TagColumn::Size => 3,
            TagColumn::Modified => 4,
        }
    }

    pub fn i18n_key(self) -> &'static str {
        match self {
            TagColumn::Name => "explorer_cols_name",
            TagColumn::Type => "explorer_cols_type",
            TagColumn::Location => "explorer_cols_location",
            TagColumn::Size => "explorer_cols_size",
            TagColumn::Modified => "explorer_cols_modified",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagColumnFitRequest {
    Column(TagColumn),
    All,
}

#[derive(Clone, Debug)]
pub struct TagColumnState {
    pub order: Vec<TagColumn>,
    widths: [f32; 5],
    pub pending_fit_request: Option<TagColumnFitRequest>,
    /// Same render-time-driven "fit once, on first appearance" trick used by
    /// the main item viewer's `ItemViewerColumnState` - see its doc comment.
    pub auto_fit_checked: bool,
    pub layout_generation: u64,
}

impl Default for TagColumnState {
    fn default() -> Self {
        Self {
            order: vec![
                TagColumn::Name,
                TagColumn::Type,
                TagColumn::Location,
                TagColumn::Size,
                TagColumn::Modified,
            ],
            widths: [260.0, 110.0, 150.0, 90.0, 150.0],
            pending_fit_request: None,
            auto_fit_checked: false,
            layout_generation: 0,
        }
    }
}

impl TagColumnState {
    pub fn width(&self, column: TagColumn) -> f32 {
        self.widths[column.index()]
    }

    pub fn set_width(&mut self, column: TagColumn, width: f32) {
        self.widths[column.index()] = width;
    }

    pub fn move_left(&mut self, column: TagColumn) {
        if let Some(idx) = self.order.iter().position(|c| *c == column)
            && idx > 0
        {
            self.order.swap(idx, idx - 1);
        }
    }

    pub fn move_right(&mut self, column: TagColumn) {
        if let Some(idx) = self.order.iter().position(|c| *c == column)
            && idx + 1 < self.order.len()
        {
            self.order.swap(idx, idx + 1);
        }
    }

    pub fn move_to_start(&mut self, column: TagColumn) {
        if let Some(idx) = self.order.iter().position(|c| *c == column)
            && idx > 0
        {
            let column = self.order.remove(idx);
            self.order.insert(0, column);
        }
    }

    pub fn move_to_end(&mut self, column: TagColumn) {
        if let Some(idx) = self.order.iter().position(|c| *c == column)
            && idx + 1 < self.order.len()
        {
            let column = self.order.remove(idx);
            self.order.push(column);
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
            column_state: TagColumnState::default(),
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

    /// Bulk toggle used by the multi-select tag picker: if every path in
    /// `paths` is already in the group, removes them all; otherwise adds
    /// whichever ones are missing. Treating a multi-selection as one unit
    /// (rather than toggling each path independently) matches how
    /// `create_group_and_add` already treats a multi-path selection.
    pub fn toggle_group_for_paths(&mut self, group_id: u64, paths: &[PathBuf]) -> bool {
        let Some(target_index) = self.groups.iter().position(|group| group.id == group_id) else {
            return false;
        };

        let mut paths: Vec<PathBuf> = paths.iter().cloned().collect();
        paths.sort();
        paths.dedup();
        if paths.is_empty() {
            return false;
        }

        let target_group = &mut self.groups[target_index];
        let all_tagged = paths.iter().all(|p| target_group.items.contains(p));

        if all_tagged {
            let remove: HashSet<PathBuf> = paths.into_iter().collect();
            let before = target_group.items.len();
            target_group.items.retain(|item| !remove.contains(item));
            target_group.items.len() != before
        } else {
            let mut changed = false;
            for path in paths {
                if !target_group.items.contains(&path) {
                    target_group.items.push(path);
                    changed = true;
                }
            }
            changed
        }
    }

    /// All tags currently on `path`, in `self.groups` order - used by the
    /// Tags column and the picker's "already tagged" checked-state.
    pub fn tags_for_path(&self, path: &Path) -> Vec<(u64, String, egui::Color32)> {
        self.groups
            .iter()
            .filter(|group| group.items.iter().any(|item| item == path))
            .map(|group| (group.id, group.name.clone(), group.color))
            .collect()
    }

    /// Removes `paths` from one specific group only, used by "Remove Tag"
    /// invoked while browsing inside that group's own folder view - unlike
    /// `remove_paths`, which clears every tag a path has.
    pub fn remove_paths_from_group(&mut self, group_id: u64, paths: &[PathBuf]) -> bool {
        let Some(group) = self.groups.iter_mut().find(|group| group.id == group_id) else {
            return false;
        };
        let paths: HashSet<PathBuf> = paths.iter().cloned().collect();
        let before = group.items.len();
        group.items.retain(|item| !paths.contains(item));
        group.items.len() != before
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

        for path in paths {
            if !group.items.contains(&path) {
                group.items.push(path);
            }
        }

        self.groups.push(group);
        true
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
    pub ordered_columns: Vec<ItemViewerHeaderColumn>,
    pub name_width: f32,
    pub type_width: f32,
    pub size_width: f32,
    pub usage_width: f32,
    pub modified_width: f32,
    pub created_width: f32,
    pub deleted_width: f32,
    pub original_directory_width: f32,
    pub tags_width: f32,
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
    pub tags_width: f32,
}

#[cfg(test)]
mod saved_searches_tests {
    use super::{SavedSearchesState, MAX_SAVED_SEARCHES};

    #[test]
    fn add_uses_query_as_default_name_and_assigns_increasing_ids() {
        let mut state = SavedSearchesState::default();
        assert!(state.add("*.png".to_string(), None));
        assert!(state.add("report ext:pdf".to_string(), Some("D:\\Docs".into())));
        assert_eq!(state.items.len(), 2);
        assert_eq!(state.items[0].name, "*.png");
        assert_eq!(state.items[0].query, "*.png");
        assert_eq!(state.items[0].scope_folder, None);
        assert_eq!(state.items[1].scope_folder, Some("D:\\Docs".into()));
        assert_ne!(state.items[0].id, state.items[1].id);
    }

    #[test]
    fn add_refuses_past_the_cap() {
        let mut state = SavedSearchesState::default();
        for i in 0..MAX_SAVED_SEARCHES {
            assert!(state.add(format!("query{i}"), None));
        }
        assert_eq!(state.items.len(), MAX_SAVED_SEARCHES);
        assert!(!state.add("one_too_many".to_string(), None));
        assert_eq!(state.items.len(), MAX_SAVED_SEARCHES);
    }

    #[test]
    fn remove_drops_the_item_and_clears_a_matching_rename_state() {
        let mut state = SavedSearchesState::default();
        state.add("a".to_string(), None);
        state.add("b".to_string(), None);
        let id_to_remove = state.items[0].id;
        state.rename_state = Some(super::SavedSearchRenameState {
            id: id_to_remove,
            buffer: "a".to_string(),
            should_focus: false,
        });

        state.remove(id_to_remove);

        assert_eq!(state.items.len(), 1);
        assert!(state.items.iter().all(|item| item.id != id_to_remove));
        assert!(state.rename_state.is_none());
    }

    #[test]
    fn snapshot_round_trip_preserves_items_and_next_id() {
        let mut state = SavedSearchesState::default();
        state.add("*.md".to_string(), Some("D:\\Notes".into()));
        let snapshot = state.to_snapshot();

        let restored = SavedSearchesState::from_snapshot(snapshot);

        assert_eq!(restored.items.len(), 1);
        assert_eq!(restored.items[0].query, "*.md");
        assert_eq!(restored.items[0].scope_folder, Some("D:\\Notes".into()));
        assert_eq!(restored.next_id, state.next_id);
    }
}

#[cfg(test)]
mod recent_locations_tests {
    use super::RecentLocationsState;
    use std::path::PathBuf;

    #[test]
    fn record_visit_puts_newest_first() {
        let mut state = RecentLocationsState::default();
        state.record_visit(PathBuf::from("C:/a"));
        state.record_visit(PathBuf::from("C:/b"));
        state.record_visit(PathBuf::from("C:/c"));

        assert_eq!(
            state.items,
            vec![
                PathBuf::from("C:/c"),
                PathBuf::from("C:/b"),
                PathBuf::from("C:/a"),
            ]
        );
    }

    #[test]
    fn revisiting_a_path_moves_it_to_front_instead_of_duplicating() {
        let mut state = RecentLocationsState::default();
        state.record_visit(PathBuf::from("C:/a"));
        state.record_visit(PathBuf::from("C:/b"));
        state.record_visit(PathBuf::from("C:/a"));

        assert_eq!(state.items, vec![PathBuf::from("C:/a"), PathBuf::from("C:/b")]);
    }

    #[test]
    fn list_is_capped_at_max_recent_locations() {
        let mut state = RecentLocationsState::default();
        for i in 0..super::MAX_RECENT_LOCATIONS + 5 {
            state.record_visit(PathBuf::from(format!("C:/dir{i}")));
        }
        assert_eq!(state.items.len(), super::MAX_RECENT_LOCATIONS);
        // Most recent (highest i) should still be at the front.
        assert_eq!(
            state.items[0],
            PathBuf::from(format!("C:/dir{}", super::MAX_RECENT_LOCATIONS + 4))
        );
    }

    #[test]
    fn remove_drops_the_matching_path_only() {
        let mut state = RecentLocationsState::default();
        state.record_visit(PathBuf::from("C:/a"));
        state.record_visit(PathBuf::from("C:/b"));

        state.remove(&PathBuf::from("C:/a"));

        assert_eq!(state.items, vec![PathBuf::from("C:/b")]);
    }

    #[test]
    fn snapshot_round_trip_preserves_order() {
        let mut state = RecentLocationsState::default();
        state.record_visit(PathBuf::from("C:/a"));
        state.record_visit(PathBuf::from("C:/b"));

        let restored = RecentLocationsState::from_snapshot(state.to_snapshot());
        assert_eq!(restored.items, state.items);
    }

    #[test]
    fn clear_empties_the_list() {
        let mut state = RecentLocationsState::default();
        state.record_visit(PathBuf::from("C:/a"));
        state.record_visit(PathBuf::from("C:/b"));

        state.clear();

        assert!(state.items.is_empty());
    }
}

#[cfg(test)]
mod tags_state_tests {
    use super::TagsState;
    use eframe::egui::Color32;
    use std::path::PathBuf;

    fn add_group(state: &mut TagsState, name: &str, color: Color32, paths: &[PathBuf]) -> u64 {
        assert!(state.create_group_and_add(name.to_string(), color, paths));
        state.groups.last().unwrap().id
    }

    #[test]
    fn a_path_can_belong_to_more_than_one_group() {
        let mut state = TagsState::default();
        let path = PathBuf::from("C:/file.txt");
        let group_a = add_group(&mut state, "A", Color32::RED, &[path.clone()]);
        let group_b = add_group(&mut state, "B", Color32::BLUE, &[path.clone()]);

        assert!(
            state
                .groups
                .iter()
                .find(|g| g.id == group_a)
                .unwrap()
                .items
                .contains(&path)
        );
        assert!(
            state
                .groups
                .iter()
                .find(|g| g.id == group_b)
                .unwrap()
                .items
                .contains(&path)
        );
    }

    #[test]
    fn toggle_group_for_paths_adds_when_not_all_tagged_and_removes_when_all_tagged() {
        let mut state = TagsState::default();
        let path = PathBuf::from("C:/file.txt");
        let group_id = add_group(&mut state, "A", Color32::RED, &[]);

        assert!(state.toggle_group_for_paths(group_id, &[path.clone()]));
        assert!(
            state
                .groups
                .iter()
                .find(|g| g.id == group_id)
                .unwrap()
                .items
                .contains(&path)
        );

        assert!(state.toggle_group_for_paths(group_id, &[path.clone()]));
        assert!(
            !state
                .groups
                .iter()
                .find(|g| g.id == group_id)
                .unwrap()
                .items
                .contains(&path)
        );
    }

    #[test]
    fn tags_for_path_returns_every_matching_group_in_order() {
        let mut state = TagsState::default();
        let path = PathBuf::from("C:/file.txt");
        let group_a = add_group(&mut state, "A", Color32::RED, &[path.clone()]);
        let group_b = add_group(&mut state, "B", Color32::BLUE, &[path.clone()]);

        let tags = state.tags_for_path(&path);

        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].0, group_a);
        assert_eq!(tags[1].0, group_b);
    }

    #[test]
    fn remove_paths_from_group_only_affects_that_group() {
        let mut state = TagsState::default();
        let path = PathBuf::from("C:/file.txt");
        let group_a = add_group(&mut state, "A", Color32::RED, &[path.clone()]);
        let group_b = add_group(&mut state, "B", Color32::BLUE, &[path.clone()]);

        assert!(state.remove_paths_from_group(group_a, &[path.clone()]));

        assert!(
            !state
                .groups
                .iter()
                .find(|g| g.id == group_a)
                .unwrap()
                .items
                .contains(&path)
        );
        assert!(
            state
                .groups
                .iter()
                .find(|g| g.id == group_b)
                .unwrap()
                .items
                .contains(&path)
        );
    }
}
