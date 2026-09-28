//! Two more Disk Usage dashboard tabs (the window itself is
//! `disk_usage_ui`): Duplicates (`core::duplicates`: find identical files,
//! mark all but the newest or oldest copy, recycle or move them) and
//! Clean Up (`core::cleanup`: Storage Sense-style shortcuts for temp files,
//! the Windows Update cache, browser caches, the Recycle Bin and old
//! Downloads).

use crate::core::cleanup::{CleanupKind, Cleaned, Measure};
use crate::core::duplicates::{DupEvent, DupGroup, DupHandle, DupProgress, DupStage, Keep};
use crate::core::utils::files::format_size;
use crate::core::utils::widgets::eden_button;
use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::disk_usage_ui::{DiskUsageAction, DiskUsageState, copy_paths, format_count, muted};
use crossbeam_channel::{Receiver, TryRecvError};
use eframe::egui;
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Smallest file the duplicate finder looks at, per choice in the combo.
const MIN_SIZES: [u64; 5] = [1024, 64 * 1024, 1024 * 1024, 10 * 1024 * 1024, 100 * 1024 * 1024];
const DOWNLOAD_AGES: [u32; 4] = [30, 60, 90, 180];

pub(crate) struct DupState {
    handle: Option<DupHandle>,
    progress: DupProgress,
    pub(crate) groups: Option<Vec<DupGroup>>,
    pub(crate) marked: HashSet<PathBuf>,
    min_size: usize,
    cancelled: bool,
    /// Worked out when `groups` or `marked` change (see `invalidate`), not
    /// every frame - there can be tens of thousands of files.
    summary: Option<DupSummary>,
    /// Table rows: (group, file within it; `None` = the group's header).
    rows: Option<Vec<(usize, Option<usize>)>>,
}

#[derive(Clone, Copy, Default)]
struct DupSummary {
    marked: usize,
    marked_size: u64,
    /// At least one copy of every group stays unmarked.
    safe: bool,
    extra_copies: usize,
    wasted: u64,
}

impl Default for DupState {
    fn default() -> Self {
        Self {
            handle: None,
            progress: DupProgress::default(),
            groups: None,
            marked: HashSet::new(),
            min_size: 2,
            cancelled: false,
            summary: None,
            rows: None,
        }
    }
}

impl DupState {
    pub(crate) fn busy(&self) -> bool {
        self.handle.is_some()
    }

    /// Call after changing `groups` or `marked`.
    fn invalidate(&mut self) {
        self.summary = None;
        self.rows = None;
    }

    fn summary(&mut self) -> DupSummary {
        if let Some(summary) = self.summary {
            return summary;
        }
        let groups = self.groups.as_deref().unwrap_or_default();
        let mut summary = DupSummary { safe: true, ..Default::default() };
        for group in groups {
            let marked = group.files.iter().filter(|f| self.marked.contains(&f.path)).count();
            summary.marked += marked;
            summary.marked_size += group.size * marked as u64;
            summary.safe &= marked < group.files.len();
            summary.extra_copies += group.files.len().saturating_sub(1);
            summary.wasted += group.wasted();
        }
        self.summary = Some(summary);
        summary
    }

    fn rows(&mut self) -> &[(usize, Option<usize>)] {
        if self.rows.is_none() {
            let groups = self.groups.as_deref().unwrap_or_default();
            let mut rows = Vec::with_capacity(groups.iter().map(|g| g.files.len() + 1).sum());
            for (g, group) in groups.iter().enumerate() {
                rows.push((g, None));
                rows.extend((0..group.files.len()).map(|f| (g, Some(f))));
            }
            self.rows = Some(rows);
        }
        self.rows.as_deref().unwrap_or_default()
    }

    /// Drops a deleted or moved-away file (or everything under a folder).
    pub(crate) fn forget(&mut self, path: &Path) {
        self.invalidate();
        if let Some(groups) = &mut self.groups {
            crate::core::duplicates::forget(groups, |p| p.starts_with(path));
        }
        self.marked.retain(|p| !p.starts_with(path));
    }
}

