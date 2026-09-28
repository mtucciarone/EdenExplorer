//! Disk Usage dashboard tools (the window itself is `disk_usage_ui`): the
//! cleanup menu every view shares, the Export menu (CSV, HTML report,
//! JSON, Copy Summary), and snapshots: save a scan, compare a later scan
//! against it in the Changes tab. The work lives in
//! `core::disk_usage_export` and `core::disk_usage_snapshot`.

use crate::core::disk_usage::DirNode;
use crate::core::disk_usage_export::{ExportKind, export, summary_text};
use crate::core::disk_usage_snapshot::{self as snapshot, ChangeKind, Comparison, SnapshotInfo};
use crate::core::utils::files::format_size;
use crate::core::utils::widgets::eden_button;
use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::disk_usage_ui::{
    DiskUsageAction, DiskUsageState, Tab, copy_paths, format_count, muted, select_row, ui_modifiers,
};
use crossbeam_channel::{Receiver, TryRecvError};
use eframe::egui;
use egui::Color32;
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a status message ("Saved…") stays in the header.
const STATUS_TIME: Duration = Duration::from_secs(6);
/// Rows the Changes list shows.
const CHANGES_SHOWN: usize = 1000;

type CompareResult = Result<(Arc<DirNode>, Comparison), String>;

/// Comparing the current scan with a saved snapshot.
pub(crate) struct CompareState {
    pub(crate) info: SnapshotInfo,
    /// The snapshot's tree, once loaded.
    base: Option<Arc<DirNode>>,
    pub(crate) result: Option<Comparison>,
    /// The `DiskUsageState::revision` `result` is for; the comparison is
    /// redone when the results change (a delete, a rescanned branch).
    revision: Option<u64>,
    job: Option<Receiver<CompareResult>>,
    error: Option<String>,
    show_files: bool,
    selected: HashSet<PathBuf>,
    anchor: Option<usize>,
}

#[derive(Default)]
pub(crate) struct ToolsState {
    /// A message for the header, and whether it's an error.
    status: Option<(String, bool, Instant)>,
    /// Saved snapshots of this folder, newest first (listed when the menu
    /// opens).
    snapshots: Vec<SnapshotInfo>,
    snapshots_listed: bool,
    save_job: Option<Receiver<Result<SnapshotInfo, String>>>,
    pub(crate) compare: Option<CompareState>,
}

impl ToolsState {
    pub(crate) fn busy(&self) -> bool {
        self.save_job.is_some() || self.compare.as_ref().is_some_and(|c| c.job.is_some())
    }
}

fn set_status(state: &mut DiskUsageState, text: String, error: bool) {
    state.tools.status = Some((text, error, Instant::now()));
}

/// Collects finished background saves and comparisons, and redoes the
/// comparison when the results changed.
pub(crate) fn poll_tools(i18n: &I18n, state: &mut DiskUsageState) {
    if let Some(rx) = &state.tools.save_job {
        match rx.try_recv() {
            Ok(result) => {
                state.tools.save_job = None;
                state.tools.snapshots_listed = false;
                match result {
                    Ok(_) => set_status(state, i18n.tr("disk_usage_snapshot_saved"), false),
                    Err(e) => set_status(state, format!("{}: {e}", i18n.tr("disk_usage_snapshot_save_failed")), true),
                }
            }
            Err(TryRecvError::Disconnected) => state.tools.save_job = None,
            Err(TryRecvError::Empty) => {}
        }
    }

    let revision = state.revision;
    let scanning = state.scanning();
    let Some(compare) = state.tools.compare.as_mut() else { return };
    if let Some(rx) = &compare.job {
        match rx.try_recv() {
            Ok(Ok((base, result))) => {
                compare.base = Some(base);
                compare.result = Some(result);
                compare.job = None;
                let listed: HashSet<&PathBuf> = compare
                    .result
                    .iter()
                    .flat_map(|r| r.folders.iter().chain(&r.files))
                    .map(|c| &c.path)
                    .collect();
                compare.selected.retain(|p| listed.contains(p));
                compare.anchor = None;
            }
            Ok(Err(e)) => {
                compare.error = Some(e);
                compare.job = None;
            }
            Err(TryRecvError::Disconnected) => compare.job = None,
            Err(TryRecvError::Empty) => {}
        }
    }
    if compare.job.is_none() && compare.error.is_none() && compare.revision != Some(revision) && !scanning {
        let Some(tree) = state.tree.clone() else { return };
        let root = state.root.clone();
        let base = compare.base.clone();
        let id = compare.info.id.clone();
        let (tx, rx) = crossbeam_channel::bounded(1);
        compare.job = Some(rx);
        compare.revision = Some(revision);
        std::thread::spawn(move || {
            let base = match base {
                Some(base) => base,
                None => match snapshot::load(&id) {
                    Ok(tree) => Arc::new(tree),
                    Err(e) => {
                        let _ = tx.send(Err(e.to_string()));
                        return;
                    }
                },
            };
            let result = snapshot::compare(&base, &tree, &root);
            let _ = tx.send(Ok((base, result)));
        });
    }
}

