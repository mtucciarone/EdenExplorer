//! The "Analyze Disk Usage…" dashboard: a large window showing where the
//! space in a folder or drive goes, as an expandable tree with a bar for
//! each item's share of its parent folder. Opened from the right-click
//! menu of a folder, a folder's background, a drive in This PC, or a drive
//! or favorite in the sidebar. The scanning itself (fast MFT scan or
//! folder-by-folder, progress, Cancel, "Rescan This Branch") lives in
//! `core::disk_usage`.

use crate::core::disk_usage::{
    DirNode, LargeFile, LargeFolder, largest_files, largest_folders, FastScan, FastScanNote, FileEntry, ScanEvent, ScanHandle, ScanMethod, ScanProgress,
    branch_components, drive_root_letter, start_scan,
};
use crate::core::utils::files::format_size;
use crate::core::utils::widgets::{eden_button, modal_frame};
use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use crossbeam_channel::TryRecvError;
use eframe::egui;
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How many files (and folders) the Largest Files (Folders) lists show.
const LARGEST_COUNT: usize = 100;
/// How long a moved file is watched for (see `files_moving`).
const MOVE_WATCH_LIMIT: Duration = Duration::from_secs(600);

/// Folders and files listed under one expanded folder before the rest
/// are summed up in a "N more items" row.
const MAX_CHILD_ROWS: usize = 1000;
const INDENT: f32 = 16.0;

/// What the dashboard asks the main window to do.
pub enum DiskUsageAction {
    OpenInNewTab(PathBuf),
    /// Open the file's folder in a new tab with the file selected.
    Reveal(PathBuf),
    Delete { paths: Vec<PathBuf>, permanent: bool },
    /// Ask for a folder and move these files there.
    MoveTo(Vec<PathBuf>),
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Tree,
    Largest,
    LargestFolders,
}

/// A file or folder being moved from a Largest list: removed from the
/// results once it's gone from `path`; `rescan` is its destination folder
/// when that's inside the analyzed folder (so it shows up there).
struct PendingMove {
    path: PathBuf,
    rescan: Option<PathBuf>,
    since: Instant,
}

struct FinishedInfo {
    method: ScanMethod,
    note: Option<FastScanNote>,
    elapsed: Duration,
}

#[derive(Default)]
pub struct DiskUsageState {
    open: bool,
    root: PathBuf,
    scan: Option<ScanHandle>,
    progress: ScanProgress,
    tree: Option<DirNode>,
    info: Option<FinishedInfo>,
    cancelled: bool,
    /// A "Rescan This Branch" in progress: the folder and its scan.
    branch_scan: Option<(PathBuf, ScanHandle)>,
    branch_progress: ScanProgress,
    expanded: HashSet<PathBuf>,
    selected: Option<PathBuf>,
    /// (total, free) bytes when the root is a whole drive.
    drive_space: Option<(u64, u64)>,
    /// Ask for administrator permission for the fast scan
    /// (Settings > Advanced).
    ask_admin: bool,
    /// Bumped whenever the tree or the expanded folders change, so the
    /// flattened rows (and the selected item's kind) are rebuilt only then
    /// rather than on every frame - a fully expanded drive can mean
    /// thousands of rows.
    revision: u64,
    rows: Vec<Row>,
    rows_revision: Option<u64>,
    /// (revision, selected path, whether it's a folder - `None` if it's
    /// no longer in the tree).
    selected_kind: Option<(u64, PathBuf, Option<bool>)>,
    tab: Tab,
    /// The Largest Files list, rebuilt when `revision` changes.
    largest: Vec<LargeFile>,
    largest_revision: Option<u64>,
    largest_selected: HashSet<PathBuf>,
    largest_anchor: Option<usize>,
    /// The Largest Folders list, likewise.
    folders: Vec<LargeFolder>,
    folders_revision: Option<u64>,
    folders_selected: HashSet<PathBuf>,
    folders_anchor: Option<usize>,
    pending_moves: Vec<PendingMove>,
    last_move_check: Option<Instant>,
    /// Destination folders waiting to be rescanned (one scan at a time).
    rescan_queue: Vec<PathBuf>,
}

impl DiskUsageState {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the dashboard for `root` and starts scanning it, replacing
    /// (and cancelling) whatever it showed before. Reopening the folder
    /// it was last hidden on shows that result again without rescanning.
    pub fn open_for(&mut self, root: PathBuf, ask_admin: bool) {
        self.ask_admin = ask_admin;
        if self.root == root && (self.tree.is_some() || self.scan.is_some()) {
            self.open = true;
            return;
        }
        *self = Self {
            open: true,
            root,
            ask_admin,
            ..Default::default()
        };
        self.start_full_scan();
    }

    /// Call after deleting `paths` (the delete has finished, or was
    /// declined): the ones that are gone are dropped from the results.
    pub fn files_removed(&mut self, paths: &[PathBuf]) {
        for path in paths {
            if std::fs::symlink_metadata(path).is_err() {
                self.forget_file(path);
            }
        }
    }

    /// Call after starting to move `paths` into `destination`: each is
    /// dropped from the results once the move takes it away, and the
    /// destination is rescanned if it's inside the analyzed folder.
    pub fn files_moving(&mut self, paths: &[PathBuf], destination: &Path) {
        let rescan = branch_components(&self.root, destination).map(|_| destination.to_path_buf());
        let since = Instant::now();
        self.pending_moves.extend(paths.iter().map(|path| PendingMove {
            path: path.clone(),
            rescan: rescan.clone(),
            since,
        }));
    }

    /// Drops a deleted or moved-away file or folder from the results.
    fn forget_file(&mut self, path: &Path) {
        let removed = match (self.tree.as_mut(), branch_components(&self.root, path)) {
            (Some(tree), Some(components)) => {
                tree.remove_file(&components).is_some() || tree.remove_dir(&components).is_some()
            }
            _ => false,
        };
        if removed {
            self.revision += 1;
        }
        self.largest_selected.retain(|p| !p.starts_with(path));
        self.folders_selected.retain(|p| !p.starts_with(path));
        self.expanded.retain(|p| !p.starts_with(path));
        if self.selected.as_deref().is_some_and(|p| p.starts_with(path)) {
            self.selected = None;
        }
    }

    /// Drops moved files that have left, every half second.
    fn poll_moves(&mut self) {
        if self.pending_moves.is_empty() && self.rescan_queue.is_empty() {
            return;
        }
        if self
            .last_move_check
            .is_some_and(|at| at.elapsed() < Duration::from_millis(500))
        {
            return;
        }
        self.last_move_check = Some(Instant::now());
        let mut still_pending = Vec::new();
        for pending in std::mem::take(&mut self.pending_moves) {
            if std::fs::symlink_metadata(&pending.path).is_err() {
                self.forget_file(&pending.path);
                if let Some(folder) = pending.rescan
                    && !self.rescan_queue.contains(&folder)
                {
                    self.rescan_queue.push(folder);
                }
            } else if pending.since.elapsed() < MOVE_WATCH_LIMIT {
                still_pending.push(pending);
            }
        }
        self.pending_moves = still_pending;
        // Rescan destinations once everything headed there has arrived.
        if !self.scanning()
            && let Some(index) = self.rescan_queue.iter().position(|folder| {
                !self.pending_moves.iter().any(|p| p.rescan.as_ref() == Some(folder))
            })
        {
            let folder = self.rescan_queue.remove(index);
            self.start_branch_scan(folder);
        }
    }

    /// Hides the dashboard but keeps its result (see `open_for`).
    pub fn hide(&mut self) {
        self.open = false;
    }

    pub fn close(&mut self) {
        // Dropping the handles cancels any running scans.
        *self = Self::default();
    }

    fn start_full_scan(&mut self) {
        // The previous result (if any) stays until the new one arrives, so
        // cancelling a rescan keeps it.
        self.branch_scan = None;
        self.cancelled = false;
        self.progress = ScanProgress::default();
        self.drive_space = drive_root_letter(&self.root)
            .and_then(|_| crate::core::fs::get_drive_space(&self.root));
        let fast = if self.ask_admin {
            FastScan::AskForAdmin
        } else {
            FastScan::IfElevated
        };
        self.scan = Some(start_scan(self.root.clone(), fast));
    }