pub(crate) struct CleanupState {
    measures: HashMap<CleanupKind, Measure>,
    measuring: Option<Receiver<(CleanupKind, Measure)>>,
    cleaning: Option<(CleanupKind, Receiver<Cleaned>)>,
    results: HashMap<CleanupKind, Cleaned>,
    /// Asking "delete permanently?" for this one.
    confirm: Option<CleanupKind>,
    download_days: u32,
}

impl Default for CleanupState {
    fn default() -> Self {
        Self {
            measures: HashMap::new(),
            measuring: None,
            cleaning: None,
            results: HashMap::new(),
            confirm: None,
            download_days: 60,
        }
    }
}

impl CleanupState {
    /// Call after files were deleted: Old Downloads is measured again if
    /// any were in Downloads.
    pub(crate) fn files_removed(&mut self, paths: &[PathBuf]) {
        let in_downloads = crate::core::cleanup::downloads_folder().is_some_and(|d| paths.iter().any(|p| p.starts_with(&d)));
        if in_downloads && self.measures.contains_key(&CleanupKind::OldDownloads) && self.measuring.is_none() {
            self.measure(vec![CleanupKind::OldDownloads]);
        }
    }

    pub(crate) fn busy(&self) -> bool {
        self.measuring.is_some() || self.cleaning.is_some()
    }

    fn measure(&mut self, kinds: Vec<CleanupKind>) {
        for kind in &kinds {
            self.measures.remove(kind);
        }
        let age = Duration::from_secs(self.download_days as u64 * 24 * 60 * 60);
        let (tx, rx) = crossbeam_channel::unbounded();
        self.measuring = Some(rx);
        std::thread::spawn(move || {
            crate::core::disk_usage::lower_thread_priority();
            for kind in kinds {
                if tx.send((kind, crate::core::cleanup::measure(kind, age))).is_err() {
                    return;
                }
            }
        });
    }
}

/// Collects finished duplicate searches, measurements and cleanups.
pub(crate) fn poll_cleanup(state: &mut DiskUsageState) {
    if let Some(handle) = &state.dups.handle {
        loop {
            match handle.rx.try_recv() {
                Ok(DupEvent::Progress(p)) => state.dups.progress = p,
                Ok(DupEvent::Finished(groups)) => {
                    state.dups.cancelled = groups.is_none();
                    if groups.is_some() {
                        state.dups.groups = groups;
                        state.dups.marked.clear();
                        state.dups.invalidate();
                    }
                    state.dups.handle = None;
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    state.dups.handle = None;
                    break;
                }
            }
        }
    }

    let cleanup = &mut state.cleanup;
    if let Some(rx) = &cleanup.measuring {
        loop {
            match rx.try_recv() {
                Ok((kind, measure)) => {
                    cleanup.measures.insert(kind, measure);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    cleanup.measuring = None;
                    break;
                }
            }
        }
    }
    let mut cleaned = None;
    if let Some((kind, rx)) = &cleanup.cleaning
        && let Ok(result) = rx.try_recv()
    {
        cleaned = Some((*kind, result));
    }
    if let Some((kind, result)) = cleaned {
        cleanup.cleaning = None;
        cleanup.results.insert(kind, result);
        cleanup.measure(vec![kind]);
        // Folders the dashboard shows that changed get rescanned.
        let mut changed = crate::core::cleanup::folders_for(kind);
        if kind == CleanupKind::RecycleBin
            && let Some(letter) = crate::core::disk_usage::drive_root_letter(&state.root)
        {
            changed.push(PathBuf::from(format!(r"{letter}:\$Recycle.Bin")));
        }
        for folder in changed {
            state.folder_changed(&folder);
        }
    }
}

fn start_search(state: &mut DiskUsageState) {
    let Some(tree) = state.tree.clone() else { return };
    state.dups.cancelled = false;
    state.dups.progress = DupProgress::default();
    state.dups.handle = Some(crate::core::duplicates::start_duplicate_search(
        tree,
        state.root.clone(),
        state.filter.clone(),
        MIN_SIZES[state.dups.min_size],
    ));
}

