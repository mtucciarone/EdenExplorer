use crate::core::robocopy::{RoboFinish, RobocopyHandle, RobocopyUpdate};
use crate::core::utils::widgets::{clickable_icon, clickable_icon_sized, eden_button};
use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use eframe::egui;
use egui::{Align, Align2, Color32, CornerRadius, FontId, Layout, RichText, Stroke};
use egui_phosphor::regular;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long a completion toast stays on screen before auto-hiding.
const TOAST_DURATION: Duration = Duration::from_secs(4);
const PANEL_WIDTH: f32 = 460.0;
const PANEL_MAX_HEIGHT: f32 = 480.0;
/// Rough height of one `draw_operation_row` (icon/title/status line, its
/// progress bar slot, and the spacing+separator drawn after it) - used only
/// to pre-reserve room for the rows about to be drawn, not as an exact
/// measurement.
const ROW_HEIGHT_ESTIMATE: f32 = 46.0;
/// The bell icon's own glyph size - bumped up from the previous default
/// (`clickable_icon`'s plain `FontId::default().size`) so it reads clearly
/// at a glance next to the window controls, rather than blending in.
const BELL_ICON_SIZE: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileOpKind {
    Copy,
    Move,
    Delete,
    Rename,
    Compress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileOpStatus {
    InProgress,
    /// Stopped by the user via the panel's Pause button - robocopy jobs
    /// only, see `core::robocopy`. Resumable.
    Paused,
    Completed,
    Failed,
    Cancelled,
}

/// Marks a notification entry as the result of an Undo or Redo, so its title
/// says so ("Undo: Renaming 1 Item") instead of looking like a fresh action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryAction {
    Undo,
    Redo,
}

/// One entry in the notification list - a single copy/move/delete operation,
/// from the moment it's kicked off through however it ends. Phase A wires
/// this to the existing `IFileOperation`-backed engine (see
/// `mainwindow_imp.rs`): operations that already run on a background thread
/// (paste/cut-move) get a real `InProgress` -> `Completed` transition,
/// while ones that still run synchronously (delete, drag-drop moves) are
/// recorded already-finished, since the native OS dialog already covers
/// their in-flight progress for now.
pub struct FileOperation {
    pub id: u64,
    pub kind: FileOpKind,
    pub status: FileOpStatus,
    pub item_count: usize,
    /// Human-readable destination/context, e.g. a folder name or "Recycle Bin" -
    /// already localized/formatted by the caller, not a raw path (paths can be
    /// long and this is meant to fit a narrow notification row).
    pub destination_label: String,
    /// Reserved for a future elapsed-time/duration display in the panel.
    #[allow(dead_code)]
    pub started_at: Instant,
    pub finished_at: Option<Instant>,
    /// 0.0..=1.0 - only ever set for robocopy-backed operations (see
    /// `core::robocopy`); `IFileOperation`-backed ones have no progress
    /// callback to source this from, so they stay `None` and the panel
    /// just shows their status text with no bar.
    pub progress: Option<f32>,
    /// Set when this operation was carried out by Undo/Redo - see
    /// `NotificationsState::mark_history`.
    pub history: Option<HistoryAction>,
}

struct ActiveToast {
    op_id: u64,
    shown_at: Instant,
}

#[derive(Default)]
pub struct NotificationsState {
    /// Newest-first.
    pub operations: Vec<FileOperation>,
    pub panel_open: bool,
    next_id: u64,
    active_toast: Option<ActiveToast>,
    /// Live/paused robocopy jobs, keyed by their operation's id - present
    /// only for copy/move operations running the robocopy-backed engine,
    /// and removed once a job reaches a terminal state. Its mere presence
    /// is what the panel uses to decide whether to show Pause/Resume/
    /// Cancel controls for a row instead of the plain dismiss button.
    robocopy_jobs: HashMap<u64, RobocopyHandle>,
}

