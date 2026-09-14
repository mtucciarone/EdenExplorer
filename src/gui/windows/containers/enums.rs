use crate::gui::utils::SortColumn;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub enum ItemViewerNavAction {
    Back,
    Forward,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ItemViewerHeaderColumn {
    Name,
    Type,
    Size,
    Modified,
    Created,
    Usage,
    Deleted,
    OriginalDirectory,
    Tags,
}

pub enum ItemViewerAction {
    Sort {
        column: SortColumn,
        additive: bool,
        remove: bool,
    },
    ToggleColumnVisibility(ItemViewerHeaderColumn),
    FitColumn(ItemViewerHeaderColumn),
    FitAllColumns,
    CreateFolder,
    CreateFile,
    RefreshCurrentDirectory,
    OpenTerminal,
    MoveColumnLeft(ItemViewerHeaderColumn),
    MoveColumnRight(ItemViewerHeaderColumn),
    MoveColumnToStart(ItemViewerHeaderColumn),
    MoveColumnToEnd(ItemViewerHeaderColumn),
    Select(PathBuf),
    Deselect(PathBuf),
    SelectAll,
    DeselectAll,
    RangeSelect(Vec<PathBuf>),
    Open(PathBuf),
    OpenWithDefault(Vec<PathBuf>),
    OpenInNewTab(PathBuf),
    OpenInSplitView(PathBuf),
    Context(ItemViewerContextAction),
    StartEdit(PathBuf),
    FilesDropped(Vec<PathBuf>),
    ReplaceSelection(PathBuf),
    MoveItems {
        sources: Vec<PathBuf>,
        target_dir: PathBuf,
    },
    ColumnSizesChanged,
    /// Run a user-defined custom context menu command (see
    /// `core::context_menu_settings`) against `paths` - the selection it was
    /// invoked on, or empty for the folder-background menu (in which case
    /// the handler runs it against the current directory instead).
    RunCustomCommand {
        entry_id: u64,
        paths: Vec<PathBuf>,
    },
}

#[derive(Clone, Debug)]
pub enum ItemViewerContextAction {
    Copy(Vec<PathBuf>),
    CopyPath(Vec<PathBuf>),
    Cut(Vec<PathBuf>),
    Paste,
    Restore(Vec<PathBuf>),
    AddTag(Vec<PathBuf>),
    RemoveTag(Vec<PathBuf>),
    /// Removes `paths` from one specific tag group only (the `u64`) - used
    /// by "Remove Tag" invoked while browsing inside that group's own
    /// folder view, where the group being viewed provides an unambiguous
    /// "current tag" scope. `RemoveTag` above still means "clear every tag"
    /// for the main item viewer's own context menu, which has no such scope.
    RemoveTagFromGroup(u64, Vec<PathBuf>),
    AddFavorite(Vec<PathBuf>),
    /// Compress the selection into a single new `.zip` in the same folder -
    /// see `core::compress::compress_target_path` for how the zip's name is
    /// chosen.
    Compress(Vec<PathBuf>),
    RenameRequest(PathBuf, String),
    RenameCancel,
    /// More than one item was selected when "Rename" was chosen - opens the
    /// bulk-rename dialog instead of the single-item click-to-rename gesture.
    BulkRenameRequest(Vec<PathBuf>),
    /// (original_path, new_name) pairs, already validated and collision-
    /// checked by the bulk-rename dialog - commits them as one batch.
    BulkRenameCommit(Vec<(PathBuf, String)>),
    /// The `bool` is whether to skip the Recycle Bin and delete permanently
    /// (Shift+Delete, or deleting an item that's already in the Recycle Bin
    /// - both match native Explorer).
    Delete(Vec<PathBuf>, bool),
    Properties(Vec<PathBuf>),
    /// Computes CRC32/MD5/SHA-1/SHA-256 for a single file - single-file
    /// only by design, unlike every other variant here that takes a `Vec`.
    Checksum(PathBuf),
}