    fn start_branch_scan(&mut self, branch: PathBuf) {
        if self.scan.is_some() || self.tree.is_none() {
            return;
        }
        if branch == self.root {
            self.start_full_scan();
            return;
        }
        self.branch_progress = ScanProgress::default();
        let handle = start_scan(branch.clone(), FastScan::Off);
        self.branch_scan = Some((branch, handle));
    }

    fn cancel_scans(&mut self) {
        if let Some(scan) = &self.scan {
            scan.cancel();
        }
        if let Some((_, scan)) = &self.branch_scan {
            scan.cancel();
        }
    }

    fn scanning(&self) -> bool {
        self.scan.is_some() || self.branch_scan.is_some()
    }

    fn poll(&mut self) {
        if let Some(scan) = &self.scan {
            let mut finished = None;
            loop {
                match scan.rx.try_recv() {
                    Ok(ScanEvent::Progress(p)) => self.progress = p,
                    Ok(ScanEvent::Finished(outcome)) => {
                        finished = Some(Some(outcome));
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        finished = Some(None);
                        break;
                    }
                }
            }
            if let Some(outcome) = finished {
                self.scan = None;
                match outcome {
                    Some(outcome) => {
                        self.info = Some(FinishedInfo {
                            method: outcome.method,
                            note: outcome.fast_scan_note,
                            elapsed: outcome.elapsed,
                        });
                        match outcome.tree {
                            Some(tree) => {
                                self.tree = Some(tree);
                                self.expanded.insert(self.root.clone());
                                self.revision += 1;
                            }
                            None => self.cancelled = true,
                        }
                    }
                    None => self.cancelled = true,
                }
            }
        }

        if let Some((branch, scan)) = &self.branch_scan {
            let mut finished = None;
            loop {
                match scan.rx.try_recv() {
                    Ok(ScanEvent::Progress(p)) => self.branch_progress = p,
                    Ok(ScanEvent::Finished(outcome)) => {
                        finished = Some(outcome.tree);
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        finished = Some(None);
                        break;
                    }
                }
            }
            if let Some(new_tree) = finished {
                let branch = branch.clone();
                self.branch_scan = None;
                if let (Some(new_tree), Some(tree), Some(components)) = (
                    new_tree,
                    self.tree.as_mut(),
                    branch_components(&self.root, &branch),
                ) {
                    tree.replace_branch(&components, new_tree);
                    self.revision += 1;
                }
            }
        }
    }
}

fn format_count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn format_elapsed(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 60.0 {
        format!("{secs:.1} s")
    } else {
        format!("{}:{:02}", d.as_secs() / 60, d.as_secs() % 60)
    }
}

enum RowKind {
    Dir {
        has_children: bool,
        expanded: bool,
        unreadable: bool,
        is_link: bool,
        files: u64,
        dirs: u64,
    },
    File,
    /// The rest of a long folder listing, summed up.
    More { count: usize },
}

struct Row {
    kind: RowKind,
    depth: usize,
    path: PathBuf,
    name: String,
    size: u64,
    allocated: u64,
    /// Share of the parent folder's size, 0..1.
    share: f32,
}

enum Child<'a> {
    Dir(&'a DirNode),
    File(&'a FileEntry),
}

impl Child<'_> {
    fn size(&self) -> u64 {
        match self {
            Child::Dir(d) => d.size,
            Child::File(f) => f.size,
        }
    }
}

/// Folders and files of `node` merged into one list, largest first.
fn children_by_size(node: &DirNode) -> Vec<Child<'_>> {
    let mut out = Vec::with_capacity(node.dirs.len() + node.files.len());
    let (mut d, mut f) = (0, 0);
    while d < node.dirs.len() || f < node.files.len() {
        let take_dir = match (node.dirs.get(d), node.files.get(f)) {
            (Some(dir), Some(file)) => dir.size >= file.size,
            (Some(_), None) => true,
            _ => false,
        };
        if take_dir {
            out.push(Child::Dir(&node.dirs[d]));
            d += 1;
        } else {
            out.push(Child::File(&node.files[f]));
            f += 1;
        }
    }
    out
}

fn share(size: u64, parent: u64) -> f32 {
    if parent == 0 {
        0.0
    } else {
        (size as f64 / parent as f64) as f32
    }
}

fn push_rows(
    node: &DirNode,
    path: PathBuf,
    name: String,
    depth: usize,
    parent_size: u64,
    expanded: &HashSet<PathBuf>,
    out: &mut Vec<Row>,
) {
    let is_expanded = expanded.contains(&path);
    let has_children = !node.dirs.is_empty() || !node.files.is_empty();
    out.push(Row {
        kind: RowKind::Dir {
            has_children,
            expanded: is_expanded,
            unreadable: node.unreadable,
            is_link: node.is_link,
            files: node.file_count,
            dirs: node.dir_count,
        },
        depth,
        path: path.clone(),
        name,
        size: node.size,
        allocated: node.allocated,
        share: share(node.size, parent_size),
    });
    if !is_expanded || !has_children {
        return;
    }

    let children = children_by_size(node);
    let shown = children.len().min(MAX_CHILD_ROWS);
    for child in &children[..shown] {
        match child {
            Child::Dir(dir) => push_rows(
                dir,
                path.join(&*dir.name),
                dir.name.to_string(),
                depth + 1,
                node.size,
                expanded,
                out,
            ),
            Child::File(file) => out.push(Row {
                kind: RowKind::File,
                depth: depth + 1,
                path: path.join(&*file.name),
                name: file.name.to_string(),
                size: file.size,
                allocated: file.allocated,
                share: share(file.size, node.size),
            }),
        }
    }
    let rest = &children[shown..];
    if !rest.is_empty() {
        let size: u64 = rest.iter().map(Child::size).sum();
        out.push(Row {
            kind: RowKind::More { count: rest.len() },
            depth: depth + 1,
            path: path.join("\0more"),
            name: String::new(),
            size,
            allocated: 0,
            share: share(size, node.size),
        });
    }
}

fn muted(palette: &ThemePalette, text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text.into())
        .size(palette.text_size - 1.0)
        .color(palette.text_normal.gamma_multiply(0.7))
}

/// Draws the dashboard while it's open.
pub fn draw_disk_usage_window(
    ctx: &egui::Context,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut DiskUsageState,
) -> Option<DiskUsageAction> {
    if !state.open {
        return None;
    }
    state.poll();
    state.poll_moves();

    let mut action = None;
    let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let screen = ctx.content_rect();
    let size = egui::vec2(
        (screen.width() - 80.0).clamp(520.0, 1180.0),
        (screen.height() - 80.0).clamp(360.0, 780.0),
    );

    egui::Window::new(i18n.tr("disk_usage_title"))
        .id(egui::Id::new("disk_usage_window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .fixed_size(size)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .frame(modal_frame(&ctx.style_of(ctx.theme()), palette).inner_margin(egui::Margin::same(16)))
        .show(ctx, |ui| {
            ui.set_min_size(size);
            draw_header(ui, i18n, palette, state, &mut close);
            ui.add_space(6.0);
            if state.scan.is_none() {
                draw_summary(ui, i18n, palette, state);
                ui.add_space(8.0);
            }
            if state.tree.is_some() && state.scan.is_none() {
                draw_tabs(ui, i18n, palette, state);
                ui.add_space(8.0);
            }
            match state.tab {
                Tab::Largest if state.tree.is_some() && state.scan.is_none() => {
                    draw_largest_toolbar(ui, i18n, palette, state, &mut action)
                }
                Tab::LargestFolders if state.tree.is_some() && state.scan.is_none() => {
                    draw_folders_toolbar(ui, i18n, palette, state, &mut action)
                }
                _ => draw_toolbar(ui, i18n, palette, state, &mut action),
            }
            ui.add_space(6.0);

            if state.scan.is_some() {
                draw_full_scan_progress(ui, i18n, palette, state);
            } else if state.tree.is_some() {
                if state.branch_scan.is_some() {
                    draw_branch_progress(ui, i18n, palette, state);
                    ui.add_space(4.0);
                }
                match state.tab {
                    Tab::Tree => draw_tree(ui, i18n, palette, state, &mut action),
                    Tab::Largest => draw_largest(ui, i18n, palette, state, &mut action),
                    Tab::LargestFolders => draw_folders(ui, i18n, palette, state, &mut action),
                }
            } else if state.cancelled {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.label(muted(palette, i18n.tr("disk_usage_cancelled")));
                    ui.add_space(8.0);
                    if eden_button(
                        ui,
                        palette,
                        &format!("{} {}", regular::ARROW_CLOCKWISE, i18n.tr("disk_usage_scan_again")),
                    )
                    .clicked()
                    {
                        state.start_full_scan();
                    }
                });
            }
        });

    if close {
        state.close();
    } else if state.scanning() {
        ctx.request_repaint_after(Duration::from_millis(100));
    }
    action
}