impl NotificationsState {
    /// Records a new operation as `InProgress` and returns its id, to be
    /// passed to `finish_operation` once it completes. Also surfaces the
    /// transient toast immediately (previously only shown on completion via
    /// `finish_operation`/`poll_robocopy_jobs`'s terminal-state handling) -
    /// a copy/move that's just been started (via the robocopy-backed
    /// engine, `mainwindow_imp.rs::start_robocopy_paste`) otherwise gave no
    /// visible sign anything was happening beyond a small badge count on
    /// the bell icon, which is easy to miss. `draw_toast` re-reads the
    /// operation's live status/progress by id every frame, so this same
    /// toast instance naturally updates in place as the copy progresses;
    /// if the operation outlives the toast's own 4-second display window,
    /// reaching a terminal state resets `active_toast`'s timer again (see
    /// `poll_robocopy_jobs`), giving a second, separate "it's done" toast
    /// rather than leaving the user with no completion signal at all.
    ///
    /// Also opens the bell's dropdown panel itself (`panel_open`), not just
    /// the toast - the toast is a compact, auto-dismissing summary with no
    /// progress bar or Pause/Resume/Cancel controls, while the panel shows
    /// the operation's live progress fraction and those controls for as
    /// long as the user wants to watch it. Requested directly: a copy/
    /// delete/rename/paste-conflict-replace starting should make that
    /// detail visible without the user having to remember to click the
    /// bell. The panel still closes the normal way (clicking elsewhere, or
    /// clicking the bell again - see the `clicked_elsewhere` handling in
    /// `draw_notifications_button`), so this doesn't pin it open forever.
    ///
    /// `auto_open_panel` gates only the panel auto-opening (Settings ->
    /// Behavior's "Automatically show notification panel" toggle) - the
    /// toast's own visibility is a separate, later decision made by
    /// `draw_toast`'s own caller, not by this method.
    pub fn start_operation(
        &mut self,
        kind: FileOpKind,
        item_count: usize,
        destination_label: String,
        auto_open_panel: bool,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.operations.insert(
            0,
            FileOperation {
                id,
                kind,
                status: FileOpStatus::InProgress,
                item_count,
                destination_label,
                started_at: Instant::now(),
                finished_at: None,
                progress: None,
                history: None,
            },
        );
        self.active_toast = Some(ActiveToast {
            op_id: id,
            shown_at: Instant::now(),
        });
        if auto_open_panel {
            self.panel_open = true;
        }
        id
    }

    /// Labels an existing entry (and so its toast, which reads the same
    /// entry) as the result of an Undo or Redo.
    pub fn mark_history(&mut self, id: u64, action: HistoryAction) {
        if let Some(op) = self.operations.iter_mut().find(|op| op.id == id) {
            op.history = Some(action);
        }
    }

    /// Attaches a live robocopy job to an operation created via
    /// `start_operation` - its presence is what makes the panel show
    /// Pause/Resume/Cancel controls for that row instead of the plain
    /// dismiss button.
    pub fn attach_robocopy_job(&mut self, id: u64, handle: RobocopyHandle) {
        self.robocopy_jobs.insert(id, handle);
    }

    pub fn has_robocopy_job(&self, id: u64) -> bool {
        self.robocopy_jobs.contains_key(&id)
    }

    pub fn pause_job(&mut self, id: u64) {
        if let Some(handle) = self.robocopy_jobs.get(&id) {
            handle.request_pause();
        }
    }

    pub fn resume_job(&mut self, id: u64) {
        if let Some(handle) = self.robocopy_jobs.get_mut(&id) {
            handle.resume();
        }
        if let Some(op) = self.operations.iter_mut().find(|o| o.id == id) {
            op.status = FileOpStatus::InProgress;
        }
    }

    pub fn cancel_job(&mut self, id: u64) {
        if let Some(handle) = self.robocopy_jobs.get(&id) {
            handle.request_cancel();
        }
    }

