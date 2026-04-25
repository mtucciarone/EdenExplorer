use eframe::egui;
use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub struct DropTargetRegion {
    pub target: Option<PathBuf>,
    pub rect: Option<egui::Rect>,
}

#[derive(Clone, Debug, Default)]
pub struct DropTargets {
    pub item_target: DropTargetRegion,
    pub tab_target: DropTargetRegion,
    pub breadcrumb_target: DropTargetRegion,
}

#[derive(Clone, Debug)]
pub enum NativeDropCommand {
    ImportFiles(Vec<PathBuf>),
    MoveFiles {
        sources: Vec<PathBuf>,
        target_dir: PathBuf,
    },
}

pub trait DragDropBackend {
    fn begin_file_drag(&self, paths: &[PathBuf]) -> bool;
    fn is_drag_active(&self) -> bool;
    fn is_inbound_drag_active(&self) -> bool;
    fn hovered_drop_target(&self) -> Option<PathBuf>;
    fn set_scale_factor(&self, scale_factor: f32);
    fn update_drop_targets(&self, targets: DropTargets);
    fn poll_command(&self) -> Option<NativeDropCommand>;
}
