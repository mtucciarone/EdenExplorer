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
}

pub enum ItemViewerAction {
    Sort(SortColumn),
    ToggleColumnVisibility(ItemViewerHeaderColumn),
    FitColumn(ItemViewerHeaderColumn),
    FitAllColumns,
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
    BackNavigation,
    MoveItems {
        sources: Vec<PathBuf>,
        target_dir: PathBuf,
    },
    MoveFilesToBreadcrumbDirectory {
        sources: Vec<PathBuf>,
        target_dir: PathBuf,
    },
    MoveFilesToTabDirectory {
        sources: Vec<PathBuf>,
        target_dir: PathBuf,
    },
}

#[derive(Clone, Debug)]
pub enum ItemViewerContextAction {
    Copy(Vec<PathBuf>),
    CopyPath(Vec<PathBuf>),
    Cut(Vec<PathBuf>),
    Paste,
    AddTag(Vec<PathBuf>),
    RemoveTag(Vec<PathBuf>),
    RenameRequest(PathBuf, String),
    RenameCancel,
    Delete(Vec<PathBuf>),
    Properties(Vec<PathBuf>),
}
