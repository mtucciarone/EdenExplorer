use crate::core::drives::{get_drive_infos, is_raw_physical_drive_path};
use crate::core::everything::{SearchScope, check_everything_available, search_everything_async};
use crate::core::fs::{FileItem, get_shell_item_metadata};
use crate::core::fs::{
    MY_RECYCLE_BIN_PATH, SETTINGS_PATH, parallel_directory_scan, parse_search_view_path,
    parse_tag_view_path, scan_dir_async, search_view_path, tag_view_path,
};
use crate::core::indexer::{
    DirectorySettingsSnapshot, SETTINGS_EXPORT_FORMAT_VERSION, SettingsExportBundle,
    load_app_settings, save_app_settings, save_favorites, save_tags, save_theme_settings,
};
use crate::gui::MainWindow;
use crate::gui::theme::{
    ThemeMode, ThemePalette, apply_font_to_context, get_default_palette, set_palette,
};
use crate::gui::utils::{
    SortColumn, SortKey, clear_clipboard_files, get_clipboard_files, is_clipboard_cut,
    set_clipboard_files, shell_delete_to_recycle_bin, show_copy_move_dialog, sort_files_by_keys,
};
use crate::gui::windows::about::draw_about_window;
use crate::gui::windows::containers::enums::{
    ItemViewerAction, ItemViewerContextAction, ItemViewerHeaderColumn, ItemViewerNavAction,
};
use crate::gui::windows::containers::itemviewer_navbar::open_default_terminal;
use crate::gui::windows::containers::structs::{
    FavoriteItem, FilterState, GalleryThumbnailSize, ItemViewerColumnFitRequest,
    ItemViewerColumnState, ItemViewerDisplayMode, ItemViewerFolderSizeState,
    ItemViewerNavBarAction, RenameState, SidebarAction, SplitSide, TabState, TabView, TabsAction,
    TagsState, TopbarAction,
};
use crate::gui::windows::enums::{SettingsAction, ThemeCustomizerAction};
use crate::gui::windows::structs::{AppSettings, Navigation};
use crate::gui::windows::windowsoverrides::mark_clipboard_dirty;
use crate::gui::windows::windowsoverrides::toggle_window_fullscreen;
use crossbeam_channel::Receiver;
use crossbeam_channel::{Sender, unbounded};
use eframe::egui;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    IShellItem, SEE_MASK_INVOKEIDLIST, SHELLEXECUTEINFOW, ShellExecuteExW, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{Error, HRESULT};

use windows::{Win32::System::Com::*, Win32::UI::Shell::*, core::*};

pub(crate) fn default_column_state(settings: &AppSettings) -> ItemViewerColumnState {
    ItemViewerColumnState::from_orders(
        settings.item_viewer_file_column_order.clone(),
        settings.item_viewer_drive_column_order.clone(),
        settings.recycle_bin_column_order.clone(),
        settings.item_viewer_file_column_sizes.clone(),
        settings.item_viewer_drive_column_sizes.clone(),
        settings.recycle_bin_column_sizes.clone(),
    )
}

pub(crate) fn default_directory_settings_snapshot(directory: PathBuf) -> DirectorySettingsSnapshot {
    let default_settings = AppSettings::default();

    DirectorySettingsSnapshot {
        directory,
        item_viewer_file_column_order: default_settings.item_viewer_file_column_order,
        item_viewer_drive_column_order: default_settings.item_viewer_drive_column_order,
        recycle_bin_column_order: default_settings.recycle_bin_column_order,
        item_viewer_file_column_sizes: default_settings.item_viewer_file_column_sizes,
        item_viewer_drive_column_sizes: default_settings.item_viewer_drive_column_sizes,
        recycle_bin_column_sizes: default_settings.recycle_bin_column_sizes,
        filter_query: String::new(),
        display_mode: ItemViewerDisplayMode::Details,
        gallery_thumbnail_size: GalleryThumbnailSize::Medium,
        sort_column: SortColumn::Name,
        sort_ascending: true,
        sort_keys: vec![SortKey {
            column: SortColumn::Name,
            ascending: true,
        }],
    }
}

pub(crate) fn directory_settings_snapshot_for_view(view: &TabView) -> DirectorySettingsSnapshot {
    DirectorySettingsSnapshot {
        directory: view.nav.current.clone(),
        item_viewer_file_column_order: view.column_state.file_column_order.clone(),
        item_viewer_drive_column_order: view.column_state.drive_column_order.clone(),
        recycle_bin_column_order: view.column_state.recycle_bin_column_order.clone(),
        item_viewer_file_column_sizes: view.column_state.file_column_sizes.clone(),
        item_viewer_drive_column_sizes: view.column_state.drive_column_sizes.clone(),
        recycle_bin_column_sizes: view.column_state.recycle_bin_column_sizes.clone(),
        filter_query: view.item_viewer_filter_state.query.clone(),
        display_mode: view.display_mode,
        gallery_thumbnail_size: view.gallery_state.thumbnail_size,
        sort_column: view.sort_column,
        sort_ascending: view.sort_ascending,
        sort_keys: view.sort_keys.clone(),
    }
}

/// Controls what `apply_directory_settings_to_view` does with `view.display_mode`
/// when the folder being navigated to has no saved per-folder preference of its
/// own.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DisplayModeFallback {
    /// Always fall back to `settings.default_display_mode`. Used for every
    /// navigation trigger except drilling into a folder from within the
    /// currently active view (new tab/split, breadcrumb, sidebar, tabs,
    /// back/forward/up, address bar, favorites, session restore, ...): every
    /// folder's display mode is either its own explicit saved choice or the
    /// app-wide default, full stop.
    Default,
    /// Keep the *current* display mode instead of falling back, but only when
    /// it's Columns or ColumnPreview - those are multi-pane "drill in"
    /// browsers where clicking a folder in one pane opens it in the next pane
    /// of the *same* browser rather than replacing it, so browsing deeper
    /// into folders that have never had a view explicitly chosen for them
    /// shouldn't keep kicking the user back out to Details. Used only by the
    /// "open this folder in the current view" action (double-click, Enter,
    /// single-click-to-drill-in in Columns).
    PreserveColumnsDrillIn,
}

/// Applies the per-directory settings for `view.nav.current` (if any exist) to
/// `view`. A folder that *does* have an explicit saved preference always wins;
/// `fallback` only controls what happens when it doesn't - see
/// `DisplayModeFallback`.
pub(crate) fn apply_directory_settings_to_view(
    view: &mut TabView,
    settings: &AppSettings,
    fallback: DisplayModeFallback,
) {
    let sticky_display_mode = fallback == DisplayModeFallback::PreserveColumnsDrillIn
        && matches!(
            view.display_mode,
            ItemViewerDisplayMode::Columns | ItemViewerDisplayMode::ColumnPreview
        );

    let snapshot = settings
        .directory_settings
        .iter()
        .find(|entry| entry.directory == view.nav.current);

    if let Some(snapshot) = snapshot {
        view.column_state = ItemViewerColumnState::from_orders(
            snapshot.item_viewer_file_column_order.clone(),
            snapshot.item_viewer_drive_column_order.clone(),
            snapshot.recycle_bin_column_order.clone(),
            snapshot.item_viewer_file_column_sizes.clone(),
            snapshot.item_viewer_drive_column_sizes.clone(),
            snapshot.recycle_bin_column_sizes.clone(),
        );
        view.item_viewer_filter_state = FilterState {
            active: !snapshot.filter_query.is_empty(),
            query: snapshot.filter_query.clone(),
            ..Default::default()
        };
        if !sticky_display_mode {
            view.display_mode = snapshot.display_mode;
        }
        view.gallery_state
            .set_thumbnail_size(snapshot.gallery_thumbnail_size);
        view.sort_keys = if snapshot.sort_keys.is_empty() {
            vec![SortKey {
                column: snapshot.sort_column,
                ascending: snapshot.sort_ascending,
            }]
        } else {
            snapshot.sort_keys.clone()
        };
        let primary = view.sort_keys[0];
        view.sort_column = primary.column;
        view.sort_ascending = primary.ascending;
    } else {
        view.column_state = default_column_state(settings);
        view.item_viewer_filter_state = FilterState::default();
        if !sticky_display_mode {
            view.display_mode = settings.default_display_mode;
        }
        view.gallery_state = Default::default();
        view.sort_keys = vec![SortKey {
            column: settings.sort_column,
            ascending: settings.sort_ascending,
        }];
        view.sort_column = settings.sort_column;
        view.sort_ascending = settings.sort_ascending;
    }
}

pub(crate) fn persist_directory_settings_snapshot(
    entries: &mut Vec<DirectorySettingsSnapshot>,
    snapshot: DirectorySettingsSnapshot,
) -> bool {
    let default_snapshot = default_directory_settings_snapshot(snapshot.directory.clone());

    if snapshot == default_snapshot {
        if let Some(pos) = entries
            .iter()
            .position(|entry| entry.directory == snapshot.directory)
        {
            entries.remove(pos);
            return true;
        }
        return false;
    }

    if let Some(pos) = entries
        .iter()
        .position(|entry| entry.directory == snapshot.directory)
    {
        if entries[pos] == snapshot {
            return false;
        }
        entries[pos] = snapshot;
        true
    } else {
        entries.push(snapshot);
        true
    }
}

/// A short, human-readable label for a destination folder shown in a
/// notification row - the folder's own name, or the full path for a drive
/// root (which has no file name component).
fn path_display_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

/// A single reversible action, recorded on `MainWindow::undo_stack`/
/// `redo_stack` after it completes successfully. Every variant is
/// symmetric: undo walks it one direction, redo walks the exact same data
/// the other direction - see `MainWindow::undo`/`redo`.
///
/// Deliberately not persisted anywhere: an entry is tied to specific paths
/// at a specific moment, and if the app restarted and the user touched
/// those files through Explorer or another program meanwhile, replaying a
/// stale undo could silently act on the wrong current file. Every
/// mainstream file manager drops undo history on restart for this exact
/// reason - this stays purely in-memory.
#[derive(Clone)]
pub enum UndoableOperation {
    Rename { old_path: PathBuf, new_path: PathBuf },
    BulkRename { pairs: Vec<(PathBuf, PathBuf)> },
    /// `pairs` is `(original_path, path_after_the_move)`. `side` is the pane
    /// the move happened in, so undo/redo can restore the right pane's
    /// selection/refresh the right view. `replaced` maps a final path (the
    /// second element of a `pairs` entry) to the pidl of whatever item was
    /// recycled to make room for it, for any pair whose destination
    /// collision was resolved via Replace - see `find_recycled_pidl`. Undo
    /// restores that item from the Recycle Bin after moving the incoming
    /// item away; redo re-recycles whatever's there fresh (producing a new
    /// pidl, which replaces this map's entry) before placing the incoming
    /// item again.
    Move {
        pairs: Vec<(PathBuf, PathBuf)>,
        side: SplitSide,
        replaced: HashMap<PathBuf, Vec<u8>>,
    },
    /// `pairs` is `(source_path, path_the_copy_was_created_at)`. Undo
    /// deletes the created copies (to the Recycle Bin, so it stays
    /// further-recoverable) and restores anything `replaced` (see `Move`'s
    /// doc comment - same meaning here); redo re-copies the sources,
    /// re-recycling+restoring-pidl for any replaced destination first.
    Copy {
        pairs: Vec<(PathBuf, PathBuf)>,
        side: SplitSide,
        replaced: HashMap<PathBuf, Vec<u8>>,
    },
}

/// Cap on `MainWindow::undo_stack`/`redo_stack` (each) - matches
/// `MAX_SAVED_SEARCHES`'s precedent for "generous but bounded" per-session
/// state that isn't persisted to disk.
const MAX_UNDO_STACK: usize = 50;

/// Metadata for a paste (or cut-move) running via the robocopy-backed
/// engine (see `core::robocopy`) - kept in `MainWindow::pending_robocopy_pastes`,
/// keyed by the notification id `paste_clipboard_native` created for it, so
/// `poll_pending_paste` can apply the right side effects (tag remapping,
/// selecting the pasted items, refreshing the view) once that specific
/// operation reaches a terminal state.
pub struct PendingPaste {
    target_dir: PathBuf,
    before_entries: HashSet<PathBuf>,
    paths: Vec<PathBuf>,
    is_cut: bool,
    side: SplitSide,
    /// Same map `start_robocopy_paste` received to build the robocopy jobs -
    /// kept around (rather than discarded after the jobs are built) so
    /// `poll_pending_paste` can compute each source's *exact* final name for
    /// Undo/Redo bookkeeping once the job completes; a source not in this
    /// map kept its own name.
    renames: HashMap<PathBuf, String>,
    /// A final path (in `target_dir`) to the pidl of whatever item was
    /// recycled to make room for it - populated only when this job's
    /// conflicts were resolved via Replace (see `handle_paste_conflict_
    /// resolution`). Threaded into a freshly-pushed `UndoableOperation::
    /// Move`/`Copy`'s own `replaced` field once this job completes; unused
    /// (always empty) for jobs `undo()`/`redo()` themselves start, since
    /// those manage replacement pidls directly on the `UndoableOperation`
    /// they're reversing/reapplying instead.
    replaced: HashMap<PathBuf, Vec<u8>>,
    /// Whether this job is a normal user-initiated paste/drag-drop, or is
    /// itself the mechanism undoing/redoing a previous Move/Copy - see
    /// `PasteOrigin`.
    origin: PasteOrigin,
}

/// What triggered a `PendingPaste` job - determines which stack
/// `poll_pending_paste` pushes the resulting `UndoableOperation` onto once
/// the job completes (see that function's doc comment for the full table).
pub enum PasteOrigin {
    /// A normal paste or drag-and-drop initiated directly by the user.
    UserAction,
    /// This job *is* `MainWindow::undo()` reversing a previously-pushed
    /// Move/Copy - carries the original operation (to push onto
    /// `redo_stack` unchanged, no re-deriving needed) and a shared group
    /// tracker, since undoing a Move whose sources came from more than one
    /// original directory fires one job per distinct directory - the
    /// operation is only pushed to `redo_stack` once every sibling job in
    /// the group has finished, and only if all of them succeeded.
    UndoOf(UndoableOperation, std::rc::Rc<UndoRedoGroup>),
    /// This job *is* `MainWindow::redo()` re-applying a previously-undone
    /// Move/Copy - same shape as `UndoOf`, pushing back onto `undo_stack`
    /// instead. Redo is always a single job (one destination folder), so
    /// the group here always has exactly one member - kept as a group
    /// anyway so `poll_pending_paste` has one shared code path.
    RedoOf(UndoableOperation, std::rc::Rc<UndoRedoGroup>),
}

/// Shared completion state for the one-or-more `start_robocopy_paste` jobs
/// that together make up a single undo or redo of a Move (see `PasteOrigin`).
/// `poll_pending_paste` decrements `remaining` as each sibling job reaches a
/// terminal state and clears `all_succeeded` if any of them failed; the
/// reversed/reapplied `UndoableOperation` is only pushed onto the opposite
/// stack once `remaining` hits zero and `all_succeeded` is still true - a
/// partial failure leaves nothing pushed, since the operation no longer
/// accurately describes the current state either way.
pub struct UndoRedoGroup {
    remaining: std::cell::Cell<usize>,
    all_succeeded: std::cell::Cell<bool>,
}

impl UndoRedoGroup {
    fn new(job_count: usize) -> Self {
        Self {
            remaining: std::cell::Cell::new(job_count),
            all_succeeded: std::cell::Cell::new(true),
        }
    }

    /// Records one sibling job's terminal state; returns `true` exactly
    /// once, when this was the last sibling to finish and every one of
    /// them succeeded.
    fn record_completion(&self, succeeded: bool) -> bool {
        if !succeeded {
            self.all_succeeded.set(false);
        }
        let remaining = self.remaining.get().saturating_sub(1);
        self.remaining.set(remaining);
        remaining == 0 && self.all_succeeded.get()
    }
}

/// The user's choice in the paste-conflict modal - see
/// `PasteConflictPrompt` and `MainWindow::handle_paste_conflict_resolution`.
/// "Cancel" isn't a variant here since it's handled inline by the modal
/// (just clears `pending_paste_conflict`, nothing to resolve).
#[derive(Clone, Copy)]
pub enum PasteConflictAction {
    Replace,
    Skip,
    Rename,
}

/// A paste `paste_clipboard_native` held back because one or more source
/// items share a name with something already in the destination - robocopy
/// has no interactive prompt of its own (unlike `IFileOperation`, which
/// shows the native "This destination already has a file named..." dialog),
/// so this is EdenExplorer's own equivalent. Resolved by the user picking
/// Replace/Skip/Rename/Cancel in the modal drawn from this state (see
/// `mainwindow.rs`'s update loop); Replace/Skip/Rename all then call
/// `paste_clipboard_native`'s resume half with the resolved path list.
pub struct PasteConflictPrompt {
    pub paths: Vec<PathBuf>,
    pub target_dir: PathBuf,
    pub before_entries: HashSet<PathBuf>,
    pub is_cut: bool,
    pub side: SplitSide,
    /// File/folder names that collided, for display in the modal - not
    /// necessarily every one of `paths`, just the ones that already exist
    /// in `target_dir`.
    pub conflicting_names: Vec<String>,
}

/// State for the "Checksums" modal - held in `MainWindow::pending_checksum`
/// while the dialog is open, from the moment the context-menu entry is
/// clicked until the user closes it. `rx` is drained by `poll_pending_checksum`
/// once per frame; `results`/`error` are populated exactly once, whichever
/// the background job (`core::checksum::compute_checksums_async`) reports.
pub struct ChecksumDialogState {
    pub file_name: String,
    pub size_label: String,
    pub rx: Option<
        crossbeam_channel::Receiver<std::result::Result<crate::core::checksum::ChecksumResults, String>>,
    >,
    pub results: Option<crate::core::checksum::ChecksumResults>,
    pub error: Option<String>,
    /// User-pasted hash to verify against `results` - compared
    /// case-insensitively against all four algorithms so the user doesn't
    /// need to know which one a downloaded file's published checksum is.
    pub compare_input: String,
}

impl Drop for MainWindow {
    fn drop(&mut self) {
        self.cleanup_resources();
    }
}

impl MainWindow {
    /// Comprehensive validation for submitted filenames
    /// Used when user submits/commits the filename (Enter, create new file/folder)
    pub(crate) fn is_submitted_filename_valid(name: &str) -> bool {
        if name.is_empty() {
            return false;
        }

        if name.len() > 255 {
            return false;
        }

        // Cannot be "." or ".."
        if name == "." || name == ".." {
            return false;
        }

        // Cannot start or end with space
        if name.starts_with(' ') || name.ends_with(' ') {
            return false;
        }

        // Cannot end with dot
        if name.ends_with('.') {
            return false;
        }

        // Invalid characters
        let invalid_chars = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
        if name.chars().any(|c| invalid_chars.contains(&c)) {
            return false;
        }

        // Control characters (0x00–0x1F)
        if name.chars().any(|c| c < '\u{20}') {
            return false;
        }

        // Reserved names (check base name before extension)
        let reserved_names = [
            "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
            "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
        ];

        let base = name.split('.').next().unwrap_or("");
        let base_upper = base.to_uppercase();

        if reserved_names.contains(&base_upper.as_str()) {
            return false;
        }

        // Optional: max length
        if name.len() > 255 {
            return false;
        }

        true
    }

    pub fn current_nav(&self) -> &Navigation {
        &self.active_tab().view(self.focused_split).nav
    }

    pub fn current_nav_mut(&mut self) -> &mut Navigation {
        let side = self.focused_split;
        &mut self.active_tab_mut().view_mut(side).nav
    }

    pub fn open_new_tab(&mut self, path: PathBuf) {
        let nav = Navigation::new(path);
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let (sort_column, sort_ascending) = {
            let view = self.active_tab().view(self.focused_split);
            (view.sort_column, view.sort_ascending)
        };
        self.tabs
            .push(TabState::new(id, nav, sort_column, sort_ascending));
        let current_settings = self.settings_window.current_settings.clone();
        apply_directory_settings_to_view(
            &mut self.tabs.last_mut().unwrap().primary_view,
            &current_settings,
            DisplayModeFallback::Default,
        );
        self.active_tab = self.tabs.len() - 1;
        self.mark_tab_infos_dirty();
    }

    /// Switches to the Settings tab if one is already open, otherwise opens a new
    /// one - Settings is a singleton tab rather than something you can duplicate.
    pub fn open_or_focus_settings_tab(&mut self) {
        if let Some(idx) = self
            .tabs
            .iter()
            .position(|t| t.primary_view.nav.current.to_string_lossy() == SETTINGS_PATH)
        {
            self.active_tab = idx;
            self.focused_split = SplitSide::Primary;
            self.pending_tab_scroll_id = Some(self.tabs[idx].id);
            self.mark_tab_infos_dirty();
            return;
        }

        self.open_new_tab(PathBuf::from(SETTINGS_PATH));
        self.pending_tab_scroll_id = self.tabs.last().map(|t| t.id);
        self.load_path();
    }