/// The status message, if one is showing.
pub(crate) fn draw_status(ui: &mut egui::Ui, palette: &ThemePalette, state: &DiskUsageState) {
    if let Some((text, error, at)) = &state.tools.status
        && at.elapsed() < STATUS_TIME
    {
        let color = if *error { palette.drive_usage_critical } else { palette.text_normal.gamma_multiply(0.75) };
        ui.label(egui::RichText::new(text).size(palette.text_size - 1.0).color(color));
        ui.ctx().request_repaint_after(STATUS_TIME.saturating_sub(at.elapsed()));
    }
}

/// Move To, Compress, Delete and Delete Permanently for `targets`, the
/// block every item menu in the dashboard ends with. Returns what was
/// chosen; `enabled` is false while scanning or when the analyzed folder
/// itself is among the targets.
pub(crate) fn cleanup_menu(
    ui: &mut egui::Ui,
    i18n: &I18n,
    targets: &[PathBuf],
    enabled: bool,
) -> Option<DiskUsageAction> {
    let mut chosen = None;
    let mut item = |ui: &mut egui::Ui, icon: &str, key: &str, make: &dyn Fn() -> DiskUsageAction| {
        if ui
            .add_enabled(enabled && !targets.is_empty(), egui::Button::new(format!("{icon}  {}", i18n.tr(key))))
            .clicked()
        {
            chosen = Some(make());
            ui.close();
        }
    };
    item(ui, regular::FOLDER_SIMPLE_DASHED, "disk_usage_move_to", &|| DiskUsageAction::MoveTo(targets.to_vec()));
    item(ui, regular::FILE_ZIP, "disk_usage_compress", &|| DiskUsageAction::Compress(targets.to_vec()));
    ui.separator();
    item(ui, regular::TRASH, "disk_usage_delete", &|| DiskUsageAction::Delete { paths: targets.to_vec(), permanent: false });
    item(ui, regular::TRASH, "recycle_bin_delete_permanently", &|| DiskUsageAction::Delete {
        paths: targets.to_vec(),
        permanent: true,
    });
    chosen
}

fn export_to_file(i18n: &I18n, state: &mut DiskUsageState, kind: ExportKind) {
    let Some(tree) = &state.tree else { return };
    let ext = kind.extension();
    let Some(dest) = crate::gui::windows::windowsoverrides::dialog()
        .add_filter(ext.to_uppercase(), &[ext])
        .set_file_name(format!("{}.{ext}", kind.file_stem()))
        .save_file()
    else {
        return;
    };
    match export(kind, tree, &state.root, &dest) {
        Ok(()) => set_status(state, format!("{} {}", i18n.tr("disk_usage_exported_to"), dest.display()), false),
        Err(e) => set_status(state, format!("{}: {e}", i18n.tr("disk_usage_export_failed")), true),
    }
}