fn marked_in_order(state: &DiskUsageState) -> Vec<PathBuf> {
    state
        .dups
        .groups
        .iter()
        .flatten()
        .flat_map(|g| &g.files)
        .filter(|f| state.dups.marked.contains(&f.path))
        .map(|f| f.path.clone())
        .collect()
}

pub(crate) fn draw_duplicates_toolbar(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut DiskUsageState,
    action: &mut Option<DiskUsageAction>,
) {
    let busy = state.scanning() || state.dups.busy();
    let has_results = state.dups.groups.as_ref().is_some_and(|g| !g.is_empty());
    ui.horizontal(|ui| {
        let label = if state.dups.groups.is_some() { "disk_usage_dup_search_again" } else { "disk_usage_dup_find" };
        if ui
            .add_enabled_ui(!busy, |ui| eden_button(ui, palette, &format!("{} {}", regular::COPY_SIMPLE, i18n.tr(label))))
            .inner
            .on_hover_text(i18n.tr("tooltip_disk_usage_dup_find"))
            .clicked()
        {
            start_search(state);
        }
        ui.label(muted(palette, i18n.tr("disk_usage_dup_min_size")));
        ui.add_enabled_ui(!busy, |ui| {
            egui::ComboBox::from_id_salt("disk_usage_dup_min_size")
                .width(80.0)
                .selected_text(format_size(MIN_SIZES[state.dups.min_size]))
                .show_ui(ui, |ui| {
                    for (i, size) in MIN_SIZES.iter().enumerate() {
                        ui.selectable_value(&mut state.dups.min_size, i, format_size(*size));
                    }
                });
        });
        ui.separator();

        for (keep, icon, key, tip) in [
            (Keep::Newest, regular::SORT_DESCENDING, "disk_usage_dup_keep_newest", "tooltip_disk_usage_dup_keep_newest"),
            (Keep::Oldest, regular::SORT_ASCENDING, "disk_usage_dup_keep_oldest", "tooltip_disk_usage_dup_keep_oldest"),
        ] {
            if ui
                .add_enabled_ui(has_results, |ui| eden_button(ui, palette, &format!("{icon} {}", i18n.tr(key))))
                .inner
                .on_hover_text(i18n.tr(tip))
                .clicked()
                && let Some(groups) = &state.dups.groups
            {
                state.dups.marked = crate::core::duplicates::all_but_one(groups, keep).into_iter().collect();
                state.dups.invalidate();
            }
        }
        let any = !state.dups.marked.is_empty();
        if ui
            .add_enabled_ui(any, |ui| eden_button(ui, palette, &format!("{} {}", regular::X, i18n.tr("disk_usage_dup_clear_marks"))))
            .inner
            .clicked()
        {
            state.dups.marked.clear();
            state.dups.invalidate();
        }
        ui.separator();

        let summary = state.dups.summary();
        let safe = summary.safe;
        let can_act = any && safe && !state.scanning();
        let recycle = ui
            .add_enabled_ui(can_act, |ui| eden_button(ui, palette, &format!("{} {}", regular::TRASH, i18n.tr("disk_usage_dup_recycle_marked"))))
            .inner;
        let recycle = if any && !safe {
            recycle.on_disabled_hover_text(i18n.tr("disk_usage_dup_every_copy_marked"))
        } else {
            recycle.on_hover_text(i18n.tr("tooltip_disk_usage_delete"))
        };
        if recycle.clicked() {
            *action = Some(DiskUsageAction::Delete { paths: marked_in_order(state), permanent: ui.input(|i| i.modifiers.shift) });
        }
        if ui
            .add_enabled_ui(can_act, |ui| eden_button(ui, palette, &format!("{} {}", regular::FOLDER_SIMPLE_DASHED, i18n.tr("disk_usage_move_to"))))
            .inner
            .clicked()
        {
            *action = Some(DiskUsageAction::MoveTo(marked_in_order(state)));
        }
        if any {
            ui.label(muted(
                palette,
                format!(
                    "{} {} · {}",
                    format_count(summary.marked as u64),
                    i18n.tr("disk_usage_dup_marked"),
                    format_size(summary.marked_size)
                ),
            ));
        }
    });
}

