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
use crate::gui::windows::containers::notifications::HistoryAction;
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
use std::collections::VecDeque;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
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

    let snapshot = settings.folder_view(&view.nav.current);

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

/// Open Select by Pattern dialog (see `MainWindow::draw_select_by_pattern_modal`).
pub(crate) struct SelectByPatternState {
    pub side: SplitSide,
    pub pattern: String,
    pub include_folders: bool,
    pub focus_requested: bool,
    /// Indices (into the view's `files`) of the items the pattern matches,
    /// and what they were computed for: (pattern, include folders, number
    /// of files, number of visible items). Matching a big folder every
    /// frame would make typing in the dialog lag.
    pub matches: Vec<usize>,
    pub matches_for: Option<(String, bool, usize, usize)>,
}

#[derive(Clone, Copy)]
enum SelectByPatternMode {
    Replace,
    Add,
    Remove,
}

/// Most folders whose view is remembered; the least recently changed are
/// forgotten first.
const MAX_REMEMBERED_FOLDER_VIEWS: usize = 5000;

/// Whether two folder paths are the same folder: Windows paths are
/// case-insensitive and may or may not end in a separator.
pub(crate) fn same_folder(a: &Path, b: &Path) -> bool {
    // Runs against every remembered folder each time a folder is opened,
    // so avoid allocating for the usual all-ASCII paths.
    let a = a.to_string_lossy();
    let b = b.to_string_lossy();
    let a = a.trim_end_matches(['\\', '/']);
    let b = b.trim_end_matches(['\\', '/']);
    if a.eq_ignore_ascii_case(b) {
        return true;
    }
    if a.is_ascii() && b.is_ascii() {
        return false;
    }
    a.to_lowercase() == b.to_lowercase()
}

impl AppSettings {
    /// The view a folder gets when nothing is remembered for it - the
    /// user's own defaults (Settings), not the app's built-in ones.
    pub(crate) fn default_folder_view(&self, directory: PathBuf) -> DirectorySettingsSnapshot {
        let columns = default_column_state(self);
        DirectorySettingsSnapshot {
            directory,
            item_viewer_file_column_order: columns.file_column_order,
            item_viewer_drive_column_order: columns.drive_column_order,
            recycle_bin_column_order: columns.recycle_bin_column_order,
            item_viewer_file_column_sizes: columns.file_column_sizes,
            item_viewer_drive_column_sizes: columns.drive_column_sizes,
            recycle_bin_column_sizes: columns.recycle_bin_column_sizes,
            filter_query: String::new(),
            display_mode: self.default_display_mode,
            gallery_thumbnail_size: GalleryThumbnailSize::Medium,
            sort_column: self.sort_column,
            sort_ascending: self.sort_ascending,
            sort_keys: vec![SortKey {
                column: self.sort_column,
                ascending: self.sort_ascending,
            }],
        }
    }

    /// The remembered view for `directory`, if per-folder views are on and
    /// one was saved.
    pub(crate) fn folder_view(&self, directory: &Path) -> Option<&DirectorySettingsSnapshot> {
        if !self.ui_prefs.remember_folder_views {
            return None;
        }
        self.directory_settings
            .iter()
            .find(|entry| same_folder(&entry.directory, directory))
    }

    /// Remembers `snapshot` as its folder's view. A view identical to the
    /// defaults isn't stored (and forgets any earlier one), so the folder
    /// follows future changes to the defaults. Returns whether anything
    /// changed.
    pub(crate) fn remember_folder_view(&mut self, snapshot: DirectorySettingsSnapshot) -> bool {
        let changed = self.remember_folder_view_inner(snapshot);
        if changed {
            self.folder_views_revision = self.folder_views_revision.wrapping_add(1);
        }
        changed
    }

    fn remember_folder_view_inner(&mut self, snapshot: DirectorySettingsSnapshot) -> bool {
        if !self.ui_prefs.remember_folder_views {
            return false;
        }
        let position = self
            .directory_settings
            .iter()
            .position(|entry| same_folder(&entry.directory, &snapshot.directory));

        if snapshot == self.default_folder_view(snapshot.directory.clone()) {
            return match position {
                Some(pos) => {
                    self.directory_settings.remove(pos);
                    true
                }
                None => false,
            };
        }

        match position {
            Some(pos) if self.directory_settings[pos] == snapshot => false,
            Some(pos) => {
                // Most recently changed last, so the cap below drops the
                // folders that haven't been touched for longest.
                self.directory_settings.remove(pos);
                self.directory_settings.push(snapshot);
                true
            }
            None => {
                self.directory_settings.push(snapshot);
                if self.directory_settings.len() > MAX_REMEMBERED_FOLDER_VIEWS {
                    let excess = self.directory_settings.len() - MAX_REMEMBERED_FOLDER_VIEWS;
                    self.directory_settings.drain(..excess);
                }
                true
            }
        }
    }

    /// Forgets `directory`'s remembered view.
    pub(crate) fn forget_folder_view(&mut self, directory: &Path) -> bool {
        let before = self.directory_settings.len();
        self.directory_settings
            .retain(|entry| !same_folder(&entry.directory, directory));
        let changed = before != self.directory_settings.len();
        if changed {
            self.folder_views_revision = self.folder_views_revision.wrapping_add(1);
        }
        changed
    }