fn draw_header(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &DiskUsageState,
    close: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{}  {}", regular::CHART_PIE_SLICE, i18n.tr("disk_usage_title")))
                .strong()
                .size(palette.text_size + 3.0)
                .color(palette.text_header_section),
        );
        ui.add_space(8.0);
        let icon = if drive_root_letter(&state.root).is_some() {
            regular::HARD_DRIVES
        } else {
            regular::FOLDER
        };
        ui.label(
            egui::RichText::new(format!("{icon} {}", state.root.display()))
                .size(palette.text_size)
                .color(palette.text_normal),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new(regular::X).frame(false))
                .on_hover_text(i18n.tr("close"))
                .clicked()
            {
                *close = true;
            }
        });
    });
}

fn draw_summary(ui: &mut egui::Ui, i18n: &I18n, palette: &ThemePalette, state: &DiskUsageState) {
    let Some(tree) = &state.tree else {
        return;
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new(format_size(tree.size))
                .strong()
                .size(palette.text_size + 1.0)
                .color(palette.text_normal),
        );
        ui.label(muted(
            palette,
            format!(
                "· {} {} · {} {} · {} {}",
                format_size(tree.allocated),
                i18n.tr("disk_usage_on_disk"),
                format_count(tree.file_count),
                i18n.tr("disk_usage_files"),
                format_count(tree.dir_count),
                i18n.tr("disk_usage_folders"),
            ),
        ));
        if let Some(info) = &state.info {
            ui.add_space(10.0);
            let (icon, key, color) = match info.method {
                ScanMethod::Mft => (
                    regular::LIGHTNING,
                    "disk_usage_method_mft",
                    palette.notification_status_success,
                ),
                ScanMethod::Standard => (
                    regular::FOLDERS,
                    "disk_usage_method_standard",
                    palette.text_normal.gamma_multiply(0.8),
                ),
            };
            ui.label(
                egui::RichText::new(format!(
                    "{icon} {} · {}",
                    i18n.tr(key),
                    format_elapsed(info.elapsed)
                ))
                .size(palette.text_size - 1.0)
                .color(color),
            )
            .on_hover_text(i18n.tr(match info.method {
                ScanMethod::Mft => "tooltip_disk_usage_method_mft",
                ScanMethod::Standard => "tooltip_disk_usage_method_standard",
            }));
        }
    });

    if let Some((total, free)) = state.drive_space {
        let used = total.saturating_sub(free);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(muted(
                palette,
                format!(
                    "{} {} / {} · {} {}",
                    i18n.tr("disk_usage_drive_used"),
                    format_size(used),
                    format_size(total),
                    format_size(free),
                    i18n.tr("disk_usage_free"),
                ),
            ));
            let missing = used.saturating_sub(tree.allocated);
            // Only worth pointing out when it's more than rounding noise.
            if total > 0 && missing > total / 200 {
                ui.label(muted(
                    palette,
                    format!("· {} {}", format_size(missing), i18n.tr("disk_usage_unaccounted")),
                ))
                .on_hover_text(i18n.tr("tooltip_disk_usage_unaccounted"));
            }
        });
    }

    if let Some(note) = state.info.as_ref().and_then(|i| i.note.as_ref()) {
        let text = match note {
            FastScanNote::NeedsAdmin => i18n.tr("disk_usage_note_admin"),
            FastScanNote::Declined => i18n.tr("disk_usage_note_declined"),
            FastScanNote::NotNtfs => i18n.tr("disk_usage_note_not_ntfs"),
            FastScanNote::Failed(reason) => format!("{} {reason}", i18n.tr("disk_usage_note_failed")),
        };
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(format!("{} {text}", regular::INFO))
                .size(palette.text_size - 1.0)
                .color(palette.notification_status_warning),
        );
    }
}

/// The selected row's path and whether it's a folder, if it's still in
/// the tree (cached until the selection or the tree changes).
fn selected_item(state: &mut DiskUsageState) -> Option<(PathBuf, bool)> {
    let selected = state.selected.clone()?;
    if let Some((revision, path, kind)) = &state.selected_kind
        && *revision == state.revision
        && *path == selected
    {
        return kind.map(|is_dir| (selected, is_dir));
    }
    let kind = find_selected_kind(state, &selected);
    state.selected_kind = Some((state.revision, selected.clone(), kind));
    kind.map(|is_dir| (selected, is_dir))
}

fn find_selected_kind(state: &DiskUsageState, selected: &Path) -> Option<bool> {
    let tree = state.tree.as_ref()?;
    let components = branch_components(&state.root, selected)?;
    if tree.find(&components).is_some() {
        return Some(true);
    }
    let (name, parent) = components.split_last()?;
    let parent = tree.find(parent)?;
    parent.files.iter().any(|f| &*f.name == name).then_some(false)
}

fn draw_toolbar(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut DiskUsageState,
    action: &mut Option<DiskUsageAction>,
) {
    let selected = selected_item(state);
    let busy = state.scanning();
    ui.horizontal(|ui| {
        let rescan = ui
            .add_enabled_ui(!busy, |ui| {
                eden_button(ui, palette, &format!("{} {}", regular::ARROW_CLOCKWISE, i18n.tr("disk_usage_rescan")))
            })
            .inner;
        if rescan.on_hover_text(i18n.tr("tooltip_disk_usage_rescan")).clicked() {
            state.start_full_scan();
        }

        let branch = selected.as_ref().filter(|(_, is_dir)| *is_dir).map(|(p, _)| p.clone());
        let rescan_branch = ui
            .add_enabled_ui(!busy && branch.is_some() && state.tree.is_some(), |ui| {
                eden_button(
                    ui,
                    palette,
                    &format!("{} {}", regular::ARROWS_CLOCKWISE, i18n.tr("disk_usage_rescan_branch")),
                )
            })
            .inner;
        if rescan_branch
            .on_hover_text(i18n.tr("tooltip_disk_usage_rescan_branch"))
            .clicked()
            && let Some(branch) = branch
        {
            state.start_branch_scan(branch);
        }

        let open_target = selected.clone();
        let open_label = match &selected {
            Some((_, false)) => i18n.tr("disk_usage_show_in_folder"),
            _ => i18n.tr("disk_usage_open_in_tab"),
        };
        let open = ui
            .add_enabled_ui(open_target.is_some(), |ui| {
                eden_button(ui, palette, &format!("{} {open_label}", regular::ARROW_SQUARE_OUT))
            })
            .inner;
        if open.clicked()
            && let Some((target, is_dir)) = open_target
        {
            *action = Some(if is_dir {
                DiskUsageAction::OpenInNewTab(target)
            } else {
                DiskUsageAction::Reveal(target)
            });
        }

        let copy = ui
            .add_enabled_ui(selected.is_some(), |ui| {
                eden_button(ui, palette, &format!("{} {}", regular::LINK, i18n.tr("disk_usage_copy_path")))
            })
            .inner;
        if copy.clicked()
            && let Some((path, _)) = &selected
        {
            crate::core::utils::clipboard::copy_text_to_clipboard(&path.display().to_string());
        }
    });
}

fn progress_text(i18n: &I18n, progress: &ScanProgress) -> String {
    let mut text = format!(
        "{} {} · {}",
        format_count(progress.files),
        i18n.tr("disk_usage_files"),
        format_size(progress.bytes),
    );
    if progress.method != Some(ScanMethod::Mft) {
        text.push_str(&format!(
            " · {} {}",
            format_count(progress.dirs),
            i18n.tr("disk_usage_folders")
        ));
    }
    text
}

