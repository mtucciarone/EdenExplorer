use std::path::PathBuf;
use crate::gui::utils::SortColumn;

pub enum TabbarNavAction {
    Back,
    Forward,
    Up,
}

pub enum ItemViewerAction {
    Sort(SortColumn),
    Select(PathBuf),
    Deselect(PathBuf),
    SelectAll,
    DeselectAll,
    RangeSelect(Vec<PathBuf>),
    BoxSelect(Vec<PathBuf>),
    Open(PathBuf),
    OpenWithDefault(PathBuf),
    OpenInNewTab(PathBuf),
    Context(ItemViewerContextAction),
    RenameRequest(PathBuf, String),
    RenameCancel,
    StartEdit(PathBuf),
    FilesDropped(Vec<PathBuf>),
    ReplaceSelection(PathBuf),
    BackNavigation,
}

#[derive(Clone, Debug)]
pub enum ItemViewerContextAction {
    Cut(PathBuf),
    Copy(PathBuf),
    Paste,
    // CopyPath(PathBuf),
    Rename(PathBuf),
    Delete(PathBuf),
    Properties(PathBuf),
    Undo,
    Redo,
}