    /// Drains every tracked robocopy job's progress/result channel, updating
    /// each operation's status/progress in place. Returns `(id, succeeded)`
    /// for every operation that just reached a terminal state
    /// (Completed/Failed/Cancelled - not Paused, which isn't terminal) this
    /// call. The caller owns paste-completion side effects (tag remapping,
    /// selecting the pasted items, refreshing the view) that only make
    /// sense when `succeeded` is true, but needs to know about failed/
    /// cancelled ids too so it can drop its own per-operation bookkeeping
    /// (see `PendingPaste` in `mainwindow_imp.rs`) rather than leaking it.
    /// Call once per frame.
    pub fn poll_robocopy_jobs(&mut self) -> Vec<(u64, bool)> {
        let mut terminal = Vec::new();

        for (&id, handle) in self.robocopy_jobs.iter() {
            while let Ok(update) = handle.rx.try_recv() {
                match update {
                    RobocopyUpdate::Progress(frac) => {
                        if let Some(op) = self.operations.iter_mut().find(|o| o.id == id) {
                            op.progress = Some(frac);
                        }
                    }
                    RobocopyUpdate::Finished(RoboFinish::Completed) => {
                        if let Some(op) = self.operations.iter_mut().find(|o| o.id == id) {
                            op.status = FileOpStatus::Completed;
                            op.progress = Some(1.0);
                            op.finished_at = Some(Instant::now());
                        }
                        self.active_toast = Some(ActiveToast {
                            op_id: id,
                            shown_at: Instant::now(),
                        });
                        terminal.push((id, true));
                    }
                    RobocopyUpdate::Finished(RoboFinish::Failed) => {
                        if let Some(op) = self.operations.iter_mut().find(|o| o.id == id) {
                            op.status = FileOpStatus::Failed;
                            op.finished_at = Some(Instant::now());
                        }
                        self.active_toast = Some(ActiveToast {
                            op_id: id,
                            shown_at: Instant::now(),
                        });
                        terminal.push((id, false));
                    }
                    RobocopyUpdate::Finished(RoboFinish::Cancelled) => {
                        if let Some(op) = self.operations.iter_mut().find(|o| o.id == id) {
                            op.status = FileOpStatus::Cancelled;
                            op.finished_at = Some(Instant::now());
                        }
                        terminal.push((id, false));
                    }
                    RobocopyUpdate::Finished(RoboFinish::Paused) => {
                        if let Some(op) = self.operations.iter_mut().find(|o| o.id == id) {
                            op.status = FileOpStatus::Paused;
                        }
                    }
                }
            }
        }

        for &(id, _) in &terminal {
            self.robocopy_jobs.remove(&id);
        }

        terminal
    }

    /// Records an operation that already ran to completion synchronously
    /// (no separate `start_operation` call) - e.g. delete, or a drag-drop
    /// move that still blocks the UI thread today.
    pub fn record_finished(
        &mut self,
        kind: FileOpKind,
        item_count: usize,
        destination_label: String,
        status: FileOpStatus,
        auto_open_panel: bool,
    ) -> u64 {
        let id = self.start_operation(kind, item_count, destination_label, auto_open_panel);
        self.finish_operation(id, status);
        id
    }

    pub fn finish_operation(&mut self, id: u64, status: FileOpStatus) {
        if let Some(op) = self.operations.iter_mut().find(|o| o.id == id) {
            op.status = status;
            op.finished_at = Some(Instant::now());
        }
        self.active_toast = Some(ActiveToast {
            op_id: id,
            shown_at: Instant::now(),
        });
    }

    pub fn in_progress_count(&self) -> usize {
        self.operations
            .iter()
            .filter(|o| o.status == FileOpStatus::InProgress)
            .count()
    }

    /// Drops every tracked operation, including ones still `InProgress` -
    /// this only clears them from the panel/badge, it doesn't touch the
    /// underlying `IFileOperation` call actually running on its background
    /// thread. If that thread later calls `finish_operation` for an id that
    /// was cleared here, it's a harmless no-op (see that function).
    pub fn clear_all(&mut self) {
        self.operations.clear();
    }
}

fn kind_icon(kind: FileOpKind) -> &'static str {
    match kind {
        FileOpKind::Copy => regular::COPY,
        FileOpKind::Move => regular::ARROWS_LEFT_RIGHT,
        FileOpKind::Delete => regular::TRASH,
        FileOpKind::Rename => regular::PENCIL_SIMPLE,
        FileOpKind::Compress => regular::FILE_ZIP,
    }
}

fn status_icon(status: FileOpStatus) -> &'static str {
    match status {
        FileOpStatus::InProgress => regular::SPINNER,
        FileOpStatus::Paused => regular::PAUSE_CIRCLE,
        FileOpStatus::Completed => regular::CHECK_CIRCLE,
        FileOpStatus::Failed => regular::WARNING_CIRCLE,
        FileOpStatus::Cancelled => regular::X_CIRCLE,
    }
}