/// The Export menu: the scan as CSV, an HTML report or JSON, or a short
/// summary on the clipboard.
pub(crate) fn draw_export_menu(ui: &mut egui::Ui, i18n: &I18n, state: &mut DiskUsageState) {
    let ready = state.tree.is_some() && state.scan.is_none();
    let mut export_choice = None;
    let mut copy_summary = false;
    ui.add_enabled_ui(ready, |ui| {
        ui.menu_button(format!("{} {}", regular::EXPORT, i18n.tr("disk_usage_export")), |ui| {
            for kind in ExportKind::ALL {
                let icon = match kind {
                    ExportKind::Html => regular::FILE_HTML,
                    ExportKind::Json => regular::BRACKETS_CURLY,
                    _ => regular::FILE_CSV,
                };
                if ui.button(format!("{icon}  {}…", i18n.tr(kind.i18n_key()))).clicked() {
                    export_choice = Some(kind);
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(format!("{}  {}", regular::COPY, i18n.tr("disk_usage_copy_summary"))).clicked() {
                copy_summary = true;
                ui.close();
            }
        })
        .response
        .on_hover_text(i18n.tr("tooltip_disk_usage_export"));
    });
    if let Some(kind) = export_choice {
        export_to_file(i18n, state, kind);
    }
    if copy_summary && let Some(tree) = &state.tree {
        crate::core::utils::clipboard::copy_text_to_clipboard(&summary_text(tree, &state.root));
        set_status(state, i18n.tr("disk_usage_summary_copied"), false);
    }

}

/// The Snapshots menu: save this scan, compare with (or delete) an earlier
/// one.
pub(crate) fn draw_snapshot_menu(ui: &mut egui::Ui, i18n: &I18n, palette: &ThemePalette, state: &mut DiskUsageState) {
    let ready = state.tree.is_some() && state.scan.is_none();
    let mut save = false;
    let mut compare_with = None;
    let mut delete = None;
    ui.add_enabled_ui(ready, |ui| {
        let response = ui.menu_button(format!("{} {}", regular::CAMERA, i18n.tr("disk_usage_snapshots")), |ui| {
            if !state.tools.snapshots_listed {
                state.tools.snapshots = snapshot::list()
                    .into_iter()
                    .filter(|s| snapshot::same_root(&s.root, &state.root))
                    .collect();
                state.tools.snapshots_listed = true;
            }
            let saving = state.tools.save_job.is_some();
            if ui
                .add_enabled(!saving, egui::Button::new(format!("{}  {}", regular::FLOPPY_DISK, i18n.tr("disk_usage_save_snapshot"))))
                .on_hover_text(i18n.tr("tooltip_disk_usage_save_snapshot"))
                .clicked()
            {
                save = true;
                ui.close();
            }
            ui.separator();
            if state.tools.snapshots.is_empty() {
                ui.label(muted(palette, i18n.tr("disk_usage_no_snapshots")));
            } else {
                ui.label(muted(palette, i18n.tr("disk_usage_compare_with")));
            }
            for snap in &state.tools.snapshots {
                let when = chrono::DateTime::from_timestamp(snap.taken, 0)
                    .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_default();
                ui.horizontal(|ui| {
                    let current = state.tools.compare.as_ref().is_some_and(|c| c.info.id == snap.id);
                    if ui
                        .add(egui::Button::selectable(
                            current,
                            format!("{}  {when}  ·  {}  ·  {} {}", regular::CLOCK_COUNTER_CLOCKWISE, format_size(snap.size), format_count(snap.file_count), i18n.tr("disk_usage_files")),
                        ))
                        .clicked()
                    {
                        compare_with = Some(snap.clone());
                        ui.close();
                    }
                    if ui
                        .add(egui::Button::new(regular::X).frame(false))
                        .on_hover_text(i18n.tr("disk_usage_delete_snapshot"))
                        .clicked()
                    {
                        delete = Some(snap.id.clone());
                    }
                });
            }
        });
        response.response.on_hover_text(i18n.tr("tooltip_disk_usage_snapshots"));
    });

    if save && let Some(tree) = state.tree.clone() {
        let root = state.root.clone();
        let (tx, rx) = crossbeam_channel::bounded(1);
        state.tools.save_job = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(snapshot::save(&tree, &root).map_err(|e| e.to_string()));
        });
    }
    if let Some(id) = delete {
        if let Err(e) = snapshot::delete(&id) {
            set_status(state, format!("{}: {e}", i18n.tr("disk_usage_snapshot_delete_failed")), true);
        }
        state.tools.snapshots.retain(|s| s.id != id);
        if state.tools.compare.as_ref().is_some_and(|c| c.info.id == id) {
            stop_comparing(state);
        }
    }
    if let Some(info) = compare_with {
        state.tools.compare = Some(CompareState {
            info,
            base: None,
            result: None,
            revision: None,
            job: None,
            error: None,
            show_files: false,
            selected: HashSet::new(),
            anchor: None,
        });
        state.tab = Tab::Changes;
    }
}