    /// Opens the (single, shared) tag-browser tab showing `group_id`'s tagged
    /// items, reusing an already-open tag-browser tab (switching what tag it
    /// shows) rather than stacking a new tab per tag clicked.
    pub fn open_or_focus_tag_view_tab(&mut self, group_id: u64) {
        let tag_path = tag_view_path(group_id);

        if let Some(idx) = self
            .tabs
            .iter()
            .position(|t| parse_tag_view_path(&t.primary_view.nav.current).is_some())
        {
            self.active_tab = idx;
            self.focused_split = SplitSide::Primary;
            self.pending_tab_scroll_id = Some(self.tabs[idx].id);
            if self.tabs[idx].primary_view.nav.current != tag_path {
                self.tabs[idx].primary_view.nav.go_to(tag_path);
            }
            self.mark_tab_infos_dirty();
            self.load_path();
            return;
        }

        self.open_new_tab(tag_path);
        self.pending_tab_scroll_id = self.tabs.last().map(|t| t.id);
        self.load_path();
    }

    /// Opens a new tab showing Everything search results for `query`/`scope`.
    /// Unlike the tag-view/settings singletons, this always opens a fresh
    /// tab rather than reusing an existing search tab - a user may want to
    /// compare two different queries side by side, or keep an old search
    /// open while refining a new one, so there's no "one true active
    /// search" the way there's one tag group being browsed at a time.
    pub fn open_or_focus_search_tab(&mut self, query: String, scope: SearchScope) {
        let scope_folder = match &scope {
            SearchScope::CurrentFolder(dir) => Some(dir.as_path()),
            SearchScope::Everywhere => None,
        };
        let search_path = search_view_path(&query, scope_folder);
        self.open_new_tab(search_path);
        self.pending_tab_scroll_id = self.tabs.last().map(|t| t.id);
        self.load_path();
    }

    pub fn open_startup_paths(&mut self, paths: &[PathBuf]) {
        let Some(first_path) = paths.first() else {
            return;
        };

        let pinned_count = self.settings_window.current_settings.pinned_tabs.len();
        if pinned_count == 0 {
            self.tabs[0].primary_view.nav = Navigation::new(first_path.clone());
            self.active_tab = 0;
            self.focused_split = SplitSide::Primary;
            self.load_path();
        } else {
            self.open_new_tab(first_path.clone());
            self.load_path();
        }

        for path in paths.iter().skip(1) {
            self.open_new_tab(path.clone());
        }

        if pinned_count > 0 {
            self.active_tab = pinned_count;
            self.focused_split = SplitSide::Primary;
            self.pending_tab_scroll_id = self.tabs.get(self.active_tab).map(|tab| tab.id);
            self.load_path();
        } else {
            self.active_tab = 0;
            self.focused_split = SplitSide::Primary;
            self.pending_tab_scroll_id = self.tabs.first().map(|tab| tab.id);
        }
        self.mark_tab_infos_dirty();
    }

    pub fn default_favorites(&self) -> Vec<FavoriteItem> {
        let mut favorites = Vec::new();
        if let Some(home) = dirs::home_dir() {
            let desktop = home.join("Desktop");
            favorites.push(FavoriteItem {
                path: desktop,
                label: "Desktop".to_string(),
                custom_icon: None,
                custom_icon_file: None,
            });
            let documents = home.join("Documents");
            favorites.push(FavoriteItem {
                path: documents,
                label: "Documents".to_string(),
                custom_icon: None,
                custom_icon_file: None,
            });
            let downloads = home.join("Downloads");
            favorites.push(FavoriteItem {
                path: downloads,
                label: "Downloads".to_string(),
                custom_icon: None,
                custom_icon_file: None,
            });
            let pictures = home.join("Pictures");
            favorites.push(FavoriteItem {
                path: pictures,
                label: "Pictures".to_string(),
                custom_icon: None,
                custom_icon_file: None,
            });
        }
        favorites
    }

    pub fn toggle_sort(&mut self, col: SortColumn, additive: bool, remove: bool) {
        let side = self.focused_split;
        let sort_keys = {
            let view = self.active_tab_mut().view_mut(side);
            if remove {
                view.sort_keys.retain(|key| key.column != col);
                if view.sort_keys.is_empty() {
                    view.sort_keys.push(SortKey {
                        column: SortColumn::Name,
                        ascending: true,
                    });
                }
            } else if additive {
                if let Some(key) = view.sort_keys.iter_mut().find(|key| key.column == col) {
                    key.ascending = !key.ascending;
                } else {
                    view.sort_keys.push(SortKey {
                        column: col,
                        ascending: true,
                    });
                }
            } else {
                let ascending = view
                    .sort_keys
                    .first()
                    .filter(|key| key.column == col)
                    .map(|key| !key.ascending)
                    .unwrap_or(true);
                view.sort_keys = vec![SortKey {
                    column: col,
                    ascending,
                }];
            }
            let primary = view.sort_keys[0];
            view.sort_column = primary.column;
            view.sort_ascending = primary.ascending;
            view.sort_keys.clone()
        };

        sort_files_by_keys(&mut self.active_tab_mut().view_mut(side).files, &sort_keys);

        let snapshot = directory_settings_snapshot_for_view(self.active_tab().view(side));
        let _ = persist_directory_settings_snapshot(
            &mut self.settings_window.current_settings.directory_settings,
            snapshot,
        );

        self.save_app_settings_to_disk();
    }

    fn save_app_settings_to_disk(&self) {
        save_app_settings(
            self.settings_window
                .current_settings
                .folder_scanning_enabled,
            self.settings_window
                .current_settings
                .show_hidden_files_folders,
            self.settings_window.current_settings.show_item_viewer_icons,
            self.settings_window
                .current_settings
                .windows_context_menu_enabled,
            &self.settings_window.current_settings.window_size_mode,
            &self.settings_window.current_settings.start_path,
            Some(match self.theme {
                ThemeMode::Dark => "dark",
                ThemeMode::Light => "light",
            }),
            &self.settings_window.current_settings.pinned_tabs,
            self.settings_window.current_settings.time_format_24h,
            self.settings_window.current_settings.sort_column,
            self.settings_window.current_settings.sort_ascending,
            &self.settings_window.current_settings.language,
            self.settings_window.current_settings.date_style,
            &self.settings_window.current_settings.custom_date_format,
            &self
                .settings_window
                .current_settings
                .item_viewer_file_column_order,
            &self
                .settings_window
                .current_settings
                .item_viewer_drive_column_order,
            &self
                .settings_window
                .current_settings
                .recycle_bin_column_order,
            &self
                .settings_window
                .current_settings
                .item_viewer_file_column_sizes,
            &self
                .settings_window
                .current_settings
                .item_viewer_drive_column_sizes,
            &self
                .settings_window
                .current_settings
                .recycle_bin_column_sizes,
            &self.settings_window.current_settings.directory_settings,
            self.settings_window
                .current_settings
                .double_click_navigates_up,
            self.settings_window
                .current_settings
                .show_selection_checkboxes,
            self.settings_window
                .current_settings
                .middle_click_opens_new_tab,
            self.settings_window
                .current_settings
                .restore_last_session_tabs,
            self.settings_window.current_settings.default_display_mode,
            self.settings_window.current_settings.default_search_scope,
            self.settings_window.current_settings.search_engine,
        );
        crate::core::context_menu_settings::save_custom_context_menu(
            &self.settings_window.current_settings.custom_context_menu,
        );
        crate::core::tab_groups::save_tab_groups(&self.settings_window.current_settings.tab_groups);
    }

    fn apply_item_viewer_column_order(
        &mut self,
        side: SplitSide,
        is_drive_view: bool,
        is_recycle_bin_view: bool,
        order: &[ItemViewerHeaderColumn],
    ) {
        let view = self.active_tab_mut().view_mut(side);
        let current_order = view
            .column_state
            .order_mut(is_drive_view, is_recycle_bin_view);
        current_order.clear();
        current_order.extend_from_slice(order);
        view.column_state.layout_generation = view.column_state.layout_generation.wrapping_add(1);
    }

    pub fn load_path(&mut self) {
        self.load_view(self.focused_split);
    }

    /// Like `load_path`, but lets the caller control what happens to the
    /// display mode when the current folder has no saved preference of its
    /// own - see `DisplayModeFallback`. Only the "open this folder in the
    /// current view" action needs anything other than the default (strict,
    /// non-sticky) behavior that `load_path`/`load_view` use.
    pub(crate) fn load_path_with_fallback(&mut self, fallback: DisplayModeFallback) {
        self.load_view_with_fallback(self.focused_split, fallback);
    }

    pub(crate) fn load_view(&mut self, side: SplitSide) {
        self.load_view_with_fallback(side, DisplayModeFallback::Default);
    }

    fn load_view_with_fallback(&mut self, side: SplitSide, fallback: DisplayModeFallback) {
        let (current_path, is_root) = {
            let view = self.active_tab().view(side);
            (view.nav.current.clone(), view.nav.is_root())
        };

        let current_settings = self.settings_window.current_settings.clone();

        {
            let view = self.active_tab_mut().view_mut(side);
            apply_directory_settings_to_view(view, &current_settings, fallback);
            view.files.clear();
            view.rx = None;
            view.size_req_tx = None;
            view.size_rx = None;
            view.pending_size_queue.clear();
            view.pending_size_set.clear();
            view.is_loading = false;
            view.explorer_state.selected_paths.clear();
            view.explorer_state.selection_anchor = None;
            view.explorer_state.selection_focus = None;
            view.item_viewer_filter_state.dirty = true;
            view.item_viewer_filter_state.cached_indices.clear();
            view.columns_view_state.needs_reload = true;
        }
        self.folder_sizes.clear();
        self.file_size_text_cache.clear();
        self.folder_size_text_cache.clear();
        self.drive_size_text_cache.clear();

        if is_raw_physical_drive_path(&current_path) {
            self.active_tab_mut()
                .view_mut(side)
                .explorer_state
                .non_ntfs_popup_path = Some(current_path.clone());
            return;
        }

        if current_path.to_string_lossy() == MY_RECYCLE_BIN_PATH {
            self.load_recycle_bin_view(side);
            return;
        }

        if current_path.to_string_lossy() == SETTINGS_PATH {
            // The Settings tab has no file listing of its own; its content is drawn
            // directly by `draw_tab_content` in place of the normal item viewer.
            return;
        }

        if parse_tag_view_path(&current_path).is_some() {
            // Likewise, a tag-view tab's list is built straight from the tag
            // group's items each frame, not from a filesystem scan.
            return;
        }

        if let Some((query, scope_folder)) = parse_search_view_path(&current_path) {
            // Everything is preferred (near-instant, index-backed) when the
            // setting asks for it and it's actually reachable - but silently
            // fall back to the built-in filesystem walk rather than erroring
            // when it isn't, so a machine that's never had Everything
            // installed just gets a (slower, always-available) search
            // instead of a dead end.
            let use_everything = matches!(
                self.settings_window.current_settings.search_engine,
                crate::core::everything::SearchEngine::Everything
            ) && check_everything_available();

            let (tx, rx) = unbounded();
            if use_everything {
                let scope = match scope_folder {
                    Some(dir) => SearchScope::CurrentFolder(dir),
                    None => SearchScope::Everywhere,
                };
                search_everything_async(
                    query,
                    scope,
                    tx,
                    self.settings_window.current_settings.date_style,
                    self.settings_window.current_settings.time_format_24h,
                    self.settings_window.current_settings.custom_date_format.clone(),
                );
            } else {
                crate::core::fs::search_builtin_async(
                    query,
                    scope_folder,
                    tx,
                    self.settings_window.current_settings.date_style,
                    self.settings_window.current_settings.time_format_24h,
                    self.settings_window.current_settings.custom_date_format.clone(),
                );
            }
            let view = self.active_tab_mut().view_mut(side);
            view.rx = Some(rx);
            view.is_loading = true;
            return;
        }

        if is_root {
            let view = self.active_tab_mut().view_mut(side);
            for d in get_drive_infos() {
                let label = d.display;
                let path = d.path;
                if let (Some(total), Some(free)) = (d.total_space, d.free_space) {
                    view.files.push(FileItem::with_drive_info(
                        label, path, true, false, None, None, None, None, None, None, None, None,
                        None, total, free,
                    ));
                } else {
                    view.files.push(FileItem::new(
                        label, path, true, false, None, None, None, None, None, None, None, None,
                        None,
                    ));
                }
            }

            // "This PC" always lists drives in drive-letter order by default,
            // regardless of whatever sort a regular folder view was left on
            // (sorting by the "Name" column would otherwise order by volume
            // label - e.g. "DATA (D:\)" before "MAIN (C:\)" - rather than by
            // drive letter). Clicking a column header can still re-sort it
            // from there like any other view.
            view.files.sort_by(|a, b| a.path.cmp(&b.path));
            return;
        }

        // A genuine real-folder load (every virtual location above has
        // already returned) - record it for the "Recent Locations" sidebar
        // section, unless it's already pinned to Favorites (that would just
        // be clutter - a folder you deliberately pinned doesn't also need
        // to show up as "recent"). Re-recording the same folder on every
        // refresh/re-visit is harmless: `record_visit` already dedupes by
        // moving the existing entry to the front instead of duplicating it.
        if !self
            .sidebar_state
            .favorites
            .iter()
            .any(|f| f.path == current_path)
        {
            self.recent_locations_state.record_visit(current_path.clone());
            self.persist_recent_locations();
        }

        // Async directory listing
        let (tx, rx) = unbounded();
        scan_dir_async(
            current_path,
            tx,
            self.settings_window.current_settings.date_style,
            self.settings_window.current_settings.time_format_24h,
            self.settings_window.current_settings.custom_date_format.clone(),
        );
        let folder_scanning_enabled = self
            .settings_window
            .current_settings
            .folder_scanning_enabled;
        let shutdown = Arc::clone(&self.shutdown);
        let view = self.active_tab_mut().view_mut(side);
        view.rx = Some(rx);
        view.is_loading = true;

        // Setup folder size calculation channels only if folder scanning is enabled
        if folder_scanning_enabled {
            let (size_req_tx, size_req_rx) = unbounded::<PathBuf>();
            let (size_done_tx, size_done_rx) = unbounded::<(PathBuf, u64, bool)>();
            view.size_req_tx = Some(size_req_tx);
            view.size_rx = Some(size_done_rx);

            // Spawn a thread pool to handle folder size requests in parallel
            let num_threads = num_cpus::get().max(2); // use all available cores
            view.size_threads =
                calculate_folder_sizes_parallel(size_req_rx, size_done_tx, shutdown, num_threads);
        }
    }

    pub fn load_recycle_bin_view(&mut self, side: SplitSide) {
        use windows::Win32::System::SystemServices::{SFGAO_FOLDER, SFGAO_HIDDEN};
        use windows::Win32::UI::Shell::{
            BHID_EnumItems, FOLDERID_RecycleBinFolder, IEnumShellItems, ILFree, ILGetSize,
            IShellItem, SHCreateItemFromIDList, SHGetIDListFromObject, SHGetKnownFolderIDList,
            SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_FILESYSPATH, SIGDN_NORMALDISPLAY,
        };

        let mut recycle_items = Vec::new();

        unsafe {
            let recycle_pidl = match SHGetKnownFolderIDList(&FOLDERID_RecycleBinFolder, 0, None) {
                Ok(pidl) => pidl,
                Err(err) => {
                    eprintln!("Failed to resolve Recycle Bin: {:?}", err);
                    return;
                }
            };

            let recycle_item: IShellItem = match SHCreateItemFromIDList(recycle_pidl) {
                Ok(item) => item,
                Err(err) => {
                    eprintln!("Failed to open Recycle Bin shell item: {:?}", err);
                    CoTaskMemFree(Some(recycle_pidl as _));
                    return;
                }
            };

            let enum_items: IEnumShellItems =
                match recycle_item.BindToHandler(None, &BHID_EnumItems) {
                    Ok(items) => items,
                    Err(err) => {
                        eprintln!("Failed to enumerate Recycle Bin items: {:?}", err);
                        CoTaskMemFree(Some(recycle_pidl as _));
                        return;
                    }
                };

            loop {
                let mut fetched_items: [Option<IShellItem>; 1] = [None];
                if let Err(err) = enum_items.Next(&mut fetched_items, None) {
                    eprintln!("Failed to read Recycle Bin item: {:?}", err);
                    break;
                }

                if fetched_items[0].is_none() {
                    break;
                }

                let Some(item) = fetched_items[0].take() else {
                    continue;
                };

                let recycle_bin_pidl = match SHGetIDListFromObject(&item) {
                    Ok(pidl) => {
                        let pidl_size = ILGetSize(Some(pidl as _)) as usize;
                        let mut pidl_bytes = vec![0u8; pidl_size];
                        std::ptr::copy_nonoverlapping(
                            pidl as *const u8,
                            pidl_bytes.as_mut_ptr(),
                            pidl_size,
                        );
                        ILFree(Some(pidl as _));
                        Some(pidl_bytes)
                    }
                    Err(err) => {
                        eprintln!("Failed to capture Recycle Bin PIDL: {:?}", err);
                        None
                    }
                };

                let display_name = item
                    .GetDisplayName(SIGDN_NORMALDISPLAY)
                    .ok()
                    .and_then(|name| {
                        let text = pwstr_to_string(name);
                        CoTaskMemFree(Some(name.0 as _));
                        if text.is_empty() { None } else { Some(text) }
                    })
                    .unwrap_or_else(|| "Unknown".to_string());

                let path = item
                    .GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING)
                    .or_else(|_| item.GetDisplayName(SIGDN_FILESYSPATH))
                    .ok()
                    .map(|name| {
                        let text = pwstr_to_string(name);
                        CoTaskMemFree(Some(name.0 as _));
                        text
                    });

                let Some(path) = path.map(PathBuf::from) else {
                    continue;
                };

                let attrs = item.GetAttributes(SFGAO_FOLDER | SFGAO_HIDDEN).ok();
                let is_dir = attrs.is_some_and(|flags| flags.contains(SFGAO_FOLDER));
                let is_hidden = attrs.is_some_and(|flags| flags.contains(SFGAO_HIDDEN));

                let (
                    file_size,
                    modified_time,
                    created_time,
                    deleted_time,
                    modified_time_raw,
                    created_time_raw,
                    deleted_time_raw,
                    original_object_name,
                    original_directory,
                ) = get_shell_item_metadata(
                    &item,
                    self.settings_window.current_settings.date_style,
                    self.settings_window.current_settings.time_format_24h,
                    &self.settings_window.current_settings.custom_date_format,
                );

                recycle_items.push(FileItem::new(
                    original_object_name.unwrap_or(display_name),
                    path,
                    is_dir,
                    is_hidden,
                    recycle_bin_pidl,
                    file_size,
                    modified_time,
                    created_time,
                    deleted_time,
                    modified_time_raw,
                    created_time_raw,
                    deleted_time_raw,
                    original_directory,
                ));
            }

            CoTaskMemFree(Some(recycle_pidl as _));
        }