fn draw_dup_progress(ui: &mut egui::Ui, i18n: &I18n, palette: &ThemePalette, state: &mut DiskUsageState) {
    let p = &state.dups.progress;
    let stage = match p.stage {
        DupStage::Sizes => i18n.tr("disk_usage_dup_stage_sizes"),
        DupStage::Quick => i18n.tr("disk_usage_dup_stage_quick"),
        DupStage::Full => i18n.tr("disk_usage_dup_stage_full"),
    };
    let fraction = if p.bytes_total > 0 { p.bytes_done as f32 / p.bytes_total as f32 } else { 0.0 };
    ui.add_space(30.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(stage).strong().color(palette.text_normal));
        ui.add_space(6.0);
        ui.add(egui::ProgressBar::new(fraction).desired_width(420.0).show_percentage());
        ui.add_space(4.0);
        ui.label(muted(
            palette,
            format!(
                "{} / {} {} · {} / {}",
                format_count(p.files_done),
                format_count(p.files_total),
                i18n.tr("disk_usage_files"),
                format_size(p.bytes_done),
                format_size(p.bytes_total)
            ),
        ));
        ui.add_space(8.0);
        if eden_button(ui, palette, &format!("{} {}", regular::X, i18n.tr("cancel"))).clicked()
            && let Some(handle) = &state.dups.handle
        {
            handle.cancel();
        }
    });
    ui.ctx().request_repaint_after(Duration::from_millis(100));
}