/// Standard, user-overridable semantic colors rather than the accent - a
/// completed operation reads as genuinely "done" (green) regardless of
/// which accent color the theme happens to use, matching the same
/// completed/in-progress/failed convention as `notification_status_*`'s own
/// doc comments. `Failed` and `Cancelled` share the "error" color rather
/// than `Cancelled` getting its own fourth shade - the icon glyph
/// (checkmark/warning/x) and status text already distinguish them, so the
/// color only needs to carry "this one didn't finish normally."
fn status_color(status: FileOpStatus, palette: &ThemePalette) -> Color32 {
    match status {
        FileOpStatus::Completed => palette.notification_status_success,
        FileOpStatus::InProgress | FileOpStatus::Paused => palette.notification_status_warning,
        FileOpStatus::Failed | FileOpStatus::Cancelled => palette.notification_status_error,
    }
}

fn kind_label(i18n: &I18n, kind: FileOpKind) -> String {
    match kind {
        FileOpKind::Copy => i18n.tr("notifications_kind_copy"),
        FileOpKind::Move => i18n.tr("notifications_kind_move"),
        FileOpKind::Delete => i18n.tr("notifications_kind_delete"),
        FileOpKind::Rename => i18n.tr("notifications_kind_rename"),
        FileOpKind::Compress => i18n.tr("notifications_kind_compress"),
    }
}

fn status_label(i18n: &I18n, status: FileOpStatus) -> String {
    match status {
        FileOpStatus::InProgress => i18n.tr("notifications_status_in_progress"),
        FileOpStatus::Paused => i18n.tr("notifications_status_paused"),
        FileOpStatus::Completed => i18n.tr("notifications_status_completed"),
        FileOpStatus::Failed => i18n.tr("notifications_status_failed"),
        FileOpStatus::Cancelled => i18n.tr("notifications_status_cancelled"),
    }
}

fn item_word(i18n: &I18n, count: usize) -> String {
    if count == 1 {
        i18n.tr("item_capital")
    } else {
        i18n.tr("items_capital")
    }
}

/// e.g. "Copying 12 Items to Downloads" / "Deleting 3 Items".
fn operation_title(i18n: &I18n, op: &FileOperation) -> String {
    let verb = kind_label(i18n, op.kind);
    let items = item_word(i18n, op.item_count);
    let title = if op.destination_label.is_empty() {
        format!("{verb} {} {items}", op.item_count)
    } else {
        format!(
            "{verb} {} {items} {} {}",
            op.item_count,
            i18n.tr("notifications_to"),
            op.destination_label
        )
    };
    match op.history {
        Some(HistoryAction::Undo) => format!("{}: {title}", i18n.tr("notifications_history_undo")),
        Some(HistoryAction::Redo) => format!("{}: {title}", i18n.tr("notifications_history_redo")),
        None => title,
    }
}