fn draw_full_scan_progress(ui: &mut egui::Ui, i18n: &I18n, palette: &ThemePalette, state: &mut DiskUsageState) {
    let progress = state.progress.clone();
    ui.add_space(30.0);
    ui.vertical_centered(|ui| {
        ui.set_max_width(560.0);
        ui.horizontal(|ui| {
            ui.spinner();
            let method = match progress.method {
                Some(ScanMethod::Mft) => format!(" · {} {}", regular::LIGHTNING, i18n.tr("disk_usage_method_mft")),
                _ => String::new(),
            };
            let title = if progress.waiting_for_permission {
                format!("{} {}", regular::SHIELD_CHECK, i18n.tr("disk_usage_waiting_for_permission"))
            } else {
                format!("{}{method}", i18n.tr("disk_usage_scanning"))
            };
            ui.label(
                egui::RichText::new(title)
                    .strong()
                    .size(palette.text_size + 1.0)
                    .color(palette.text_normal),
            );
        });
        ui.add_space(8.0);
        let bar = match progress.fraction {
            Some(fraction) => egui::ProgressBar::new(fraction).show_percentage(),
            None => egui::ProgressBar::new(0.0).animate(true),
        };
        ui.add(bar.desired_height(10.0).fill(palette.primary));
        ui.add_space(6.0);
        ui.label(muted(palette, progress_text(i18n, &progress)));
        if let Some(current) = &progress.current {
            ui.add(
                egui::Label::new(muted(palette, current.display().to_string()))
                    .truncate(),
            );
        }
        ui.add_space(12.0);
        if eden_button(ui, palette, &format!("{} {}", regular::STOP, i18n.tr("cancel"))).clicked() {
            state.cancel_scans();
        }
    });
}

fn draw_branch_progress(ui: &mut egui::Ui, i18n: &I18n, palette: &ThemePalette, state: &mut DiskUsageState) {
    let Some((branch, _)) = &state.branch_scan else {
        return;
    };
    let label = format!(
        "{} {} · {}",
        i18n.tr("disk_usage_rescanning"),
        branch.display(),
        progress_text(i18n, &state.branch_progress)
    );
    ui.horizontal(|ui| {
        ui.spinner();
        ui.add(egui::Label::new(muted(palette, label)).truncate());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if eden_button(ui, palette, &format!("{} {}", regular::STOP, i18n.tr("cancel"))).clicked() {
                state.cancel_scans();
            }
        });
    });
}

/// A bar filled to `share` of its width, with the percentage beside it.
fn share_bar(ui: &mut egui::Ui, palette: &ThemePalette, share: f32, color: egui::Color32) {
    let height = 10.0;
    let width = (ui.available_width() - 52.0).max(30.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let radius = egui::CornerRadius::same(3);
    ui.painter().rect_filled(rect, radius, palette.drive_usage_background);
    let mut fill = rect;
    fill.set_width(rect.width() * share.clamp(0.0, 1.0));
    if fill.width() > 0.5 {
        ui.painter().rect_filled(fill, radius, color);
    }
    ui.label(
        egui::RichText::new(format!("{:.1}%", share * 100.0))
            .family(egui::FontFamily::Monospace)
            .size(palette.text_size - 2.0)
            .color(palette.text_normal.gamma_multiply(0.8)),
    );
}

fn draw_tree(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut DiskUsageState,
    action: &mut Option<DiskUsageAction>,
) {
    let Some(tree) = &state.tree else {
        return;
    };
    if state.rows_revision != Some(state.revision) {
        let mut rows = std::mem::take(&mut state.rows);
        rows.clear();
        push_rows(
            tree,
            state.root.clone(),
            state.root.display().to_string(),
            0,
            tree.size,
            &state.expanded,
            &mut rows,
        );
        state.rows = rows;
        state.rows_revision = Some(state.revision);
    }
    let rows = std::mem::take(&mut state.rows);

    let branch_in_progress = state.branch_scan.as_ref().map(|(p, _)| p.clone());
    let busy = state.scanning();
    let mut toggle: Option<PathBuf> = None;
    let mut select: Option<PathBuf> = None;
    let mut rescan_branch: Option<PathBuf> = None;
    let row_height = (palette.text_size + 10.0).max(22.0);
    let header_color = palette.text_normal.gamma_multiply(0.75);
    // Selectable labels would take the click before the row gets it.
    ui.style_mut().interaction.selectable_labels = false;

    TableBuilder::new(ui)
        .id_salt("disk_usage_table")
        .striped(false)
        .resizable(true)
        .sense(egui::Sense::click())
        .auto_shrink([false, false])
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::remainder().at_least(240.0).clip(true))
        .column(Column::initial(96.0).at_least(70.0))
        .column(Column::initial(190.0).at_least(110.0))
        .column(Column::initial(96.0).at_least(70.0))
        .column(Column::initial(84.0).at_least(56.0))
        .column(Column::initial(76.0).at_least(56.0))
        .header(row_height, |mut header| {
            for key in [
                "disk_usage_col_name",
                "disk_usage_col_size",
                "disk_usage_col_share",
                "disk_usage_col_on_disk",
                "disk_usage_col_files",
                "disk_usage_col_folders",
            ] {
                header.col(|ui| {
                    ui.label(
                        egui::RichText::new(i18n.tr(key))
                            .strong()
                            .size(palette.text_size - 1.0)
                            .color(header_color),
                    );
                });
            }
        })
        .body(|body| {
            body.rows(row_height, rows.len(), |mut table_row| {
                let row = &rows[table_row.index()];
                let is_selected = state.selected.as_ref() == Some(&row.path);
                table_row.set_selected(is_selected);
                let text_color = if is_selected {
                    palette.item_viewer_row_text_selected
                } else {
                    palette.text_normal
                };

                table_row.col(|ui| {
                    ui.add_space(row.depth as f32 * INDENT);
                    match &row.kind {
                        RowKind::Dir {
                            has_children,
                            expanded,
                            unreadable,
                            is_link,
                            ..
                        } => {
                            let caret = if !has_children {
                                " "
                            } else if *expanded {
                                regular::CARET_DOWN
                            } else {
                                regular::CARET_RIGHT
                            };
                            if ui
                                .add(egui::Button::new(caret).frame(false).min_size(egui::vec2(14.0, 14.0)))
                                .clicked()
                                && *has_children
                            {
                                toggle = Some(row.path.clone());
                            }
                            let (icon, hint) = if *unreadable {
                                (regular::WARNING, Some("disk_usage_unreadable"))
                            } else if *is_link {
                                (regular::LINK, Some("disk_usage_link"))
                            } else if row.depth == 0 && drive_root_letter(&row.path).is_some() {
                                (regular::HARD_DRIVES, None)
                            } else {
                                (regular::FOLDER, None)
                            };
                            let spinner = branch_in_progress.as_ref() == Some(&row.path);
                            if spinner {
                                ui.spinner();
                            }
                            let label = ui.label(
                                egui::RichText::new(format!("{icon} {}", row.name))
                                    .size(palette.text_size)
                                    .color(text_color),
                            );
                            if let Some(hint) = hint {
                                label.on_hover_text(i18n.tr(hint));
                            }
                        }
                        RowKind::File => {
                            ui.add_space(18.0);
                            ui.label(
                                egui::RichText::new(format!("{} {}", regular::FILE, row.name))
                                    .size(palette.text_size)
                                    .color(text_color),
                            );
                        }
                        RowKind::More { count } => {
                            ui.add_space(18.0);
                            ui.label(muted(
                                palette,
                                format!("{} {} {}", regular::DOTS_THREE, format_count(*count as u64), i18n.tr("disk_usage_more_items")),
                            ));
                        }
                    }
                });

                table_row.col(|ui| {
                    ui.label(
                        egui::RichText::new(format_size(row.size))
                            .family(egui::FontFamily::Monospace)
                            .size(palette.text_size - 1.0)
                            .color(text_color),
                    );
                });

                table_row.col(|ui| {
                    let color = match row.kind {
                        RowKind::Dir { .. } => palette.primary,
                        _ => palette.primary.gamma_multiply(0.5),
                    };
                    share_bar(ui, palette, row.share, color);
                });

                table_row.col(|ui| {
                    if !matches!(row.kind, RowKind::More { .. }) {
                        ui.label(
                            egui::RichText::new(format_size(row.allocated))
                                .family(egui::FontFamily::Monospace)
                                .size(palette.text_size - 1.0)
                                .color(text_color.gamma_multiply(0.85)),
                        );
                    }
                });

                let (files, dirs) = match row.kind {
                    RowKind::Dir { files, dirs, .. } => (Some(files), Some(dirs)),
                    _ => (None, None),
                };
                for value in [files, dirs] {
                    table_row.col(|ui| {
                        if let Some(value) = value {
                            ui.label(
                                egui::RichText::new(format_count(value))
                                    .size(palette.text_size - 1.0)
                                    .color(text_color.gamma_multiply(0.85)),
                            );
                        }
                    });
                }

                if matches!(row.kind, RowKind::More { .. }) {
                    return;
                }
                let response = table_row.response();
                if response.clicked() {
                    select = Some(row.path.clone());
                }
                if response.double_clicked()
                    && matches!(row.kind, RowKind::Dir { has_children: true, .. })
                {
                    toggle = Some(row.path.clone());
                }
                let is_dir = matches!(row.kind, RowKind::Dir { .. });
                response.context_menu(|ui| {
                    select = Some(row.path.clone());
                    if is_dir
                        && ui
                            .add_enabled(
                                !busy,
                                egui::Button::new(format!(
                                    "{}  {}",
                                    regular::ARROWS_CLOCKWISE,
                                    i18n.tr("disk_usage_rescan_branch")
                                )),
                            )
                            .clicked()
                    {
                        rescan_branch = Some(row.path.clone());
                        ui.close();
                    }
                    let label = if is_dir {
                        i18n.tr("disk_usage_open_in_tab")
                    } else {
                        i18n.tr("disk_usage_show_in_folder")
                    };
                    if ui
                        .button(format!("{}  {label}", regular::ARROW_SQUARE_OUT))
                        .clicked()
                    {
                        *action = Some(if is_dir {
                            DiskUsageAction::OpenInNewTab(row.path.clone())
                        } else {
                            DiskUsageAction::Reveal(row.path.clone())
                        });
                        ui.close();
                    }
                    if ui
                        .button(format!("{}  {}", regular::LINK, i18n.tr("disk_usage_copy_path")))
                        .clicked()
                    {
                        crate::core::utils::clipboard::copy_text_to_clipboard(&row.path.display().to_string());
                        ui.close();
                    }
                });
            });
        });

    state.rows = rows;
    if let Some(path) = toggle {
        if !state.expanded.remove(&path) {
            state.expanded.insert(path);
        }
        state.revision += 1;
    }
    if let Some(path) = select {
        state.selected = Some(path);
    }
    if let Some(path) = rescan_branch {
        state.start_branch_scan(path);
    }
}