        let view = self.active_tab_mut().view_mut(side);
        view.files = recycle_items;
        sort_files_by_keys(&mut view.files, &view.sort_keys);
    }

    pub fn create_new_folder(&mut self) {
        if self.current_nav().is_root()
            || self.current_nav().is_recycle_bin()
            || self.current_nav().is_tag_view()
        {
            return;
        }

        let base = self.current_nav().current.clone();
        let mut name = "New Folder".to_string();
        let mut counter = 1;
        let mut path = base.join(&name);

        // Find a valid, non-existent name
        while path.exists() {
            counter += 1;
            name = format!("New Folder ({})", counter);
            path = base.join(&name);
        }

        // Validate the final name before creating
        if Self::is_submitted_filename_valid(&name) {
            if std::fs::create_dir(&path).is_ok() {
                let path_for_selection = path.clone();
                self.load_path();
                // Immediately start renaming the new folder
                self.rename_state = Some(RenameState {
                    path: path.clone(),
                    new_name: name,
                    should_focus: true,
                    validation_error_show: false,
                });
                let side = self.focused_split;
                self.active_tab_mut()
                    .view_mut(side)
                    .explorer_state
                    .pending_selection_paths = Some(vec![path_for_selection]);
            }
        } else {
            // Don't create the folder, just start rename with invalid name so user can fix it
            self.rename_state = Some(RenameState {
                path: base.clone(), // Use parent directory as the path since we're not creating yet
                new_name: name,
                should_focus: true,
                validation_error_show: true, // Show error immediately
            });
        }
    }

    pub fn create_new_file(&mut self) {
        if self.current_nav().is_root()
            || self.current_nav().is_recycle_bin()
            || self.current_nav().is_tag_view()
        {
            return;
        }

        let base = self.current_nav().current.clone();
        let mut name = "New File.txt".to_string();
        let mut counter = 1;
        let mut path = base.join(&name);

        // Find a valid, non-existent name
        while path.exists() {
            counter += 1;
            name = format!("New File ({}).txt", counter);
            path = base.join(&name);
        }

        // Validate the final name before creating
        if Self::is_submitted_filename_valid(&name) {
            // Create an empty file
            if std::fs::write(&path, "").is_ok() {
                let path_for_selection = path.clone();
                self.load_path();
                // Immediately start renaming the new file
                self.rename_state = Some(RenameState {
                    path: path.clone(),
                    new_name: name,
                    should_focus: true,
                    validation_error_show: false,
                });
                let side = self.focused_split;
                self.active_tab_mut()
                    .view_mut(side)
                    .explorer_state
                    .pending_selection_paths = Some(vec![path_for_selection]);
            }
        } else {
            // Don't create the file, just start rename with invalid name so user can fix it
            self.rename_state = Some(RenameState {
                path: base.clone(), // Use parent directory as the path since we're not creating yet
                new_name: name,
                should_focus: true,
                validation_error_show: true, // Show error immediately
            });
        }
    }

    pub fn add_favorite(&mut self) {
        if self.current_nav().is_root()
            || self.current_nav().is_recycle_bin()
            || self.current_nav().is_tag_view()
        {
            return;
        }

        let path = self.current_nav().current.clone();
        self.add_favorite_path(path);
    }

    /// Adds an arbitrary folder to the sidebar Favorites list (e.g. from a right-click
    /// in the item viewer, rather than the currently open directory). No-op if it's
    /// already favorited.
    pub fn add_favorite_path(&mut self, path: PathBuf) {
        if self
            .sidebar_state
            .favorites
            .iter()
            .any(|fav| fav.path == path)
        {
            return;
        }

        let label = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());

        self.sidebar_state.favorites.push(FavoriteItem {
            path,
            label,
            custom_icon: None,
                custom_icon_file: None,
        });
        self.persist_favorites();
    }

    pub fn remove_favorite(&mut self, path: &PathBuf) {
        self.sidebar_state.favorites.retain(|fav| &fav.path != path);
        self.persist_favorites();
        if self
            .sidebar_state
            .item_clicked
            .as_ref()
            .map(|p| p == path)
            .unwrap_or(false)
        {
            self.sidebar_state.item_clicked = None;
        }
    }

    pub fn persist_favorites(&self) {
        save_favorites('C', &self.sidebar_state.favorites);
    }

    pub fn persist_tags(&self) {
        save_tags(&self.tags_state.to_snapshot());
    }

    pub fn persist_saved_searches(&self) {
        crate::core::indexer::save_saved_searches(&self.saved_searches_state.to_snapshot());
    }

    pub fn persist_recent_locations(&self) {
        crate::core::indexer::save_recent_locations(&self.recent_locations_state.to_snapshot());
    }

    fn move_tagged_paths_to_dir(&mut self, sources: &[PathBuf], target_dir: &Path) -> bool {
        let mut changed = false;

        for source in sources {
            if let Some(file_name) = source.file_name() {
                let new_path = target_dir.join(file_name);
                changed |= self.tags_state.remap_path_prefix(source, &new_path);
            }
        }

        changed
    }

    pub fn handle_context_action(&mut self, action: ItemViewerContextAction) {
        match action {
            ItemViewerContextAction::Cut(paths) => {
                let _ = set_clipboard_files(&paths, true);
                mark_clipboard_dirty();
                if let Some(first) = paths.first() {
                    let side = self.focused_split;
                    let explorer_state = &mut self.active_tab_mut().view_mut(side).explorer_state;
                    explorer_state.selected_paths.clear();
                    explorer_state.selected_paths.insert(first.clone());
                }
            }
            ItemViewerContextAction::Copy(paths) => {
                let _ = set_clipboard_files(&paths, false);
                mark_clipboard_dirty();
            }
            ItemViewerContextAction::CopyPath(paths) => {
                use crate::gui::utils::copy_text_to_clipboard;

                // Convert paths to strings and join with newlines
                let path_strings: Vec<String> =
                    paths.iter().map(|p| p.display().to_string()).collect();

                let text = path_strings.join("\r\n");
                let _ = copy_text_to_clipboard(&text);
            }
            ItemViewerContextAction::Paste => {
                self.paste_clipboard_native();
            }
            ItemViewerContextAction::Restore(paths) => {
                let recycle_bin_pidls: Vec<Vec<u8>> = {
                    let view = self.active_tab().view(self.focused_split);
                    paths
                        .iter()
                        .filter_map(|path| {
                            view.files
                                .iter()
                                .find(|item| &item.path == path)
                                .and_then(|item| item.recycle_bin_pidl.clone())
                        })
                        .collect()
                };

                if let Err(e) = self.restore_paths_native(recycle_bin_pidls) {
                    eprintln!("Recycle bin restore failed: {:?}", e);
                }

                self.load_path();
            }
            ItemViewerContextAction::AddTag(paths) => {
                self.tags_state.open_picker(paths);
            }
            ItemViewerContextAction::AddFavorite(paths) => {
                for path in paths {
                    self.add_favorite_path(path);
                }
            }
            ItemViewerContextAction::RemoveTag(paths) => {
                if self.tags_state.remove_paths(&paths) {
                    self.persist_tags();
                }
            }
            ItemViewerContextAction::RemoveTagFromGroup(group_id, paths) => {
                if self.tags_state.remove_paths_from_group(group_id, &paths) {
                    self.persist_tags();
                }
            }
            ItemViewerContextAction::Compress(paths) => {
                if let Some(dest_zip) = crate::core::compress::compress_target_path(&paths) {
                    let notification_id = self.notifications_state.start_operation(
                        crate::gui::windows::containers::notifications::FileOpKind::Compress,
                        paths.len(),
                        path_display_label(&dest_zip),
                    );
                    let (tx, rx) = crossbeam_channel::unbounded();
                    crate::core::compress::compress_paths_async(paths, dest_zip.clone(), tx);
                    self.pending_compress_jobs
                        .insert(notification_id, (rx, dest_zip));
                }
            }
            ItemViewerContextAction::RenameRequest(path, new_name) => {
                let trimmed = new_name.trim();

                if trimmed.is_empty() {
                    self.rename_state = None;
                    return;
                }

                // Validate the new name for Windows filename rules
                if !Self::is_submitted_filename_valid(trimmed) {
                    // Don't cancel the rename, keep user in input state with error shown
                    if let Some(rename_state) = &mut self.rename_state {
                        rename_state.new_name = trimmed.to_string();
                        rename_state.should_focus = true;
                        rename_state.validation_error_show = true;
                    }
                    return;
                }

                if let Some(parent) = path.parent() {
                    let target = parent.join(trimmed);

                    // Avoid no-op rename
                    if path != target {
                        let renamed_ok = match Self::rename_one_native(&path, trimmed) {
                            Ok(()) => true,
                            Err(e) => {
                                eprintln!("Native rename failed: {:?}", e);
                                false
                            }
                        };

                        // Queue the renamed file for auto-selection after refresh
                        let side = self.focused_split;
                        self.active_tab_mut()
                            .view_mut(side)
                            .explorer_state
                            .pending_selection_paths = Some(vec![target.clone()]);

                        if self.tags_state.remap_path_prefix(path.as_path(), &target) {
                            self.persist_tags();
                        }

                        if renamed_ok {
                            self.push_undo(UndoableOperation::Rename {
                                old_path: path.clone(),
                                new_path: target.clone(),
                            });
                        }
                    }
                }

                self.rename_state = None;
                self.load_path();
            }

            ItemViewerContextAction::RenameCancel => {
                self.rename_state = None;
            }
            ItemViewerContextAction::BulkRenameRequest(paths) => {
                self.pending_bulk_rename =
                    Some(crate::gui::windows::containers::bulk_rename::BulkRenameState::new(paths));
            }
            ItemViewerContextAction::BulkRenameCommit(renames) => {
                let attempted = renames;
                let mut succeeded: Vec<(PathBuf, PathBuf)> = Vec::new();
                let mut needs_fallback: Vec<(PathBuf, String)> = Vec::new();

                match self.rename_paths_native(attempted.clone()) {
                    Ok(targets) => {
                        let target_map: std::collections::HashMap<PathBuf, PathBuf> =
                            targets.into_iter().collect();
                        for (path, new_name) in &attempted {
                            match target_map.get(path) {
                                Some(target) if target.exists() => {
                                    succeeded.push((path.clone(), target.clone()));
                                }
                                _ => needs_fallback.push((path.clone(), new_name.clone())),
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Native bulk rename failed: {:?}", e);
                        needs_fallback = attempted.clone();
                    }
                }

                let mut still_failed = 0usize;
                for (path, new_name) in needs_fallback {
                    if Self::rename_one_native(&path, &new_name).is_ok() {
                        if let Some(parent) = path.parent() {
                            succeeded.push((path.clone(), parent.join(&new_name)));
                        }
                    } else {
                        still_failed += 1;
                    }
                }

                let status = if still_failed == 0 {
                    crate::gui::windows::containers::notifications::FileOpStatus::Completed
                } else {
                    crate::gui::windows::containers::notifications::FileOpStatus::Failed
                };
                self.notifications_state.record_finished(
                    crate::gui::windows::containers::notifications::FileOpKind::Rename,
                    attempted.len(),
                    String::new(),
                    status,
                );

                let mut tags_changed = false;
                for (old, new) in &succeeded {
                    tags_changed |= self.tags_state.remap_path_prefix(old, new);
                }
                if tags_changed {
                    self.persist_tags();
                }

                let side = self.focused_split;
                self.active_tab_mut()
                    .view_mut(side)
                    .explorer_state
                    .pending_selection_paths =
                    Some(succeeded.iter().map(|(_, new)| new.clone()).collect());

                if !succeeded.is_empty() {
                    self.push_undo(UndoableOperation::BulkRename { pairs: succeeded });
                }

                self.load_path();
            }
            ItemViewerContextAction::Delete(paths, permanent) => {
                let allow_undo = !permanent && !self.current_nav().is_recycle_bin();
                let destination_label = if allow_undo {
                    self.i18n.tr("recycle_bin")
                } else {
                    String::new()
                };
                let delete_status = if let Err(e) = self.delete_paths_native(paths.clone(), allow_undo, false) {
                    eprintln!("Native delete failed: {:?}", e);

                    // fallback (rare, but safe)
                    for path in &paths {
                        self.delete_path(path);
                    }
                    crate::gui::windows::containers::notifications::FileOpStatus::Failed
                } else {
                    crate::gui::windows::containers::notifications::FileOpStatus::Completed
                };
                self.notifications_state.record_finished(
                    crate::gui::windows::containers::notifications::FileOpKind::Delete,
                    paths.len(),
                    destination_label,
                    delete_status,
                );

                let mut tags_changed = false;
                for path in &paths {
                    tags_changed |= self.tags_state.remove_path_prefix(path);
                }

                if tags_changed {
                    self.persist_tags();
                }

                self.load_path();
            }
            ItemViewerContextAction::Properties(paths) => {
                self.open_properties_multi(&paths);
            }
            ItemViewerContextAction::Checksum(path) => {
                let file_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string());
                let size_label = std::fs::metadata(&path)
                    .map(|m| crate::core::utils::files::format_size(m.len()))
                    .unwrap_or_default();

                let (tx, rx) = crossbeam_channel::unbounded();
                crate::core::checksum::compute_checksums_async(path, tx);

                self.pending_checksum = Some(ChecksumDialogState {
                    file_name,
                    size_label,
                    rx: Some(rx),
                    results: None,
                    error: None,
                    compare_input: String::new(),
                });
            }
        }
    }

    /// Drains the checksum background job's result once it's ready - called
    /// once per frame from the main update loop, same shape as
    /// `poll_pending_compress`. A job with no result yet (still hashing) is
    /// left alone for the next frame to check again.
    pub fn poll_pending_checksum(&mut self) {
        let Some(state) = &mut self.pending_checksum else {
            return;
        };
        let Some(rx) = &state.rx else {
            return;
        };
        if let Ok(result) = rx.try_recv() {
            match result {
                Ok(results) => state.results = Some(results),
                Err(e) => state.error = Some(e),
            }
            state.rx = None;
        }
    }

    /// Kicks off a paste (or cut-move) using the robocopy-backed engine
    /// (see `core::robocopy`) - or, if any source item shares a name with
    /// something already in the destination, holds it for confirmation
    /// instead (robocopy has no interactive overwrite prompt of its own;
    /// see `PasteConflictPrompt`'s doc comment). Multiple pastes can be in
    /// flight at once - each gets its own notification id and its own
    /// `PendingPaste` entry in `pending_robocopy_pastes`, so a second paste
    /// started while the first is still copying doesn't clobber it.
    pub fn paste_clipboard_native(&mut self) {
        let paths = match get_clipboard_files() {
            Some(p) if !p.is_empty() => p,
            _ => return,
        };

        if self.current_nav().is_recycle_bin() {
            return;
        }

        let is_cut = is_clipboard_cut();
        let target_dir = self.current_nav().current.clone();
        let before_entries = Self::directory_child_paths(&target_dir);
        let side = self.focused_split;

        let conflicting_names: Vec<String> = paths
            .iter()
            .filter_map(|p| {
                let name = p.file_name()?.to_string_lossy().to_string();
                target_dir.join(&name).exists().then_some(name)
            })
            .collect();

        if !conflicting_names.is_empty() {
            self.pending_paste_conflict = Some(PasteConflictPrompt {
                paths,
                target_dir,
                before_entries,
                is_cut,
                side,
                conflicting_names,
            });
            return;
        }

        self.start_robocopy_paste(
            paths,
            target_dir,
            before_entries,
            is_cut,
            side,
            HashMap::new(),
            HashMap::new(),
            PasteOrigin::UserAction,
        );
    }

    /// Actually kicks off the background robocopy job and its notification/
    /// bookkeeping entries - shared by the no-conflict fast path in
    /// `paste_clipboard_native` and by the Replace/Skip/Rename resolution of
    /// a `PasteConflictPrompt` (see `handle_paste_conflict_resolution`).
    /// `renames` maps a source path to the name it should be given in
    /// `target_dir` instead of its own name - see `core::robocopy::build_jobs`.
    /// `replaced` is `PendingPaste::replaced`'s doc comment - pass
    /// `HashMap::new()` unless this call is resolving a paste conflict via
    /// Replace.
    fn start_robocopy_paste(
        &mut self,
        paths: Vec<PathBuf>,
        target_dir: PathBuf,
        before_entries: HashSet<PathBuf>,
        is_cut: bool,
        side: SplitSide,
        renames: HashMap<PathBuf, String>,
        replaced: HashMap<PathBuf, Vec<u8>>,
        origin: PasteOrigin,
    ) {
        if paths.is_empty() {
            return;
        }

        let (jobs, total_bytes) =
            crate::core::robocopy::build_jobs(&paths, &target_dir, is_cut, &renames);

        let notification_id = self.notifications_state.start_operation(
            if is_cut {
                crate::gui::windows::containers::notifications::FileOpKind::Move
            } else {
                crate::gui::windows::containers::notifications::FileOpKind::Copy
            },
            paths.len(),
            path_display_label(&target_dir),
        );

        let handle = crate::core::robocopy::RobocopyHandle::start(jobs, total_bytes);
        self.notifications_state
            .attach_robocopy_job(notification_id, handle);

        self.pending_robocopy_pastes.insert(
            notification_id,
            PendingPaste {
                target_dir,
                before_entries,
                paths,
                is_cut,
                side,
                renames,
                replaced,
                origin,
            },
        );
    }

    /// Applies the user's Replace/Skip/Rename choice from the paste-conflict
    /// modal and starts the (possibly filtered/renamed) paste.
    /// `Skip` drops every source item whose name collided - if that empties
    /// the list entirely, `start_robocopy_paste` is a no-op, matching "Skip
    /// Existing" when literally everything was a duplicate. `Rename` keeps
    /// every item, computing a fresh `<name>-001`-style name (see
    /// `core::robocopy::next_available_name`) for each one that collided.
    pub fn handle_paste_conflict_resolution(&mut self, action: PasteConflictAction) {
        let Some(prompt) = self.pending_paste_conflict.take() else {
            return;
        };

        let is_conflicting = |p: &Path| {
            p.file_name()
                .map(|n| {
                    prompt
                        .conflicting_names
                        .iter()
                        .any(|c| c == n.to_string_lossy().as_ref())
                })
                .unwrap_or(false)
        };

        let mut renames = HashMap::new();
        let mut replaced: HashMap<PathBuf, Vec<u8>> = HashMap::new();
        let paths = match action {
            PasteConflictAction::Replace => {
                // Make Replace non-destructive: recycle the item that's
                // about to be overwritten first (instead of letting
                // robocopy overwrite it directly, which would be permanent
                // data loss with no undo), stashing its Recycle Bin pidl so
                // undo/redo can restore or re-recycle it later - see
                // `find_recycled_pidl`'s doc comment.
                for path in &prompt.paths {
                    if is_conflicting(path)
                        && let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string())
                    {
                        let existing_path = prompt.target_dir.join(&name);
                        if self
                            .delete_paths_native(vec![existing_path.clone()], true, true)
                            .is_ok()
                            && let Some(pidl) = self.find_recycled_pidl(&prompt.target_dir, &name)
                        {
                            replaced.insert(existing_path, pidl);
                        }
                    }
                }
                prompt.paths
            }
            PasteConflictAction::Skip => prompt
                .paths
                .into_iter()
                .filter(|p| !is_conflicting(p))
                .collect(),
            PasteConflictAction::Rename => {
                for path in &prompt.paths {
                    if is_conflicting(path) {
                        if let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) {
                            let new_name =
                                crate::core::robocopy::next_available_name(&prompt.target_dir, &name);
                            renames.insert(path.clone(), new_name);
                        }
                    }
                }
                prompt.paths
            }
        };

        self.start_robocopy_paste(
            paths,
            prompt.target_dir,
            prompt.before_entries,
            prompt.is_cut,
            prompt.side,
            renames,
            replaced,
            PasteOrigin::UserAction,
        );
    }

    /// Draws the Replace/Skip/Rename/Cancel modal for a paste held back by
    /// `paste_clipboard_native` because of a name collision - a no-op when
    /// there's nothing pending. Called once per frame from the update loop.
    pub fn draw_paste_conflict_modal(&mut self, ctx: &egui::Context, palette: &crate::gui::theme::ThemePalette) {
        use crate::core::utils::widgets::{
            ghost_dialog_button, modal_frame, modal_icon_header, primary_dialog_button,
            secondary_dialog_button,
        };
        use egui_phosphor::regular;

        // Cloned out inside its own block so this borrow of
        // `self.pending_paste_conflict` ends before the modal closure below,
        // which needs to clear that same field on Cancel.
        let (count, preview, more) = {
            let Some(prompt) = self.pending_paste_conflict.as_ref() else {
                return;
            };
            let count = prompt.conflicting_names.len();
            let preview: Vec<String> = prompt.conflicting_names.iter().take(5).cloned().collect();
            let more = count.saturating_sub(preview.len());
            (count, preview, more)
        };

        let mut resolution: Option<PasteConflictAction> = None;
        let mut cancelled = false;

        // Dimming scrim behind the dialog, same pattern as the About
        // window's `modal_bg` - clicking it cancels, matching the Cancel
        // button rather than silently doing nothing.
        let scrim_clicked = egui::Area::new(egui::Id::new("paste_conflict_scrim"))
            .order(egui::Order::Middle)
            .interactable(true)
            .show(ctx, |ui| {
                let rect = ctx.content_rect();
                ui.painter()
                    .rect_filled(rect, 0.0, palette.modal_background_effect_color);
                ui.interact(
                    rect,
                    ui.id().with("paste_conflict_scrim_click"),
                    egui::Sense::click(),
                )
                .clicked()
            })
            .inner;
        if scrim_clicked {
            cancelled = true;
        }

        // `egui::Window` is the wrong tool here and every previous attempt
        // at this modal (a forced `fixed_size`, a flat-height `ScrollArea`,
        // `.auto_sized()`, keying the Id by row count) was fighting the
        // same root problem from a different angle: `Window` remembers its
        // rendered size *per Id*, in the running app's memory, indefinitely
        // - and once a bad size gets recorded under a given Id (e.g. from
        // the `.auto_sized()` + `ScrollArea` combination that briefly
        // blew this up to nearly full-window), every later `show()` call
        // with that same Id keeps reusing it, no matter how correct the
        // surrounding code becomes, because nothing ever tells egui to
        // forget it. `egui::Area` (the same primitive already used for the
        // hamburger menu and the notifications panel elsewhere in this
        // file/module) has no such memory: it has no "remembered size" to
        // go stale in the first place, since it lays out fresh from
        // content every single frame. Switching to it sidesteps the whole
        // class of bug instead of chasing another angle on it.
        let popup_width = 440.0;
        egui::Area::new(egui::Id::new("paste_conflict_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                modal_frame(&ctx.style_of(ctx.theme()), palette)
                    .show(ui, |ui| {
                        ui.set_width(popup_width);
                        ui.vertical(|ui| {
                            let subtitle =
                                format!("{} {}", count, self.i18n.tr("paste_conflict_message"));
                            modal_icon_header(
                                ui,
                                palette,
                                regular::WARNING_CIRCLE,
                                palette.drive_usage_warning,
                                &self.i18n.tr("paste_conflict_title"),
                                Some(&subtitle),
                            );

                            ui.add_space(14.0);
                            ui.separator();
                            ui.add_space(14.0);

                            egui::Frame::NONE
                                .fill(palette.row_bg)
                                .corner_radius(egui::CornerRadius::same(palette.medium_radius))
                                .stroke(egui::Stroke::new(1.0, palette.borders_default))
                                .inner_margin(egui::Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    for (i, name) in preview.iter().enumerate() {
                                        if i > 0 {
                                            ui.add_space(2.0);
                                        }
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new(regular::FILE)
                                                    .size(palette.text_size + 1.0)
                                                    .color(palette.icon_color),
                                            );
                                            ui.add_space(6.0);
                                            ui.label(
                                                egui::RichText::new(name)
                                                    .size(palette.text_size)
                                                    .color(palette.text_normal),
                                            );
                                        });
                                    }
                                    if more > 0 {
                                        ui.add_space(2.0);
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "+ {more} {}",
                                                self.i18n.tr("paste_conflict_more")
                                            ))
                                            .size(palette.text_size)
                                            .italics()
                                            .color(palette.text_normal.gamma_multiply(0.6)),
                                        );
                                    }
                                });

                            ui.add_space(16.0);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if primary_dialog_button(
                                        ui,
                                        palette,
                                        &self.i18n.tr("paste_conflict_rename"),
                                    )
                                    .on_hover_text(
                                        egui::RichText::new(
                                            self.i18n.tr("tooltip_paste_conflict_rename"),
                                        )
                                        .size(palette.tooltip_text_size)
                                        .color(palette.tooltip_text_color),
                                    )
                                    .clicked()
                                    {
                                        resolution = Some(PasteConflictAction::Rename);
                                    }
                                    ui.add_space(6.0);
                                    if secondary_dialog_button(
                                        ui,
                                        palette,
                                        &self.i18n.tr("paste_conflict_skip"),
                                    )
                                    .clicked()
                                    {
                                        resolution = Some(PasteConflictAction::Skip);
                                    }
                                    ui.add_space(6.0);
                                    if secondary_dialog_button(
                                        ui,
                                        palette,
                                        &self.i18n.tr("paste_conflict_replace"),
                                    )
                                    .clicked()
                                    {
                                        resolution = Some(PasteConflictAction::Replace);
                                    }

                                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        if ghost_dialog_button(ui, palette, &self.i18n.tr("cancel"))
                                            .clicked()
                                        {
                                            cancelled = true;
                                        }
                                    });
                                },
                            );
                        });
                    });
            });

        if cancelled {
            self.pending_paste_conflict = None;
        } else if let Some(action) = resolution {
            self.handle_paste_conflict_resolution(action);
        }
    }

    pub fn draw_bulk_rename_modal(&mut self, ctx: &egui::Context, palette: &crate::gui::theme::ThemePalette) {
        use crate::gui::windows::containers::bulk_rename::{draw_bulk_rename_modal, BulkRenameModalAction};

        let Some(state) = self.pending_bulk_rename.as_mut() else {
            return;
        };

        match draw_bulk_rename_modal(ctx, &self.i18n, palette, state) {
            BulkRenameModalAction::None => {}
            BulkRenameModalAction::Cancelled => {
                self.pending_bulk_rename = None;
            }
            BulkRenameModalAction::Commit(renames) => {
                self.pending_bulk_rename = None;
                self.handle_context_action(
                    crate::gui::windows::containers::enums::ItemViewerContextAction::BulkRenameCommit(renames),
                );
            }
        }
    }

    /// Draws the "Checksums" modal - a no-op when `pending_checksum` is
    /// `None`. Follows `draw_paste_conflict_modal`'s `Area` + `Frame::popup`
    /// structure exactly (see that function's doc comment for why - not
    /// `egui::Window`, which has a remembered-size bug this app already hit
    /// once).
    pub fn draw_checksum_modal(&mut self, ctx: &egui::Context, palette: &crate::gui::theme::ThemePalette) {
        use crate::core::utils::widgets::{ghost_dialog_button, modal_frame, modal_icon_header};
        use egui_phosphor::regular;

        if self.pending_checksum.is_none() {
            return;
        }

        let mut close_clicked = false;

        let scrim_clicked = egui::Area::new(egui::Id::new("checksum_scrim"))
            .order(egui::Order::Middle)
            .interactable(true)
            .show(ctx, |ui| {
                let rect = ctx.content_rect();
                ui.painter()
                    .rect_filled(rect, 0.0, palette.modal_background_effect_color);
                ui.interact(rect, ui.id().with("checksum_scrim_click"), egui::Sense::click())
                    .clicked()
            })
            .inner;
        if scrim_clicked {
            close_clicked = true;
        }

        let popup_width = 420.0;
        egui::Area::new(egui::Id::new("checksum_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                modal_frame(&ctx.style_of(ctx.theme()), palette)
                    .show(ui, |ui| {
                        ui.set_width(popup_width);
                        ui.vertical(|ui| {
                            let Some(state) = self.pending_checksum.as_mut() else {
                                return;
                            };

                            let subtitle = if state.size_label.is_empty() {
                                state.file_name.clone()
                            } else {
                                format!("{} · {}", state.file_name, state.size_label)
                            };
                            modal_icon_header(
                                ui,
                                palette,
                                regular::HASH,
                                palette.primary,
                                &self.i18n.tr("checksum_title"),
                                Some(&subtitle),
                            );

                            ui.add_space(14.0);
                            ui.separator();
                            ui.add_space(14.0);

                            if let Some(error) = &state.error {
                                ui.colored_label(palette.drive_usage_critical, error);
                            } else if let Some(results) = &state.results {
                                let rows: [(&str, &str); 4] = [
                                    ("CRC32", &results.crc32),
                                    ("MD5", &results.md5),
                                    ("SHA-1", &results.sha1),
                                    ("SHA-256", &results.sha256),
                                ];

                                egui::Frame::NONE
                                    .fill(palette.row_bg)
                                    .corner_radius(egui::CornerRadius::same(palette.medium_radius))
                                    .stroke(egui::Stroke::new(1.0, palette.borders_default))
                                    .inner_margin(egui::Margin::symmetric(12, 10))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        egui::Grid::new("checksum_results_grid")
                                            .num_columns(3)
                                            .spacing([10.0, 8.0])
                                            .show(ui, |ui| {
                                                for (label, hash) in rows {
                                                    ui.label(
                                                        egui::RichText::new(label)
                                                            .strong()
                                                            .size(palette.text_size)
                                                            .color(palette.primary),
                                                    );
                                                    ui.add(
                                                        egui::Label::new(
                                                            egui::RichText::new(hash)
                                                                .monospace()
                                                                .size(palette.text_size)
                                                                .color(palette.text_normal),
                                                        )
                                                        .selectable(true),
                                                    );
                                                    if ui
                                                        .add(egui::Button::new(regular::COPY).frame(false))
                                                        .on_hover_text(
                                                            self.i18n.tr("tooltip_checksum_copy"),
                                                        )
                                                        .clicked()
                                                    {
                                                        crate::gui::utils::copy_text_to_clipboard(hash);
                                                    }
                                                    ui.end_row();
                                                }
                                            });
                                    });

                                ui.add_space(12.0);
                                ui.label(
                                    egui::RichText::new(self.i18n.tr("checksum_compare_label"))
                                        .size(palette.text_size)
                                        .color(palette.text_normal),
                                );
                                ui.add_space(4.0);
                                ui.add(
                                    egui::TextEdit::singleline(&mut state.compare_input)
                                        .hint_text(self.i18n.tr("checksum_compare_placeholder"))
                                        .desired_width(ui.available_width()),
                                );

                                let trimmed = state.compare_input.trim();
                                if !trimmed.is_empty() {
                                    let matched = rows
                                        .iter()
                                        .find(|(_, hash)| hash.eq_ignore_ascii_case(trimmed));
                                    ui.add_space(6.0);
                                    let (color, icon, text) = match matched {
                                        Some((label, _)) => (
                                            palette.drive_usage_normal,
                                            regular::CHECK_CIRCLE,
                                            format!(
                                                "{} ({label})",
                                                self.i18n.tr("checksum_compare_match")
                                            ),
                                        ),
                                        None => (
                                            palette.drive_usage_critical,
                                            regular::X_CIRCLE,
                                            self.i18n.tr("checksum_compare_no_match"),
                                        ),
                                    };
                                    egui::Frame::NONE
                                        .fill(color.linear_multiply(0.15))
                                        .corner_radius(egui::CornerRadius::same(
                                            palette.medium_radius,
                                        ))
                                        .inner_margin(egui::Margin::symmetric(10, 6))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(egui::RichText::new(icon).color(color));
                                                ui.label(
                                                    egui::RichText::new(text)
                                                        .size(palette.text_size)
                                                        .color(color),
                                                );
                                            });
                                        });
                                }
                            } else {
                                ui.horizontal(|ui| {
                                    ui.add(egui::Spinner::new());
                                    ui.add_space(8.0);
                                    ui.label(
                                        egui::RichText::new(self.i18n.tr("checksum_computing"))
                                            .size(palette.text_size)
                                            .color(palette.text_normal),
                                    );
                                });
                            }

                            ui.add_space(16.0);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ghost_dialog_button(ui, palette, &self.i18n.tr("close")).clicked() {
                                    close_clicked = true;
                                }
                            });
                        });
                    });
            });

        if close_clicked {
            self.pending_checksum = None;
        }
    }

    /// Drains every robocopy-backed paste's progress/result and applies
    /// completion side effects (selection, tag remapping, refresh) for any
    /// that just succeeded - failed/cancelled ones just get their
    /// bookkeeping dropped, there's nothing left to apply. Called once per
    /// frame from the main update loop.
    pub fn poll_pending_paste(&mut self) {
        for (id, succeeded) in self.notifications_state.poll_robocopy_jobs() {
            let Some(pending) = self.pending_robocopy_pastes.remove(&id) else {
                continue;
            };

            if !succeeded {
                // The per-job notification (created in `start_robocopy_paste`)
                // already surfaced as Failed via `poll_robocopy_jobs` above,
                // for undo/redo jobs same as any other paste - nothing extra
                // to show here. Just keep the group's completion count
                // accurate so a partial failure doesn't leave `remaining`
                // stuck above zero forever.
                if let PasteOrigin::UndoOf(_, group) | PasteOrigin::RedoOf(_, group) =
                    &pending.origin
                {
                    group.record_completion(false);
                }
                continue;
            }

            if pending.is_cut && self.move_tagged_paths_to_dir(&pending.paths, &pending.target_dir)
            {
                self.persist_tags();
            }

            let pasted_paths = Self::selection_paths_after_paste(
                &pending.target_dir,
                &pending.before_entries,
                &pending.paths,
            );
            if !pasted_paths.is_empty() {
                self.active_tab_mut()
                    .view_mut(pending.side)
                    .explorer_state
                    .pending_selection_paths = Some(pasted_paths);
            }

            let final_pairs: Vec<(PathBuf, PathBuf)> = pending
                .paths
                .iter()
                .filter_map(|source| {
                    let final_path = match pending.renames.get(source) {
                        Some(name) => pending.target_dir.join(name),
                        None => pending.target_dir.join(source.file_name()?),
                    };
                    Some((source.clone(), final_path))
                })
                .collect();

            match &pending.origin {
                PasteOrigin::UserAction => {
                    if !final_pairs.is_empty() {
                        let op = if pending.is_cut {
                            UndoableOperation::Move {
                                pairs: final_pairs,
                                side: pending.side,
                                replaced: pending.replaced.clone(),
                            }
                        } else {
                            UndoableOperation::Copy {
                                pairs: final_pairs,
                                side: pending.side,
                                replaced: pending.replaced.clone(),
                            }
                        };
                        self.push_undo(op);
                    }
                }
                PasteOrigin::UndoOf(op, group) => {
                    if group.record_completion(true) {
                        let replaced = match op {
                            UndoableOperation::Move { replaced, .. }
                            | UndoableOperation::Copy { replaced, .. } => Some(replaced),
                            _ => None,
                        };
                        if let Some(replaced) = replaced
                            && !replaced.is_empty()
                        {
                            let pidls: Vec<Vec<u8>> = replaced.values().cloned().collect();
                            let _ = self.restore_paths_native(pidls);
                        }
                        self.redo_stack.push_back(op.clone());
                    }
                }
                PasteOrigin::RedoOf(op, group) => {
                    if group.record_completion(true) {
                        self.undo_stack.push_back(op.clone());
                    }
                }
            }

            self.load_path();
        }
    }

    /// Drains every background compress job's result once it's ready -
    /// called once per frame from the main update loop, same as
    /// `poll_pending_paste`. A job with no result yet (still writing the
    /// archive) is left in the map for the next frame to check again.
    pub fn poll_pending_compress(&mut self) {
        let mut finished = Vec::new();
        for (&id, (rx, _)) in &self.pending_compress_jobs {
            if let Ok(result) = rx.try_recv() {
                finished.push((id, result));
            }
        }

        for (id, result) in finished {
            self.pending_compress_jobs.remove(&id);
            let status = match result {
                Ok(()) => crate::gui::windows::containers::notifications::FileOpStatus::Completed,
                Err(e) => {
                    eprintln!("Compress failed: {e}");
                    crate::gui::windows::containers::notifications::FileOpStatus::Failed
                }
            };
            self.notifications_state.finish_operation(id, status);
            self.load_path();
        }
    }

    fn directory_child_paths(dir: &Path) -> HashSet<PathBuf> {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn selection_paths_after_paste(
        target_dir: &Path,
        before_entries: &HashSet<PathBuf>,
        sources: &[PathBuf],
    ) -> Vec<PathBuf> {
        let mut pasted_paths: Vec<PathBuf> = Self::directory_child_paths(target_dir)
            .difference(before_entries)
            .cloned()
            .collect();

        if pasted_paths.is_empty() {
            for source in sources {
                if let Some(name) = source.file_name() {
                    let candidate = target_dir.join(name);
                    if candidate.exists() {
                        pasted_paths.push(candidate);
                    }
                }
            }
        }

        pasted_paths
    }

    pub fn delete_path(&self, path: &PathBuf) {
        if !shell_delete_to_recycle_bin(path) {
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(path);
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    /// `silent` suppresses any native "are you sure?" confirmation and
    /// progress UI (`FOF_NOCONFIRMATION | FOF_SILENT`) - use this for
    /// internal housekeeping deletes the user never directly asked for
    /// (recycling an item out of the way for Replace, or for Undo/Redo's
    /// own bookkeeping), where a delete confirmation popping up mid-
    /// operation would be a confusing surprise. A genuine user-initiated
    /// delete (the Delete/Delete Permanently context menu entries, the
    /// Delete key) must pass `silent: false` to keep the real native
    /// confirmation Explorer itself shows (`FOF_WANTNUKEWARNING`).
    pub fn delete_paths_native(
        &self,
        paths: Vec<PathBuf>,
        allow_undo: bool,
        silent: bool,
    ) -> windows::core::Result<()> {
        use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
        use windows::Win32::UI::Shell::{
            FOF_ALLOWUNDO, FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName,
        };
        use windows::core::HSTRING;

        unsafe {
            let file_op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;

            // Recycle-bin view needs permanent delete; normal view keeps undo.
            let flags = match (allow_undo, silent) {
                (true, true) => FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT,
                (true, false) => FOF_ALLOWUNDO | FOF_WANTNUKEWARNING,
                (false, true) => FOF_NOCONFIRMATION | FOF_SILENT,
                (false, false) => FOF_WANTNUKEWARNING,
            };
            file_op.SetOperationFlags(flags)?;

            for path in paths {
                let item: IShellItem = SHCreateItemFromParsingName(
                    &HSTRING::from(path.to_string_lossy().to_string()),
                    None,
                )?;

                file_op.DeleteItem(&item, None)?;
            }

            file_op.PerformOperations()?;
        }

        Ok(())
    }

    /// Records a freshly-completed, reversible operation on `undo_stack`,
    /// clearing `redo_stack` - a fresh user action always invalidates
    /// whatever redo history existed (it no longer describes "what comes
    /// after the current state"). Every push site that represents a *new*
    /// user action (as opposed to an undo/redo of a previous one) must clear
    /// redo this way; undo/redo completions push to the *opposite* stack
    /// instead (see `undo`/`redo`), never through this method.
    fn push_undo(&mut self, op: UndoableOperation) {
        self.undo_stack.push_back(op);
        while self.undo_stack.len() > MAX_UNDO_STACK {
            self.undo_stack.pop_front();
        }
        self.redo_stack.clear();
    }

    /// Reverses the most recently completed reversible operation, if any -
    /// wired to Ctrl+Z (see `handle_global_shortcuts`). Pops the entry
    /// immediately (rather than waiting for the reversal itself to finish)
    /// so a second Ctrl+Z can't double-fire on the same in-flight entry;
    /// on success the entry moves to `redo_stack` so Ctrl+Y can restore it.
    pub fn undo(&mut self) {
        let Some(op) = self.undo_stack.pop_back() else {
            return;
        };

        match &op {
            UndoableOperation::Rename { old_path, new_path } => {
                let Some(new_name) = old_path.file_name().map(|n| n.to_string_lossy().to_string())
                else {
                    return;
                };
                if Self::rename_one_native(new_path, &new_name).is_ok() {
                    self.redo_stack.push_back(op);
                    self.load_path();
                } else {
                    eprintln!("Undo failed: could not rename back to original name");
                }
            }
            UndoableOperation::BulkRename { pairs } => {
                let reversed: Vec<(PathBuf, String)> = pairs
                    .iter()
                    .filter_map(|(old, new)| {
                        old.file_name()
                            .map(|n| (new.clone(), n.to_string_lossy().to_string()))
                    })
                    .collect();
                if self.rename_paths_native(reversed).is_ok() {
                    self.redo_stack.push_back(op);
                    self.load_path();
                } else {
                    eprintln!("Undo failed: could not restore original names");
                }
            }
            UndoableOperation::Move { pairs, side, .. } => {
                // Sources for the "move back" job(s) are the *current*
                // (post-move) paths; each job's target is one distinct
                // original parent directory - a multi-directory source
                // selection needs one job per directory, since a single
                // robocopy/IFileOperation job has exactly one destination.
                // Restoring anything replaced happens after these jobs
                // complete (see `poll_pending_paste`'s `UndoOf` branch),
                // using this same `op`'s `replaced` map - not here.
                let mut groups: HashMap<PathBuf, Vec<(PathBuf, PathBuf)>> = HashMap::new();
                for (orig, cur) in pairs {
                    if let Some(parent) = orig.parent() {
                        groups
                            .entry(parent.to_path_buf())
                            .or_default()
                            .push((orig.clone(), cur.clone()));
                    }
                }
                if groups.is_empty() {
                    return;
                }

                let group = std::rc::Rc::new(UndoRedoGroup::new(groups.len()));
                for (orig_parent, group_pairs) in groups {
                    let sources: Vec<PathBuf> =
                        group_pairs.iter().map(|(_, cur)| cur.clone()).collect();
                    let before_entries = Self::directory_child_paths(&orig_parent);
                    let mut renames = HashMap::new();
                    for (orig, cur) in &group_pairs {
                        if let (Some(orig_name), Some(cur_name)) =
                            (orig.file_name(), cur.file_name())
                            && orig_name != cur_name
                        {
                            renames.insert(cur.clone(), orig_name.to_string_lossy().to_string());
                        }
                    }
                    self.start_robocopy_paste(
                        sources,
                        orig_parent,
                        before_entries,
                        true,
                        *side,
                        renames,
                        HashMap::new(),
                        PasteOrigin::UndoOf(op.clone(), group.clone()),
                    );
                }
            }
            UndoableOperation::Copy { pairs, replaced, .. } => {
                // Undoing a copy just deletes what it created - synchronous,
                // same shape as Rename/BulkRename's undo. `allow_undo: true`
                // sends the deleted copies to the Recycle Bin, so this stays
                // further-recoverable through Explorer's own Recycle Bin,
                // same as everything else this app deletes.
                let created: Vec<PathBuf> = pairs.iter().map(|(_, created)| created.clone()).collect();
                if self.delete_paths_native(created, true, true).is_ok() {
                    if !replaced.is_empty() {
                        let pidls: Vec<Vec<u8>> = replaced.values().cloned().collect();
                        let _ = self.restore_paths_native(pidls);
                    }
                    self.redo_stack.push_back(op);
                    self.load_path();
                } else {
                    eprintln!("Undo failed: could not delete the copied item(s)");
                }
            }
        }
    }

    /// Re-applies the most recently undone operation, if any - wired to
    /// Ctrl+Y (primary) / Ctrl+Shift+Z (secondary) in
    /// `handle_global_shortcuts`. Mirrors `undo`: pops from `redo_stack`
    /// first, pushes back to `undo_stack` only on success.
    pub fn redo(&mut self) {
        let Some(op) = self.redo_stack.pop_back() else {
            return;
        };

        match &op {
            UndoableOperation::Rename { old_path, new_path } => {
                let Some(new_name) = new_path.file_name().map(|n| n.to_string_lossy().to_string())
                else {
                    return;
                };
                if Self::rename_one_native(old_path, &new_name).is_ok() {
                    self.undo_stack.push_back(op);
                    self.load_path();
                } else {
                    eprintln!("Redo failed: could not re-apply rename");
                }
            }
            UndoableOperation::BulkRename { pairs } => {
                let forward: Vec<(PathBuf, String)> = pairs
                    .iter()
                    .filter_map(|(old, new)| {
                        new.file_name()
                            .map(|n| (old.clone(), n.to_string_lossy().to_string()))
                    })
                    .collect();
                if self.rename_paths_native(forward).is_ok() {
                    self.undo_stack.push_back(op);
                    self.load_path();
                } else {
                    eprintln!("Redo failed: could not re-apply bulk rename");
                }
            }
            UndoableOperation::Move { pairs, side, replaced } => {
                // Redo re-applies the original move, which always had one
                // single destination folder - unlike undo, this is always
                // exactly one job, sourcing from each pair's original path.
                let Some(target_dir) = pairs
                    .first()
                    .and_then(|(_, cur)| cur.parent())
                    .map(|p| p.to_path_buf())
                else {
                    return;
                };

                // A pair whose destination was originally a Replace
                // resolution needs the exact same treatment redone fresh:
                // recycle whatever's there now, capture its (new) pidl. The
                // job below then places the incoming item into the
                // now-empty spot, same as any other paste.
                let mut updated_replaced: HashMap<PathBuf, Vec<u8>> = HashMap::new();
                for (_, cur) in pairs {
                    if replaced.contains_key(cur)
                        && let Some(name) = cur.file_name().map(|n| n.to_string_lossy().to_string())
                        && self.delete_paths_native(vec![cur.clone()], true, true).is_ok()
                        && let Some(pidl) = self.find_recycled_pidl(&target_dir, &name)
                    {
                        updated_replaced.insert(cur.clone(), pidl);
                    }
                }

                let sources: Vec<PathBuf> = pairs.iter().map(|(orig, _)| orig.clone()).collect();
                let before_entries = Self::directory_child_paths(&target_dir);
                let mut renames = HashMap::new();
                for (orig, cur) in pairs {
                    if let (Some(orig_name), Some(cur_name)) = (orig.file_name(), cur.file_name())
                        && orig_name != cur_name
                    {
                        renames.insert(orig.clone(), cur_name.to_string_lossy().to_string());
                    }
                }

                let new_op = UndoableOperation::Move {
                    pairs: pairs.clone(),
                    side: *side,
                    replaced: updated_replaced,
                };
                let group = std::rc::Rc::new(UndoRedoGroup::new(1));
                self.start_robocopy_paste(
                    sources,
                    target_dir,
                    before_entries,
                    true,
                    *side,
                    renames,
                    HashMap::new(),
                    PasteOrigin::RedoOf(new_op, group),
                );
            }
            UndoableOperation::Copy { pairs, side, replaced } => {
                // Redo re-copies the sources - always one job, one
                // destination folder (wherever the copies were originally
                // created).
                let Some(target_dir) = pairs
                    .first()
                    .and_then(|(_, created)| created.parent())
                    .map(|p| p.to_path_buf())
                else {
                    return;
                };

                let mut updated_replaced: HashMap<PathBuf, Vec<u8>> = HashMap::new();
                for (_, created) in pairs {
                    if replaced.contains_key(created)
                        && let Some(name) =
                            created.file_name().map(|n| n.to_string_lossy().to_string())
                        && self.delete_paths_native(vec![created.clone()], true, true).is_ok()
                        && let Some(pidl) = self.find_recycled_pidl(&target_dir, &name)
                    {
                        updated_replaced.insert(created.clone(), pidl);
                    }
                }

                let sources: Vec<PathBuf> = pairs.iter().map(|(source, _)| source.clone()).collect();
                let before_entries = Self::directory_child_paths(&target_dir);
                let mut renames = HashMap::new();
                for (source, created) in pairs {
                    if let (Some(source_name), Some(created_name)) =
                        (source.file_name(), created.file_name())
                        && source_name != created_name
                    {
                        renames.insert(source.clone(), created_name.to_string_lossy().to_string());
                    }
                }

                let new_op = UndoableOperation::Copy {
                    pairs: pairs.clone(),
                    side: *side,
                    replaced: updated_replaced,
                };
                let group = std::rc::Rc::new(UndoRedoGroup::new(1));
                self.start_robocopy_paste(
                    sources,
                    target_dir,
                    before_entries,
                    false,
                    *side,
                    renames,
                    HashMap::new(),
                    PasteOrigin::RedoOf(new_op, group),
                );
            }
        }
    }

    /// Renames one item in place (same parent, new name only) via a single
    /// `IFileOperation`. Shared by the single-item rename handler
    /// (`RenameRequest`) and bulk rename's per-item fallback for anything
    /// that didn't land as part of the batched `rename_paths_native` call.
    fn rename_one_native(path: &Path, new_name: &str) -> windows::core::Result<()> {
        use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
        use windows::Win32::UI::Shell::{
            FOF_ALLOWUNDO, FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName,
        };
        use windows::core::HSTRING;

        unsafe {
            let file_op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
            file_op.SetOperationFlags(FOF_ALLOWUNDO)?;

            let source_item: IShellItem = SHCreateItemFromParsingName(
                &HSTRING::from(path.to_string_lossy().to_string()),
                None,
            )?;

            file_op.RenameItem(&source_item, &HSTRING::from(new_name), None)?;
            file_op.PerformOperations()?;
        }

        Ok(())
    }

    /// Batched rename: one `IFileOperation`, one `RenameItem` call queued
    /// per pair, and - unlike `delete_paths_native`'s per-item
    /// `PerformOperations` shape, which isn't needed there since delete has
    /// no equivalent "did it actually land" ambiguity - exactly one
    /// `PerformOperations()` at the very end, so the whole batch commits
    /// (and undoes, via `FOF_ALLOWUNDO`) as one unit. Returns the
    /// `(original_path, intended_target_path)` pairs that were queued; the
    /// caller checks which targets actually exist afterward; there's no
    /// `IFileOperationProgressSink` wired up in this codebase to get
    /// per-item results directly.
    pub fn rename_paths_native(
        &self,
        renames: Vec<(PathBuf, String)>,
    ) -> windows::core::Result<Vec<(PathBuf, PathBuf)>> {
        use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
        use windows::Win32::UI::Shell::{
            FOF_ALLOWUNDO, FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName,
        };
        use windows::core::HSTRING;

        let mut targets: Vec<(PathBuf, PathBuf)> = Vec::with_capacity(renames.len());

        unsafe {
            let file_op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
            file_op.SetOperationFlags(FOF_ALLOWUNDO)?;

            for (path, new_name) in &renames {
                let item: IShellItem = SHCreateItemFromParsingName(
                    &HSTRING::from(path.to_string_lossy().to_string()),
                    None,
                )?;

                file_op.RenameItem(&item, &HSTRING::from(new_name.as_str()), None)?;

                let target = path
                    .parent()
                    .map(|p| p.join(new_name))
                    .unwrap_or_else(|| PathBuf::from(new_name));
                targets.push((path.clone(), target));
            }

            file_op.PerformOperations()?;
        }

        Ok(targets)
    }

    pub fn restore_paths_native(&self, pidls: Vec<Vec<u8>>) -> windows::core::Result<()> {
        use windows::Win32::System::Com::{CoTaskMemAlloc, CoTaskMemFree};

        unsafe {
            for pidl_bytes in pidls {
                let pidl = CoTaskMemAlloc(pidl_bytes.len()) as *mut u8;
                if pidl.is_null() {
                    return Err(Error::from(HRESULT(0x80004005u32 as i32)));
                }

                let result = (|| -> windows::core::Result<()> {
                    std::ptr::copy_nonoverlapping(pidl_bytes.as_ptr(), pidl, pidl_bytes.len());

                    let shell_item: IShellItem = SHCreateItemFromIDList(pidl as *const ITEMIDLIST)?;
                    let context_menu: IContextMenu =
                        shell_item.BindToHandler(None, &BHID_SFUIObject)?;

                    let restore_verb = PCSTR(b"undelete\0".as_ptr());
                    let info = CMINVOKECOMMANDINFOEX {
                        cbSize: std::mem::size_of::<CMINVOKECOMMANDINFOEX>() as u32,
                        fMask: 0,
                        hwnd: HWND(std::ptr::null_mut()),
                        lpVerb: restore_verb,
                        lpVerbW: PCWSTR::null(),
                        nShow: SW_SHOWNORMAL.0,
                        ..Default::default()
                    };

                    context_menu.InvokeCommand(&info as *const _ as *const CMINVOKECOMMANDINFO)
                })();

                CoTaskMemFree(Some(pidl as _));
                result?;
            }
        }

        Ok(())
    }

    /// Finds the pidl of the most-recently-deleted Recycle Bin item whose
    /// original directory and name match - used right after Replace
    /// resolution recycles the pre-existing destination item, so its pidl
    /// can be stashed for `undo`/`redo` to restore later (see
    /// `UndoableOperation::Move`/`Copy`'s `replaced` field).
    ///
    /// This is a match-by-name-and-original-directory heuristic, not a
    /// guaranteed-unique identifier - `delete_paths_native` doesn't return
    /// the resulting Recycle Bin item's identity directly, and building a
    /// full `IFileOperationProgressSink` COM callback to get it would be
    /// disproportionate to this feature's scope. In the extremely unlikely
    /// case of two files with the identical name being recycled from the
    /// identical folder within the same instant by something else
    /// concurrently, this could pick the wrong one - picking the most
    /// recently deleted match (by `deleted_time_raw`) is the best available
    /// tie-break.
    fn find_recycled_pidl(&self, original_dir: &Path, name: &str) -> Option<Vec<u8>> {
        use windows::Win32::System::Com::CoTaskMemFree;
        use windows::Win32::UI::Shell::{
            BHID_EnumItems, FOLDERID_RecycleBinFolder, IEnumShellItems, ILFree, ILGetSize,
            IShellItem, SHCreateItemFromIDList, SHGetIDListFromObject, SHGetKnownFolderIDList,
        };

        let original_dir_str = original_dir.to_string_lossy().to_string();
        let mut best: Option<(i64, Vec<u8>)> = None;

        unsafe {
            let recycle_pidl = SHGetKnownFolderIDList(&FOLDERID_RecycleBinFolder, 0, None).ok()?;
            let recycle_item: IShellItem = match SHCreateItemFromIDList(recycle_pidl) {
                Ok(item) => item,
                Err(_) => {
                    CoTaskMemFree(Some(recycle_pidl as _));
                    return None;
                }
            };

            let enum_items: IEnumShellItems =
                match recycle_item.BindToHandler(None, &BHID_EnumItems) {
                    Ok(items) => items,
                    Err(_) => {
                        CoTaskMemFree(Some(recycle_pidl as _));
                        return None;
                    }
                };

            loop {
                let mut fetched_items: [Option<IShellItem>; 1] = [None];
                if enum_items.Next(&mut fetched_items, None).is_err() {
                    break;
                }
                let Some(item) = fetched_items[0].take() else {
                    break;
                };

                let (_, _, _, _, _, _, deleted_time_raw, original_object_name, original_directory) =
                    crate::core::fs::get_shell_item_metadata(
                        &item,
                        self.settings_window.current_settings.date_style,
                        self.settings_window.current_settings.time_format_24h,
                        &self.settings_window.current_settings.custom_date_format,
                    );

                let matches = original_object_name.as_deref() == Some(name)
                    && original_directory.as_deref() == Some(original_dir_str.as_str());

                if matches
                    && let Ok(pidl) = SHGetIDListFromObject(&item)
                {
                    let pidl_size = ILGetSize(Some(pidl as _)) as usize;
                    let mut pidl_bytes = vec![0u8; pidl_size];
                    std::ptr::copy_nonoverlapping(pidl as *const u8, pidl_bytes.as_mut_ptr(), pidl_size);
                    ILFree(Some(pidl as _));

                    let deleted_time = deleted_time_raw.unwrap_or(0);
                    let is_better = match &best {
                        Some((t, _)) => deleted_time > *t,
                        None => true,
                    };
                    if is_better {
                        best = Some((deleted_time, pidl_bytes));
                    }
                }
            }

            CoTaskMemFree(Some(recycle_pidl as _));
        }

        best.map(|(_, pidl)| pidl)
    }

    pub fn open_properties_multi(&self, paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }

        if paths.len() == 1 {
            self.open_properties(&paths[0]);
            return;
        }

        if let Some(data_object) = create_data_object(paths) {
            unsafe {
                let _ = SHMultiFileProperties(&data_object, 0);
            }
        }
    }

    pub fn open_properties(&self, path: &Path) {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let verb: Vec<u16> = OsStr::new("properties")
            .encode_wide()
            .chain(Some(0))
            .collect();

        unsafe {
            let mut info = SHELLEXECUTEINFOW {
                cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
                fMask: SEE_MASK_INVOKEIDLIST,
                lpVerb: PCWSTR(verb.as_ptr()),
                lpFile: PCWSTR(wide.as_ptr()),
                nShow: SW_SHOW.0,
                ..Default::default()
            };
            let _ = ShellExecuteExW(&mut info);
        }
    }

    fn cleanup_resources(&mut self) {
        println!("Cleaning up resources...");

        // Tell background workers to exit.
        self.shutdown.store(true, Ordering::Relaxed);

        for tab in &mut self.tabs {
            for view in std::iter::once(&mut tab.primary_view).chain(tab.split_view.iter_mut()) {
                // Dropping the senders/receivers wakes any blocked workers.
                view.size_req_tx.take();
                view.size_rx.take();
                view.rx.take();

                // Wait for all workers to terminate.
                for handle in view.size_threads.drain(..) {
                    let _ = handle.join();
                }
            }
        }
    }

    /// Handles the action produced (if any) while drawing the Settings tab's
    /// content this frame.
    pub fn handle_pending_settings_action(&mut self, ctx: &egui::Context) {
        if let Some(action) = self.settings_window.pending_action.take() {
            match action {
                SettingsAction::ApplySettings => {
                    self.save_app_settings_to_disk();

                    if let Some(hwnd) = self.hwnd {
                        crate::gui::windows::windowsoverrides::set_window_mode(
                            hwnd,
                            &self.settings_window.current_settings.window_size_mode,
                        );
                    }
                }
                SettingsAction::ResetToDefaults => {
                    // Custom context menu entries and tab groups are
                    // user-authored content, like favorites/tags - a general
                    // settings reset shouldn't wipe them out.
                    let custom_context_menu = std::mem::take(
                        &mut self.settings_window.current_settings.custom_context_menu,
                    );
                    let tab_groups =
                        std::mem::take(&mut self.settings_window.current_settings.tab_groups);
                    self.settings_window.current_settings = Default::default();
                    self.settings_window.current_settings.custom_context_menu = custom_context_menu;
                    self.settings_window.current_settings.tab_groups = tab_groups;
                    if let Some(hwnd) = self.hwnd {
                        crate::gui::windows::windowsoverrides::set_window_mode(
                            hwnd,
                            &self.settings_window.current_settings.window_size_mode,
                        );
                    }
                }
                SettingsAction::ResetFavourites => {
                    self.sidebar_state.favorites = self.default_favorites();
                    self.persist_favorites();
                }
                SettingsAction::ExportSettings => {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Eden Explorer Settings", &["json"])
                        .set_file_name("eden_explorer_settings.json")
                        .save_file()
                    {
                        let bundle = SettingsExportBundle {
                            format_version: SETTINGS_EXPORT_FORMAT_VERSION,
                            settings: self.settings_window.current_settings.clone(),
                            favorites: self.sidebar_state.favorites.clone(),
                            tags: Some(self.tags_state.to_snapshot()),
                            theme_light: self.theme_customizer.light_palette.clone(),
                            theme_dark: self.theme_customizer.dark_palette.clone(),
                        };

                        match serde_json::to_string_pretty(&bundle) {
                            Ok(json) => {
                                if let Err(err) = std::fs::write(&path, json) {
                                    eprintln!("Failed to export settings: {}", err);
                                }
                            }
                            Err(err) => eprintln!("Failed to serialize settings: {}", err),
                        }
                    }
                }
                SettingsAction::ImportSettings => {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Eden Explorer Settings", &["json"])
                        .pick_file()
                    {
                        match std::fs::read_to_string(&path) {
                            Ok(json) => match serde_json::from_str::<SettingsExportBundle>(&json) {
                                Ok(bundle) => self.apply_imported_settings(ctx, bundle),
                                Err(err) => eprintln!("Failed to parse settings file: {}", err),
                            },
                            Err(err) => eprintln!("Failed to read settings file: {}", err),
                        }
                    }
                }
                SettingsAction::ExportContextMenu => {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Eden Explorer Context Menu", &["json"])
                        .set_file_name("eden_explorer_context_menu.json")
                        .save_file()
                    {
                        let bundle = crate::core::context_menu_settings::ContextMenuExportBundle {
                            format_version:
                                crate::core::context_menu_settings::CONTEXT_MENU_EXPORT_FORMAT_VERSION,
                            entries: self.settings_window.current_settings.custom_context_menu.clone(),
                        };

                        match serde_json::to_string_pretty(&bundle) {
                            Ok(json) => {
                                if let Err(err) = std::fs::write(&path, json) {
                                    eprintln!("Failed to export context menu: {}", err);
                                }
                            }
                            Err(err) => eprintln!("Failed to serialize context menu: {}", err),
                        }
                    }
                }
                SettingsAction::ImportContextMenu => {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Eden Explorer Context Menu", &["json"])
                        .pick_file()
                    {
                        match std::fs::read_to_string(&path) {
                            Ok(json) => match serde_json::from_str::<
                                crate::core::context_menu_settings::ContextMenuExportBundle,
                            >(&json)
                            {
                                Ok(bundle) => {
                                    self.settings_window.current_settings.custom_context_menu =
                                        bundle.entries;
                                    crate::core::context_menu_settings::save_custom_context_menu(
                                        &self.settings_window.current_settings.custom_context_menu,
                                    );
                                }
                                Err(err) => eprintln!("Failed to parse context menu file: {}", err),
                            },
                            Err(err) => eprintln!("Failed to read context menu file: {}", err),
                        }
                    }
                }
                SettingsAction::ThemeCustomizer(theme_action) => {
                    self.apply_theme_customizer_action(ctx, theme_action);
                }
            }
        }
    }

    /// Applies a settings bundle produced by `ExportSettings`: persists everything to
    /// disk and live-applies what can be safely changed without a restart (favorites,
    /// tags, theme, language, and window mode). General toggles/column layout are
    /// picked up immediately too since the rest of the UI reads them straight off
    /// `current_settings` each frame.
    fn apply_imported_settings(&mut self, ctx: &egui::Context, bundle: SettingsExportBundle) {
        self.settings_window.current_settings = bundle.settings;
        self.i18n
            .set_locale(&self.settings_window.current_settings.language);
        self.save_app_settings_to_disk();

        if let Some(hwnd) = self.hwnd {
            crate::gui::windows::windowsoverrides::set_window_mode(
                hwnd,
                &self.settings_window.current_settings.window_size_mode,
            );
        }

        self.sidebar_state.favorites = bundle.favorites;
        self.persist_favorites();

        if let Some(tags) = bundle.tags {
            self.tags_state = TagsState::from_snapshot(tags);
            self.persist_tags();
        }

        self.theme_customizer.light_palette = bundle.theme_light.clone();
        self.theme_customizer.dark_palette = bundle.theme_dark.clone();
        set_palette(ThemeMode::Light, bundle.theme_light.clone());
        set_palette(ThemeMode::Dark, bundle.theme_dark.clone());
        save_theme_settings(&bundle.theme_light, &bundle.theme_dark);

        let active_palette = match self.theme {
            ThemeMode::Dark => &bundle.theme_dark,
            ThemeMode::Light => &bundle.theme_light,
        };
        apply_font_to_context(ctx, active_palette);
        self.theme_dirty = true;

        self.mark_tab_infos_dirty();
        self.load_path();
    }

    pub fn handle_draw_about_window(&mut self, ctx: &egui::Context, palette: &ThemePalette) {
        // TODO: Implement about window
        draw_about_window(&mut self.i18n, ctx, &mut self.about_window, palette);
    }

    pub fn handle_tabbar_action(
        &mut self,
        tabbar_action: Option<ItemViewerNavBarAction>,
        drag_sources: Option<&[PathBuf]>,
    ) {
        if let Some(action) = tabbar_action.as_ref().and_then(|t| t.nav.as_ref()) {
            match action {
                ItemViewerNavAction::Back => {
                    let snapshot = directory_settings_snapshot_for_view(
                        self.active_tab().view(self.focused_split),
                    );
                    let _ = persist_directory_settings_snapshot(
                        &mut self.settings_window.current_settings.directory_settings,
                        snapshot,
                    );
                    // Store current path in navigation history before going back
                    if let Some(parent) = self.current_nav().get_parent() {
                        let current = self.current_nav().current.clone();
                        let side = self.focused_split;
                        self.active_tab_mut()
                            .view_mut(side)
                            .explorer_state
                            .navigation_history
                            .insert(parent, current);
                    }
                    self.current_nav_mut().go_back();
                }
                ItemViewerNavAction::Forward => {
                    let snapshot = directory_settings_snapshot_for_view(
                        self.active_tab().view(self.focused_split),
                    );
                    let _ = persist_directory_settings_snapshot(
                        &mut self.settings_window.current_settings.directory_settings,
                        snapshot,
                    );
                    self.current_nav_mut().go_forward();
                }
                ItemViewerNavAction::Up => {
                    let snapshot = directory_settings_snapshot_for_view(
                        self.active_tab().view(self.focused_split),
                    );
                    let _ = persist_directory_settings_snapshot(
                        &mut self.settings_window.current_settings.directory_settings,
                        snapshot,
                    );
                    // Store current path in navigation history before going up
                    if let Some(parent) = self.current_nav().get_parent() {
                        let current = self.current_nav().current.clone();
                        let side = self.focused_split;
                        self.active_tab_mut()
                            .view_mut(side)
                            .explorer_state
                            .navigation_history
                            .insert(parent, current);
                    }
                    self.current_nav_mut().go_up();
                }
            }
            self.mark_tab_infos_dirty();
            self.load_path();

            // Restore selection for Back and Up actions
            if matches!(action, ItemViewerNavAction::Back | ItemViewerNavAction::Up) {
                let current_path = self.current_nav().current.clone();
                let side = self.focused_split;
                let explorer_state = &mut self.active_tab_mut().view_mut(side).explorer_state;
                if let Some(last_visited) = explorer_state.navigation_history.get(&current_path) {
                    explorer_state.navigation_selection = Some(last_visited.clone());
                } else {
                    explorer_state.navigation_selection = None;
                }
                explorer_state.selection_anchor = None;
                explorer_state.selected_paths.clear();
                explorer_state.selection_focus = None;
            }
        } else {
            if let Some(path) = tabbar_action.as_ref().and_then(|t| t.nav_to.as_ref()) {
                let snapshot = directory_settings_snapshot_for_view(
                    self.active_tab().view(self.focused_split),
                );
                let _ = persist_directory_settings_snapshot(
                    &mut self.settings_window.current_settings.directory_settings,
                    snapshot,
                );
                // Store current path in navigation history before navigating
                if let Some(parent) = self.current_nav().get_parent() {
                    let current = self.current_nav().current.clone();
                    let side = self.focused_split;
                    self.active_tab_mut()
                        .view_mut(side)
                        .explorer_state
                        .navigation_history
                        .insert(parent, current);
                }

                self.current_nav_mut().go_to(path.clone());
                self.mark_tab_infos_dirty();
                self.load_path();
            }
            if tabbar_action
                .as_ref()
                .map(|t| t.refresh_current_directory)
                .unwrap_or(false)
            {
                self.load_path();
            }
            if let Some(target_dir) = tabbar_action
                .as_ref()
                .and_then(|t| t.move_files_to_breadcrumb_dir.as_ref())
            {
                if let Some(sources) = drag_sources {
                    self.move_selected_paths_to_dir(sources, target_dir.clone());
                }
            }
            if tabbar_action
                .as_ref()
                .map(|t| t.create_folder)
                .unwrap_or(false)
            {
                self.create_new_folder();
            }
            if tabbar_action
                .as_ref()
                .map(|t| t.create_file)
                .unwrap_or(false)
            {
                self.create_new_file();
            }
            if tabbar_action
                .as_ref()
                .map(|t| t.add_favorite)
                .unwrap_or(false)
            {
                self.add_favorite();
            }
            if tabbar_action
                .as_ref()
                .map(|t| t.remove_favorite)
                .unwrap_or(false)
            {
                let path = self.current_nav().current.clone();
                self.remove_favorite(&path);
            }
            if let Some((query, scope)) = tabbar_action.as_ref().and_then(|t| t.open_search.clone()) {
                self.open_or_focus_search_tab(query, scope);
            }
            if let Some((query, scope)) = tabbar_action.as_ref().and_then(|t| t.save_search.clone()) {
                let scope_folder = match &scope {
                    SearchScope::CurrentFolder(dir) => Some(dir.clone()),
                    SearchScope::Everywhere => None,
                };
                if self.saved_searches_state.add(query, scope_folder) {
                    self.persist_saved_searches();
                }
            }
            if tabbar_action
                .as_ref()
                .map(|t| t.activate_search_box)
                .unwrap_or(false)
            {
                self.enter_search_box_edit_mode();
            }
        }
    }

    /// Switches the focused pane's breadcrumb row into the inline search
    /// box, defaulting its scope to the current folder (recursive) when
    /// this tab is showing a real folder, or Everywhere otherwise (This PC,
    /// Recycle Bin, Settings, a tag view, or another search's results).
    /// Clicking the toolbar's search icon again while the search box is
    /// already showing is a cancel, not a no-op - it toggles back to the
    /// normal breadcrumb rather than silently re-defaulting an in-progress
    /// query every time the icon is pressed.
    pub fn enter_search_box_edit_mode(&mut self) {
        let side = self.focused_split;

        if self.active_tab().view(side).search_box_editing {
            self.active_tab_mut().view_mut(side).search_box_editing = false;
            return;
        }

        let nav = &self.active_tab().view(side).nav;
        let is_real_folder = !nav.is_root()
            && !nav.is_recycle_bin()
            && !nav.is_settings()
            && !nav.is_tag_view()
            && !nav.is_search_view();
        let prefers_current_folder = matches!(
            self.settings_window.current_settings.default_search_scope,
            crate::core::everything::DefaultSearchScope::CurrentFolder
        );
        let default_scope = if is_real_folder && prefers_current_folder {
            SearchScope::CurrentFolder(nav.current.clone())
        } else {
            SearchScope::Everywhere
        };

        let view = self.active_tab_mut().view_mut(side);
        view.search_box_editing = true;
        view.search_box_buffer.clear();
        view.search_box_scope = default_scope;
    }

    pub fn handle_tabs_action(
        &mut self,
        tabs_action: Option<TabsAction>,
        drag_sources: Option<&[PathBuf]>,
    ) {
        if let Some(action) = tabs_action {
            if let Some((from, to)) = action.reorder {
                let len = self.tabs.len();
                if from < len {
                    let active_id = self.tabs.get(self.active_tab).map(|t| t.id);
                    let item = self.tabs.remove(from);

                    let mut target = to;
                    if to > from {
                        target -= 1;
                    }
                    target = target.min(self.tabs.len());

                    self.tabs.insert(target, item);

                    if let Some(id) = active_id {
                        if let Some(new_idx) = self.tabs.iter().position(|t| t.id == id) {
                            self.active_tab = new_idx;
                        }
                    }

                    self.mark_tab_infos_dirty();
                }
            }
            if let Some(id) = action.activate {
                self.active_tab = self.tabs.iter().position(|t| t.id == id).unwrap();
                self.focused_split = SplitSide::Primary;
                let has_split = {
                    let tab = self.active_tab_mut();
                    tab.primary_view.item_viewer_filter_state.dirty = true;
                    if let Some(split) = tab.split_view.as_mut() {
                        split.item_viewer_filter_state.dirty = true;
                    }
                    tab.split_view.is_some()
                };
                self.pending_tab_scroll_id = Some(id);
                self.load_view(SplitSide::Primary);
                if has_split {
                    self.load_view(SplitSide::Secondary);
                }
            }
            if action.open_new {
                let cloned_nav = self.current_nav().clone();
                let (sort_column, sort_ascending) = {
                    let view = self.active_tab().view(self.focused_split);
                    (view.sort_column, view.sort_ascending)
                };
                let id = self.next_tab_id;
                self.next_tab_id += 1;
                self.tabs
                    .push(TabState::new(id, cloned_nav, sort_column, sort_ascending));
                let current_settings = self.settings_window.current_settings.clone();
                apply_directory_settings_to_view(
                    &mut self.tabs.last_mut().unwrap().primary_view,
                    &current_settings,
                    DisplayModeFallback::Default,
                );
                self.active_tab = self.tabs.len() - 1;
                self.focused_split = SplitSide::Primary;
                self.pending_tab_scroll_id = Some(id);
                self.mark_tab_infos_dirty();
                self.load_path();
            }
            if let Some(path) = action.duplicate {
                self.open_new_tab(path);
                self.load_path();
            }
            if let Some(paths) = action.open_group {
                for path in paths {
                    self.open_new_tab(path);
                }
                self.load_path();
            }
            if let Some(paths) = action.replace_with_group {
                if !paths.is_empty() {
                    // Capture sort settings before clearing - `self.tabs`
                    // must stay non-empty for `active_tab()`'s indexing, so
                    // this can't reuse `open_new_tab` (which reads it) after
                    // the clear below.
                    let (sort_column, sort_ascending) = {
                        let view = self.active_tab().view(self.focused_split);
                        (view.sort_column, view.sort_ascending)
                    };
                    self.tabs.clear();
                    self.focused_split = SplitSide::Primary;
                    let current_settings = self.settings_window.current_settings.clone();
                    for path in paths {
                        let nav = Navigation::new(path);
                        let id = self.next_tab_id;
                        self.next_tab_id += 1;
                        self.tabs
                            .push(TabState::new(id, nav, sort_column, sort_ascending));
                        apply_directory_settings_to_view(
                            &mut self.tabs.last_mut().unwrap().primary_view,
                            &current_settings,
                            DisplayModeFallback::Default,
                        );
                    }
                    self.active_tab = 0;
                    self.mark_tab_infos_dirty();
                    self.load_path();
                }
            }
            if let Some((name, path)) = action.add_tab_to_new_group {
                let id = crate::core::tab_groups::next_group_id(
                    &self.settings_window.current_settings.tab_groups,
                );
                self.settings_window.current_settings.tab_groups.push(
                    crate::core::tab_groups::TabGroup {
                        id,
                        name,
                        paths: vec![path],
                    },
                );
                self.save_app_settings_to_disk();
            }
            if let Some((group_id, path)) = action.add_tab_to_existing_group {
                if let Some(group) = self
                    .settings_window
                    .current_settings
                    .tab_groups
                    .iter_mut()
                    .find(|g| g.id == group_id)
                {
                    // Duplicates are allowed on purpose - see `TabGroup`.
                    group.paths.push(path);
                }
                self.save_app_settings_to_disk();
            }
            if let Some(id) = action.close {
                if self.tabs.len() > 1 {
                    if let Some(idx) = self.tabs.iter().position(|t| t.id == id) {
                        if let Some(tab) = self.tabs.get(idx) {
                            let snapshot = directory_settings_snapshot_for_view(&tab.primary_view);
                            let _ = persist_directory_settings_snapshot(
                                &mut self.settings_window.current_settings.directory_settings,
                                snapshot,
                            );
                            if let Some(split) = tab.split_view.as_ref() {
                                let snapshot = directory_settings_snapshot_for_view(split);
                                let _ = persist_directory_settings_snapshot(
                                    &mut self.settings_window.current_settings.directory_settings,
                                    snapshot,
                                );
                            }
                        }
                        self.tabs.remove(idx);
                        if self.active_tab >= self.tabs.len() {
                            self.active_tab = self.tabs.len() - 1;
                        }
                        self.focused_split = SplitSide::Primary;
                        if let Some(active_id) = self.tabs.get(self.active_tab).map(|t| t.id) {
                            self.pending_tab_scroll_id = Some(active_id);
                        }
                        self.mark_tab_infos_dirty();
                        self.load_path();
                    }
                } else {
                    let snapshot = directory_settings_snapshot_for_view(&self.tabs[0].primary_view);
                    let _ = persist_directory_settings_snapshot(
                        &mut self.settings_window.current_settings.directory_settings,
                        snapshot,
                    );
                    let (
                        _folder_scanning_enabled,
                        _show_hidden_files_folders,
                        _show_item_viewer_icons,
                        _windows_context_menu_enabled,
                        _window_size_mode,
                        start_path,
                        _saved_theme,
                        _pinned_tabs,
                        _time_format_24h,
                        _sort_column,
                        _sort_ascending,
                        _language,
                        _date_style,
                        _custom_date_format,
                        _item_viewer_file_column_order,
                        _item_viewer_drive_column_order,
                        _recycle_bin_column_order,
                        _item_viewer_file_column_sizes,
                        _item_viewer_drive_column_sizes,
                        _recycle_bin_column_sizes,
                        _directory_settings,
                        _double_click_navigates_up,
                        _show_selection_checkboxes,
                        _middle_click_opens_new_tab,
                        _restore_last_session_tabs,
                        _default_display_mode,
                        _default_search_scope,
                        _search_engine,
                    ) = load_app_settings();
                    self.tabs[0].primary_view.nav = Navigation::new(start_path);
                    self.tabs[0].split_view = None;
                    self.active_tab = 0;
                    self.focused_split = SplitSide::Primary;
                    self.mark_tab_infos_dirty();
                    self.load_path();
                }
            }
            if let Some(reference_id) = action
                .close_others
                .or(action.close_to_right)
                .or(action.close_to_left)
            {
                if let Some(reference_idx) =
                    self.tabs.iter().position(|t| t.id == reference_id)
                {
                    let is_pinned = |t: &TabState| {
                        self.settings_window
                            .current_settings
                            .pinned_tabs
                            .iter()
                            .any(|p| p == &t.primary_view.nav.current)
                    };
                    let should_close = |idx: usize, t: &TabState| -> bool {
                        if idx == reference_idx || is_pinned(t) {
                            return false;
                        }
                        if action.close_others.is_some() {
                            true
                        } else if action.close_to_right.is_some() {
                            idx > reference_idx
                        } else {
                            idx < reference_idx
                        }
                    };

                    let indices_to_close: Vec<usize> = self
                        .tabs
                        .iter()
                        .enumerate()
                        .filter(|(idx, t)| should_close(*idx, t))
                        .map(|(idx, _)| idx)
                        .collect();

                    if !indices_to_close.is_empty() {
                        for &idx in &indices_to_close {
                            let tab = &self.tabs[idx];
                            let snapshot = directory_settings_snapshot_for_view(&tab.primary_view);
                            let _ = persist_directory_settings_snapshot(
                                &mut self.settings_window.current_settings.directory_settings,
                                snapshot,
                            );
                            if let Some(split) = tab.split_view.as_ref() {
                                let snapshot = directory_settings_snapshot_for_view(split);
                                let _ = persist_directory_settings_snapshot(
                                    &mut self.settings_window.current_settings.directory_settings,
                                    snapshot,
                                );
                            }
                        }

                        // Remove back-to-front so earlier indices stay valid.
                        for &idx in indices_to_close.iter().rev() {
                            self.tabs.remove(idx);
                        }

                        let new_active_id = reference_id;
                        self.active_tab = self
                            .tabs
                            .iter()
                            .position(|t| t.id == new_active_id)
                            .unwrap_or(0)
                            .min(self.tabs.len().saturating_sub(1));
                        self.focused_split = SplitSide::Primary;
                        if let Some(active_id) = self.tabs.get(self.active_tab).map(|t| t.id) {
                            self.pending_tab_scroll_id = Some(active_id);
                        }
                        self.mark_tab_infos_dirty();
                        self.load_path();
                    }
                }
            }
            if let Some(path) = action.toggle_pin {
                if self
                    .settings_window
                    .current_settings
                    .pinned_tabs
                    .iter()
                    .any(|p| p == &path)
                {
                    self.settings_window
                        .current_settings
                        .pinned_tabs
                        .retain(|p| p != &path);
                } else {
                    self.settings_window.current_settings.pinned_tabs.push(path);
                }

                self.save_app_settings_to_disk();

                self.mark_tab_infos_dirty();
            }
            if let Some(path) = action.toggle_favorite {
                if self
                    .sidebar_state
                    .favorites
                    .iter()
                    .any(|fav| fav.path == path)
                {
                    self.remove_favorite(&path);
                } else {
                    self.add_favorite_path(path);
                }
            }
            if let Some((query, scope)) = action.save_search {
                let scope_folder = match &scope {
                    SearchScope::CurrentFolder(dir) => Some(dir.clone()),
                    SearchScope::Everywhere => None,
                };
                if self.saved_searches_state.add(query, scope_folder) {
                    self.persist_saved_searches();
                }
            }
            if let Some(target_dir) = action.move_files_to_tab_dir.as_ref() {
                if let Some(sources) = drag_sources {
                    self.move_selected_paths_to_dir(sources, target_dir.clone());
                }
            }
        }
    }

    /// Toggles the active tab's split view: creates a second view (duplicating
    /// the primary view's directory+sort, with its own selection/filter/listing)
    /// if none exists, or clears it if one does.
    pub(crate) fn toggle_split_for_active_tab(&mut self) {
        let has_split = self.active_tab().split_view.is_some();
        if has_split {
            self.active_tab_mut().split_view = None;
            if self.focused_split == SplitSide::Secondary {
                self.focused_split = SplitSide::Primary;
            }
        } else {
            let tab = self.active_tab_mut();
            tab.split_view = Some(tab.primary_view.duplicate_as_new());
            self.focused_split = SplitSide::Secondary;
            self.load_view(SplitSide::Secondary);
        }
    }

    /// Opens `path` as the active tab's split view, creating the split if none
    /// exists yet, or replacing the current split target if one does.
    pub(crate) fn open_path_in_split(&mut self, path: PathBuf) {
        let mut new_view = self
            .active_tab()
            .view(SplitSide::Primary)
            .duplicate_as_new();
        new_view.nav = Navigation::new(path);
        let current_settings = self.settings_window.current_settings.clone();
        apply_directory_settings_to_view(
            &mut new_view,
            &current_settings,
            DisplayModeFallback::Default,
        );
        let tab = self.active_tab_mut();
        tab.split_view = Some(new_view);
        self.focused_split = SplitSide::Secondary;
        self.load_view(SplitSide::Secondary);
    }

    fn move_selected_paths_to_dir(&mut self, sources: &[PathBuf], target_dir: PathBuf) {
        if sources.is_empty() {
            return;
        }

        unsafe {
            let file_op: IFileOperation =
                CoCreateInstance(&FileOperation, None, CLSCTX_ALL).unwrap();

            file_op
                .SetOperationFlags(FOF_SIMPLEPROGRESS | FOF_ALLOWUNDO | FOFX_SHOWELEVATIONPROMPT)
                .ok();

            let target_item: IShellItem = SHCreateItemFromParsingName(
                &HSTRING::from(target_dir.to_string_lossy().to_string()),
                None,
            )
            .unwrap();

            for source in sources {
                let source_item: IShellItem = SHCreateItemFromParsingName(
                    &HSTRING::from(source.to_string_lossy().to_string()),
                    None,
                )
                .unwrap();

                file_op
                    .MoveItem(&source_item, &target_item, None, None)
                    .ok();
            }

            file_op.PerformOperations().ok();
        }

        self.notifications_state.record_finished(
            crate::gui::windows::containers::notifications::FileOpKind::Move,
            sources.len(),
            path_display_label(&target_dir),
            crate::gui::windows::containers::notifications::FileOpStatus::Completed,
        );

        {
            let side = self.focused_split;
            let explorer_state = &mut self.active_tab_mut().view_mut(side).explorer_state;
            explorer_state.selected_paths.clear();
            explorer_state.selection_anchor = None;
            explorer_state.selection_focus = None;
        }
        self.load_path();
    }

    pub fn handle_sidebar_action(
        &mut self,
        sidebar_action: Option<SidebarAction>,
        drag_sources: Option<&[PathBuf]>,
    ) {
        if let Some(action) = sidebar_action {
            if let Some((from, to)) = action.reorder {
                let len = self.sidebar_state.favorites.len();

                if from < len {
                    let item = self.sidebar_state.favorites.remove(from);

                    // Clamp target index AFTER removal
                    let mut target = to;

                    if to > from {
                        target -= 1;
                    }

                    target = target.min(self.sidebar_state.favorites.len());

                    self.sidebar_state.favorites.insert(target, item);
                }

                self.persist_favorites();
            }
            if let Some(path) = action.nav_to {
                // Store current path in navigation history before navigating
                if let Some(parent) = self.current_nav().get_parent() {
                    let current = self.current_nav().current.clone();
                    let side = self.focused_split;
                    self.active_tab_mut()
                        .view_mut(side)
                        .explorer_state
                        .navigation_history
                        .insert(parent, current);
                }

                self.current_nav_mut().go_to(path);
                self.mark_tab_infos_dirty();
                self.load_path();
            }
            if let Some(path) = action.open_new_tab {
                self.open_new_tab(path);
                self.load_path();
            }
            if let Some(path) = action.select_favorite {
                self.sidebar_state.item_clicked = Some(path);
            }
            if let Some(path) = action.remove_favorite {
                self.remove_favorite(&path);
            }
            if let Some(target_dir) = action.move_files_to_sidebar_dir.as_ref() {
                if let Some(sources) = drag_sources {
                    self.move_selected_paths_to_dir(sources, target_dir.clone());
                }
            }
            if action.open_network_browser {
                self.enter_network_address_bar_edit_mode();
            }
            if action.open_settings {
                self.open_or_focus_settings_tab();
            }
            if let Some(group_id) = action.open_tag_view {
                self.open_or_focus_tag_view_tab(group_id);
            }
            if let Some(id) = action.open_saved_search {
                if let Some(item) = self
                    .saved_searches_state
                    .items
                    .iter()
                    .find(|item| item.id == id)
                {
                    let scope = match &item.scope_folder {
                        Some(dir) => SearchScope::CurrentFolder(dir.clone()),
                        None => SearchScope::Everywhere,
                    };
                    self.open_or_focus_search_tab(item.query.clone(), scope);
                }
            }
            if let Some(id) = action.remove_saved_search {
                self.saved_searches_state.remove(id);
                self.persist_saved_searches();
            }
            if let Some(path) = action.remove_recent_location {
                self.recent_locations_state.remove(&path);
                self.persist_recent_locations();
            }
            if action.clear_recent_locations {
                self.recent_locations_state.clear();
                self.persist_recent_locations();
            }
        }
    }

    pub fn handle_topbar_action(&mut self, topbar_action: Option<TopbarAction>) {
        if let Some(action) = topbar_action {
            if action.toggle_theme {
                self.theme = match self.theme {
                    ThemeMode::Dark => ThemeMode::Light,
                    ThemeMode::Light => ThemeMode::Dark,
                };
                self.theme_dirty = true;

                // Save the theme setting
                self.save_app_settings_to_disk();
            }

            if action.open_settings {
                self.open_or_focus_settings_tab();
            }

            if action.about {
                self.about_window.open = true;
            }

            if action.exit {
                if let Some(hwnd) = self.hwnd {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                }
            }
            if action.toggle_sidebar {
                self.sidebar_collapsed = !self.sidebar_collapsed;
            }
            if action.toggle_active_tab_split {
                self.toggle_split_for_active_tab();
            }
        }
    }

    fn global_shortcuts_disabled(&self, ctx: &egui::Context) -> bool {
        let topbar_menu_open = ctx.memory(|mem| {
            mem.data
                .get_temp::<bool>(egui::Id::new("topbar_hamburger_menu"))
                .unwrap_or(false)
        });

        let active_tab = self.active_tab();
        self.about_window.open
            || self.tags_state.picker.is_some()
            || self.tags_state.delete_confirmation.is_some()
            || self
                .rename_state
                .as_ref()
                .map(|state| state.validation_error_show)
                .unwrap_or(false)
            || self.sidebar_state.non_ntfs_popup_path.is_some()
            || active_tab
                .primary_view
                .explorer_state
                .non_ntfs_popup_path
                .is_some()
            || active_tab
                .split_view
                .as_ref()
                .map(|view| view.explorer_state.non_ntfs_popup_path.is_some())
                .unwrap_or(false)
            || topbar_menu_open
    }

    fn enter_address_bar_edit_mode(&mut self) {
        let current_path = self.current_nav().current.clone();
        let side = self.focused_split;
        let view = self.active_tab_mut().view_mut(side);

        view.breadcrumb_path_editing = true;
        view.breadcrumb_path_buffer = current_path.to_string_lossy().to_string();
        view.breadcrumb_just_started_editing = false;
        view.breadcrumb_select_all_on_focus = true;
        view.breadcrumb_path_error = false;
        view.breadcrumb_path_error_animation_time = 0.0;
    }

    /// Opens the address bar pre-filled with `\\` so the user can type a server or
    /// share name (e.g. `\\SERVER` or `\\SERVER\Share`) to browse a network location.
    fn enter_network_address_bar_edit_mode(&mut self) {
        let side = self.focused_split;
        let view = self.active_tab_mut().view_mut(side);

        view.breadcrumb_path_editing = true;
        view.breadcrumb_path_buffer = r"\\".to_string();
        view.breadcrumb_just_started_editing = false;
        view.breadcrumb_select_all_on_focus = false;
        view.breadcrumb_path_error = false;
        view.breadcrumb_path_error_animation_time = 0.0;
    }

    fn selected_path_for_rename(&self) -> Option<PathBuf> {
        let view = self.active_tab().view(self.focused_split);

        if let Some(focus_idx) = view.explorer_state.selection_focus
            && focus_idx < view.item_viewer_filter_state.cached_indices.len()
        {
            let file_idx = view.item_viewer_filter_state.cached_indices[focus_idx];
            let focused_path = view.files.get(file_idx)?.path.clone();
            if view.explorer_state.selected_paths.contains(&focused_path) {
                return Some(focused_path);
            }
        }

        let mut selected_paths: Vec<PathBuf> =
            view.explorer_state.selected_paths.iter().cloned().collect();
        selected_paths.sort();
        selected_paths.into_iter().next()
    }

    fn selected_paths_for_properties(&self) -> Option<Vec<PathBuf>> {
        let view = self.active_tab().view(self.focused_split);
        if view.explorer_state.selected_paths.is_empty() {
            return None;
        }

        let mut paths: Vec<PathBuf> = view.explorer_state.selected_paths.iter().cloned().collect();
        paths.sort();
        paths.dedup();
        Some(paths)
    }

    pub fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
        if self.global_shortcuts_disabled(ctx) {
            return;
        }

        let shortcuts = ctx.input(|input| {
            let ctrl = input.modifiers.ctrl;
            let alt = input.modifiers.alt;
            let shift = input.modifiers.shift;

            (
                ctrl && shift && input.key_pressed(egui::Key::Tab),
                ctrl && input.key_pressed(egui::Key::Tab),
                ctrl && input.key_pressed(egui::Key::T),
                ctrl && input.key_pressed(egui::Key::W),
                ctrl && shift && input.key_pressed(egui::Key::N),
                ctrl && input.key_pressed(egui::Key::R),
                input.key_pressed(egui::Key::F5),
                input.key_pressed(egui::Key::F1),
                alt && input.key_pressed(egui::Key::D),
                input.key_pressed(egui::Key::F2),
                alt && input.key_pressed(egui::Key::Enter),
                ctrl && input.key_pressed(egui::Key::F),
                ctrl && !shift && input.key_pressed(egui::Key::Z),
                ctrl && input.key_pressed(egui::Key::Y),
                ctrl && shift && input.key_pressed(egui::Key::Z),
            )
        });

        if shortcuts.0 {
            self.activate_tab_relative(-1);
            return;
        }

        if shortcuts.1 {
            self.activate_tab_relative(1);
            return;
        }

        if shortcuts.2 {
            let action = TabsAction {
                open_new: true,
                ..Default::default()
            };
            self.handle_tabs_action(Some(action), None);
            return;
        }

        if shortcuts.3 {
            let action = TabsAction {
                close: Some(self.active_tab().id),
                ..Default::default()
            };
            self.handle_tabs_action(Some(action), None);
            return;
        }

        if shortcuts.4 {
            self.create_new_folder();
            return;
        }

        if shortcuts.5 || shortcuts.6 {
            self.load_path();
            return;
        }

        if shortcuts.7 {
            self.toggle_fullscreen();
        }

        if shortcuts.8 {
            self.enter_address_bar_edit_mode();
            return;
        }

        if shortcuts.9 {
            if self.rename_state.is_none()
                && !self
                    .active_tab()
                    .view(self.focused_split)
                    .breadcrumb_path_editing
            {
                let selected_count = self
                    .active_tab()
                    .view(self.focused_split)
                    .explorer_state
                    .selected_paths
                    .len();

                if selected_count > 1 {
                    let mut paths: Vec<PathBuf> = self
                        .active_tab()
                        .view(self.focused_split)
                        .explorer_state
                        .selected_paths
                        .iter()
                        .cloned()
                        .collect();
                    paths.sort();
                    self.handle_context_action(ItemViewerContextAction::BulkRenameRequest(paths));
                } else if let Some(path) = self.selected_path_for_rename() {
                    let action = ItemViewerAction::StartEdit(path);
                    handle_pending_actions(Some(action), self);
                }
            }
            return;
        }

        if shortcuts.10 {
            if !self
                .active_tab()
                .view(self.focused_split)
                .breadcrumb_path_editing
                && let Some(paths) = self.selected_paths_for_properties()
            {
                self.open_properties_multi(&paths);
            }
        }

        if shortcuts.11 {
            self.enter_search_box_edit_mode();
        }

        // Ctrl+Z/Ctrl+Y/Ctrl+Shift+Z must not also fire while a rename/
        // bulk-rename dialog is actively open - same guard as F2 above.
        let rename_ui_open =
            self.rename_state.is_some() || self.pending_bulk_rename.is_some();

        if shortcuts.12 && !rename_ui_open {
            self.undo();
            return;
        }

        if (shortcuts.13 || shortcuts.14) && !rename_ui_open {
            self.redo();
            return;
        }
    }

    fn activate_tab_relative(&mut self, delta: isize) {
        if self.tabs.is_empty() {
            return;
        }

        let len = self.tabs.len() as isize;
        let next = (self.active_tab as isize + delta).rem_euclid(len) as usize;
        let id = self.tabs[next].id;
        let action = TabsAction {
            activate: Some(id),
            ..Default::default()
        };
        self.handle_tabs_action(Some(action), None);
    }

    fn toggle_fullscreen(&mut self) {
        if let Some(hwnd) = self.hwnd {
            toggle_window_fullscreen(hwnd);
        }
    }

    pub fn handle_throttle_size_requests(&mut self, ctx: &egui::Context) {
        // Throttle size requests to keep UI responsive
        let should_pause =
            ctx.input(|i| i.pointer.any_down() || i.smooth_scroll_delta.y.abs() > 0.0);
        if should_pause {
            return;
        }
        self.handle_throttle_size_requests_for(SplitSide::Primary);
        if self.active_tab().split_view.is_some() {
            self.handle_throttle_size_requests_for(SplitSide::Secondary);
        }
    }

    fn handle_throttle_size_requests_for(&mut self, side: SplitSide) {
        let view = self.active_tab_mut().view_mut(side);
        if view.size_req_tx.is_none() {
            return;
        }
        for _ in 0..6 {
            match view.pending_size_queue.pop_front() {
                Some(path) => {
                    let _ = view.size_req_tx.as_ref().unwrap().send(path);
                }
                None => break,
            }
        }
    }

    pub fn handle_directory_size_updates(&mut self, ctx: &egui::Context) {
        let mut any_updated = self.handle_directory_size_updates_for(SplitSide::Primary);
        if self.active_tab().split_view.is_some() {
            any_updated |= self.handle_directory_size_updates_for(SplitSide::Secondary);
        }
        if any_updated {
            ctx.request_repaint();
        }
    }

    fn handle_directory_size_updates_for(&mut self, side: SplitSide) -> bool {
        if self.active_tab().view(side).size_rx.is_none() {
            return false;
        }

        let mut updated = false;
        for _ in 0..128 {
            let received = self
                .active_tab()
                .view(side)
                .size_rx
                .as_ref()
                .unwrap()
                .try_recv();
            match received {
                Ok((path, size, done)) => {
                    if done {
                        self.active_tab_mut()
                            .view_mut(side)
                            .pending_size_set
                            .remove(&path);
                    }
                    self.folder_sizes.insert(
                        path.clone(),
                        ItemViewerFolderSizeState { bytes: size, done },
                    );
                    let view = self.active_tab_mut().view_mut(side);
                    if let Some(item) = view.files.iter_mut().find(|f| f.path == path) {
                        item.file_size = Some(size);
                        updated = true;
                    }
                }
                Err(_) => break,
            }
        }

        if updated {
            let view = self.active_tab_mut().view_mut(side);
            sort_files_by_keys(&mut view.files, &view.sort_keys);
        }
        updated
    }

    pub fn handle_directory_batch_recieve(&mut self, ctx: &egui::Context) {
        let mut any_updated = self.handle_directory_batch_recieve_for(SplitSide::Primary);
        if self.active_tab().split_view.is_some() {
            any_updated |= self.handle_directory_batch_recieve_for(SplitSide::Secondary);
        }
        if any_updated {
            ctx.request_repaint();
        }
    }

    fn handle_directory_batch_recieve_for(&mut self, side: SplitSide) -> bool {
        if self.active_tab().view(side).rx.is_none() {
            return false;
        }

        let mut any_change = false;
        let mut batch = Vec::with_capacity(128);
        let mut disconnected = false;

        {
            let view = self.active_tab().view(side);
            let rx = view.rx.as_ref().unwrap();
            for _ in 0..128 {
                match rx.try_recv() {
                    Ok(item) => batch.push(item),
                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }

        if !batch.is_empty() {
            let folder_scanning_enabled = self
                .settings_window
                .current_settings
                .folder_scanning_enabled;
            for item in batch.iter() {
                if item.is_dir && folder_scanning_enabled {
                    // Only set up folder size tracking if scanning is enabled
                    self.folder_sizes.entry(item.path.clone()).or_insert(
                        ItemViewerFolderSizeState {
                            bytes: 0,
                            done: false,
                        },
                    );
                    let view = self.active_tab_mut().view_mut(side);
                    if view.pending_size_set.insert(item.path.clone()) {
                        view.pending_size_queue.push_back(item.path.clone());
                    }
                }
            }

            let view = self.active_tab_mut().view_mut(side);
            view.files.extend(batch);
            sort_files_by_keys(&mut view.files, &view.sort_keys);
            any_change = true;
        }

        if disconnected {
            let view = self.active_tab_mut().view_mut(side);
            view.rx = None;
            view.is_loading = false;
            any_change = true;
        }

        any_change
    }
}

unsafe fn pwstr_to_string(pw: windows::core::PWSTR) -> String {
    let mut len = 0usize;
    let mut ptr = pw.0;
    while !ptr.is_null() && unsafe { *ptr } != 0 {
        len += 1;
        ptr = unsafe { ptr.add(1) };
    }
    let slice = unsafe { std::slice::from_raw_parts(pw.0, len) };
    String::from_utf16_lossy(slice)
}

fn split_parent(paths: &[PathBuf]) -> Option<(PathBuf, Vec<PathBuf>)> {
    if paths.is_empty() {
        return None;
    }

    let parent = paths[0].parent()?.to_path_buf();

    // Ensure all share same parent (Explorer requirement)
    if !paths.iter().all(|p| p.parent() == Some(parent.as_path())) {
        return None;
    }

    let children: Vec<PathBuf> = paths
        .iter()
        .filter_map(|p| p.file_name().map(PathBuf::from))
        .collect();

    Some((parent, children))
}

pub fn create_data_object(paths: &[PathBuf]) -> Option<IDataObject> {
    let (parent, children) = split_parent(paths)?;

    unsafe {
        // Parent PIDL
        let parent_w: Vec<u16> = parent.as_os_str().encode_wide().chain(Some(0)).collect();
        let parent_pidl = ILCreateFromPathW(PCWSTR(parent_w.as_ptr()));
        if parent_pidl.is_null() {
            return None;
        }

        let mut child_pidls: Vec<*const ITEMIDLIST> = Vec::new();
        let mut full_pidls: Vec<*mut ITEMIDLIST> = Vec::new();

        for child in children {
            let full = parent.join(child);
            let wide: Vec<u16> = full.as_os_str().encode_wide().chain(Some(0)).collect();

            let full_pidl = ILCreateFromPathW(PCWSTR(wide.as_ptr()));
            if !full_pidl.is_null() {
                // 🔥 Convert to relative PIDL
                let rel = ILFindLastID(full_pidl);
                child_pidls.push(rel);
                full_pidls.push(full_pidl);
            }
        }

        let result: Result<IDataObject> =
            SHCreateDataObject(Some(parent_pidl), Some(&child_pidls), None);

        for full_pidl in full_pidls {
            CoTaskMemFree(Some(full_pidl as _));
        }
        CoTaskMemFree(Some(parent_pidl as _));

        result.ok()
    }
}

pub fn calculate_folder_sizes_parallel(
    req_rx: Receiver<PathBuf>,
    done_tx: Sender<(PathBuf, u64, bool)>,
    shutdown: Arc<AtomicBool>,
    num_threads: usize,
) -> Vec<std::thread::JoinHandle<()>> {
    let req_rx = Arc::new(req_rx);
    let mut handles = Vec::with_capacity(num_threads);

    for _ in 0..num_threads {
        let rx = Arc::clone(&req_rx);
        let tx = done_tx.clone();
        let shutdown = Arc::clone(&shutdown);

        let handle = thread::spawn(move || {
            // Loop until channel closes or shutdown is triggered
            while !shutdown.load(Ordering::Relaxed) {
                // Use blocking recv; will wake immediately on a message or channel close
                let path = match rx.recv() {
                    Ok(p) => p,
                    Err(_) => break, // Channel closed
                };

                // Optional: check shutdown inside heavy work
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }

                parallel_directory_scan(path, tx.clone());

                // Optional: if parallel_directory_scan is very CPU heavy,
                // you can add a small yield:
                // thread::yield_now();
            }
        });

        handles.push(handle);
    }

    handles
}

pub fn handle_pending_actions(pending_action: Option<ItemViewerAction>, explorer: &mut MainWindow) {
    if let Some(action) = pending_action {
        let side = explorer.focused_split;
        let is_drive_view = explorer.current_nav().is_root();
        let is_recycle_bin_view = explorer.current_nav().is_recycle_bin();

        if is_recycle_bin_view {
            match &action {
                ItemViewerAction::CreateFolder
                | ItemViewerAction::CreateFile
                | ItemViewerAction::OpenTerminal
                | ItemViewerAction::Open(_)
                | ItemViewerAction::OpenWithDefault(_)
                | ItemViewerAction::OpenInNewTab(_)
                | ItemViewerAction::OpenInSplitView(_)
                | ItemViewerAction::StartEdit(_)
                | ItemViewerAction::FilesDropped(_)
                | ItemViewerAction::MoveItems { .. }
                | ItemViewerAction::Context(ItemViewerContextAction::Copy(_))
                | ItemViewerAction::Context(ItemViewerContextAction::CopyPath(_))
                | ItemViewerAction::Context(ItemViewerContextAction::Paste)
                | ItemViewerAction::Context(ItemViewerContextAction::AddTag(_))
                | ItemViewerAction::Context(ItemViewerContextAction::RemoveTag(_))
                | ItemViewerAction::Context(ItemViewerContextAction::RemoveTagFromGroup(_, _))
                | ItemViewerAction::Context(ItemViewerContextAction::AddFavorite(_))
                | ItemViewerAction::Context(ItemViewerContextAction::RenameRequest(_, _))
                | ItemViewerAction::Context(ItemViewerContextAction::RenameCancel) => {
                    return;
                }
                _ => {}
            }
        }

        match action {
            ItemViewerAction::Sort {
                column,
                additive,
                remove,
            } => explorer.toggle_sort(column, additive, remove),
            ItemViewerAction::ToggleColumnVisibility(column) => {
                let new_order = {
                    let view = explorer.active_tab_mut().view_mut(side);
                    let column_state = &mut view.column_state;

                    if column_state.toggle_column_visibility(
                        is_drive_view,
                        is_recycle_bin_view,
                        column,
                    ) {
                        Some(
                            column_state
                                .order(is_drive_view, is_recycle_bin_view)
                                .to_vec(),
                        )
                    } else {
                        None
                    }
                };

                if let Some(order) = new_order {
                    explorer.apply_item_viewer_column_order(
                        side,
                        is_drive_view,
                        is_recycle_bin_view,
                        &order,
                    );
                    let snapshot =
                        directory_settings_snapshot_for_view(explorer.active_tab().view(side));
                    let _ = persist_directory_settings_snapshot(
                        &mut explorer.settings_window.current_settings.directory_settings,
                        snapshot,
                    );
                }
            }
            ItemViewerAction::FitColumn(column) => {
                let view = explorer.active_tab_mut().view_mut(side);

                view.column_state.pending_fit_request =
                    Some(ItemViewerColumnFitRequest::Column(column));

                view.column_state.layout_generation =
                    view.column_state.layout_generation.wrapping_add(1);
            }

            ItemViewerAction::FitAllColumns => {
                let view = explorer.active_tab_mut().view_mut(side);

                view.column_state.pending_fit_request = Some(ItemViewerColumnFitRequest::All);

                view.column_state.layout_generation =
                    view.column_state.layout_generation.wrapping_add(1);
            }
            ItemViewerAction::CreateFolder => explorer.create_new_folder(),
            ItemViewerAction::CreateFile => explorer.create_new_file(),
            ItemViewerAction::RefreshCurrentDirectory => {
                clear_clipboard_files();
                explorer.load_path();
            }
            ItemViewerAction::OpenTerminal => {
                let current_dir = explorer.current_nav().current.clone();
                open_default_terminal(&current_dir);
            }
            ItemViewerAction::MoveColumnLeft(column) => {
                let moved = {
                    let view = explorer.active_tab_mut().view_mut(side);
                    view.column_state
                        .move_column(is_drive_view, is_recycle_bin_view, column, -1)
                };
                if moved {
                    let order = explorer
                        .active_tab()
                        .view(side)
                        .column_state
                        .order(is_drive_view, is_recycle_bin_view)
                        .to_vec();
                    explorer.apply_item_viewer_column_order(
                        side,
                        is_drive_view,
                        is_recycle_bin_view,
                        &order,
                    );
                    let snapshot =
                        directory_settings_snapshot_for_view(explorer.active_tab().view(side));
                    let _ = persist_directory_settings_snapshot(
                        &mut explorer.settings_window.current_settings.directory_settings,
                        snapshot,
                    );
                }
            }
            ItemViewerAction::MoveColumnRight(column) => {
                let moved = {
                    let view = explorer.active_tab_mut().view_mut(side);
                    view.column_state
                        .move_column(is_drive_view, is_recycle_bin_view, column, 1)
                };
                if moved {
                    let order = explorer
                        .active_tab()
                        .view(side)
                        .column_state
                        .order(is_drive_view, is_recycle_bin_view)
                        .to_vec();
                    explorer.apply_item_viewer_column_order(
                        side,
                        is_drive_view,
                        is_recycle_bin_view,
                        &order,
                    );
                    let snapshot =
                        directory_settings_snapshot_for_view(explorer.active_tab().view(side));
                    let _ = persist_directory_settings_snapshot(
                        &mut explorer.settings_window.current_settings.directory_settings,
                        snapshot,
                    );
                }
            }
            ItemViewerAction::MoveColumnToStart(column) => {
                let moved = {
                    let view = explorer.active_tab_mut().view_mut(side);
                    view.column_state.move_column_to_edge(
                        is_drive_view,
                        is_recycle_bin_view,
                        column,
                        true,
                    )
                };
                if moved {
                    let order = explorer
                        .active_tab()
                        .view(side)
                        .column_state
                        .order(is_drive_view, is_recycle_bin_view)
                        .to_vec();
                    explorer.apply_item_viewer_column_order(
                        side,
                        is_drive_view,
                        is_recycle_bin_view,
                        &order,
                    );
                    let snapshot =
                        directory_settings_snapshot_for_view(explorer.active_tab().view(side));
                    let _ = persist_directory_settings_snapshot(
                        &mut explorer.settings_window.current_settings.directory_settings,
                        snapshot,
                    );
                }
            }
            ItemViewerAction::MoveColumnToEnd(column) => {
                let moved = {
                    let view = explorer.active_tab_mut().view_mut(side);
                    view.column_state.move_column_to_edge(
                        is_drive_view,
                        is_recycle_bin_view,
                        column,
                        false,
                    )
                };
                if moved {
                    let order = explorer
                        .active_tab()
                        .view(side)
                        .column_state
                        .order(is_drive_view, is_recycle_bin_view)
                        .to_vec();
                    explorer.apply_item_viewer_column_order(
                        side,
                        is_drive_view,
                        is_recycle_bin_view,
                        &order,
                    );
                    let snapshot =
                        directory_settings_snapshot_for_view(explorer.active_tab().view(side));
                    let _ = persist_directory_settings_snapshot(
                        &mut explorer.settings_window.current_settings.directory_settings,
                        snapshot,
                    );
                }
            }
            ItemViewerAction::Select(path) => {
                let idx = {
                    let view = explorer.active_tab().view(explorer.focused_split);
                    view.item_viewer_filter_state
                        .cached_indices
                        .iter()
                        .position(|&i| view.files[i].path == path)
                        .or_else(|| view.files.iter().position(|f| f.path == path))
                };

                let side = explorer.focused_split;
                let view = explorer.active_tab_mut().view_mut(side);
                view.explorer_state.selected_paths.insert(path.clone());
                if let Some(idx) = idx {
                    view.explorer_state.selection_anchor = Some(idx);
                    view.explorer_state.selection_focus = Some(idx);
                }
            }
            ItemViewerAction::Deselect(path) => {
                let side = explorer.focused_split;
                explorer
                    .active_tab_mut()
                    .view_mut(side)
                    .explorer_state
                    .selected_paths
                    .remove(&path);
            }
            ItemViewerAction::SelectAll => {
                let selected: Vec<PathBuf> = {
                    let view = explorer.active_tab().view(explorer.focused_split);
                    view.item_viewer_filter_state
                        .cached_indices
                        .iter()
                        .map(|&idx| view.files[idx].path.clone())
                        .collect()
                };
                let side = explorer.focused_split;
                let view = explorer.active_tab_mut().view_mut(side);
                view.explorer_state.selected_paths.clear();
                view.explorer_state.selected_paths.extend(selected);
            }
            ItemViewerAction::DeselectAll => {
                let side = explorer.focused_split;
                explorer
                    .active_tab_mut()
                    .view_mut(side)
                    .explorer_state
                    .selected_paths
                    .clear();
            }
            ItemViewerAction::RangeSelect(paths) => {
                // Set selection_focus to the edge of the range that is farthest from the anchor
                let new_focus = {
                    let view = explorer.active_tab().view(explorer.focused_split);
                    if let Some(anchor_idx) = view.explorer_state.selection_anchor {
                        if let (Some(first_path), Some(last_path)) = (paths.first(), paths.last()) {
                            // Check if we're in a filtered view
                            let is_filtered = (view.item_viewer_filter_state.active
                                && !view.item_viewer_filter_state.query.is_empty())
                                || view.item_viewer_filter_state.cached_indices.len()
                                    != view.files.len();

                            let (first_idx, last_idx) = if is_filtered {
                                // Use filtered indices
                                (
                                    view.item_viewer_filter_state
                                        .cached_indices
                                        .iter()
                                        .position(|&i| &view.files[i].path == first_path)
                                        .unwrap_or(anchor_idx),
                                    view.item_viewer_filter_state
                                        .cached_indices
                                        .iter()
                                        .position(|&i| &view.files[i].path == last_path)
                                        .unwrap_or(anchor_idx),
                                )
                            } else {
                                // Use original file indices (unfiltered view)
                                (
                                    view.files
                                        .iter()
                                        .position(|f| &f.path == first_path)
                                        .unwrap_or(anchor_idx),
                                    view.files
                                        .iter()
                                        .position(|f| &f.path == last_path)
                                        .unwrap_or(anchor_idx),
                                )
                            };

                            // If moving down, focus the last item; if moving up, focus the first item
                            Some(if anchor_idx <= first_idx {
                                last_idx // moved down
                            } else {
                                first_idx // moved up
                            })
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };

                // Clear current selection and add all range-selected files
                let side = explorer.focused_split;
                let view = explorer.active_tab_mut().view_mut(side);
                view.explorer_state.selected_paths.clear();
                for path in &paths {
                    view.explorer_state.selected_paths.insert(path.clone());
                }
                if let Some(focus) = new_focus {
                    view.explorer_state.selection_focus = Some(focus);
                }
            }
            ItemViewerAction::Open(path) => {
                {
                    let side = explorer.focused_split;
                    let view = explorer.active_tab_mut().view_mut(side);
                    view.explorer_state.selected_paths.clear();
                    view.explorer_state.selected_paths.insert(path.clone());
                    view.item_viewer_filter_state.dirty = true;
                    view.item_viewer_filter_state.cached_indices.clear();
                }
                let snapshot = directory_settings_snapshot_for_view(
                    explorer.active_tab().view(explorer.focused_split),
                );
                let _ = persist_directory_settings_snapshot(
                    &mut explorer.settings_window.current_settings.directory_settings,
                    snapshot,
                );

                // Store current path in navigation history before navigating
                if let Some(parent) = explorer.current_nav().get_parent() {
                    let current = explorer.current_nav().current.clone();
                    let side = explorer.focused_split;
                    explorer
                        .active_tab_mut()
                        .view_mut(side)
                        .explorer_state
                        .navigation_history
                        .insert(parent, current);
                }

                explorer.current_nav_mut().go_to(path);
                explorer.mark_tab_infos_dirty();
                explorer.load_path_with_fallback(DisplayModeFallback::PreserveColumnsDrillIn);
            }
            ItemViewerAction::OpenWithDefault(paths) => {
                for path in paths {
                    let path_str = path.to_string_lossy().to_string();
                    let wide_path: Vec<u16> = OsStr::new(&path_str)
                        .encode_wide()
                        .chain(std::iter::once(0))
                        .collect();

                    unsafe {
                        let result = ShellExecuteW(
                            None,
                            PCWSTR::null(),
                            PCWSTR(wide_path.as_ptr()),
                            PCWSTR::null(),
                            PCWSTR::null(),
                            SW_SHOWNORMAL,
                        );

                        if result.0 <= std::ptr::null_mut() {
                            eprintln!("Failed to open file: {}", path.display());
                        }
                    }
                }
            }
            ItemViewerAction::OpenInNewTab(path) => {
                explorer.open_new_tab(path);
                explorer.load_path();
            }
            ItemViewerAction::OpenInSplitView(path) => {
                explorer.open_path_in_split(path);
            }
            ItemViewerAction::Context(action) => {
                explorer.handle_context_action(action);
            }
            ItemViewerAction::StartEdit(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string());
                explorer.rename_state = Some(RenameState {
                    path,
                    new_name: name,
                    should_focus: true,
                    validation_error_show: false,
                });
            }
            ItemViewerAction::ReplaceSelection(path) => {
                let idx = {
                    let view = explorer.active_tab().view(explorer.focused_split);
                    // Check if we're in a filtered view
                    let is_filtered = view.item_viewer_filter_state.active
                        && !view.item_viewer_filter_state.query.is_empty();

                    if is_filtered {
                        // Use filtered indices
                        view.item_viewer_filter_state
                            .cached_indices
                            .iter()
                            .position(|&i| view.files[i].path == path)
                    } else {
                        // Use original file indices (unfiltered view)
                        view.files.iter().position(|f| f.path == path)
                    }
                };

                let side = explorer.focused_split;
                let view = explorer.active_tab_mut().view_mut(side);
                view.explorer_state.selected_paths.clear();
                view.explorer_state.selected_paths.insert(path.clone());
                if let Some(idx) = idx {
                    view.explorer_state.selection_anchor = Some(idx);
                    view.explorer_state.selection_focus = Some(idx);
                }
            }
            ItemViewerAction::FilesDropped(dropped_files) => {
                let valid_files: Vec<PathBuf> =
                    dropped_files.into_iter().filter(|p| p.exists()).collect();

                if valid_files.is_empty() {
                    return;
                }

                explorer.dropped_files = valid_files.clone();

                let current_path = explorer.current_nav().current.clone();

                if let Err(e) = show_copy_move_dialog(valid_files, &current_path) {
                    eprintln!("Failed to show copy/move dialog: {}", e);
                }

                // ✅ Defer refresh (important)
                explorer.dropped_files_pending_ui_refresh = true;
            }
            ItemViewerAction::MoveItems {
                sources,
                target_dir,
            } => {
                // Dropping an item onto the empty background of the folder
                // it's already in (e.g. an accidental small drag past the
                // 4px threshold) resolves `target_dir` to that item's own
                // parent via the "drop on background = current dir" fallback
                // in itemviewer.rs/itemviewer_gallery.rs. Moving something
                // into the folder it's already in is a no-op, not a real
                // move - filter those out before either the conflict check
                // (source == dest isn't a "conflict" to ask Replace/Skip/
                // Rename about) or the native call used to hit this exact
                // case and surface a raw "source and destination filenames
                // are the same" Windows dialog.
                let sources: Vec<PathBuf> = sources
                    .into_iter()
                    .filter(|s| s.parent() != Some(target_dir.as_path()))
                    .collect();
                if sources.is_empty() {
                    return;
                }

                // Route drag-and-drop moves through the same conflict-check
                // + robocopy pipeline as clipboard paste (`paste_clipboard_native`)
                // instead of calling `IFileOperation::MoveItem` directly. The
                // native call used to let `IFileOperation` show its own
                // "confirm replace" dialog on a name collision - a jarring,
                // differently-styled prompt that also bypassed the app's
                // Replace/Skip/Rename choices entirely. Reusing this pipeline
                // gives drag-and-drop the exact same themed conflict modal,
                // safe rename-on-collision staging, and progress notification
                // that paste already has.
                let before_entries = MainWindow::directory_child_paths(&target_dir);
                let conflicting_names: Vec<String> = sources
                    .iter()
                    .filter_map(|p| {
                        let name = p.file_name()?.to_string_lossy().to_string();
                        target_dir.join(&name).exists().then_some(name)
                    })
                    .collect();

                if !conflicting_names.is_empty() {
                    explorer.pending_paste_conflict = Some(PasteConflictPrompt {
                        paths: sources,
                        target_dir,
                        before_entries,
                        is_cut: true,
                        side,
                        conflicting_names,
                    });
                } else {
                    explorer.start_robocopy_paste(
                        sources,
                        target_dir,
                        before_entries,
                        true,
                        side,
                        HashMap::new(),
                        HashMap::new(),
                        PasteOrigin::UserAction,
                    );
                }

                {
                    let view = explorer.active_tab_mut().view_mut(side);
                    view.explorer_state.selected_paths.clear();
                    view.explorer_state.selection_anchor = None;
                    view.explorer_state.selection_focus = None;
                }
            }
            ItemViewerAction::ColumnSizesChanged => {
                let snapshot =
                    directory_settings_snapshot_for_view(explorer.active_tab().view(side));
                let _ = persist_directory_settings_snapshot(
                    &mut explorer.settings_window.current_settings.directory_settings,
                    snapshot,
                );

                explorer.save_app_settings_to_disk();
            }
            ItemViewerAction::RunCustomCommand { entry_id, paths } => {
                fn find_entry(
                    entries: &[crate::core::context_menu_settings::CustomContextMenuEntry],
                    id: u64,
                ) -> Option<&crate::core::context_menu_settings::CustomContextMenuEntry>
                {
                    for entry in entries {
                        if entry.id == id {
                            return Some(entry);
                        }
                        if let Some(found) = find_entry(&entry.children, id) {
                            return Some(found);
                        }
                    }
                    None
                }

                if let Some(entry) = find_entry(
                    &explorer
                        .settings_window
                        .current_settings
                        .custom_context_menu,
                    entry_id,
                ) {
                    let context_dir = explorer.current_nav().current.clone();
                    crate::core::context_menu_settings::run(entry, &paths, &context_dir);

                    // The launched program runs detached and may never take
                    // window focus at all (a short-lived script, or one that
                    // never shows a window), so queue a refresh on a timer
                    // rather than relying solely on focus regain.
                    let now = std::time::Instant::now();
                    explorer
                        .pending_command_refreshes
                        .push(now + std::time::Duration::from_millis(900));
                }
            }
        }
    }
}

impl MainWindow {
    /// Applies an action produced by the theme editor - now embedded in the
    /// Settings page's Appearance category instead of its own floating
    /// window, but the underlying apply/persist logic is unchanged.
    pub(crate) fn apply_theme_customizer_action(
        &mut self,
        ctx: &egui::Context,
        action: ThemeCustomizerAction,
    ) {
        let current_mode = self.theme;
        let theme_customizer = &mut self.theme_customizer;

        match action {
            ThemeCustomizerAction::ThemeUpdated(mode) => {
                let updated = match mode {
                    ThemeMode::Dark => theme_customizer.dark_palette.clone(),
                    ThemeMode::Light => theme_customizer.light_palette.clone(),
                };
                apply_font_to_context(ctx, &updated);
                set_palette(mode, updated);
                save_theme_settings(
                    &theme_customizer.light_palette,
                    &theme_customizer.dark_palette,
                );

                if mode == current_mode {
                    self.theme_dirty = true;
                }
            }
            ThemeCustomizerAction::ResetToDefaults(mode) => {
                let default = get_default_palette(mode);
                match mode {
                    ThemeMode::Dark => theme_customizer.dark_palette = default.clone(),
                    ThemeMode::Light => theme_customizer.light_palette = default.clone(),
                }
                if mode == current_mode {
                    apply_font_to_context(ctx, &default);
                }
                set_palette(mode, default);
                save_theme_settings(
                    &theme_customizer.light_palette,
                    &theme_customizer.dark_palette,
                );

                if mode == current_mode {
                    self.theme_dirty = true;
                }
            }
            ThemeCustomizerAction::ExportTheme(mode) => {
                let palette_to_export = match mode {
                    ThemeMode::Dark => &theme_customizer.dark_palette,
                    ThemeMode::Light => &theme_customizer.light_palette,
                };

                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Theme JSON", &["json"])
                    .set_file_name(match mode {
                        ThemeMode::Dark => "eden_theme_dark.json",
                        ThemeMode::Light => "eden_theme_light.json",
                    })
                    .save_file()
                {
                    if let Ok(json) = serde_json::to_string_pretty(palette_to_export) {
                        let _ = std::fs::write(path, json);
                    }
                }
            }
            ThemeCustomizerAction::ImportTheme(mode) => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Theme JSON", &["json"])
                    .pick_file()
                {
                    if let Ok(json) = std::fs::read_to_string(path) {
                        if let Ok(imported) = serde_json::from_str::<ThemePalette>(&json) {
                            match mode {
                                ThemeMode::Dark => theme_customizer.dark_palette = imported.clone(),
                                ThemeMode::Light => {
                                    theme_customizer.light_palette = imported.clone()
                                }
                            }
                            if mode == current_mode {
                                apply_font_to_context(ctx, &imported);
                            }
                            set_palette(mode, imported);
                            save_theme_settings(
                                &theme_customizer.light_palette,
                                &theme_customizer.dark_palette,
                            );

                            if mode == current_mode {
                                self.theme_dirty = true;
                            }
                        }
                    }
                }
            }
            ThemeCustomizerAction::SetLiveMode(mode) => {
                self.theme = mode;
                self.theme_dirty = true;
            }
            ThemeCustomizerAction::CustomThemesChanged => {
                let snapshot = crate::core::indexer::CustomThemesSnapshot {
                    next_id: theme_customizer.custom_themes_next_id,
                    items: theme_customizer.custom_themes.clone(),
                };
                crate::core::indexer::save_custom_themes(&snapshot);
            }
            ThemeCustomizerAction::SidebarWidthChanged(width) => {
                self.sidebar_state.sidebar_default_width = width;
                crate::core::indexer::save_sidebar_sections(
                    &crate::core::indexer::SidebarSectionsSnapshot {
                        places: self.sidebar_state.places_expanded,
                        storage: self.sidebar_state.storage_expanded,
                        favorites: self.sidebar_state.favorites_expanded,
                        tags: self.sidebar_state.tags_expanded,
                        shared_network: self.sidebar_state.shared_network_expanded,
                        saved_searches: self.sidebar_state.saved_searches_expanded,
                        recent_locations: self.sidebar_state.recent_locations_expanded,
                        sidebar_width: width,
                    },
                );
            }
        }
    }
}