pub(crate) fn draw_duplicates(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut DiskUsageState,
    action: &mut Option<DiskUsageAction>,
) {
    if state.dups.busy() {
        draw_dup_progress(ui, i18n, palette, state);
        return;
    }
    if state.dups.groups.is_none() {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(regular::COPY_SIMPLE).size(34.0).color(palette.text_normal.gamma_multiply(0.5)));
            ui.add_space(6.0);
            ui.label(muted(palette, i18n.tr(if state.dups.cancelled { "disk_usage_cancelled" } else { "disk_usage_dup_intro" })));
            ui.add_space(10.0);
            if eden_button(ui, palette, &format!("{} {}", regular::MAGNIFYING_GLASS, i18n.tr("disk_usage_dup_find"))).clicked() {
                start_search(state);
            }
        });
        return;
    }
    if state.dups.groups.as_ref().is_some_and(|g| g.is_empty()) {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| ui.label(muted(palette, i18n.tr("disk_usage_dup_none"))));
        return;
    }
    let summary = state.dups.summary();
    let (copies, wasted) = (summary.extra_copies, summary.wasted);
    // Borrowed out of the state for the draw (not cloned: there can be tens
    // of thousands of files), and put back below.
    state.dups.rows();
    let rows = state.dups.rows.take().unwrap_or_default();
    let groups = state.dups.groups.take().unwrap_or_default();
    ui.label(muted(
        palette,
        format!(
            "{} {} · {} {} · {} {}",
            format_count(groups.len() as u64),
            i18n.tr("disk_usage_dup_groups"),
            format_count(copies as u64),
            i18n.tr("disk_usage_dup_extra_copies"),
            format_size(wasted),
            i18n.tr("disk_usage_dup_can_be_freed"),
        ),
    ));
    ui.add_space(4.0);

    let row_height = (palette.text_size + 10.0).max(22.0);
    let header_color = palette.text_normal.gamma_multiply(0.75);
    ui.style_mut().interaction.selectable_labels = false;
    let busy = state.scanning();
    let mut toggle: Option<PathBuf> = None;
    let mut chosen: Option<DiskUsageAction> = None;
    let mut copy: Option<Vec<PathBuf>> = None;

    TableBuilder::new(ui)
        .id_salt("disk_usage_dups_table")
        .striped(false)
        .resizable(true)
        .sense(egui::Sense::click())
        .auto_shrink([false, false])
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(30.0))
        .column(Column::remainder().at_least(260.0).clip(true))
        .column(Column::initial(140.0).at_least(100.0))
        .column(Column::initial(100.0).at_least(70.0))
        .header(row_height, |mut header| {
            for key in ["", "disk_usage_col_name", "disk_usage_col_modified", "disk_usage_col_size"] {
                header.col(|ui| {
                    if !key.is_empty() {
                        ui.label(egui::RichText::new(i18n.tr(key)).strong().size(palette.text_size - 1.0).color(header_color));
                    }
                });
            }
        })
        .body(|body| {
            body.rows(row_height, rows.len(), |mut row| {
                let (g, file_index) = rows[row.index()];
                let group = &groups[g];
                match file_index {
                    None => {
                        row.col(|_| {});
                        row.col(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} {} × {} · {} {}",
                                    regular::COPY_SIMPLE,
                                    format_size(group.size),
                                    group.files.len(),
                                    format_size(group.wasted()),
                                    i18n.tr("disk_usage_dup_can_be_freed")
                                ))
                                .strong()
                                .size(palette.text_size - 1.0)
                                .color(palette.text_header_section),
                            );
                        });
                        row.col(|_| {});
                        row.col(|_| {});
                    }
                    Some(index) => {
                        let file = &group.files[index];
                        let is_marked = state.dups.marked.contains(&file.path);
                        let color = if is_marked { palette.text_normal.gamma_multiply(0.55) } else { palette.text_normal };
                        row.col(|ui| {
                            let mut on = is_marked;
                            if ui.checkbox(&mut on, "").on_hover_text(i18n.tr("tooltip_disk_usage_dup_mark")).changed() {
                                toggle = Some(file.path.clone());
                            }
                        });
                        row.col(|ui| {
                            ui.add_space(8.0);
                            let name = file.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            let mut text = egui::RichText::new(format!("{} {name}", regular::FILE)).size(palette.text_size).color(color);
                            if is_marked {
                                text = text.strikethrough();
                            }
                            ui.label(text);
                            if let Some(parent) = file.path.parent() {
                                ui.label(egui::RichText::new(parent.display().to_string()).size(palette.text_size - 1.0).color(color.gamma_multiply(0.6)));
                            }
                        });
                        row.col(|ui| {
                            ui.label(
                                egui::RichText::new(crate::core::disk_usage_export::filetime_text(file.modified))
                                    .family(egui::FontFamily::Monospace)
                                    .size(palette.text_size - 1.0)
                                    .color(color),
                            );
                        });
                        row.col(|ui| {
                            ui.label(egui::RichText::new(format_size(group.size)).family(egui::FontFamily::Monospace).size(palette.text_size - 1.0).color(color));
                        });
                        let response = row.response();
                        if response.clicked() {
                            toggle = Some(file.path.clone());
                        }
                        if response.double_clicked() {
                            chosen = Some(DiskUsageAction::Reveal(file.path.clone()));
                        }
                        response.context_menu(|ui| {
                            if ui.button(format!("{}  {}", regular::ARROW_SQUARE_OUT, i18n.tr("disk_usage_show_in_folder"))).clicked() {
                                chosen = Some(DiskUsageAction::Reveal(file.path.clone()));
                                ui.close();
                            }
                            let targets = [file.path.clone()];
                            if let Some(a) = crate::gui::windows::disk_usage_tools::cleanup_menu(ui, i18n, &targets, !busy) {
                                chosen = Some(a);
                            }
                            ui.separator();
                            if ui.button(format!("{}  {}", regular::LINK, i18n.tr("disk_usage_copy_path"))).clicked() {
                                copy = Some(targets.to_vec());
                                ui.close();
                            }
                        });
                    }
                }
            });
        });

    state.dups.groups = Some(groups);
    state.dups.rows = Some(rows);
    if let Some(path) = toggle {
        if !state.dups.marked.remove(&path) {
            state.dups.marked.insert(path);
        }
        state.dups.invalidate();
    }
    if let Some(paths) = copy {
        copy_paths(&paths);
    }
    if let Some(chosen) = chosen {
        *action = Some(chosen);
    }
}