/// Draws the bell icon button (with an in-progress badge) and, when clicked,
/// its dropdown panel listing every tracked operation. Call once, anywhere a
/// single icon-sized widget would go - designed for the app's top-right
/// window-controls row.
pub fn draw_notifications_button(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut NotificationsState,
) {
    let in_progress = state.in_progress_count();
    let icon = if in_progress > 0 {
        regular::BELL_RINGING
    } else {
        regular::BELL
    };

    // A few px of breathing room on the right, inside the bell's own slot -
    // the in-progress badge extends past the icon's own glyph, and without
    // this it can crowd right up against whatever sits immediately to the
    // right of this widget (the window's minimize button, in practice).
    ui.add_space(4.0);

    let resp = clickable_icon_sized(ui, icon, palette, BELL_ICON_SIZE)
        .on_hover_text(
            RichText::new(i18n.tr("tooltip_notifications"))
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);

    // The badge shows the total notification count (not just in-progress
    // ones) - "how many are in the panel" rather than "how many are still
    // running" - using the user's own themeable `badge_color` rather than
    // the accent, since the accent is used for so many other things already
    // that a badge sharing it can blend in instead of standing out; the text
    // color is then picked for contrast against whatever that badge color
    // turns out to be (same luminance check used for selection text in
    // `itemviewer_preview.rs`).
    let badge_count = state.operations.len();
    if badge_count > 0 {
        let badge_center = resp.rect.right_top() + egui::vec2(-2.0, 2.0);
        let fill_luminance = 0.299 * palette.badge_color.r() as f32
            + 0.587 * palette.badge_color.g() as f32
            + 0.114 * palette.badge_color.b() as f32;
        let badge_text_color = if fill_luminance > 140.0 {
            Color32::BLACK
        } else {
            Color32::WHITE
        };
        ui.painter()
            .circle_filled(badge_center, 8.0, palette.badge_color);
        ui.painter().text(
            badge_center,
            Align2::CENTER_CENTER,
            badge_count.to_string(),
            FontId::proportional(9.0),
            badge_text_color,
        );
    }

    if resp.clicked() {
        state.panel_open = !state.panel_open;
    }

    if state.panel_open {
        let popup_pos = egui::pos2(resp.rect.right() - PANEL_WIDTH, resp.rect.bottom() + 6.0);

        let area_resp = egui::Area::new(egui::Id::new("notifications_panel_area"))
            .fixed_pos(popup_pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .fill(palette.notification_bg_color)
                    .stroke(Stroke::new(1.0, palette.notification_border_color))
                    .show(ui, |ui| {
                    ui.set_width(PANEL_WIDTH);

                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(i18n.tr("notifications_title"))
                                .size(palette.text_size)
                                .strong()
                                .color(palette.notification_header_text_color),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if !state.operations.is_empty()
                                && eden_button(ui, palette, &i18n.tr("notifications_clear_all"))
                                    .clicked()
                            {
                                state.clear_all();
                            }
                        });
                    });

                    ui.add_space(4.0);
                    ui.separator();

                    if state.operations.is_empty() {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(i18n.tr("notifications_empty"))
                                .size(palette.text_size)
                                .color(palette.icon_color),
                        );
                        ui.add_space(8.0);
                    } else {
                        let mut actions = Vec::new();
                        // `ScrollArea`'s own `max_height` only ever *caps*
                        // whatever height it thinks is actually available
                        // (`available_outer.size().at_most(max_size)`) - it
                        // doesn't force a minimum. Inside this `Area` (as
                        // opposed to a `Window`/`CentralPanel`), that
                        // available-height figure can come back far smaller
                        // than the real room below the panel, which silently
                        // clipped the row list after only ~1 row regardless
                        // of how many operations there actually were.
                        // Reserving the room a row count of this size
                        // actually needs (capped at `PANEL_MAX_HEIGHT`) before
                        // the `ScrollArea` runs guarantees it never shrinks
                        // below that, while still capping/scrolling once the
                        // list genuinely exceeds the panel's max height.
                        let wanted_height = (state.operations.len() as f32 * ROW_HEIGHT_ESTIMATE)
                            .min(PANEL_MAX_HEIGHT);
                        ui.set_min_height(wanted_height);
                        egui::ScrollArea::vertical()
                            .max_height(PANEL_MAX_HEIGHT)
                            .show(ui, |ui| {
                                for op in &state.operations {
                                    let has_job = state.has_robocopy_job(op.id);
                                    if let Some(action) =
                                        draw_operation_row(ui, i18n, palette, op, has_job)
                                    {
                                        actions.push(action);
                                    }
                                }
                            });
                        for action in actions {
                            match action {
                                RowAction::Dismiss(id) => {
                                    state.operations.retain(|o| o.id != id);
                                }
                                RowAction::Pause(id) => state.pause_job(id),
                                RowAction::Resume(id) => state.resume_job(id),
                                RowAction::Cancel(id) => state.cancel_job(id),
                            }
                        }
                    }
                });
            })
            .response;

        // `resp.clicked()` is excluded here because the Area is drawn (and
        // its `clicked_elsewhere` computed) in the very same frame the bell
        // is clicked to open it - without this guard, that opening click
        // would register as "elsewhere" (it landed on the bell icon, not
        // yet inside the Area) and close the panel on the same frame it
        // opened.
        if area_resp.clicked_elsewhere() && !resp.clicked() {
            state.panel_open = false;
        }
    }
}

enum RowAction {
    /// Remove this row from the panel only - doesn't touch any underlying
    /// operation (there's nothing to cancel for a plain `IFileOperation`
    /// call, and for a robocopy job this action is only ever offered once
    /// the job has already reached a terminal state).
    Dismiss(u64),
    Pause(u64),
    Resume(u64),
    Cancel(u64),
}