fn stop_comparing(state: &mut DiskUsageState) {
    state.tools.compare = None;
    if state.tab == Tab::Changes {
        state.tab = Tab::Overview;
    }
}

fn change_color(palette: &ThemePalette, kind: ChangeKind) -> Color32 {
    match kind {
        ChangeKind::Added | ChangeKind::Grew => palette.drive_usage_critical,
        ChangeKind::Removed | ChangeKind::Shrank => palette.drive_usage_normal,
    }
}

fn signed_size(delta: i64) -> String {
    let sign = if delta > 0 { "+" } else if delta < 0 { "−" } else { "" };
    format!("{sign}{}", format_size(delta.unsigned_abs()))
}

fn shown_changes(compare: &CompareState) -> &[crate::core::disk_usage_snapshot::Change] {
    let Some(result) = &compare.result else { return &[] };
    let list = if compare.show_files { &result.files } else { &result.folders };
    &list[..list.len().min(CHANGES_SHOWN)]
}

fn changes_selection(compare: &CompareState) -> Vec<PathBuf> {
    shown_changes(compare)
        .iter()
        .filter(|c| compare.selected.contains(&c.path))
        .map(|c| c.path.clone())
        .collect()
}

pub(crate) fn draw_changes_toolbar(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut DiskUsageState,
    action: &mut Option<DiskUsageAction>,
) {
    let Some(compare) = state.tools.compare.as_mut() else { return };
    let selection = changes_selection(compare);
    let mut stop = false;
    ui.horizontal(|ui| {
        for (files, icon, key) in [
            (false, regular::FOLDERS, "disk_usage_folders_label"),
            (true, regular::FILES, "disk_usage_files_label"),
        ] {
            let on = compare.show_files == files;
            let text = egui::RichText::new(format!("{icon} {}", i18n.tr(key))).color(if on {
                palette.item_viewer_row_text_selected
            } else {
                palette.text_normal
            });
            if ui.add(egui::Button::selectable(on, text)).clicked() && !on {
                compare.show_files = files;
                compare.selected.clear();
                compare.anchor = None;
            }
        }
        ui.add_space(8.0);
        let single = selection.len() == 1;
        let exists = single && std::fs::symlink_metadata(&selection[0]).is_ok();
        if ui
            .add_enabled_ui(exists, |ui| {
                eden_button(ui, palette, &format!("{} {}", regular::ARROW_SQUARE_OUT, i18n.tr("disk_usage_show_in_folder")))
            })
            .inner
            .clicked()
        {
            *action = Some(if compare.show_files {
                DiskUsageAction::Reveal(selection[0].clone())
            } else {
                DiskUsageAction::OpenInNewTab(selection[0].clone())
            });
        }
        if ui
            .add_enabled_ui(!selection.is_empty(), |ui| {
                eden_button(ui, palette, &format!("{} {}", regular::LINK, i18n.tr("disk_usage_copy_path")))
            })
            .inner
            .clicked()
        {
            copy_paths(&selection);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if eden_button(ui, palette, &format!("{} {}", regular::X, i18n.tr("disk_usage_stop_comparing"))).clicked() {
                stop = true;
            }
        });
    });
    if stop {
        stop_comparing(state);
    }
}