fn kind_text(kind: CleanupKind) -> (&'static str, &'static str, &'static str) {
    match kind {
        CleanupKind::TempFiles => (regular::FILE_DASHED, "disk_usage_clean_temp", "disk_usage_clean_temp_desc"),
        CleanupKind::WindowsUpdate => (regular::ARROWS_CLOCKWISE, "disk_usage_clean_windows_update", "disk_usage_clean_windows_update_desc"),
        CleanupKind::BrowserCaches => (regular::GLOBE, "disk_usage_clean_browser", "disk_usage_clean_browser_desc"),
        CleanupKind::RecycleBin => (regular::TRASH, "disk_usage_clean_recycle_bin", "disk_usage_clean_recycle_bin_desc"),
        CleanupKind::OldDownloads => (regular::DOWNLOAD_SIMPLE, "disk_usage_clean_downloads", "disk_usage_clean_downloads_desc"),
    }
}

pub(crate) fn draw_cleanup_toolbar(ui: &mut egui::Ui, i18n: &I18n, palette: &ThemePalette, state: &mut DiskUsageState) {
    let busy = state.cleanup.busy();
    ui.horizontal(|ui| {
        if ui
            .add_enabled_ui(!busy, |ui| eden_button(ui, palette, &format!("{} {}", regular::ARROW_CLOCKWISE, i18n.tr("disk_usage_clean_measure_again"))))
            .inner
            .clicked()
        {
            state.cleanup.measure(CleanupKind::ALL.to_vec());
        }
        let total: u64 = state.cleanup.measures.values().map(|m| m.size).sum();
        if !state.cleanup.measures.is_empty() {
            ui.add_space(6.0);
            ui.label(muted(palette, format!("{} {}", i18n.tr("disk_usage_clean_up_to"), format_size(total))));
        }
        if busy {
            ui.spinner();
        }
    });
}

