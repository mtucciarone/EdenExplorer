use crate::gui::utils::SortColumn;
use std::path::PathBuf;

pub enum ItemViewerNavAction {
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