    /// Makes `snapshot`'s view (display mode, sort, columns) the default for
    /// every folder that has no view of its own.
    pub(crate) fn use_folder_view_as_default(&mut self, snapshot: &DirectorySettingsSnapshot) {
        self.default_display_mode = snapshot.display_mode;
        let primary = snapshot.sort_keys.first().copied().unwrap_or(SortKey {
            column: snapshot.sort_column,
            ascending: snapshot.sort_ascending,
        });
        self.sort_column = primary.column;
        self.sort_ascending = primary.ascending;
        self.item_viewer_file_column_order = snapshot.item_viewer_file_column_order.clone();
        self.item_viewer_file_column_sizes = snapshot.item_viewer_file_column_sizes.clone();
        self.item_viewer_drive_column_order = snapshot.item_viewer_drive_column_order.clone();
        self.item_viewer_drive_column_sizes = snapshot.item_viewer_drive_column_sizes.clone();
        self.recycle_bin_column_order = snapshot.recycle_bin_column_order.clone();
        self.recycle_bin_column_sizes = snapshot.recycle_bin_column_sizes.clone();
        self.forget_folder_view(&snapshot.directory);
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

/// A Send To batch: the same selection (`paths`) queued to be copied (or
/// moved, per `is_cut`) into every folder of a group, one at a time -
/// clicking a Send To group acts on *all* of its folders, not just one
/// you'd otherwise have to pick from a submenu. `remaining` is popped one
/// destination at a time by `MainWindow::advance_send_to_queue`, since a
/// name collision at any given destination needs the same single-slot
/// `pending_paste_conflict` modal a regular paste uses. `sticky_action`
/// implements "Replace All"/"Skip All"/"Rename All" - once the user picks
/// one of those (instead of a plain, one-destination-only Replace/Skip/
/// Rename) on any conflict in this batch, every later conflict in the same
/// batch resolves the same way automatically with no further prompt; see
/// `advance_send_to_queue`'s doc comment for exactly how.
pub struct PendingSendTo {
    pub paths: Vec<PathBuf>,
    pub remaining: VecDeque<PathBuf>,
    pub is_cut: bool,
    pub sticky_action: Option<PasteConflictAction>,
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

/// Everything `MainWindow::finish_robocopy_paste` needs to start tracking a
/// paste-conflict resolution's robocopy jobs, computed entirely on a
/// background thread by `handle_paste_conflict_resolution` - see that
/// method's doc comment for why. `jobs`/`total_bytes` are the direct
/// output of `core::robocopy::build_jobs`, already run off the UI thread.
pub struct ResolvedConflictPaste {
    jobs: Vec<crate::core::robocopy::RobocopyJobSpec>,
    total_bytes: u64,
    paths: Vec<PathBuf>,
    target_dir: PathBuf,
    before_entries: HashSet<PathBuf>,
    is_cut: bool,
    side: SplitSide,
    renames: HashMap<PathBuf, String>,
    replaced: HashMap<PathBuf, Vec<u8>>,
}

/// Free-function core of `MainWindow::delete_paths_native` - pulled out so
/// a background thread (the paste-conflict modal's Replace resolution, see
/// `handle_paste_conflict_resolution`) can call it without needing a
/// `MainWindow` reference, which can't cross a thread boundary. The method
/// never actually read `self` for anything besides the receiver syntax, so
/// this is a pure extraction, not a behavior change.
fn delete_paths_native_standalone(
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

        // Declining the "are you sure?" confirmation doesn't always come back
        // as an error - the operation can "succeed" with everything aborted.
        // Report it the same way as an explicit cancel so callers can tell
        // it apart from a real failure.
        if file_op.GetAnyOperationsAborted()?.as_bool() {
            return Err(user_cancelled_error());
        }
    }

    Ok(())
}

/// Whether two paths name the same item on Windows, where file names are
/// case-insensitive (`C:\a.txt` and `C:\A.TXT` are one file).
fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

/// `HRESULT_FROM_WIN32(ERROR_CANCELLED)` - the error a shell file operation
/// returns when the user cancels it.
const HRESULT_ERROR_CANCELLED: i32 = 0x8007_04C7_u32 as i32;
/// `COPYENGINE_E_USER_CANCELLED` - the copy engine's own "user cancelled".
const HRESULT_COPYENGINE_USER_CANCELLED: i32 = 0x8027_0000_u32 as i32;

fn user_cancelled_error() -> windows::core::Error {
    windows::core::Error::from(windows::core::HRESULT(HRESULT_ERROR_CANCELLED))
}

/// Whether a shell file operation error means the user cancelled it (e.g.
/// answered No to the delete confirmation), as opposed to a real failure.
fn is_user_cancelled(error: &windows::core::Error) -> bool {
    matches!(
        error.code().0,
        HRESULT_ERROR_CANCELLED | HRESULT_COPYENGINE_USER_CANCELLED
    )
}

/// Free-function core of `MainWindow::find_recycled_pidl` - pulled out for
/// the same reason as `delete_paths_native_standalone` above (a background
/// thread can't hold a `MainWindow` reference). `get_shell_item_metadata`
/// needs date/time-formatting settings only to build the *formatted*
/// strings this function immediately discards (it reads only the raw
/// `deleted_time_raw`/`original_object_name`/`original_directory` fields),
/// so passing `DateStyle::default()`/arbitrary formatting args here is
/// safe - the discarded output never reaches anything user-visible.
///
/// This enumerates the *entire* Recycle Bin looking for a name+directory
/// match, which is genuinely slow on a machine whose Recycle Bin has
/// accumulated many items - exactly why callers on the paste-conflict
/// modal's Replace path (`handle_paste_conflict_resolution`) must run this
/// on a background thread rather than the UI thread, where it would
/// otherwise freeze that frame (and the still-visible modal) until this
/// finishes.
fn find_recycled_pidl_standalone(original_dir: &Path, name: &str) -> Option<Vec<u8>> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{
        BHID_EnumItems, FOLDERID_RecycleBinFolder, IEnumShellItems, ILFree, ILGetSize, IShellItem,
        SHCreateItemFromIDList, SHGetIDListFromObject, SHGetKnownFolderIDList,
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

        let enum_items: IEnumShellItems = match recycle_item.BindToHandler(None, &BHID_EnumItems) {
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
                    crate::core::fs::DateStyle::default(),
                    false,
                    "",
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
        self.open_new_tab_with_split(path, None);
    }

    /// Like `open_new_tab`, but when `split_path` is `Some`, the new tab
    /// also opens with a Secondary split-view pane already showing that
    /// second folder - used when opening a `TabGroupEntry` that captured a
    /// dual-pane tab, so reopening the group restores the same layout
    /// rather than just the primary folder.
    pub fn open_new_tab_with_split(&mut self, path: PathBuf, split_path: Option<PathBuf>) {
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
        if let Some(split_path) = split_path {
            let mut split_view = TabView::new(
                Navigation::new(split_path),
                sort_column,
                sort_ascending,
            );
            apply_directory_settings_to_view(
                &mut split_view,
                &current_settings,
                DisplayModeFallback::Default,
            );
            self.tabs.last_mut().unwrap().split_view = Some(split_view);
        }
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
        let _ = self.settings_window.current_settings.remember_folder_view(snapshot);

        self.save_app_settings_to_disk();
    }

    /// Saves settings a couple of seconds after a folder's remembered view
    /// last changed (view mode, sort, column widths ...), so those changes
    /// survive even if the app doesn't exit cleanly. Call once per frame.
    pub(crate) fn save_folder_views_if_due(&mut self, ctx: &egui::Context) {
        const DELAY: std::time::Duration = std::time::Duration::from_secs(2);
        let revision = self.settings_window.current_settings.folder_views_revision;
        if revision == self.saved_folder_views_revision {
            self.folder_views_changed_at = None;
            return;
        }
        let changed_at = *self
            .folder_views_changed_at
            .get_or_insert_with(std::time::Instant::now);
        if changed_at.elapsed() >= DELAY {
            self.save_app_settings_to_disk();
            self.saved_folder_views_revision = revision;
            self.folder_views_changed_at = None;
        } else {
            ctx.request_repaint_after(DELAY);
        }
    }

    /// Re-applies each open view's folder view settings (e.g. after
    /// remembered views were cleared) and reloads the visible ones.
    fn reload_all_views(&mut self) {
        let settings = self.settings_window.current_settings.clone();
        for tab in &mut self.tabs {
            apply_directory_settings_to_view(
                &mut tab.primary_view,
                &settings,
                DisplayModeFallback::Default,
            );
            if let Some(split) = tab.split_view.as_mut() {
                apply_directory_settings_to_view(split, &settings, DisplayModeFallback::Default);
            }
        }
        self.load_view(SplitSide::Primary);
        if self.active_tab().split_view.is_some() {
            self.load_view(SplitSide::Secondary);
        }
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
            self.settings_window
                .current_settings
                .auto_open_notification_panel,
            self.settings_window.current_settings.show_operation_toasts,
        );
        crate::core::context_menu_settings::save_custom_context_menu(
            &self.settings_window.current_settings.custom_context_menu,
            self.settings_window.current_settings.custom_context_menu_enabled,
        );
        crate::core::tab_groups::save_tab_groups(&self.settings_window.current_settings.tab_groups);
        crate::core::send_to::save_send_to(
            &self.settings_window.current_settings.send_to,
            self.settings_window.current_settings.send_to_context_menu_enabled,
        );
        crate::core::indexer::save_tag_icon_style(
            self.settings_window.current_settings.tag_icon_style,
        );
        crate::core::indexer::save_sidebar_visibility(
            &self.settings_window.current_settings.sidebar_visibility,
        );
        crate::core::perf::save_performance_panel_visible(
            self.settings_window.current_settings.show_performance_panel,
        );
        crate::core::ui_prefs::save_ui_prefs(&self.settings_window.current_settings.ui_prefs);
        crate::core::context_menu_order::save_context_menu_order(
            &self.settings_window.current_settings.context_menu_order,
        );
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
            view.load_started_at = None;
            view.size_scan_started_at = None;
            view.size_scan_folders = 0;
            view.is_loading = false;
            view.network_share_error = Arc::new(Mutex::new(None));
            view.scan_token = Arc::new(());
            view.explorer_state.selected_paths.clear();
            view.explorer_state.selection_anchor = None;
            view.explorer_state.selection_focus = None;
            // Don't keep previewing a file from the folder just left.
            if view
                .preview_selection
                .as_ref()
                .is_some_and(|p| p.parent() != Some(current_path.as_path()))
            {
                view.preview_selection = None;
            }
            view.item_viewer_filter_state.dirty = true;
            view.item_viewer_filter_state.cached_indices.clear();
            view.columns_view_state.needs_reload = true;
        }
        // Folder sizes are shared by both panes of a split: keep the other
        // pane's (its folder isn't being reloaded), drop everything else.
        let other_side = match side {
            SplitSide::Primary => SplitSide::Secondary,
            SplitSide::Secondary => SplitSide::Primary,
        };
        let other_dir = (self.active_tab().split_view.is_some())
            .then(|| self.active_tab().view(other_side).nav.current.clone());
        self.folder_sizes
            .retain(|path, _| other_dir.as_deref().is_some_and(|dir| path.parent() == Some(dir)));
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
                let still_wanted = Arc::downgrade(&self.active_tab_mut().view_mut(side).scan_token);
                crate::core::fs::search_builtin_async(
                    query,
                    scope_folder,
                    tx,
                    still_wanted,
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
        let network_share_error = Arc::new(Mutex::new(None));
        scan_dir_async(
            current_path,
            tx,
            self.settings_window.current_settings.date_style,
            self.settings_window.current_settings.time_format_24h,
            self.settings_window.current_settings.custom_date_format.clone(),
            Arc::clone(&network_share_error),
        );
        let folder_scanning_enabled = self
            .settings_window
            .current_settings
            .folder_scanning_enabled;
        let shutdown = Arc::clone(&self.shutdown);
        let view = self.active_tab_mut().view_mut(side);
        view.rx = Some(rx);
        view.is_loading = true;
        view.load_started_at = Some(std::time::Instant::now());
        view.network_share_error = network_share_error;

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

    /// New File > template: creates the file, reloads, selects it, and
    /// starts renaming it (same as the plain New File).
    pub fn create_file_from_template(&mut self, template: &crate::core::templates::Template) {
        if self.current_nav().is_root()
            || self.current_nav().is_recycle_bin()
            || self.current_nav().is_tag_view()
        {
            return;
        }
        let dir = self.current_nav().current.clone();
        match crate::core::templates::create_from_template(&dir, template) {
            Ok(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                self.load_path();
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
                    .pending_selection_paths = Some(vec![path]);
            }
            Err(err) => {
                eprintln!("Couldn't create file from template: {err}");
                let auto_open = self.settings_window.current_settings.auto_open_notification_panel;
                self.notifications_state.record_finished(
                    crate::gui::windows::containers::notifications::FileOpKind::Copy,
                    1,
                    path_display_label(&dir),
                    crate::gui::windows::containers::notifications::FileOpStatus::Failed,
                    auto_open,
                );
            }
        }
    }

    /// Opens the templates folder in a new tab, creating it first.
    pub fn open_templates_folder(&mut self) {
        let custom = self.settings_window.current_settings.ui_prefs.templates_folder.clone();
        let Some(dir) = crate::core::templates::templates_dir(custom.as_deref()) else {
            return;
        };
        let _ = std::fs::create_dir_all(&dir);
        self.open_new_tab(dir);
        self.load_path();
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

    /// Background "Create Shortcut" (an empty-space right-click, matching
    /// Windows' own "New > Shortcut") - prompts for a target file, then
    /// creates a `.lnk` pointing to it in the current directory. Unlike
    /// Windows' own multi-step wizard (browse, then type a name), this
    /// collapses to one step: the native picker's own default name already
    /// seeds a sensible shortcut name via `shortcut_file_name`.
    pub fn create_shortcut_here(&mut self) {
        if self.current_nav().is_root()
            || self.current_nav().is_recycle_bin()
            || self.current_nav().is_tag_view()
        {
            return;
        }

        let Some(target) = crate::gui::windows::windowsoverrides::dialog().pick_file() else {
            return;
        };

        let dir = self.current_nav().current.clone();
        let shortcut_path = crate::core::shortcuts::shortcut_file_name(&target, &dir);

        match crate::core::shortcuts::create_shortcut(&target, &shortcut_path) {
            Ok(()) => {
                let side = self.focused_split;
                self.active_tab_mut()
                    .view_mut(side)
                    .explorer_state
                    .pending_selection_paths = Some(vec![shortcut_path]);
                self.load_path();
            }
            Err(e) => eprintln!("Failed to create shortcut: {:?}", e),
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
                        self.settings_window
                            .current_settings
                            .auto_open_notification_panel,
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

                        self.notifications_state.record_finished(
                            crate::gui::windows::containers::notifications::FileOpKind::Rename,
                            1,
                            String::new(),
                            if renamed_ok {
                                crate::gui::windows::containers::notifications::FileOpStatus::Completed
                            } else {
                                crate::gui::windows::containers::notifications::FileOpStatus::Failed
                            },
                            self.settings_window
                                .current_settings
                                .auto_open_notification_panel,
                        );
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
                    self.settings_window
                        .current_settings
                        .auto_open_notification_panel,
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
                use crate::gui::windows::containers::notifications::FileOpStatus;
                // Whatever the shell API reports, the result on disk is what
                // counts: answering No to the confirmation doesn't reliably
                // come back as an error or an "aborted" flag, so a delete that
                // left every item in place is reported as Cancelled rather
                // than Completed, and one that removed only some as Failed.
                let status_on_disk = |paths: &[PathBuf]| {
                    let remaining = paths.iter().filter(|path| path.exists()).count();
                    if remaining == 0 {
                        FileOpStatus::Completed
                    } else if remaining == paths.len() {
                        FileOpStatus::Cancelled
                    } else {
                        FileOpStatus::Failed
                    }
                };
                let delete_status = match self.delete_paths_native(paths.clone(), allow_undo, false) {
                    Ok(()) => status_on_disk(&paths),
                    // The user answered No to the confirmation: nothing was
                    // deleted, and nothing else may be attempted.
                    Err(e) if is_user_cancelled(&e) => FileOpStatus::Cancelled,
                    Err(e) => {
                        eprintln!("Native delete failed: {:?}", e);
                        // Fallback for a genuinely broken native operation:
                        // only ever the Recycle Bin (which asks for its own
                        // confirmation), never a silent permanent delete.
                        for path in &paths {
                            self.delete_path(path);
                        }
                        status_on_disk(&paths)
                    }
                };
                self.notifications_state.record_finished(
                    crate::gui::windows::containers::notifications::FileOpKind::Delete,
                    paths.len(),
                    destination_label,
                    delete_status,
                    self.settings_window
                        .current_settings
                        .auto_open_notification_panel,
                );

                // Only untag items that are actually gone - a cancelled or
                // partly failed delete must not strip tags from files that
                // are still there.
                let mut tags_changed = false;
                for path in paths.iter().filter(|path| !path.exists()) {
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
            ItemViewerContextAction::CreateShortcut(paths) => {
                let mut created = Vec::new();
                for target in &paths {
                    let Some(parent) = target.parent() else {
                        continue;
                    };
                    let shortcut_path = crate::core::shortcuts::shortcut_file_name(target, parent);
                    match crate::core::shortcuts::create_shortcut(target, &shortcut_path) {
                        Ok(()) => created.push(shortcut_path),
                        Err(e) => eprintln!("Failed to create shortcut: {:?}", e),
                    }
                }
                if !created.is_empty() {
                    let side = self.focused_split;
                    self.active_tab_mut()
                        .view_mut(side)
                        .explorer_state
                        .pending_selection_paths = Some(created);
                    self.load_path();
                }
            }
            ItemViewerContextAction::AnalyzeDiskUsage(path) => {
                self.analyze_disk_usage(path);
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
            ItemViewerContextAction::SendTo(paths, target_dirs, is_cut) => {
                self.send_to_folders(paths, target_dirs, is_cut);
            }
        }
    }

    /// Kicks off copying (or moving, per `is_cut`) `paths` into every one of
    /// `target_dirs` - the Send To feature's own action (see
    /// `core::send_to`), triggered by clicking a group (not an individual
    /// folder): every folder in that group gets its own transfer of the
    /// selection. Queued and processed one destination at a time via
    /// `advance_send_to_queue`, rather than fired off all at once, because a
    /// name collision needs the same Replace/Skip/Rename modal
    /// `paste_clipboard_native` already uses - and that modal has only one
    /// slot (`pending_paste_conflict`), so a second destination's conflict
    /// can't be raised until the first one's is resolved or cancelled.
    fn send_to_folders(&mut self, paths: Vec<PathBuf>, target_dirs: Vec<PathBuf>, is_cut: bool) {
        if paths.is_empty() || target_dirs.is_empty() {
            return;
        }
        self.pending_send_to = Some(PendingSendTo {
            paths,
            remaining: target_dirs.into(),
            is_cut,
            sticky_action: None,
        });
        self.advance_send_to_queue();
    }

    /// Pops the next destination off a pending Send To batch and either
    /// starts it immediately (no name collision) or holds it for the user
    /// via `pending_paste_conflict`, same as a single-destination paste.
    /// Keeps popping and starting destinations with no conflict in one go;
    /// stops at the first one that needs the modal, resuming from
    /// `poll_pending_conflict_resolution` (a clean resolution) or the
    /// paste-conflict modal's own Cancel handler (skips that destination)
    /// once the user has dealt with it. A no-op when no batch is pending -
    /// safe to call after every ordinary paste-conflict resolution too.
    ///
    /// If `pending.sticky_action` is set (the user picked "Replace All"/
    /// "Skip All"/"Rename All" on an earlier conflict in this same batch,
    /// rather than a plain one-destination Replace/Skip/Rename), a
    /// conflicting destination is resolved automatically instead of
    /// stopping for the modal: this reuses the *exact* same
    /// `pending_paste_conflict` + `handle_paste_conflict_resolution` path a
    /// manual click would (so the same safety logic - recycle-before-
    /// replace, safe rename staging - applies identically), just calling
    /// `handle_paste_conflict_resolution` immediately instead of waiting for
    /// a button click. That call is itself async (spawns a background
    /// thread and only sets `pending_conflict_resolution`), so this
    /// function returns right after starting it rather than looping again -
    /// `poll_pending_conflict_resolution` already calls this function once
    /// that resolution lands, continuing the batch (and applying the same
    /// sticky action again if the *next* destination also collides).
    fn advance_send_to_queue(&mut self) {
        loop {
            let Some(pending) = &mut self.pending_send_to else {
                return;
            };
            let Some(target_dir) = pending.remaining.pop_front() else {
                self.pending_send_to = None;
                return;
            };
            let paths = pending.paths.clone();
            let is_cut = pending.is_cut;
            let sticky_action = pending.sticky_action;

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
                if let Some(action) = sticky_action {
                    self.handle_paste_conflict_resolution(action);
                }
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

        self.finish_robocopy_paste(
            jobs,
            total_bytes,
            paths,
            target_dir,
            before_entries,
            is_cut,
            side,
            renames,
            replaced,
            origin,
        );
    }

    /// The lightweight back half of `start_robocopy_paste` - actually kicks
    /// off the background robocopy job and its notification/bookkeeping
    /// entries, once `jobs`/`total_bytes` already exist. Split out so
    /// `poll_pending_conflict_resolution` can call it directly with jobs a
    /// background thread already built, without redoing (or blocking the
    /// UI thread on) `core::robocopy::build_jobs`'s own work.
    #[allow(clippy::too_many_arguments)]
    fn finish_robocopy_paste(
        &mut self,
        jobs: Vec<crate::core::robocopy::RobocopyJobSpec>,
        total_bytes: u64,
        paths: Vec<PathBuf>,
        target_dir: PathBuf,
        before_entries: HashSet<PathBuf>,
        is_cut: bool,
        side: SplitSide,
        renames: HashMap<PathBuf, String>,
        replaced: HashMap<PathBuf, Vec<u8>>,
        origin: PasteOrigin,
    ) {
        if jobs.is_empty() {
            return;
        }

        let notification_id = self.notifications_state.start_operation(
            if is_cut {
                crate::gui::windows::containers::notifications::FileOpKind::Move
            } else {
                crate::gui::windows::containers::notifications::FileOpKind::Copy
            },
            paths.len(),
            path_display_label(&target_dir),
            self.settings_window
                .current_settings
                .auto_open_notification_panel,
        );

        match &origin {
            PasteOrigin::UndoOf(..) => self.notifications_state.mark_history(
                notification_id,
                HistoryAction::Undo,
            ),
            PasteOrigin::RedoOf(..) => self.notifications_state.mark_history(
                notification_id,
                HistoryAction::Redo,
            ),
            _ => {}
        }
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
    /// modal and starts the (possibly filtered/renamed) paste. The actual
    /// resolution work runs entirely on a background thread rather than
    /// inline here - `PasteConflictAction::Replace` recycles the colliding
    /// item via `delete_paths_native_standalone` and then searches for its
    /// Recycle Bin pidl via `find_recycled_pidl_standalone`, which
    /// enumerates the *entire* Recycle Bin and can take a very noticeable
    /// amount of time on a machine that's accumulated many recycled items;
    /// separately, `core::robocopy::build_jobs` (needed for every action,
    /// not just Replace) synchronously walks and sums the size of any
    /// pasted *folder* via `calculate_folder_size_fast`, which is slow for
    /// a large tree. Running either of those directly in this method - the
    /// click handler for the modal's Replace/Rename buttons - blocked the
    /// whole frame from finishing until they completed, and since egui is
    /// immediate-mode, this frame's modal draw calls (queued *before* this
    /// handler ran) don't actually reach the screen until the frame
    /// finishes - so the modal visibly stayed frozen on screen for however
    /// long this took, reported by the user as Replace/Rename "taking a
    /// while before the dialog box hides." Clearing `pending_paste_conflict`
    /// immediately (already the first thing this function does) doesn't
    /// help by itself, since that only changes what the *next* frame draws.
    /// Deferring the slow work to a background thread lets this function
    /// return immediately, so the very next frame already has no modal to
    /// draw; `poll_pending_conflict_resolution` picks up the result once
    /// the thread finishes and starts the actual paste then.
    ///
    /// `Skip` drops every source item whose name collided - if that empties
    /// the list entirely, the resulting job list is empty and
    /// `finish_robocopy_paste` is a no-op, matching "Skip Existing" when
    /// literally everything was a duplicate. `Rename` keeps every item,
    /// computing a fresh `<name>-001`-style name (see
    /// `core::robocopy::next_available_name`) for each one that collided.
    pub fn handle_paste_conflict_resolution(&mut self, action: PasteConflictAction) {
        let Some(prompt) = self.pending_paste_conflict.take() else {
            return;
        };

        let (tx, rx) = crossbeam_channel::bounded(1);

        std::thread::spawn(move || {
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
                    // robocopy overwrite it directly, which would be
                    // permanent data loss with no undo), stashing its
                    // Recycle Bin pidl so undo/redo can restore or
                    // re-recycle it later - see
                    // `find_recycled_pidl_standalone`'s doc comment.
                    for path in &prompt.paths {
                        if is_conflicting(path)
                            && let Some(name) =
                                path.file_name().map(|n| n.to_string_lossy().to_string())
                        {
                            let existing_path = prompt.target_dir.join(&name);
                            if delete_paths_native_standalone(
                                vec![existing_path.clone()],
                                true,
                                true,
                            )
                            .is_ok()
                                && let Some(pidl) =
                                    find_recycled_pidl_standalone(&prompt.target_dir, &name)
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
                            if let Some(name) =
                                path.file_name().map(|n| n.to_string_lossy().to_string())
                            {
                                let new_name = crate::core::robocopy::next_available_name(
                                    &prompt.target_dir,
                                    &name,
                                );
                                renames.insert(path.clone(), new_name);
                            }
                        }
                    }
                    prompt.paths
                }
            };

            let (jobs, total_bytes) = crate::core::robocopy::build_jobs(
                &paths,
                &prompt.target_dir,
                prompt.is_cut,
                &renames,
            );

            let _ = tx.send(ResolvedConflictPaste {
                jobs,
                total_bytes,
                paths,
                target_dir: prompt.target_dir,
                before_entries: prompt.before_entries,
                is_cut: prompt.is_cut,
                side: prompt.side,
                renames,
                replaced,
            });
        });

        self.pending_conflict_resolution = Some(rx);
    }

    /// Picks up a paste-conflict resolution's result once the background
    /// thread `handle_paste_conflict_resolution` spawned finishes, and
    /// starts the actual robocopy job - called once per frame from the
    /// update loop, same shape as `poll_pending_compress`/
    /// `poll_pending_checksum`. A still-running resolution is left alone
    /// for the next frame to check again.
    pub fn poll_pending_conflict_resolution(&mut self) {
        let Some(rx) = &self.pending_conflict_resolution else {
            return;
        };

        let Ok(resolved) = rx.try_recv() else {
            return;
        };

        self.pending_conflict_resolution = None;
        self.finish_robocopy_paste(
            resolved.jobs,
            resolved.total_bytes,
            resolved.paths,
            resolved.target_dir,
            resolved.before_entries,
            resolved.is_cut,
            resolved.side,
            resolved.renames,
            resolved.replaced,
            PasteOrigin::UserAction,
        );
        // No-op unless this resolution was one destination of a Send To
        // batch (`pending_send_to` is only ever `Some` mid-batch) - moves
        // on to the next queued destination, if any.
        self.advance_send_to_queue();
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

        // Only a Send To batch (copying/moving the same selection into
        // several destinations in one go) ever has more than one
        // destination left to resolve conflicts for, so the "All" buttons
        // - which set a sticky resolution for every *remaining* destination
        // in the batch, not just this one - only make sense (and only
        // render) while one is in progress.
        let show_apply_all = self.pending_send_to.is_some();

        let mut resolution: Option<PasteConflictAction> = None;
        let mut apply_to_all = false;
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
                            // A single `ui.horizontal` bounds the row's height to
                            // its actual button content; a bare `ui.with_layout`
                            // used directly as a `vertical()` child (the previous
                            // shape here) let its child `Ui` inherit the *full*
                            // remaining available height from the Area rather
                            // than shrinking to the button row, and centering the
                            // buttons within that inflated rect produced a large
                            // visible gap below them (and the `Area`'s own
                            // content-driven sizing then reserved room for it).
                            // Nesting a second `with_layout` for Cancel inside the
                            // first made it worse; a plain `horizontal` with Cancel
                            // packed left and the other three right-aligned inside
                            // it is the same pattern every other dialog here uses.
                            ui.horizontal(|ui| {
                                if ghost_dialog_button(ui, palette, &self.i18n.tr("cancel"))
                                    .clicked()
                                {
                                    cancelled = true;
                                }
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
                                    },
                                );
                            });

                            // "Apply to all remaining destinations" row -
                            // only shown mid-Send-To-batch (see
                            // `show_apply_all`'s own doc comment above).
                            // Saves clicking Replace/Skip/Rename separately
                            // for every destination a multi-folder Send To
                            // collides at.
                            if show_apply_all {
                                ui.add_space(10.0);
                                ui.separator();
                                ui.add_space(10.0);
                                ui.label(
                                    egui::RichText::new(
                                        self.i18n.tr("paste_conflict_apply_to_all"),
                                    )
                                    .size(palette.text_size)
                                    .color(palette.text_normal.gamma_multiply(0.75)),
                                );
                                ui.add_space(6.0);
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if primary_dialog_button(
                                            ui,
                                            palette,
                                            &self.i18n.tr("paste_conflict_rename_all"),
                                        )
                                        .clicked()
                                        {
                                            resolution = Some(PasteConflictAction::Rename);
                                            apply_to_all = true;
                                        }
                                        ui.add_space(6.0);
                                        if secondary_dialog_button(
                                            ui,
                                            palette,
                                            &self.i18n.tr("paste_conflict_skip_all"),
                                        )
                                        .clicked()
                                        {
                                            resolution = Some(PasteConflictAction::Skip);
                                            apply_to_all = true;
                                        }
                                        ui.add_space(6.0);
                                        if secondary_dialog_button(
                                            ui,
                                            palette,
                                            &self.i18n.tr("paste_conflict_replace_all"),
                                        )
                                        .clicked()
                                        {
                                            resolution = Some(PasteConflictAction::Replace);
                                            apply_to_all = true;
                                        }
                                    },
                                );
                            }
                        });
                    });
            });

        if cancelled {
            self.pending_paste_conflict = None;
            // Cancelling one destination's conflict skips only that
            // destination - the rest of a Send To batch (if any) still
            // continues; a no-op for an ordinary single-destination paste.
            self.advance_send_to_queue();
        } else if let Some(action) = resolution {
            if apply_to_all && let Some(pending) = self.pending_send_to.as_mut() {
                pending.sticky_action = Some(action);
            }
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
    /// The Select by Pattern dialog: wildcard patterns matched against the
    /// names of the items currently shown in one view.
    pub fn draw_select_by_pattern_modal(
        &mut self,
        ctx: &egui::Context,
        palette: &crate::gui::theme::ThemePalette,
    ) {
        use crate::core::utils::widgets::{
            eden_button, modal_frame, modal_icon_header, primary_dialog_button,
        };
        use egui_phosphor::regular;

        let Some(mut state) = self.select_by_pattern.take() else {
            return;
        };

        // The items the view is showing (filters and hidden-file settings
        // already applied) that match - recomputed only when the pattern,
        // the Include Folders option, or the listing changes.
        {
            let view = self.active_tab().view(state.side);
            let visible = &view.item_viewer_filter_state.cached_indices;
            let key = (
                state.pattern.clone(),
                state.include_folders,
                view.files.len(),
                visible.len(),
            );
            if state.matches_for.as_ref() != Some(&key) {
                state.matches = visible
                    .iter()
                    .copied()
                    .filter(|&idx| {
                        let file = &view.files[idx];
                        (state.include_folders || !file.is_dir)
                            && crate::core::pattern::matches_any(&state.pattern, &file.name)
                    })
                    .collect();
                state.matches_for = Some(key);
            }
        }
        let match_count = state.matches.len();

        let mut outcome: Option<SelectByPatternMode> = None;
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));

        egui::Area::new(egui::Id::new("select_by_pattern_scrim"))
            .order(egui::Order::Middle)
            .interactable(true)
            .show(ctx, |ui| {
                let rect = ctx.content_rect();
                ui.painter()
                    .rect_filled(rect, 0.0, palette.modal_background_effect_color);
                if ui
                    .interact(rect, ui.id().with("click"), egui::Sense::click())
                    .clicked()
                {
                    close = true;
                }
            });

        egui::Area::new(egui::Id::new("select_by_pattern_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                modal_frame(&ctx.style_of(ctx.theme()), palette).show(ui, |ui| {
                    ui.set_width(420.0);
                    modal_icon_header(
                        ui,
                        palette,
                        regular::ASTERISK,
                        palette.primary,
                        &self.i18n.tr("select_by_pattern_title"),
                        Some(&self.i18n.tr("select_by_pattern_hint")),
                    );
                    ui.add_space(14.0);

                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut state.pattern)
                            .hint_text("*.jpg; *.png")
                            .desired_width(f32::INFINITY),
                    );
                    if state.focus_requested {
                        edit.request_focus();
                        state.focus_requested = false;
                    }
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        outcome = Some(SelectByPatternMode::Replace);
                    }
                    ui.add_space(8.0);
                    ui.checkbox(
                        &mut state.include_folders,
                        self.i18n.tr("select_by_pattern_include_folders"),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "{} {}",
                            match_count,
                            self.i18n.tr("select_by_pattern_matching")
                        ))
                        .size(palette.text_size - 1.0)
                        .color(palette.text_normal.gamma_multiply(0.75)),
                    );
                    ui.add_space(14.0);

                    ui.horizontal(|ui| {
                        let any = match_count > 0;
                        if ui
                            .add_enabled_ui(any, |ui| {
                                primary_dialog_button(ui, palette, &self.i18n.tr("select_by_pattern_select"))
                            })
                            .inner
                            .clicked()
                        {
                            outcome = Some(SelectByPatternMode::Replace);
                        }
                        if ui
                            .add_enabled_ui(any, |ui| {
                                eden_button(ui, palette, &self.i18n.tr("select_by_pattern_add"))
                            })
                            .inner
                            .clicked()
                        {
                            outcome = Some(SelectByPatternMode::Add);
                        }
                        if ui
                            .add_enabled_ui(any, |ui| {
                                eden_button(ui, palette, &self.i18n.tr("select_by_pattern_deselect"))
                            })
                            .inner
                            .clicked()
                        {
                            outcome = Some(SelectByPatternMode::Remove);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if eden_button(ui, palette, &self.i18n.tr("close")).clicked() {
                                close = true;
                            }
                        });
                    });
                });
            });

        if let Some(mode) = outcome
            && !state.matches.is_empty()
        {
            let side = state.side;
            let view = self.active_tab_mut().view_mut(side);
            let matches: Vec<PathBuf> = state
                .matches
                .iter()
                .filter_map(|&idx| view.files.get(idx).map(|f| f.path.clone()))
                .collect();
            let selected = &mut view.explorer_state.selected_paths;
            match mode {
                SelectByPatternMode::Replace => {
                    selected.clear();
                    selected.extend(matches);
                }
                SelectByPatternMode::Add => selected.extend(matches),
                SelectByPatternMode::Remove => {
                    for path in &matches {
                        selected.remove(path);
                    }
                }
            }
            view.explorer_state.selection_anchor = None;
            view.explorer_state.selection_focus = None;
            self.last_select_pattern = state.pattern.clone();
            close = true;
        }

        if !close {
            self.select_by_pattern = Some(state);
        }
    }

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

            // Reload the pane the paste actually happened in
            // (`pending.side`), not whichever pane happens to be focused by
            // the time this async job finishes - those can differ (e.g. the
            // user pasted into the other pane, or switched panes again while
            // a large copy was still running), and reloading the wrong one
            // left the pane that actually received the files showing stale
            // content until something else refreshed it.
            self.load_view(pending.side);
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
            let dest_zip = self.pending_compress_jobs.remove(&id).map(|(_, zip)| zip);
            let status = match result {
                Ok(()) => {
                    // A zip made from the Disk Usage dashboard shows up there.
                    if let Some(folder) = dest_zip.as_deref().and_then(Path::parent) {
                        self.disk_usage_state.folder_changed(folder);
                    }
                    crate::gui::windows::containers::notifications::FileOpStatus::Completed
                }
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

    /// Fallback delete used only when the native `IFileOperation` delete
    /// fails outright: moves `path` to the Recycle Bin via `SHFileOperation`
    /// (which shows its own confirmation). Returns whether it succeeded.
    /// Deliberately never falls back to `std::fs::remove_*` - that would
    /// permanently delete files the user may just have declined to delete
    /// in the confirmation dialog.
    pub fn delete_path(&self, path: &PathBuf) -> bool {
        shell_delete_to_recycle_bin(path)
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
        delete_paths_native_standalone(paths, allow_undo, silent)
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

    /// Whether undoing `op` would land on a name that something else now
    /// occupies. A name freed by another item of the same batch (e.g. a
    /// bulk rename that swapped two names) doesn't count, and neither does a
    /// case-only rename (Windows names are case-insensitive, so the "old"
    /// name still resolves to the very same item).
    fn undo_target_occupied(op: &UndoableOperation) -> bool {
        match op {
            UndoableOperation::Rename { old_path, new_path } => {
                !same_path(old_path, new_path) && old_path.exists()
            }
            UndoableOperation::BulkRename { pairs } => pairs.iter().any(|(old, _)| {
                old.exists() && !pairs.iter().any(|(_, new)| same_path(new, old))
            }),
            UndoableOperation::Move { pairs, .. } => pairs
                .iter()
                .any(|(orig, cur)| !same_path(orig, cur) && orig.exists()),
            // Undoing a copy only removes the copies it created.
            UndoableOperation::Copy { .. } => false,
        }
    }

    /// Whether redoing `op` would land on a name that something else now
    /// occupies. Destinations that were originally a Replace resolution are
    /// expected to be occupied - redo recycles them again first.
    fn redo_target_occupied(op: &UndoableOperation) -> bool {
        match op {
            UndoableOperation::Rename { old_path, new_path } => {
                !same_path(old_path, new_path) && new_path.exists()
            }
            UndoableOperation::BulkRename { pairs } => pairs.iter().any(|(_, new)| {
                new.exists() && !pairs.iter().any(|(old, _)| same_path(old, new))
            }),
            UndoableOperation::Move { pairs, replaced, .. }
            | UndoableOperation::Copy { pairs, replaced, .. } => pairs.iter().any(|(src, dest)| {
                !same_path(src, dest) && dest.exists() && !replaced.contains_key(dest)
            }),
        }
    }

    /// What an undo/redo of `op` does, as a notification kind + item count
    /// + destination: undoing a copy deletes the copies (to the Recycle
    /// Bin); everything else repeats or reverses its own kind.
    fn undo_redo_notification_parts(
        &self,
        op: &UndoableOperation,
        action: HistoryAction,
    ) -> (crate::gui::windows::containers::notifications::FileOpKind, usize, String) {
        use crate::gui::windows::containers::notifications::FileOpKind;
        match op {
            UndoableOperation::Rename { .. } => (FileOpKind::Rename, 1, String::new()),
            UndoableOperation::BulkRename { pairs } => {
                (FileOpKind::Rename, pairs.len(), String::new())
            }
            UndoableOperation::Move { pairs, .. } => (FileOpKind::Move, pairs.len(), String::new()),
            UndoableOperation::Copy { pairs, .. } if action == HistoryAction::Undo => {
                (FileOpKind::Delete, pairs.len(), self.i18n.tr("recycle_bin"))
            }
            UndoableOperation::Copy { pairs, .. } => (FileOpKind::Copy, pairs.len(), String::new()),
        }
    }

    /// Adds a finished, Undo/Redo-labelled entry to the notification panel
    /// (and toast) for an undo/redo that completed synchronously, or that
    /// couldn't be carried out - so neither happens silently. Move/Copy
    /// undo/redo that run as robocopy jobs are labelled in
    /// `start_robocopy_paste` instead.
    fn report_undo_redo(
        &mut self,
        op: &UndoableOperation,
        action: HistoryAction,
        status: crate::gui::windows::containers::notifications::FileOpStatus,
    ) {
        let (kind, count, destination) = self.undo_redo_notification_parts(op, action);
        let auto_open = self.settings_window.current_settings.auto_open_notification_panel;
        let id = self
            .notifications_state
            .record_finished(kind, count, destination, status, auto_open);
        self.notifications_state.mark_history(id, action);
    }

    fn report_undo_redo_failure(
        &mut self,
        op: &UndoableOperation,
        action: HistoryAction,
    ) {
        self.report_undo_redo(
            op,
            action,
            crate::gui::windows::containers::notifications::FileOpStatus::Failed,
        );
    }

    fn report_undo_redo_success(
        &mut self,
        op: &UndoableOperation,
        action: HistoryAction,
    ) {
        self.report_undo_redo(
            op,
            action,
            crate::gui::windows::containers::notifications::FileOpStatus::Completed,
        );
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

        // Undo must never overwrite something that now occupies an original
        // name (robocopy would silently replace it). Leave the entry on the
        // stack so it can be retried once the user moves that item away.
        if Self::undo_target_occupied(&op) {
            self.report_undo_redo_failure(&op, HistoryAction::Undo);
            self.undo_stack.push_back(op);
            return;
        }

        match &op {
            UndoableOperation::Rename { old_path, new_path } => {
                let Some(new_name) = old_path.file_name().map(|n| n.to_string_lossy().to_string())
                else {
                    return;
                };
                if Self::rename_one_native(new_path, &new_name).is_ok() {
                    self.report_undo_redo_success(&op, HistoryAction::Undo);
                    self.redo_stack.push_back(op);
                    self.load_path();
                } else {
                    eprintln!("Undo failed: could not rename back to original name");
                    self.report_undo_redo_failure(&op, HistoryAction::Undo);
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
                    self.report_undo_redo_success(&op, HistoryAction::Undo);
                    self.redo_stack.push_back(op);
                    self.load_path();
                } else {
                    eprintln!("Undo failed: could not restore original names");
                    self.report_undo_redo_failure(&op, HistoryAction::Undo);
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
                    self.report_undo_redo_success(&op, HistoryAction::Undo);
                    self.redo_stack.push_back(op);
                    self.load_path();
                } else {
                    eprintln!("Undo failed: could not delete the copied item(s)");
                    self.report_undo_redo_failure(&op, HistoryAction::Undo);
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

        // Same rule as `undo`: never overwrite something that now occupies
        // the name the operation is about to recreate.
        if Self::redo_target_occupied(&op) {
            self.report_undo_redo_failure(&op, HistoryAction::Redo);
            self.redo_stack.push_back(op);
            return;
        }

        match &op {
            UndoableOperation::Rename { old_path, new_path } => {
                let Some(new_name) = new_path.file_name().map(|n| n.to_string_lossy().to_string())
                else {
                    return;
                };
                if Self::rename_one_native(old_path, &new_name).is_ok() {
                    self.report_undo_redo_success(&op, HistoryAction::Redo);
                    self.undo_stack.push_back(op);
                    self.load_path();
                } else {
                    eprintln!("Redo failed: could not re-apply rename");
                    self.report_undo_redo_failure(&op, HistoryAction::Redo);
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
                    self.report_undo_redo_success(&op, HistoryAction::Redo);
                    self.undo_stack.push_back(op);
                    self.load_path();
                } else {
                    eprintln!("Redo failed: could not re-apply bulk rename");
                    self.report_undo_redo_failure(&op, HistoryAction::Redo);
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
        find_recycled_pidl_standalone(original_dir, name)
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
                    // Custom context menu entries, tab groups, and Send To
                    // groups are user-authored content, like favorites/tags -
                    // a general settings reset shouldn't wipe them out.
                    let custom_context_menu = std::mem::take(
                        &mut self.settings_window.current_settings.custom_context_menu,
                    );
                    let custom_context_menu_enabled = self
                        .settings_window
                        .current_settings
                        .custom_context_menu_enabled;
                    let tab_groups =
                        std::mem::take(&mut self.settings_window.current_settings.tab_groups);
                    let send_to =
                        std::mem::take(&mut self.settings_window.current_settings.send_to);
                    let send_to_context_menu_enabled = self
                        .settings_window
                        .current_settings
                        .send_to_context_menu_enabled;
                    self.settings_window.current_settings = Default::default();
                    self.settings_window.current_settings.custom_context_menu = custom_context_menu;
                    self.settings_window.current_settings.custom_context_menu_enabled =
                        custom_context_menu_enabled;
                    self.settings_window.current_settings.tab_groups = tab_groups;
                    self.settings_window.current_settings.send_to = send_to;
                    self.settings_window.current_settings.send_to_context_menu_enabled =
                        send_to_context_menu_enabled;
                    if let Some(hwnd) = self.hwnd {
                        crate::gui::windows::windowsoverrides::set_window_mode(
                            hwnd,
                            &self.settings_window.current_settings.window_size_mode,
                        );
                    }
                }
                SettingsAction::OpenTemplatesFolder => self.open_templates_folder(),
                SettingsAction::ResetData(target) => {
                    use crate::gui::windows::enums::ResetTarget;
                    let settings = &mut self.settings_window.current_settings;
                    match target {
                        ResetTarget::Favorites => {
                            self.sidebar_state.favorites = self.default_favorites();
                            self.persist_favorites();
                        }
                        ResetTarget::CustomContextMenu => {
                            settings.custom_context_menu.clear();
                            settings.custom_context_menu_enabled = false;
                            crate::core::context_menu_settings::save_custom_context_menu(
                                &settings.custom_context_menu,
                                settings.custom_context_menu_enabled,
                            );
                        }
                        ResetTarget::CustomThemes => {
                            let customizer = &mut self.theme_customizer;
                            customizer.custom_themes.clear();
                            customizer.selected_custom_theme_id = None;
                            customizer.custom_theme_delete_confirm = None;
                            customizer.new_custom_theme_name.clear();
                            crate::core::indexer::save_custom_themes(
                                &crate::core::indexer::CustomThemesSnapshot {
                                    next_id: customizer.custom_themes_next_id,
                                    items: Vec::new(),
                                },
                            );
                            crate::core::indexer::save_selected_custom_theme(
                                &crate::core::indexer::SelectedCustomThemeSnapshot { id: None },
                            );
                        }
                        ResetTarget::SendTo => {
                            settings.send_to.clear();
                            settings.send_to_context_menu_enabled = false;
                            crate::core::send_to::save_send_to(
                                &settings.send_to,
                                settings.send_to_context_menu_enabled,
                            );
                        }
                        ResetTarget::TabGroups => {
                            settings.tab_groups.clear();
                            crate::core::tab_groups::save_tab_groups(&settings.tab_groups);
                        }
                        ResetTarget::Tags => {
                            self.tags_state.groups.clear();
                            self.tags_state.picker = None;
                            self.tags_state.delete_confirmation = None;
                            self.persist_tags();
                        }
                        ResetTarget::FolderSizeCache => {
                            self.folder_size_cache.clear();
                        }
                        ResetTarget::FolderViews => {
                            settings.directory_settings.clear();
                            self.save_app_settings_to_disk();
                            self.reload_all_views();
                        }
                    }
                }
                SettingsAction::ExportSettings => {
                    if let Some(path) = crate::gui::windows::windowsoverrides::dialog()
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
                    if let Some(path) = crate::gui::windows::windowsoverrides::dialog()
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
                    if let Some(path) = crate::gui::windows::windowsoverrides::dialog()
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
                    if let Some(path) = crate::gui::windows::windowsoverrides::dialog()
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
                                        self.settings_window
                                            .current_settings
                                            .custom_context_menu_enabled,
                                    );
                                }
                                Err(err) => eprintln!("Failed to parse context menu file: {}", err),
                            },
                            Err(err) => eprintln!("Failed to read context menu file: {}", err),
                        }
                    }
                }
                SettingsAction::ExportTabGroups => {
                    if let Some(path) = crate::gui::windows::windowsoverrides::dialog()
                        .add_filter("Eden Explorer Tab Groups", &["json"])
                        .set_file_name("eden_explorer_tab_groups.json")
                        .save_file()
                    {
                        let bundle = crate::core::tab_groups::TabGroupsExportBundle {
                            format_version: crate::core::tab_groups::TAB_GROUPS_EXPORT_FORMAT_VERSION,
                            groups: self.settings_window.current_settings.tab_groups.clone(),
                        };

                        match serde_json::to_string_pretty(&bundle) {
                            Ok(json) => {
                                if let Err(err) = std::fs::write(&path, json) {
                                    eprintln!("Failed to export tab groups: {}", err);
                                }
                            }
                            Err(err) => eprintln!("Failed to serialize tab groups: {}", err),
                        }
                    }
                }
                SettingsAction::ImportTabGroups => {
                    if let Some(path) = crate::gui::windows::windowsoverrides::dialog()
                        .add_filter("Eden Explorer Tab Groups", &["json"])
                        .pick_file()
                    {
                        match std::fs::read_to_string(&path) {
                            Ok(json) => match serde_json::from_str::<
                                crate::core::tab_groups::TabGroupsExportBundle,
                            >(&json)
                            {
                                Ok(bundle) => {
                                    self.settings_window.current_settings.tab_groups = bundle.groups;
                                    crate::core::tab_groups::save_tab_groups(
                                        &self.settings_window.current_settings.tab_groups,
                                    );
                                }
                                Err(err) => eprintln!("Failed to parse tab groups file: {}", err),
                            },
                            Err(err) => eprintln!("Failed to read tab groups file: {}", err),
                        }
                    }
                }
                SettingsAction::ExportSendTo => {
                    if let Some(path) = crate::gui::windows::windowsoverrides::dialog()
                        .add_filter("Eden Explorer Send To", &["json"])
                        .set_file_name("eden_explorer_send_to.json")
                        .save_file()
                    {
                        let bundle = crate::core::send_to::SendToExportBundle {
                            format_version: crate::core::send_to::SEND_TO_EXPORT_FORMAT_VERSION,
                            groups: self.settings_window.current_settings.send_to.clone(),
                            context_menu_enabled: self
                                .settings_window
                                .current_settings
                                .send_to_context_menu_enabled,
                        };

                        match serde_json::to_string_pretty(&bundle) {
                            Ok(json) => {
                                if let Err(err) = std::fs::write(&path, json) {
                                    eprintln!("Failed to export Send To groups: {}", err);
                                }
                            }
                            Err(err) => eprintln!("Failed to serialize Send To groups: {}", err),
                        }
                    }
                }
                SettingsAction::ImportSendTo => {
                    if let Some(path) = crate::gui::windows::windowsoverrides::dialog()
                        .add_filter("Eden Explorer Send To", &["json"])
                        .pick_file()
                    {
                        match std::fs::read_to_string(&path) {
                            Ok(json) => match serde_json::from_str::<
                                crate::core::send_to::SendToExportBundle,
                            >(&json)
                            {
                                Ok(bundle) => {
                                    self.settings_window.current_settings.send_to = bundle.groups;
                                    self.settings_window
                                        .current_settings
                                        .send_to_context_menu_enabled =
                                        bundle.context_menu_enabled;
                                    crate::core::send_to::save_send_to(
                                        &self.settings_window.current_settings.send_to,
                                        self.settings_window
                                            .current_settings
                                            .send_to_context_menu_enabled,
                                    );
                                }
                                Err(err) => eprintln!("Failed to parse Send To file: {}", err),
                            },
                            Err(err) => eprintln!("Failed to read Send To file: {}", err),
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
        // `save_app_settings_to_disk` already persists Tab Groups, Send To,
        // Custom Context Menu, and Tag Icon Style to their own dedicated
        // files (each lives on `AppSettings` in memory but is saved
        // separately - see their own `core` modules' doc comments), so a
        // full settings import doesn't need to call any of those again here.
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
        if let Some(command) = tabbar_action.as_ref().and_then(|t| t.toolbar_command) {
            use crate::core::toolbar::ToolbarItem;
            let item_action = match command {
                ToolbarItem::SelectAll => Some(ItemViewerAction::SelectAll),
                ToolbarItem::InvertSelection => Some(ItemViewerAction::InvertSelection),
                ToolbarItem::SelectByPattern => Some(ItemViewerAction::SelectByPattern),
                ToolbarItem::PerformancePanel => {
                    self.toggle_performance_panel();
                    None
                }
                ToolbarItem::Settings => {
                    self.open_or_focus_settings_tab();
                    None
                }
                _ => None,
            };
            if item_action.is_some() {
                handle_pending_actions(item_action, self);
            }
        }

        if let Some(action) = tabbar_action.as_ref().and_then(|t| t.nav.as_ref()) {
            match action {
                ItemViewerNavAction::Back => {
                    let snapshot = directory_settings_snapshot_for_view(
                        self.active_tab().view(self.focused_split),
                    );
                    let _ = self.settings_window.current_settings.remember_folder_view(snapshot);
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
                    let _ = self.settings_window.current_settings.remember_folder_view(snapshot);
                    self.current_nav_mut().go_forward();
                }
                ItemViewerNavAction::Up => {
                    let snapshot = directory_settings_snapshot_for_view(
                        self.active_tab().view(self.focused_split),
                    );
                    let _ = self.settings_window.current_settings.remember_folder_view(snapshot);
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
                let _ = self.settings_window.current_settings.remember_folder_view(snapshot);
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
            if let Some(path) = tabbar_action.as_ref().and_then(|t| t.open_in_new_tab.clone()) {
                self.open_new_tab(path);
                self.load_path();
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
            if let Some(entries) = action.open_group {
                for entry in entries {
                    self.open_new_tab_with_split(entry.path, entry.split_path);
                }
                self.load_path();
                // `load_path()` only loads the newly-active tab's Primary
                // side (`self.focused_split` stays `Primary` here) - every
                // *other* opened tab gets its content loaded lazily once the
                // user actually clicks it (see the `action.activate` handler
                // above, which loads both sides), but this last tab never
                // receives that click, so its own Secondary split - if this
                // entry had one - would otherwise stay unloaded and show as
                // an incorrect "this folder is empty" until switched away
                // from and back.
                if self.active_tab().split_view.is_some() {
                    self.load_view(SplitSide::Secondary);
                }
            }
            if let Some(entries) = action.replace_with_group {
                if !entries.is_empty() {
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
                    for entry in entries {
                        let nav = Navigation::new(entry.path);
                        let id = self.next_tab_id;
                        self.next_tab_id += 1;
                        let mut tab = TabState::new(id, nav, sort_column, sort_ascending);
                        apply_directory_settings_to_view(
                            &mut tab.primary_view,
                            &current_settings,
                            DisplayModeFallback::Default,
                        );
                        if let Some(split_path) = entry.split_path {
                            let mut split_view =
                                TabView::new(Navigation::new(split_path), sort_column, sort_ascending);
                            apply_directory_settings_to_view(
                                &mut split_view,
                                &current_settings,
                                DisplayModeFallback::Default,
                            );
                            tab.split_view = Some(split_view);
                        }
                        self.tabs.push(tab);
                    }
                    self.active_tab = 0;
                    self.mark_tab_infos_dirty();
                    self.load_path();
                    // Same reasoning as `action.open_group` above: only the
                    // active tab's Primary side gets loaded here, so its own
                    // Secondary split (if this entry had one) needs loading
                    // explicitly too.
                    if self.active_tab().split_view.is_some() {
                        self.load_view(SplitSide::Secondary);
                    }
                }
            }
            if let Some((name, path, split_path)) = action.add_tab_to_new_group {
                let id = crate::core::tab_groups::next_group_id(
                    &self.settings_window.current_settings.tab_groups,
                );
                self.settings_window.current_settings.tab_groups.push(
                    crate::core::tab_groups::TabGroup {
                        id,
                        name,
                        entries: vec![crate::core::tab_groups::TabGroupEntry { path, split_path }],
                        icon: crate::core::tab_groups::TabGroupIcon::None,
                    },
                );
                self.save_app_settings_to_disk();
            }
            if let Some((group_id, path, split_path)) = action.add_tab_to_existing_group {
                if let Some(group) = self
                    .settings_window
                    .current_settings
                    .tab_groups
                    .iter_mut()
                    .find(|g| g.id == group_id)
                {
                    // Duplicates are allowed on purpose - see `TabGroup`.
                    group
                        .entries
                        .push(crate::core::tab_groups::TabGroupEntry { path, split_path });
                }
                self.save_app_settings_to_disk();
            }
            if let Some(id) = action.close {
                if self.tabs.len() > 1 {
                    if let Some(idx) = self.tabs.iter().position(|t| t.id == id) {
                        if let Some(tab) = self.tabs.get(idx) {
                            let snapshot = directory_settings_snapshot_for_view(&tab.primary_view);
                            let _ = self.settings_window.current_settings.remember_folder_view(snapshot);
                            if let Some(split) = tab.split_view.as_ref() {
                                let snapshot = directory_settings_snapshot_for_view(split);
                                let _ = self.settings_window.current_settings.remember_folder_view(snapshot);
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
                    let _ = self.settings_window.current_settings.remember_folder_view(snapshot);
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
                        _auto_open_notification_panel,
                        _show_operation_toasts,
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
                            let _ = self.settings_window.current_settings.remember_folder_view(snapshot);
                            if let Some(split) = tab.split_view.as_ref() {
                                let snapshot = directory_settings_snapshot_for_view(split);
                                let _ = self.settings_window.current_settings.remember_folder_view(snapshot);
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
            self.settings_window
                .current_settings
                .auto_open_notification_panel,
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
            if let Some(path) = action.analyze_disk_usage {
                self.analyze_disk_usage(path);
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
            || self.disk_usage_state.is_open()
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

    /// Shows/hides the Performance panel (Ctrl+K, or its close
    /// button) and remembers the choice, same as the Settings checkbox.
    pub(crate) fn toggle_performance_panel(&mut self) {
        let settings = &mut self.settings_window.current_settings;
        settings.show_performance_panel = !settings.show_performance_panel;
        crate::core::perf::save_performance_panel_visible(settings.show_performance_panel);
    }

    /// Draws the Performance panel when it's switched on.
    pub(crate) fn draw_performance_panel(
        &mut self,
        ctx: &egui::Context,
        palette: &crate::gui::theme::ThemePalette,
        cpu_usage: Option<f32>,
    ) {
        if !self.settings_window.current_settings.show_performance_panel {
            return;
        }
        self.performance_state.record_frame(cpu_usage);

        let view = self.active_tab().view(self.focused_split);
        let current = &view.nav.current;
        let panel = crate::gui::windows::performance_ui::PanelContext {
            current_folder: (!current.as_os_str().is_empty() && current.is_dir())
                .then(|| current.clone()),
            size_scan_in_progress: view
                .size_scan_started_at
                .map(|started| (view.size_scan_folders, started.elapsed())),
            folder_scanning_enabled: self.settings_window.current_settings.folder_scanning_enabled,
        };
        if crate::gui::windows::performance_ui::draw_performance_panel(
            ctx,
            &self.i18n,
            palette,
            &mut self.performance_state,
            panel,
        ) {
            self.toggle_performance_panel();
        }
    }

    pub(crate) fn draw_disk_usage_window(
        &mut self,
        ctx: &egui::Context,
        palette: &crate::gui::theme::ThemePalette,
    ) {
        use crate::gui::windows::disk_usage_ui::{DiskUsageAction, draw_disk_usage_window};
        match draw_disk_usage_window(ctx, &self.i18n, palette, &mut self.disk_usage_state) {
            Some(DiskUsageAction::OpenInNewTab(path)) => {
                // Keep the result: "Analyze Disk Usage…" on the same folder
                // brings it straight back.
                self.disk_usage_state.hide();
                self.open_new_tab(path);
                self.load_path();
            }
            Some(DiskUsageAction::Reveal(file)) => {
                let Some(folder) = file.parent().map(Path::to_path_buf) else {
                    return;
                };
                self.disk_usage_state.hide();
                self.open_new_tab(folder);
                // Selected (and scrolled to) once the listing arrives.
                self.active_tab_mut().primary_view.explorer_state.navigation_selection = Some(file);
                self.load_path();
            }
            Some(DiskUsageAction::Delete { paths, permanent }) => {
                // The usual delete: confirmation, notification, and Undo.
                // It's finished (or declined) when this returns.
                self.handle_context_action(ItemViewerContextAction::Delete(paths.clone(), permanent));
                self.disk_usage_state.files_removed(&paths);
            }
            Some(DiskUsageAction::MoveTo(paths)) => {
                let picked = crate::gui::windows::windowsoverrides::dialog()
                    .set_title(self.i18n.tr("disk_usage_move_to"))
                    .pick_folder();
                if let Some(destination) = picked {
                    // A folder can't go inside itself, and moving something
                    // to where it already is does nothing.
                    let paths: Vec<PathBuf> = paths
                        .into_iter()
                        .filter(|p| !destination.starts_with(p) && p.parent() != Some(destination.as_path()))
                        .collect();
                    if paths.is_empty() {
                        return;
                    }
                    // Same transfer as Send To > Move: conflict prompts,
                    // progress notification, and Undo.
                    self.send_to_folders(paths.clone(), vec![destination.clone()], true);
                    self.disk_usage_state.files_moving(&paths, &destination);
                }
            }
            Some(DiskUsageAction::Compress(paths)) => {
                // The usual Compress: a zip next to the first item, with a
                // progress notification; the folder it lands in is rescanned
                // when it's done (see `poll_pending_compress`).
                self.handle_context_action(ItemViewerContextAction::Compress(paths));
            }
            None => {}
        }
    }

    /// Opens the Disk Usage dashboard for a folder or drive.
    pub(crate) fn analyze_disk_usage(&mut self, path: PathBuf) {
        if path.is_dir() {
            let ask_admin = self.settings_window.current_settings.ui_prefs.disk_usage_ask_admin;
            self.disk_usage_state.open_for(path, ask_admin);
        }
    }

    pub fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
        // Keep the shared keymap in step with the settings (edits in
        // Settings > Shortcuts, Reset Settings, Import Settings).
        crate::core::keymap::set_overrides(&self.settings_window.current_settings.ui_prefs.shortcuts);
        crate::core::keymap::expire_recording(ctx.cumulative_pass_nr());

        if self.global_shortcuts_disabled(ctx) {
            return;
        }

        use crate::core::keymap::{ShortcutAction, pressed};

        if !ctx.egui_wants_keyboard_input()
            && ctx.input(|input| pressed(input, ShortcutAction::PerformancePanel))
        {
            self.toggle_performance_panel();
            return;
        }

        let shortcuts = ctx.input(|input| {
            (
                pressed(input, ShortcutAction::PreviousTab),
                pressed(input, ShortcutAction::NextTab),
                pressed(input, ShortcutAction::NewTab),
                pressed(input, ShortcutAction::CloseTab),
                pressed(input, ShortcutAction::NewFolder),
                pressed(input, ShortcutAction::Refresh),
                false,
                pressed(input, ShortcutAction::Fullscreen),
                pressed(input, ShortcutAction::AddressBar),
                pressed(input, ShortcutAction::Rename),
                pressed(input, ShortcutAction::Properties),
                pressed(input, ShortcutAction::Search),
                pressed(input, ShortcutAction::Undo),
                pressed(input, ShortcutAction::Redo),
                false,
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
        // Also skip them while any text field has keyboard focus (address
        // bar, search, filter, Find in Preview, Settings fields): there,
        // Ctrl+Z/Ctrl+Y belong to the text box's own undo, and must not also
        // undo the last file rename/move/copy behind the user's back.
        let rename_ui_open = self.rename_state.is_some()
            || self.pending_bulk_rename.is_some()
            || ctx.egui_wants_keyboard_input();

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
                    if self.settings_window.current_settings.ui_prefs.persist_folder_sizes {
                        if done {
                            self.folder_size_cache.insert(&path, size);
                        } else if self.folder_size_cache.get(&path).is_some() {
                            // Keep showing the remembered size while the
                            // re-check runs, rather than counting up from 0.
                            continue;
                        }
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
        self.finish_size_scan_metric_if_done(side);
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
            let use_size_cache = self.settings_window.current_settings.ui_prefs.persist_folder_sizes;
            for item in batch.iter_mut() {
                if item.is_dir && folder_scanning_enabled {
                    // Show a size remembered from an earlier visit straight
                    // away (still marked in-progress until re-checked).
                    let cached = use_size_cache
                        .then(|| self.folder_size_cache.get(&item.path))
                        .flatten();
                    if cached.is_some() {
                        item.file_size = cached;
                    }
                    // Only set up folder size tracking if scanning is enabled
                    self.folder_sizes.entry(item.path.clone()).or_insert(
                        ItemViewerFolderSizeState {
                            bytes: cached.unwrap_or(0),
                            done: false,
                        },
                    );
                    let view = self.active_tab_mut().view_mut(side);
                    if view.pending_size_set.insert(item.path.clone()) {
                        view.pending_size_queue.push_back(item.path.clone());
                        view.size_scan_started_at.get_or_insert_with(std::time::Instant::now);
                        view.size_scan_folders += 1;
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
            if let Some(started) = view.load_started_at.take() {
                let sample = crate::gui::windows::performance_ui::TimedSample {
                    path: view.nav.current.clone(),
                    count: view.files.len(),
                    elapsed: started.elapsed(),
                };
                // A folder with no subfolders has nothing to size; record
                // that instead of leaving the previous folder's scan showing.
                let no_subfolders = view.size_rx.is_some() && view.size_scan_folders == 0;
                if no_subfolders {
                    self.performance_state.last_size_scan = Some(
                        crate::gui::windows::performance_ui::TimedSample {
                            count: 0,
                            elapsed: std::time::Duration::ZERO,
                            ..sample.clone()
                        },
                    );
                }
                self.performance_state.last_listing = Some(sample);
            }
            self.finish_size_scan_metric_if_done(side);
        }

        any_change
    }

    /// Records the Performance panel's "Folder Size Scan" metric once every
    /// folder in the current listing has reported its final size.
    fn finish_size_scan_metric_if_done(&mut self, side: SplitSide) {
        let view = self.active_tab_mut().view_mut(side);
        if view.rx.is_some()
            || !view.pending_size_set.is_empty()
            || !view.pending_size_queue.is_empty()
        {
            return;
        }
        if let Some(started) = view.size_scan_started_at.take() {
            let sample = crate::gui::windows::performance_ui::TimedSample {
                path: view.nav.current.clone(),
                count: view.size_scan_folders,
                elapsed: started.elapsed(),
            };
            self.performance_state.last_size_scan = Some(sample);
        }
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
                | ItemViewerAction::CreateFileFromTemplate(_)
                | ItemViewerAction::CreateShortcutHere
                | ItemViewerAction::Context(ItemViewerContextAction::CreateShortcut(_))
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
                    let _ = explorer.settings_window.current_settings.remember_folder_view(snapshot);
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
            ItemViewerAction::CreateFileFromTemplate(template) => {
                explorer.create_file_from_template(&template)
            }
            ItemViewerAction::OpenTemplatesFolder => explorer.open_templates_folder(),
            ItemViewerAction::CreateShortcutHere => explorer.create_shortcut_here(),
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
                    let _ = explorer.settings_window.current_settings.remember_folder_view(snapshot);
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
                    let _ = explorer.settings_window.current_settings.remember_folder_view(snapshot);
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
                    let _ = explorer.settings_window.current_settings.remember_folder_view(snapshot);
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
                    let _ = explorer.settings_window.current_settings.remember_folder_view(snapshot);
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
            ItemViewerAction::InvertSelection => {
                let side = explorer.focused_split;
                let view = explorer.active_tab_mut().view_mut(side);
                let visible: Vec<PathBuf> = view
                    .item_viewer_filter_state
                    .cached_indices
                    .iter()
                    .map(|&idx| view.files[idx].path.clone())
                    .collect();
                let selected = &mut view.explorer_state.selected_paths;
                let inverted: HashSet<PathBuf> =
                    visible.into_iter().filter(|path| !selected.contains(path)).collect();
                *selected = inverted;
                view.explorer_state.selection_anchor = None;
                view.explorer_state.selection_focus = None;
            }
            ItemViewerAction::SelectByPattern => {
                explorer.select_by_pattern = Some(SelectByPatternState {
                    side: explorer.focused_split,
                    pattern: explorer.last_select_pattern.clone(),
                    include_folders: false,
                    focus_requested: true,
                    matches: Vec::new(),
                    matches_for: None,
                });
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
                let _ = explorer.settings_window.current_settings.remember_folder_view(snapshot);

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
            ItemViewerAction::ToggleQuickLook => {
                explorer.toggle_quick_look();
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
            ItemViewerAction::ResetFolderView => {
                let directory = explorer.active_tab().view(side).nav.current.clone();
                if explorer
                    .settings_window
                    .current_settings
                    .forget_folder_view(&directory)
                {
                    explorer.save_app_settings_to_disk();
                }
                // Reloading re-applies the (now default) view.
                explorer.load_view(side);
            }
            ItemViewerAction::UseFolderViewAsDefault => {
                let snapshot =
                    directory_settings_snapshot_for_view(explorer.active_tab().view(side));
                explorer
                    .settings_window
                    .current_settings
                    .use_folder_view_as_default(&snapshot);
                explorer.save_app_settings_to_disk();
            }
            ItemViewerAction::ColumnSizesChanged => {
                let snapshot =
                    directory_settings_snapshot_for_view(explorer.active_tab().view(side));
                let _ = explorer.settings_window.current_settings.remember_folder_view(snapshot);

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
                }
                .clone();

                if let Some(path) = crate::gui::windows::windowsoverrides::dialog()
                    .add_filter("Theme JSON", &["json"])
                    .set_file_name(match mode {
                        ThemeMode::Dark => "eden_theme_dark.json",
                        ThemeMode::Light => "eden_theme_light.json",
                    })
                    .save_file()
                {
                    // Bundles the Custom Themes list in alongside the active
                    // palette (not just the one palette, like this used to)
                    // so importing this file elsewhere restores the whole
                    // list too - see `ThemeFileExportBundle`'s own doc
                    // comment.
                    let bundle = crate::core::indexer::ThemeFileExportBundle {
                        palette: palette_to_export,
                        custom_themes: theme_customizer.custom_themes.clone(),
                    };
                    if let Ok(json) = serde_json::to_string_pretty(&bundle) {
                        let _ = std::fs::write(path, json);
                    }
                }
            }
            ThemeCustomizerAction::ImportTheme(mode) => {
                if let Some(path) = crate::gui::windows::windowsoverrides::dialog()
                    .add_filter("Theme JSON", &["json"])
                    .pick_file()
                {
                    if let Ok(json) = std::fs::read_to_string(path) {
                        // Try the current bundle shape first; fall back to a
                        // bare `ThemePalette` for a file exported before
                        // this feature existed (just the one palette, no
                        // custom themes to merge) - both are real files a
                        // user could still have on disk.
                        let bundle = serde_json::from_str::<
                            crate::core::indexer::ThemeFileExportBundle,
                        >(&json)
                        .ok()
                        .or_else(|| {
                            serde_json::from_str::<ThemePalette>(&json)
                                .ok()
                                .map(|palette| crate::core::indexer::ThemeFileExportBundle {
                                    palette,
                                    custom_themes: Vec::new(),
                                })
                        });

                        if let Some(bundle) = bundle {
                            let imported = bundle.palette;
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

                            if crate::core::indexer::merge_imported_custom_themes(
                                &mut theme_customizer.custom_themes,
                                &mut theme_customizer.custom_themes_next_id,
                                bundle.custom_themes,
                            ) {
                                crate::core::indexer::save_custom_themes(
                                    &crate::core::indexer::CustomThemesSnapshot {
                                        next_id: theme_customizer.custom_themes_next_id,
                                        items: theme_customizer.custom_themes.clone(),
                                    },
                                );
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
            ThemeCustomizerAction::TabGapChanged(gap) => {
                self.tab_gap = gap;
                crate::core::indexer::save_tab_layout(
                    &crate::core::indexer::TabLayoutSnapshot {
                        tab_gap: gap,
                        min_tab_width: self.min_tab_width,
                    },
                );
            }
            ThemeCustomizerAction::MinTabWidthChanged(width) => {
                self.min_tab_width = width;
                crate::core::indexer::save_tab_layout(
                    &crate::core::indexer::TabLayoutSnapshot {
                        tab_gap: self.tab_gap,
                        min_tab_width: width,
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod undo_redo_safety_tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eden_undo_test_{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn undo_rename_is_blocked_when_a_new_item_took_the_old_name() {
        let dir = scratch("rename");
        let (old, new) = (dir.join("a.txt"), dir.join("b.txt"));
        std::fs::write(&new, "renamed").unwrap();
        let op = UndoableOperation::Rename { old_path: old.clone(), new_path: new };
        assert!(!MainWindow::undo_target_occupied(&op));
        std::fs::write(&old, "someone else's new file").unwrap();
        assert!(MainWindow::undo_target_occupied(&op));
    }

    #[test]
    fn case_only_rename_can_still_be_undone() {
        let dir = scratch("case");
        let new = dir.join("Report.txt");
        std::fs::write(&new, "x").unwrap();
        let op = UndoableOperation::Rename { old_path: dir.join("report.txt"), new_path: new };
        assert!(!MainWindow::undo_target_occupied(&op));
    }

    #[test]
    fn bulk_rename_that_swapped_names_is_not_blocked() {
        let dir = scratch("swap");
        let (a, b) = (dir.join("a.txt"), dir.join("b.txt"));
        std::fs::write(&a, "was b").unwrap();
        std::fs::write(&b, "was a").unwrap();
        let op = UndoableOperation::BulkRename { pairs: vec![(a.clone(), b.clone()), (b, a)] };
        assert!(!MainWindow::undo_target_occupied(&op));
        assert!(!MainWindow::redo_target_occupied(&op));
    }

    #[test]
    fn undo_move_is_blocked_when_the_original_location_is_occupied() {
        let dir = scratch("move");
        let (orig, cur) = (dir.join("src").join("f.txt"), dir.join("dst").join("f.txt"));
        std::fs::create_dir_all(orig.parent().unwrap()).unwrap();
        std::fs::create_dir_all(cur.parent().unwrap()).unwrap();
        std::fs::write(&cur, "moved").unwrap();
        let op = UndoableOperation::Move {
            pairs: vec![(orig.clone(), cur)],
            side: SplitSide::Primary,
            replaced: HashMap::new(),
        };
        assert!(!MainWindow::undo_target_occupied(&op));
        std::fs::write(&orig, "new file at the old spot").unwrap();
        assert!(MainWindow::undo_target_occupied(&op));
    }

    #[test]
    fn redo_copy_is_blocked_unless_the_destination_was_a_replace() {
        let dir = scratch("copy");
        let (src, dest) = (dir.join("src.txt"), dir.join("dest.txt"));
        std::fs::write(&src, "source").unwrap();
        std::fs::write(&dest, "something new").unwrap();
        let mut op = UndoableOperation::Copy {
            pairs: vec![(src, dest.clone())],
            side: SplitSide::Primary,
            replaced: HashMap::new(),
        };
        assert!(MainWindow::redo_target_occupied(&op));
        if let UndoableOperation::Copy { replaced, .. } = &mut op {
            replaced.insert(dest, Vec::new());
        }
        assert!(!MainWindow::redo_target_occupied(&op));
    }
}

#[cfg(test)]
mod folder_view_memory_tests {
    use super::*;

    fn snapshot_with_mode(
        settings: &AppSettings,
        dir: &str,
        mode: ItemViewerDisplayMode,
    ) -> DirectorySettingsSnapshot {
        let mut snapshot = settings.default_folder_view(PathBuf::from(dir));
        snapshot.display_mode = mode;
        snapshot
    }

    #[test]
    fn a_view_matching_the_built_in_default_is_kept_when_the_user_default_differs() {
        let mut settings = AppSettings::default();
        settings.default_display_mode = ItemViewerDisplayMode::Gallery;
        let details = snapshot_with_mode(&settings, r"C:\Photos", ItemViewerDisplayMode::Details);
        assert!(settings.remember_folder_view(details));
        assert_eq!(
            settings.folder_view(Path::new(r"C:\Photos")).map(|s| s.display_mode),
            Some(ItemViewerDisplayMode::Details)
        );
    }

    #[test]
    fn a_view_matching_the_user_default_is_forgotten() {
        let mut settings = AppSettings::default();
        let gallery = snapshot_with_mode(&settings, r"C:\Photos", ItemViewerDisplayMode::Gallery);
        assert!(settings.remember_folder_view(gallery));
        let default = settings.default_folder_view(PathBuf::from(r"C:\Photos"));
        assert!(settings.remember_folder_view(default));
        assert!(settings.folder_view(Path::new(r"C:\Photos")).is_none());
    }

    #[test]
    fn lookup_ignores_case_and_trailing_separator() {
        let mut settings = AppSettings::default();
        let gallery = snapshot_with_mode(&settings, r"C:\Photos", ItemViewerDisplayMode::Gallery);
        settings.remember_folder_view(gallery.clone());
        assert!(settings.folder_view(Path::new(r"c:\photos\")).is_some());

        // Updating through a differently-cased path replaces, not duplicates.
        let mut columns = gallery;
        columns.directory = PathBuf::from(r"c:\PHOTOS");
        columns.display_mode = ItemViewerDisplayMode::Columns;
        settings.remember_folder_view(columns);
        assert_eq!(settings.directory_settings.len(), 1);
    }

    #[test]
    fn turning_it_off_ignores_saved_views_without_deleting_them() {
        let mut settings = AppSettings::default();
        let gallery = snapshot_with_mode(&settings, r"C:\A", ItemViewerDisplayMode::Gallery);
        settings.remember_folder_view(gallery);
        settings.ui_prefs.remember_folder_views = false;
        assert!(settings.folder_view(Path::new(r"C:\A")).is_none());
        let columns = snapshot_with_mode(&settings, r"C:\B", ItemViewerDisplayMode::Columns);
        assert!(!settings.remember_folder_view(columns));
        settings.ui_prefs.remember_folder_views = true;
        assert!(settings.folder_view(Path::new(r"C:\A")).is_some());
        assert!(settings.folder_view(Path::new(r"C:\B")).is_none());
    }

    #[test]
    fn oldest_views_are_dropped_past_the_cap() {
        let mut settings = AppSettings::default();
        for i in 0..MAX_REMEMBERED_FOLDER_VIEWS + 3 {
            let snapshot = snapshot_with_mode(
                &settings,
                &format!(r"C:\F{i}"),
                ItemViewerDisplayMode::Gallery,
            );
            settings.remember_folder_view(snapshot);
        }
        assert_eq!(settings.directory_settings.len(), MAX_REMEMBERED_FOLDER_VIEWS);
        assert!(settings.folder_view(Path::new(r"C:\F0")).is_none());
        assert!(settings.folder_view(Path::new(r"C:\F3")).is_some());
    }

    #[test]
    fn use_as_default_updates_defaults_and_forgets_the_folder() {
        let mut settings = AppSettings::default();
        let mut snapshot = snapshot_with_mode(&settings, r"C:\A", ItemViewerDisplayMode::Columns);
        snapshot.sort_keys = vec![SortKey { column: SortColumn::Size, ascending: false }];
        settings.remember_folder_view(snapshot.clone());
        settings.use_folder_view_as_default(&snapshot);
        assert_eq!(settings.default_display_mode, ItemViewerDisplayMode::Columns);
        assert_eq!(settings.sort_column, SortColumn::Size);
        assert!(!settings.sort_ascending);
        assert!(settings.directory_settings.is_empty());
    }
}

#[cfg(test)]
mod same_folder_tests {
    use super::same_folder;
    use std::path::Path;

    #[test]
    fn compares_case_insensitively_including_non_ascii() {
        assert!(same_folder(Path::new(r"C:\Photos\"), Path::new(r"c:\photos")));
        assert!(same_folder(Path::new(r"C:\Éclair"), Path::new(r"c:\éclair")));
        assert!(!same_folder(Path::new(r"C:\Photos"), Path::new(r"C:\Photos2")));
    }
}