pub(crate) fn draw_cleanup(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut DiskUsageState,
    action: &mut Option<DiskUsageAction>,
) {
    if state.cleanup.measures.is_empty() && state.cleanup.measuring.is_none() {
        state.cleanup.measure(CleanupKind::ALL.to_vec());
    }
    if state.cleanup.busy() {
        ui.ctx().request_repaint_after(Duration::from_millis(150));
    }
    let mut remeasure_downloads = false;
    let mut start_clean = None;
    let cleanup = &mut state.cleanup;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.label(muted(palette, i18n.tr("disk_usage_clean_intro")));
        ui.add_space(6.0);
        for kind in CleanupKind::ALL {
            let (icon, title, desc) = kind_text(kind);
            let measure = cleanup.measures.get(&kind);
            let cleaning_this = cleanup.cleaning.as_ref().is_some_and(|(k, _)| *k == kind);
            egui::Frame::NONE
                .fill(palette.row_bg)
                .corner_radius(egui::CornerRadius::same(palette.medium_radius))
                .stroke(egui::Stroke::new(1.0, palette.borders_default))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(icon).size(26.0).color(palette.primary));
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(i18n.tr(title)).strong().size(palette.text_size + 1.0).color(palette.text_header_section));
                            ui.label(muted(palette, i18n.tr(desc)));
                            if let Some(done) = cleanup.results.get(&kind) {
                                let mut text = format!("{} {} ({} {})", i18n.tr("disk_usage_clean_freed"), format_size(done.freed), format_count(done.deleted), i18n.tr("disk_usage_files"));
                                if done.skipped > 0 {
                                    text.push_str(&format!(" · {} {}", format_count(done.skipped), i18n.tr("disk_usage_clean_skipped")));
                                }
                                ui.label(egui::RichText::new(text).size(palette.text_size - 1.0).color(palette.drive_usage_normal));
                            }
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let size_text = match measure {
                                None => i18n.tr("disk_usage_clean_measuring"),
                                Some(m) if m.denied && m.size == 0 => i18n.tr("disk_usage_clean_needs_admin"),
                                Some(m) => format!("{} · {} {}", format_size(m.size), format_count(m.files), i18n.tr(if kind == CleanupKind::RecycleBin { "disk_usage_clean_items" } else { "disk_usage_files" })),
                            };
                            let has_something = measure.is_some_and(|m| m.files > 0 || m.size > 0);
                            let enabled = has_something && cleanup.cleaning.is_none() && cleanup.measuring.is_none();

                            if cleaning_this {
                                ui.spinner();
                                ui.label(muted(palette, i18n.tr("disk_usage_clean_cleaning")));
                            } else if cleanup.confirm == Some(kind) {
                                if eden_button(ui, palette, &i18n.tr("cancel")).clicked() {
                                    cleanup.confirm = None;
                                }
                                if eden_button(ui, palette, &format!("{} {}", regular::TRASH, i18n.tr("recycle_bin_delete_permanently"))).clicked() {
                                    cleanup.confirm = None;
                                    start_clean = Some(kind);
                                }
                                ui.label(egui::RichText::new(i18n.tr("disk_usage_clean_confirm")).color(palette.drive_usage_critical));
                            } else {
                                let label = if kind == CleanupKind::OldDownloads { "disk_usage_clean_recycle" } else { "disk_usage_clean_button" };
                                if ui
                                    .add_enabled_ui(enabled, |ui| eden_button(ui, palette, &format!("{} {}", regular::BROOM, i18n.tr(label))))
                                    .inner
                                    .clicked()
                                {
                                    if kind == CleanupKind::OldDownloads {
                                        if let Some(m) = measure {
                                            // Measured again once they're gone (`files_removed`).
                                            *action = Some(DiskUsageAction::Delete { paths: m.paths.clone(), permanent: false });
                                        }
                                    } else {
                                        cleanup.confirm = Some(kind);
                                    }
                                }
                                if kind == CleanupKind::OldDownloads
                                    && let Some(dir) = crate::core::cleanup::downloads_folder()
                                    && ui.add(egui::Button::new(regular::FOLDER_OPEN).frame(false)).on_hover_text(i18n.tr("disk_usage_open_in_tab")).clicked()
                                {
                                    *action = Some(DiskUsageAction::OpenInNewTab(dir));
                                }
                                ui.add_space(6.0);
                                ui.label(egui::RichText::new(size_text).family(egui::FontFamily::Monospace).size(palette.text_size - 1.0).color(palette.text_normal));
                                if kind == CleanupKind::OldDownloads {
                                    ui.add_space(6.0);
                                    let before = cleanup.download_days;
                                    egui::ComboBox::from_id_salt("disk_usage_download_age")
                                        .width(90.0)
                                        .selected_text(format!("{} {}", before, i18n.tr("disk_usage_clean_days")))
                                        .show_ui(ui, |ui| {
                                            for days in DOWNLOAD_AGES {
                                                ui.selectable_value(&mut cleanup.download_days, days, format!("{days} {}", i18n.tr("disk_usage_clean_days")));
                                            }
                                        });
                                    if cleanup.download_days != before {
                                        remeasure_downloads = true;
                                    }
                                    ui.label(muted(palette, i18n.tr("disk_usage_clean_older_than")));
                                }
                            }
                        });
                    });
                });
            ui.add_space(6.0);
        }
        ui.label(muted(palette, i18n.tr("disk_usage_clean_note")));
    });

    if let Some(kind) = start_clean {
        let (tx, rx) = crossbeam_channel::bounded(1);
        state.cleanup.cleaning = Some((kind, rx));
        state.cleanup.results.remove(&kind);
        std::thread::spawn(move || {
            let _ = tx.send(crate::core::cleanup::clean(kind));
        });
    }
    if remeasure_downloads && state.cleanup.measuring.is_none() {
        state.cleanup.measure(vec![CleanupKind::OldDownloads]);
    }
}