pub(crate) fn draw_changes(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut DiskUsageState,
    action: &mut Option<DiskUsageAction>,
) {
    let busy = state.scanning();
    let root = state.root.clone();
    let Some(compare) = state.tools.compare.as_mut() else { return };

    let when = chrono::DateTime::from_timestamp(compare.info.taken, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default();
    if let Some(error) = &compare.error {
        ui.label(egui::RichText::new(format!("{}: {error}", i18n.tr("disk_usage_compare_failed"))).color(palette.drive_usage_critical));
        return;
    }
    let Some(result) = &compare.result else {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.spinner();
            ui.label(muted(palette, i18n.tr("disk_usage_comparing")));
        });
        ui.ctx().request_repaint_after(Duration::from_millis(100));
        return;
    };

    // "Since 2026-09-20 14:02: 12.1 GB → 13.0 GB (+900 MB) · 1,000 → 1,200 files"
    let delta = result.after_size as i64 - result.before_size as i64;
    ui.horizontal_wrapped(|ui| {
        ui.label(muted(palette, format!("{} {when}:", i18n.tr("disk_usage_since_snapshot"))));
        ui.label(
            egui::RichText::new(format!("{} → {}", format_size(result.before_size), format_size(result.after_size)))
                .strong()
                .color(palette.text_normal),
        );
        let color = if delta > 0 {
            change_color(palette, ChangeKind::Grew)
        } else {
            change_color(palette, ChangeKind::Shrank)
        };
        ui.label(egui::RichText::new(format!("({})", signed_size(delta))).strong().color(color));
        ui.label(muted(
            palette,
            format!(
                "· {} → {} {} · {} {} {}",
                format_count(result.before_files),
                format_count(result.after_files),
                i18n.tr("disk_usage_files"),
                format_count(if compare.show_files { result.files.len() } else { result.folders.len() } as u64),
                i18n.tr("disk_usage_changed_items"),
                if compare.show_files { i18n.tr("disk_usage_files") } else { i18n.tr("disk_usage_folders") },
            ),
        ));
    });
    ui.add_space(4.0);

    // Borrowed (just the `result` field, so the selection can still be
    // updated below) rather than copying up to 1,000 rows every frame.
    let show_files = compare.show_files;
    let changes: &[crate::core::disk_usage_snapshot::Change] = compare
        .result
        .as_ref()
        .map(|r| {
            let list = if show_files { &r.files } else { &r.folders };
            &list[..list.len().min(CHANGES_SHOWN)]
        })
        .unwrap_or_default();
    if changes.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| ui.label(muted(palette, i18n.tr("disk_usage_no_changes"))));
        return;
    }

    let mut clicked: Option<(usize, egui::Modifiers)> = None;
    let mut chosen: Option<DiskUsageAction> = None;
    let mut copy: Option<Vec<PathBuf>> = None;
    let row_height = (palette.text_size + 10.0).max(22.0);
    let header_color = palette.text_normal.gamma_multiply(0.75);
    ui.style_mut().interaction.selectable_labels = false;
    let show_files = compare.show_files;

    TableBuilder::new(ui)
        .id_salt(("disk_usage_changes_table", show_files))
        .striped(false)
        .resizable(true)
        .sense(egui::Sense::click())
        .auto_shrink([false, false])
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(100.0).at_least(80.0))
        .column(Column::remainder().at_least(220.0).clip(true))
        .column(Column::initial(90.0).at_least(70.0))
        .column(Column::initial(90.0).at_least(70.0))
        .column(Column::initial(100.0).at_least(70.0))
        .header(row_height, |mut header| {
            for key in ["disk_usage_col_change", "disk_usage_col_path", "disk_usage_col_before", "disk_usage_col_after", "disk_usage_col_difference"] {
                header.col(|ui| {
                    ui.label(egui::RichText::new(i18n.tr(key)).strong().size(palette.text_size - 1.0).color(header_color));
                });
            }
        })
        .body(|body| {
            body.rows(row_height, changes.len(), |mut row| {
                let index = row.index();
                let change = &changes[index];
                let is_selected = compare.selected.contains(&change.path);
                row.set_selected(is_selected);
                let color = if is_selected { palette.item_viewer_row_text_selected } else { palette.text_normal };
                let mono = |s: String| {
                    egui::RichText::new(s).family(egui::FontFamily::Monospace).size(palette.text_size - 1.0).color(color)
                };
                let (icon, key) = match change.kind {
                    ChangeKind::Added => (regular::PLUS_CIRCLE, "disk_usage_change_new"),
                    ChangeKind::Removed => (regular::MINUS_CIRCLE, "disk_usage_change_removed"),
                    ChangeKind::Grew => (regular::TREND_UP, "disk_usage_change_grew"),
                    ChangeKind::Shrank => (regular::TREND_DOWN, "disk_usage_change_shrank"),
                };
                row.col(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{icon} {}", i18n.tr(key)))
                            .size(palette.text_size - 1.0)
                            .color(if is_selected { color } else { change_color(palette, change.kind) }),
                    );
                });
                row.col(|ui| {
                    let shown = change.path.strip_prefix(&root).ok().filter(|p| !p.as_os_str().is_empty());
                    let text = match shown {
                        Some(rel) => rel.display().to_string(),
                        None => change.path.display().to_string(),
                    };
                    let icon = if change.is_dir { regular::FOLDER } else { regular::FILE };
                    ui.label(egui::RichText::new(format!("{icon} {text}")).size(palette.text_size).color(color))
                        .on_hover_text(change.path.display().to_string());
                });
                row.col(|ui| {
                    ui.label(mono(if change.kind == ChangeKind::Added { "—".into() } else { format_size(change.before) }));
                });
                row.col(|ui| {
                    ui.label(mono(if change.kind == ChangeKind::Removed { "—".into() } else { format_size(change.after) }));
                });
                row.col(|ui| {
                    ui.label(mono(signed_size(change.delta())).color(if is_selected { color } else { change_color(palette, change.kind) }));
                });

                let response = row.response();
                if response.clicked() {
                    clicked = Some((index, ui_modifiers(&response)));
                }
                let exists = change.kind != ChangeKind::Removed;
                if response.double_clicked() && exists {
                    chosen = Some(if change.is_dir {
                        DiskUsageAction::OpenInNewTab(change.path.clone())
                    } else {
                        DiskUsageAction::Reveal(change.path.clone())
                    });
                }
                response.context_menu(|ui| {
                    if !is_selected {
                        clicked = Some((index, egui::Modifiers::NONE));
                    }
                    let targets: Vec<PathBuf> = if is_selected {
                        changes.iter().filter(|c| compare.selected.contains(&c.path)).map(|c| c.path.clone()).collect()
                    } else {
                        vec![change.path.clone()]
                    };
                    let all_exist = targets.iter().all(|p| std::fs::symlink_metadata(p).is_ok());
                    if targets.len() == 1 && exists {
                        let label = if change.is_dir { i18n.tr("disk_usage_open_in_tab") } else { i18n.tr("disk_usage_show_in_folder") };
                        if ui.button(format!("{}  {label}", regular::ARROW_SQUARE_OUT)).clicked() {
                            chosen = Some(if change.is_dir {
                                DiskUsageAction::OpenInNewTab(change.path.clone())
                            } else {
                                DiskUsageAction::Reveal(change.path.clone())
                            });
                            ui.close();
                        }
                    }
                    let editable = all_exist && !busy && !targets.iter().any(|p| p == &root);
                    if let Some(a) = cleanup_menu(ui, i18n, &targets, editable) {
                        chosen = Some(a);
                    }
                    ui.separator();
                    if ui.button(format!("{}  {}", regular::LINK, i18n.tr("disk_usage_copy_path"))).clicked() {
                        copy = Some(targets.clone());
                        ui.close();
                    }
                });
            });
        });

    if let Some((index, modifiers)) = clicked {
        let paths: Vec<&PathBuf> = changes.iter().map(|c| &c.path).collect();
        select_row(&mut compare.selected, &mut compare.anchor, &paths, index, modifiers);
    }
    if let Some(paths) = copy {
        copy_paths(&paths);
    }
    if let Some(chosen) = chosen {
        *action = Some(chosen);
    }
}