fn draw_operation_row(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    op: &FileOperation,
    has_robocopy_job: bool,
) -> Option<RowAction> {
    let mut action = None;

    ui.horizontal(|ui| {
        // A fixed slot for the kind icon, with the circle and glyph both
        // drawn directly via the painter at the SAME center point - drawing
        // the circle at a hand-picked offset and the glyph separately via
        // `ui.label` (as this used to) left them uncoordinated, since the
        // label's actual glyph position depends on font/line-height metrics
        // that don't match a guessed offset. The small upward nudge on the
        // glyph compensates for Phosphor icon ink sitting low in its own
        // line box (the same effect documented in CLAUDE.md for the status
        // bar's padding).
        let (icon_rect, _) =
            ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
        ui.painter().circle_filled(
            icon_rect.center(),
            10.0,
            status_color(op.status, palette).gamma_multiply(0.18),
        );
        ui.painter().text(
            icon_rect.center() - egui::vec2(0.0, 1.0),
            Align2::CENTER_CENTER,
            kind_icon(op.kind),
            FontId::default(),
            status_color(op.status, palette),
        );

        // Live-controllable rows (robocopy jobs still in progress or
        // paused) get two icon buttons instead of the plain dismiss X -
        // dismissing those would leave the underlying process running with
        // no way left to stop or resume it.
        let controllable = has_robocopy_job
            && matches!(op.status, FileOpStatus::InProgress | FileOpStatus::Paused);
        let controls_width = if controllable { 52.0 } else { 28.0 };

        let content_width = PANEL_WIDTH - 24.0 - controls_width - 24.0;
        ui.scope(|ui| {
            ui.set_width(content_width);
            ui.vertical(|ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.label(
                        RichText::new(operation_title(i18n, op))
                            .size(palette.text_size)
                            .color(ui.visuals().text_color()),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(status_label(i18n, op.status))
                                .size(palette.tooltip_text_size)
                                .color(palette.icon_color),
                        );
                        ui.label(
                            RichText::new(status_icon(op.status))
                                .font(FontId::proportional(11.0))
                                .color(status_color(op.status, palette)),
                        );
                    });
                });

                // Only robocopy jobs ever have a progress fraction -
                // `IFileOperation`-backed operations have no progress
                // callback to source one from (see `FileOperation::progress`'s
                // doc comment), so this bar simply doesn't appear for them.
                if let Some(frac) = op.progress {
                    if matches!(op.status, FileOpStatus::InProgress | FileOpStatus::Paused) {
                        ui.add_space(3.0);
                        let bar_height = 4.0;
                        let (bar_rect, _) = ui.allocate_exact_size(
                            egui::vec2(content_width, bar_height),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(
                            bar_rect,
                            CornerRadius::same(2),
                            palette.icon_color.gamma_multiply(0.15),
                        );
                        let fill_width = bar_rect.width() * frac.clamp(0.0, 1.0);
                        if fill_width > 0.5 {
                            let fill_rect = egui::Rect::from_min_size(
                                bar_rect.min,
                                egui::vec2(fill_width, bar_height),
                            );
                            ui.painter().rect_filled(
                                fill_rect,
                                CornerRadius::same(2),
                                palette.primary,
                            );
                        }
                    }
                }
            });
        });

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if controllable {
                if clickable_icon(ui, regular::PROHIBIT, palette)
                    .on_hover_text(
                        RichText::new(i18n.tr("tooltip_notifications_cancel"))
                            .size(palette.tooltip_text_size)
                            .color(palette.tooltip_text_color),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    action = Some(RowAction::Cancel(op.id));
                }
                let (icon, tooltip_key) = if op.status == FileOpStatus::Paused {
                    (regular::PLAY, "tooltip_notifications_resume")
                } else {
                    (regular::PAUSE_CIRCLE, "tooltip_notifications_pause")
                };
                if clickable_icon(ui, icon, palette)
                    .on_hover_text(
                        RichText::new(i18n.tr(tooltip_key))
                            .size(palette.tooltip_text_size)
                            .color(palette.tooltip_text_color),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    action = Some(if op.status == FileOpStatus::Paused {
                        RowAction::Resume(op.id)
                    } else {
                        RowAction::Pause(op.id)
                    });
                }
            } else if clickable_icon(ui, regular::X, palette)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                action = Some(RowAction::Dismiss(op.id));
            }
        });
    });
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(2.0);

    action
}