fn draw_tabs(ui: &mut egui::Ui, i18n: &I18n, palette: &ThemePalette, state: &mut DiskUsageState) {
    ui.horizontal(|ui| {
        for (tab, icon, key) in [
            (Tab::Tree, regular::TREE_STRUCTURE, "disk_usage_tab_tree"),
            (Tab::Largest, regular::SORT_DESCENDING, "disk_usage_tab_largest"),
            (Tab::LargestFolders, regular::FOLDERS, "disk_usage_tab_largest_folders"),
        ] {
            let selected = state.tab == tab;
            let text = egui::RichText::new(format!("{icon} {}", i18n.tr(key)))
                .size(palette.text_size)
                .color(if selected {
                    palette.item_viewer_row_text_selected
                } else {
                    palette.text_normal
                });
            let response = ui.add(egui::Button::selectable(selected, text));
            if response.clicked() {
                state.tab = tab;
            }
        }
    });
}

/// The Largest Files list, rebuilt only after the results changed.
fn largest(state: &mut DiskUsageState) -> &[LargeFile] {
    if state.largest_revision != Some(state.revision) {
        state.largest = state
            .tree
            .as_ref()
            .map(|tree| largest_files(tree, &state.root, LARGEST_COUNT))
            .unwrap_or_default();
        state.largest_revision = Some(state.revision);
        let still_listed: HashSet<&PathBuf> = state.largest.iter().map(|f| &f.path).collect();
        state.largest_selected.retain(|p| still_listed.contains(p));
        state.largest_anchor = None;
    }
    &state.largest
}

/// The selected files in list order.
fn largest_selection(state: &DiskUsageState) -> Vec<PathBuf> {
    state
        .largest
        .iter()
        .filter(|f| state.largest_selected.contains(&f.path))
        .map(|f| f.path.clone())
        .collect()
}

fn copy_paths(paths: &[PathBuf]) {
    let text: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
    crate::core::utils::clipboard::copy_text_to_clipboard(&text.join("\r\n"));
}

fn draw_largest_toolbar(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut DiskUsageState,
    action: &mut Option<DiskUsageAction>,
) {
    let busy = state.scanning();
    let selection = largest_selection(state);
    ui.horizontal(|ui| {
        let rescan = ui
            .add_enabled_ui(!busy, |ui| {
                eden_button(ui, palette, &format!("{} {}", regular::ARROW_CLOCKWISE, i18n.tr("disk_usage_rescan")))
            })
            .inner;
        if rescan.on_hover_text(i18n.tr("tooltip_disk_usage_rescan")).clicked() {
            state.start_full_scan();
        }

        let reveal = ui
            .add_enabled_ui(selection.len() == 1, |ui| {
                eden_button(
                    ui,
                    palette,
                    &format!("{} {}", regular::ARROW_SQUARE_OUT, i18n.tr("disk_usage_show_in_folder")),
                )
            })
            .inner;
        if reveal.clicked() {
            *action = Some(DiskUsageAction::Reveal(selection[0].clone()));
        }

        let any = !selection.is_empty();
        let move_to = ui
            .add_enabled_ui(any, |ui| {
                eden_button(ui, palette, &format!("{} {}", regular::FOLDER_SIMPLE_DASHED, i18n.tr("disk_usage_move_to")))
            })
            .inner;
        if move_to.on_hover_text(i18n.tr("tooltip_disk_usage_move_to")).clicked() {
            *action = Some(DiskUsageAction::MoveTo(selection.clone()));
        }

        let delete = ui
            .add_enabled_ui(any, |ui| {
                eden_button(ui, palette, &format!("{} {}", regular::TRASH, i18n.tr("disk_usage_delete")))
            })
            .inner;
        if delete.on_hover_text(i18n.tr("tooltip_disk_usage_delete")).clicked() {
            *action = Some(DiskUsageAction::Delete {
                paths: selection.clone(),
                permanent: ui.input(|i| i.modifiers.shift),
            });
        }

        let copy = ui
            .add_enabled_ui(any, |ui| {
                eden_button(ui, palette, &format!("{} {}", regular::LINK, i18n.tr("disk_usage_copy_path")))
            })
            .inner;
        if copy.clicked() {
            copy_paths(&selection);
        }

        if any {
            ui.add_space(6.0);
            ui.label(muted(
                palette,
                format!("{} {}", format_count(selection.len() as u64), i18n.tr("disk_usage_selected")),
            ));
        }
    });
}