/// Draws the transient completion toast (if any operation finished
/// recently), anchored to the bottom-right of the window. Call once per
/// frame from the top-level update loop, after the rest of the UI. Keeps
/// requesting repaints while a toast is showing so its auto-hide timer
/// fires even if the app is otherwise idle.
pub fn draw_toast(
    ctx: &egui::Context,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut NotificationsState,
) {
    let Some(toast) = &state.active_toast else {
        return;
    };

    let elapsed = toast.shown_at.elapsed();
    if elapsed >= TOAST_DURATION {
        state.active_toast = None;
        return;
    }

    let Some(op) = state.operations.iter().find(|o| o.id == toast.op_id) else {
        state.active_toast = None;
        return;
    };

    let viewport = ctx.viewport_rect();
    let toast_width = 300.0;
    let margin = 16.0;
    let pos = egui::pos2(
        viewport.right() - toast_width - margin,
        viewport.bottom() - margin - 56.0,
    );

    // Fade out over the last half-second so it doesn't just pop out of
    // existence.
    let fade_start = TOAST_DURATION.as_secs_f32() - 0.5;
    let alpha = if elapsed.as_secs_f32() > fade_start {
        (1.0 - (elapsed.as_secs_f32() - fade_start) / 0.5).clamp(0.0, 1.0)
    } else {
        1.0
    };

    egui::Area::new(egui::Id::new("notifications_toast_area"))
        .fixed_pos(pos)
        .order(egui::Order::Tooltip)
        .interactable(false)
        .show(ctx, |ui| {
            ui.set_width(toast_width);
            let frame = egui::Frame::popup(ui.style())
                .fill(palette.navigation_toast_bg_color.gamma_multiply(alpha.max(0.05)))
                .stroke(Stroke::new(1.0, palette.navigation_toast_border_color.gamma_multiply(alpha)))
                .corner_radius(CornerRadius::same(palette.medium_radius));
            frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(status_icon(op.status))
                            .font(FontId::proportional(16.0))
                            .color(status_color(op.status, palette).gamma_multiply(alpha)),
                    );
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(operation_title(i18n, op))
                                .size(palette.text_size)
                                .color(palette.notification_header_text_color.gamma_multiply(alpha)),
                        );
                        ui.label(
                            RichText::new(status_label(i18n, op.status))
                                .size(palette.tooltip_text_size)
                                .color(palette.icon_color.gamma_multiply(alpha)),
                        );
                    });
                });
            });
        });

    ctx.request_repaint_after(Duration::from_millis(200));
}

#[cfg(test)]
mod status_color_tests {
    use super::*;
    use crate::gui::theme::get_default_palette;
    use crate::gui::theme::ThemeMode;

    #[test]
    fn completed_maps_to_success_in_progress_and_paused_to_warning_failed_and_cancelled_to_error() {
        let palette = get_default_palette(ThemeMode::Dark);

        assert_eq!(
            status_color(FileOpStatus::Completed, &palette),
            palette.notification_status_success
        );
        assert_eq!(
            status_color(FileOpStatus::InProgress, &palette),
            palette.notification_status_warning
        );
        assert_eq!(
            status_color(FileOpStatus::Paused, &palette),
            palette.notification_status_warning
        );
        assert_eq!(
            status_color(FileOpStatus::Failed, &palette),
            palette.notification_status_error
        );
        assert_eq!(
            status_color(FileOpStatus::Cancelled, &palette),
            palette.notification_status_error
        );
    }
}

#[cfg(test)]
mod history_label_tests {
    use super::*;

    #[test]
    fn undo_and_redo_entries_are_labelled_and_normal_entries_are_not() {
        let i18n = I18n::new("en-US");
        let mut state = NotificationsState::default();
        let plain = state.record_finished(FileOpKind::Rename, 1, String::new(), FileOpStatus::Completed, false);
        let undo = state.record_finished(FileOpKind::Rename, 1, String::new(), FileOpStatus::Completed, false);
        let redo = state.record_finished(FileOpKind::Move, 2, "Docs".into(), FileOpStatus::Failed, false);
        state.mark_history(undo, HistoryAction::Undo);
        state.mark_history(redo, HistoryAction::Redo);

        let title = |id: u64| {
            let op = state.operations.iter().find(|op| op.id == id).unwrap();
            operation_title(&i18n, op)
        };
        let undo_prefix = format!("{}: ", i18n.tr("notifications_history_undo"));
        let redo_prefix = format!("{}: ", i18n.tr("notifications_history_redo"));
        assert!(!title(plain).starts_with(&undo_prefix));
        assert!(title(undo).starts_with(&undo_prefix));
        assert!(title(redo).starts_with(&redo_prefix));
        assert!(title(redo).ends_with("Docs"));
    }
}