fn draw_largest(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut DiskUsageState,
    action: &mut Option<DiskUsageAction>,
) {
    let total_files = state.tree.as_ref().map(|t| t.file_count).unwrap_or(0);
    let total_size = state.tree.as_ref().map(|t| t.size).unwrap_or(0);
    let files = largest(state).to_vec();
    if files.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| ui.label(muted(palette, i18n.tr("disk_usage_no_files"))));
        return;
    }
    ui.label(muted(
        palette,
        format!(
            "{} {} {} {} {}",
            i18n.tr("disk_usage_showing"),
            format_count(files.len() as u64),
            i18n.tr("disk_usage_of"),
            format_count(total_files),
            i18n.tr("disk_usage_files"),
        ),
    ));
    ui.add_space(4.0);

    let mut clicked: Option<(usize, egui::Modifiers)> = None;
    let mut reveal: Option<PathBuf> = None;
    let mut menu_action: Option<DiskUsageAction> = None;
    let mut copy: Option<Vec<PathBuf>> = None;
    let row_height = (palette.text_size + 10.0).max(22.0);
    let header_color = palette.text_normal.gamma_multiply(0.75);
    ui.style_mut().interaction.selectable_labels = false;

    TableBuilder::new(ui)
        .id_salt("disk_usage_largest_table")
        .striped(false)
        .resizable(true)
        .sense(egui::Sense::click())
        .auto_shrink([false, false])
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(44.0))
        .column(Column::initial(240.0).at_least(140.0).clip(true))
        .column(Column::remainder().at_least(160.0).clip(true))
        .column(Column::initial(96.0).at_least(70.0))
        .column(Column::initial(170.0).at_least(110.0))
        .column(Column::initial(96.0).at_least(70.0))
        .header(row_height, |mut header| {
            for key in [
                "disk_usage_col_rank",
                "disk_usage_col_name",
                "disk_usage_col_folder",
                "disk_usage_col_size",
                "disk_usage_col_share_total",
                "disk_usage_col_on_disk",
            ] {
                header.col(|ui| {
                    ui.label(
                        egui::RichText::new(i18n.tr(key))
                            .strong()
                            .size(palette.text_size - 1.0)
                            .color(header_color),
                    );
                });
            }
        })
        .body(|body| {
            body.rows(row_height, files.len(), |mut row| {
                let index = row.index();
                let file = &files[index];
                let is_selected = state.largest_selected.contains(&file.path);
                row.set_selected(is_selected);
                let color = if is_selected {
                    palette.item_viewer_row_text_selected
                } else {
                    palette.text_normal
                };
                let text = |s: String| egui::RichText::new(s).size(palette.text_size).color(color);
                let mono = |s: String| {
                    egui::RichText::new(s)
                        .family(egui::FontFamily::Monospace)
                        .size(palette.text_size - 1.0)
                        .color(color)
                };

                row.col(|ui| {
                    ui.label(mono(format!("{}", index + 1)).color(color.gamma_multiply(0.7)));
                });
                let name = file
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                row.col(|ui| {
                    ui.label(text(format!("{} {name}", regular::FILE)));
                });
                row.col(|ui| {
                    let folder = file.path.parent().map(|p| p.display().to_string()).unwrap_or_default();
                    ui.label(
                        egui::RichText::new(folder)
                            .size(palette.text_size - 1.0)
                            .color(color.gamma_multiply(0.75)),
                    );
                });
                row.col(|ui| {
                    ui.label(mono(format_size(file.size)));
                });
                row.col(|ui| {
                    share_bar(ui, palette, share(file.size, total_size), palette.primary);
                });
                row.col(|ui| {
                    ui.label(mono(format_size(file.allocated)).color(color.gamma_multiply(0.85)));
                });

                let response = row.response();
                if response.clicked() {
                    clicked = Some((index, ui_modifiers(&response)));
                }
                if response.double_clicked() {
                    reveal = Some(file.path.clone());
                }
                response.context_menu(|ui| {
                    // Right-clicking outside the selection selects just
                    // that file, like the file list.
                    if !is_selected {
                        clicked = Some((index, egui::Modifiers::NONE));
                    }
                    let targets: Vec<PathBuf> = if is_selected {
                        largest_selection(state)
                    } else {
                        vec![file.path.clone()]
                    };
                    if targets.len() == 1
                        && ui
                            .button(format!("{}  {}", regular::ARROW_SQUARE_OUT, i18n.tr("disk_usage_show_in_folder")))
                            .clicked()
                    {
                        reveal = Some(targets[0].clone());
                        ui.close();
                    }
                    if ui
                        .button(format!("{}  {}", regular::FOLDER_SIMPLE_DASHED, i18n.tr("disk_usage_move_to")))
                        .clicked()
                    {
                        menu_action = Some(DiskUsageAction::MoveTo(targets.clone()));
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button(format!("{}  {}", regular::TRASH, i18n.tr("disk_usage_delete")))
                        .clicked()
                    {
                        menu_action = Some(DiskUsageAction::Delete {
                            paths: targets.clone(),
                            permanent: false,
                        });
                        ui.close();
                    }
                    if ui
                        .button(format!("{}  {}", regular::TRASH, i18n.tr("recycle_bin_delete_permanently")))
                        .clicked()
                    {
                        menu_action = Some(DiskUsageAction::Delete {
                            paths: targets.clone(),
                            permanent: true,
                        });
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button(format!("{}  {}", regular::LINK, i18n.tr("disk_usage_copy_path")))
                        .clicked()
                    {
                        copy = Some(targets.clone());
                        ui.close();
                    }
                });
            });
        });

    if let Some((index, modifiers)) = clicked {
        select_largest(state, &files, index, modifiers);
    }
    if let Some(paths) = copy {
        copy_paths(&paths);
    }
    if let Some(path) = reveal {
        *action = Some(DiskUsageAction::Reveal(path));
    } else if let Some(menu_action) = menu_action {
        *action = Some(menu_action);
    }
}

fn ui_modifiers(response: &egui::Response) -> egui::Modifiers {
    response.ctx.input(|i| i.modifiers)
}

fn select_largest(state: &mut DiskUsageState, files: &[LargeFile], index: usize, modifiers: egui::Modifiers) {
    let paths: Vec<&PathBuf> = files.iter().map(|f| &f.path).collect();
    select_row(&mut state.largest_selected, &mut state.largest_anchor, &paths, index, modifiers);
}

/// Click = select only this row, Ctrl+Click = toggle it, Shift+Click =
/// select the range from the last clicked row.
fn select_row(
    selected: &mut HashSet<PathBuf>,
    anchor: &mut Option<usize>,
    paths: &[&PathBuf],
    index: usize,
    modifiers: egui::Modifiers,
) {
    let path = paths[index].clone();
    if modifiers.shift
        && let Some(from_anchor) = anchor.filter(|a| *a < paths.len())
    {
        let (from, to) = if from_anchor <= index { (from_anchor, index) } else { (index, from_anchor) };
        if !modifiers.command {
            selected.clear();
        }
        selected.extend(paths[from..=to].iter().map(|p| (*p).clone()));
        return;
    }
    if modifiers.command {
        if !selected.remove(&path) {
            selected.insert(path);
        }
    } else {
        selected.clear();
        selected.insert(path);
    }
    *anchor = Some(index);
}

/// The Largest Folders list, rebuilt only after the results changed.
fn folders(state: &mut DiskUsageState) -> &[LargeFolder] {
    if state.folders_revision != Some(state.revision) {
        state.folders = state
            .tree
            .as_ref()
            .map(|tree| largest_folders(tree, &state.root, LARGEST_COUNT))
            .unwrap_or_default();
        state.folders_revision = Some(state.revision);
        let still_listed: HashSet<&PathBuf> = state.folders.iter().map(|f| &f.path).collect();
        state.folders_selected.retain(|p| still_listed.contains(p));
        state.folders_anchor = None;
    }
    &state.folders
}

/// The selected folders in list order.
fn folders_selection(state: &DiskUsageState) -> Vec<PathBuf> {
    state
        .folders
        .iter()
        .filter(|f| state.folders_selected.contains(&f.path))
        .map(|f| f.path.clone())
        .collect()
}

/// What a folder selection allows: the analyzed folder itself can't be
/// moved or deleted from here.
fn folder_edits_allowed(state: &DiskUsageState, selection: &[PathBuf]) -> bool {
    !selection.is_empty() && !selection.iter().any(|p| p == &state.root)
}

fn draw_folders_toolbar(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut DiskUsageState,
    action: &mut Option<DiskUsageAction>,
) {
    let busy = state.scanning();
    let selection = folders_selection(state);
    let single = (selection.len() == 1).then(|| selection[0].clone());
    let editable = folder_edits_allowed(state, &selection);
    let button = |ui: &mut egui::Ui, enabled: bool, icon: &str, key: &str| {
        ui.add_enabled_ui(enabled, |ui| eden_button(ui, palette, &format!("{icon} {}", i18n.tr(key))))
            .inner
    };
    ui.horizontal(|ui| {
        if button(ui, !busy, regular::ARROW_CLOCKWISE, "disk_usage_rescan")
            .on_hover_text(i18n.tr("tooltip_disk_usage_rescan"))
            .clicked()
        {
            state.start_full_scan();
        }
        if button(ui, !busy && single.is_some(), regular::ARROWS_CLOCKWISE, "disk_usage_rescan_branch")
            .on_hover_text(i18n.tr("tooltip_disk_usage_rescan_branch"))
            .clicked()
            && let Some(folder) = single.clone()
        {
            state.start_branch_scan(folder);
        }
        if button(ui, single.is_some(), regular::ARROW_SQUARE_OUT, "disk_usage_open_in_tab").clicked()
            && let Some(folder) = single.clone()
        {
            *action = Some(DiskUsageAction::OpenInNewTab(folder));
        }
        let reveal_ok = single.as_ref().is_some_and(|p| p.parent().is_some() && *p != state.root);
        if button(ui, reveal_ok, regular::FOLDER_OPEN, "disk_usage_show_in_folder").clicked()
            && let Some(folder) = single.clone()
        {
            *action = Some(DiskUsageAction::Reveal(folder));
        }
        if button(ui, editable && !busy, regular::FOLDER_SIMPLE_DASHED, "disk_usage_move_to")
            .on_hover_text(i18n.tr("tooltip_disk_usage_move_folders"))
            .clicked()
        {
            *action = Some(DiskUsageAction::MoveTo(selection.clone()));
        }
        if button(ui, editable && !busy, regular::TRASH, "disk_usage_delete")
            .on_hover_text(i18n.tr("tooltip_disk_usage_delete_folders"))
            .clicked()
        {
            *action = Some(DiskUsageAction::Delete {
                paths: selection.clone(),
                permanent: ui.input(|i| i.modifiers.shift),
            });
        }
        if button(ui, !selection.is_empty(), regular::LINK, "disk_usage_copy_path").clicked() {
            copy_paths(&selection);
        }
        if !selection.is_empty() {
            ui.add_space(6.0);
            ui.label(muted(
                palette,
                format!("{} {}", format_count(selection.len() as u64), i18n.tr("disk_usage_selected")),
            ));
        }
    });
}

fn draw_folders(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut DiskUsageState,
    action: &mut Option<DiskUsageAction>,
) {
    let total_dirs = state.tree.as_ref().map(|t| t.dir_count + 1).unwrap_or(0);
    let total_size = state.tree.as_ref().map(|t| t.size).unwrap_or(0);
    let busy = state.scanning();
    let folders = folders(state).to_vec();
    if folders.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| ui.label(muted(palette, i18n.tr("disk_usage_no_files"))));
        return;
    }
    ui.label(muted(
        palette,
        format!(
            "{} {} {} {} {} · {}",
            i18n.tr("disk_usage_showing"),
            format_count(folders.len() as u64),
            i18n.tr("disk_usage_of"),
            format_count(total_dirs),
            i18n.tr("disk_usage_folders"),
            i18n.tr("disk_usage_folders_ranked_by_own"),
        ),
    ));
    ui.add_space(4.0);

    let mut clicked: Option<(usize, egui::Modifiers)> = None;
    let mut chosen: Option<DiskUsageAction> = None;
    let mut copy: Option<Vec<PathBuf>> = None;
    let mut rescan_branch: Option<PathBuf> = None;
    let row_height = (palette.text_size + 10.0).max(22.0);
    let header_color = palette.text_normal.gamma_multiply(0.75);
    ui.style_mut().interaction.selectable_labels = false;

    TableBuilder::new(ui)
        .id_salt("disk_usage_folders_table")
        .striped(false)
        .resizable(true)
        .sense(egui::Sense::click())
        .auto_shrink([false, false])
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(44.0))
        .column(Column::remainder().at_least(220.0).clip(true))
        .column(Column::initial(96.0).at_least(70.0))
        .column(Column::initial(170.0).at_least(110.0))
        .column(Column::initial(70.0).at_least(50.0))
        .column(Column::initial(110.0).at_least(80.0))
        .header(row_height, |mut header| {
            for (key, tip) in [
                ("disk_usage_col_rank", None),
                ("disk_usage_col_folder", None),
                ("disk_usage_col_own_size", Some("tooltip_disk_usage_col_own_size")),
                ("disk_usage_col_share_total", None),
                ("disk_usage_col_files", None),
                ("disk_usage_col_with_subfolders", Some("tooltip_disk_usage_col_with_subfolders")),
            ] {
                header.col(|ui| {
                    let label = ui.label(
                        egui::RichText::new(i18n.tr(key))
                            .strong()
                            .size(palette.text_size - 1.0)
                            .color(header_color),
                    );
                    if let Some(tip) = tip {
                        label.on_hover_text(i18n.tr(tip));
                    }
                });
            }
        })
        .body(|body| {
            body.rows(row_height, folders.len(), |mut row| {
                let index = row.index();
                let folder = &folders[index];
                let is_selected = state.folders_selected.contains(&folder.path);
                row.set_selected(is_selected);
                let color = if is_selected {
                    palette.item_viewer_row_text_selected
                } else {
                    palette.text_normal
                };
                let mono = |s: String| {
                    egui::RichText::new(s)
                        .family(egui::FontFamily::Monospace)
                        .size(palette.text_size - 1.0)
                        .color(color)
                };

                row.col(|ui| {
                    ui.label(mono(format!("{}", index + 1)).color(color.gamma_multiply(0.7)));
                });
                row.col(|ui| {
                    // The folder's name, with the path it's in greyed after.
                    let name = folder
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| folder.path.display().to_string());
                    ui.label(
                        egui::RichText::new(format!("{} {name}", regular::FOLDER))
                            .size(palette.text_size)
                            .color(color),
                    );
                    if let Some(parent) = folder.path.parent().filter(|_| folder.path.file_name().is_some()) {
                        ui.label(
                            egui::RichText::new(parent.display().to_string())
                                .size(palette.text_size - 1.0)
                                .color(color.gamma_multiply(0.6)),
                        );
                    }
                });
                row.col(|ui| {
                    ui.label(mono(format_size(folder.own_size)));
                });
                row.col(|ui| {
                    share_bar(ui, palette, share(folder.own_size, total_size), palette.primary);
                });
                row.col(|ui| {
                    ui.label(mono(format_count(folder.own_files)).color(color.gamma_multiply(0.85)));
                });
                row.col(|ui| {
                    ui.label(mono(format_size(folder.total_size)).color(color.gamma_multiply(0.85)));
                });

                let response = row.response();
                if response.clicked() {
                    clicked = Some((index, ui_modifiers(&response)));
                }
                if response.double_clicked() {
                    chosen = Some(DiskUsageAction::OpenInNewTab(folder.path.clone()));
                }
                response.context_menu(|ui| {
                    if !is_selected {
                        clicked = Some((index, egui::Modifiers::NONE));
                    }
                    let targets: Vec<PathBuf> = if is_selected {
                        folders_selection(state)
                    } else {
                        vec![folder.path.clone()]
                    };
                    let editable = folder_edits_allowed(state, &targets);
                    let item = |ui: &mut egui::Ui, enabled: bool, icon: &str, label: String| {
                        ui.add_enabled(enabled, egui::Button::new(format!("{icon}  {label}")))
                            .clicked()
                    };
                    if targets.len() == 1 {
                        if item(ui, true, regular::ARROW_SQUARE_OUT, i18n.tr("disk_usage_open_in_tab")) {
                            chosen = Some(DiskUsageAction::OpenInNewTab(targets[0].clone()));
                            ui.close();
                        }
                        if item(ui, targets[0] != state.root, regular::FOLDER_OPEN, i18n.tr("disk_usage_show_in_folder")) {
                            chosen = Some(DiskUsageAction::Reveal(targets[0].clone()));
                            ui.close();
                        }
                        if item(ui, !busy, regular::ARROWS_CLOCKWISE, i18n.tr("disk_usage_rescan_branch")) {
                            rescan_branch = Some(targets[0].clone());
                            ui.close();
                        }
                    }
                    if item(ui, editable && !busy, regular::FOLDER_SIMPLE_DASHED, i18n.tr("disk_usage_move_to")) {
                        chosen = Some(DiskUsageAction::MoveTo(targets.clone()));
                        ui.close();
                    }
                    ui.separator();
                    if item(ui, editable && !busy, regular::TRASH, i18n.tr("disk_usage_delete")) {
                        chosen = Some(DiskUsageAction::Delete {
                            paths: targets.clone(),
                            permanent: false,
                        });
                        ui.close();
                    }
                    if item(ui, editable && !busy, regular::TRASH, i18n.tr("recycle_bin_delete_permanently")) {
                        chosen = Some(DiskUsageAction::Delete {
                            paths: targets.clone(),
                            permanent: true,
                        });
                        ui.close();
                    }
                    ui.separator();
                    if item(ui, true, regular::LINK, i18n.tr("disk_usage_copy_path")) {
                        copy = Some(targets.clone());
                        ui.close();
                    }
                });
            });
        });

    if let Some((index, modifiers)) = clicked {
        let paths: Vec<&PathBuf> = folders.iter().map(|f| &f.path).collect();
        select_row(&mut state.folders_selected, &mut state.folders_anchor, &paths, index, modifiers);
    }
    if let Some(paths) = copy {
        copy_paths(&paths);
    }
    if let Some(folder) = rescan_branch {
        state.start_branch_scan(folder);
    }
    if let Some(chosen) = chosen {
        *action = Some(chosen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, size: u64, dirs: Vec<DirNode>, files: Vec<(&str, u64)>) -> DirNode {
        let mut n = DirNode {
            name: name.into(),
            dirs,
            files: files
                .into_iter()
                .map(|(name, size)| FileEntry {
                    name: name.into(),
                    size,
                    allocated: size,
                })
                .collect(),
            ..Default::default()
        };
        n.recompute_totals();
        n.sort_children();
        if n.dirs.is_empty() && n.files.is_empty() {
            n.size = size;
        }
        n
    }

    #[test]
    fn rows_merge_folders_and_files_by_size_and_follow_expansion() {
        let root = node(
            r"C:\x",
            0,
            vec![node("big", 0, vec![], vec![("inside", 900)]), node("empty", 0, vec![], vec![])],
            vec![("mid.bin", 500), ("tiny.txt", 1)],
        );
        let root_path = PathBuf::from(r"C:\x");
        let mut expanded = HashSet::new();

        let mut rows = Vec::new();
        push_rows(&root, root_path.clone(), "C:\\x".into(), 0, root.size, &expanded, &mut rows);
        assert_eq!(rows.len(), 1, "collapsed root shows only itself");

        expanded.insert(root_path.clone());
        let mut rows = Vec::new();
        push_rows(&root, root_path.clone(), "C:\\x".into(), 0, root.size, &expanded, &mut rows);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec![r"C:\x", "big", "mid.bin", "tiny.txt", "empty"]);
        assert!((rows[0].share - 1.0).abs() < 1e-6);
        assert!((rows[1].share - 900.0 / 1401.0).abs() < 1e-6);
        assert_eq!(rows[1].depth, 1);
        assert_eq!(rows[2].path, root_path.join("mid.bin"));

        expanded.insert(root_path.join("big"));
        let mut rows = Vec::new();
        push_rows(&root, root_path.clone(), "C:\\x".into(), 0, root.size, &expanded, &mut rows);
        assert_eq!(rows[2].name, "inside");
        assert_eq!(rows[2].depth, 2);
        assert!((rows[2].share - 1.0).abs() < 1e-6);
    }

    #[test]
    fn long_folders_are_summed_up_after_the_limit() {
        let files: Vec<(String, u64)> = (0..MAX_CHILD_ROWS + 5).map(|i| (format!("f{i}"), 10)).collect();
        let root = node(
            r"C:\many",
            0,
            vec![],
            files.iter().map(|(n, s)| (n.as_str(), *s)).collect(),
        );
        let path = PathBuf::from(r"C:\many");
        let expanded: HashSet<PathBuf> = [path.clone()].into();
        let mut rows = Vec::new();
        push_rows(&root, path, "C:\\many".into(), 0, root.size, &expanded, &mut rows);
        assert_eq!(rows.len(), 1 + MAX_CHILD_ROWS + 1);
        let last = rows.last().unwrap();
        assert!(matches!(last.kind, RowKind::More { count: 5 }));
        assert_eq!(last.size, 50);
    }

    #[test]
    fn largest_list_selection_follows_click_modifiers() {
        let files: Vec<LargeFile> = (0..5)
            .map(|i| LargeFile {
                path: PathBuf::from(format!(r"C:\f{i}")),
                size: 100 - i,
                allocated: 0,
            })
            .collect();
        let mut state = DiskUsageState::default();
        let ctrl = egui::Modifiers::COMMAND;
        let shift = egui::Modifiers::SHIFT;
        select_largest(&mut state, &files, 1, egui::Modifiers::NONE);
        select_largest(&mut state, &files, 3, shift);
        state.largest = files.clone();
        let names = |s: &DiskUsageState| largest_selection(s);
        assert_eq!(names(&state), files[1..=3].iter().map(|f| f.path.clone()).collect::<Vec<_>>());
        select_largest(&mut state, &files, 2, ctrl);
        assert_eq!(names(&state), vec![files[1].path.clone(), files[3].path.clone()]);
        select_largest(&mut state, &files, 0, egui::Modifiers::NONE);
        assert_eq!(names(&state), vec![files[0].path.clone()]);
    }

    #[test]
    fn deleted_and_moved_files_leave_the_results() {
        let dir = std::env::temp_dir().join(format!("eden_du_ui_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let root = node(
            &dir.display().to_string(),
            0,
            vec![node("sub", 0, vec![], vec![("gone.bin", 700), ("kept.bin", 300)])],
            vec![("moved.bin", 500)],
        );
        std::fs::write(dir.join("sub").join("kept.bin"), b"x").unwrap();
        std::fs::write(dir.join("moved.bin"), b"x").unwrap();
        let mut state = DiskUsageState {
            root: dir.clone(),
            tree: Some(root),
            ..Default::default()
        };
        assert_eq!(largest(&mut state).len(), 3);

        // `gone.bin` doesn't exist on disk: a finished delete drops it;
        // `kept.bin` (declined) stays.
        state.files_removed(&[dir.join("sub").join("gone.bin"), dir.join("sub").join("kept.bin")]);
        assert_eq!(state.tree.as_ref().unwrap().size, 800);
        assert_eq!(largest(&mut state).len(), 2);

        // A move is watched until the file has left.
        let outside = std::env::temp_dir();
        state.files_moving(&[dir.join("moved.bin")], &outside);
        state.poll_moves();
        assert_eq!(state.pending_moves.len(), 1, "still there");
        std::fs::remove_file(dir.join("moved.bin")).unwrap();
        state.last_move_check = None;
        state.poll_moves();
        assert!(state.pending_moves.is_empty());
        assert_eq!(state.tree.as_ref().unwrap().size, 300);
        assert!(state.rescan_queue.is_empty(), "destination is outside the results");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deleted_folders_leave_the_results_and_the_root_is_protected() {
        let dir = std::env::temp_dir().join(format!("eden_du_dirs_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("keep")).unwrap();
        let root = node(
            &dir.display().to_string(),
            0,
            vec![
                node("gone", 0, vec![node("deep", 0, vec![], vec![("a", 900)])], vec![("b", 100)]),
                node("keep", 0, vec![], vec![("c", 300)]),
            ],
            vec![("top", 50)],
        );
        let mut state = DiskUsageState {
            root: dir.clone(),
            tree: Some(root),
            ..Default::default()
        };
        assert_eq!(folders(&mut state).len(), 4);
        state.folders_selected.insert(dir.join("gone").join("deep"));
        state.expanded.insert(dir.join("gone"));

        // `gone` doesn't exist on disk, so deleting it takes `deep` too.
        state.files_removed(&[dir.join("gone"), dir.join("keep")]);
        assert_eq!(state.tree.as_ref().unwrap().size, 350);
        let left: Vec<PathBuf> = folders(&mut state).iter().map(|f| f.path.clone()).collect();
        assert_eq!(left, vec![dir.join("keep"), dir.clone()]);
        assert!(state.folders_selected.is_empty());
        assert!(state.expanded.iter().all(|p| !p.starts_with(dir.join("gone"))));

        assert!(!folder_edits_allowed(&state, &[dir.clone()]), "the analyzed folder itself");
        assert!(folder_edits_allowed(&state, &[dir.join("keep")]));
        assert!(!folder_edits_allowed(&state, &[]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn counts_get_thousands_separators() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1000), "1,000");
        assert_eq!(format_count(1234567), "1,234,567");
    }
}
