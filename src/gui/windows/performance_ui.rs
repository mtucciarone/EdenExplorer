//! The Performance panel: a small floating window with live metrics (how
//! long the current folder took to list and size, frame time/FPS, memory)
//! and a "Benchmark This Folder" tool comparing the app's own
//! `NtQueryDirectoryFile` listing with Rust's standard `read_dir`. Toggled
//! from Settings > Advanced or with Ctrl+K. The measuring itself lives
//! in `core::perf`; the listing/size timings are recorded by
//! `MainWindow::handle_directory_batch_recieve_for` and
//! `finish_size_scan_metric_if_done`.

use crate::core::perf::{
    BenchmarkEvent, BenchmarkReport, format_duration, format_rate, process_memory,
    start_benchmark,
};
use crate::core::utils::files::format_size;
use crate::core::utils::widgets::{eden_button, modal_frame};
use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use crate::core::drive_bench::{DriveBenchEvent, DriveBenchHandle, DriveReport, Phase, start_drive_benchmark};
use crossbeam_channel::Receiver;
use eframe::egui;
use egui_phosphor::regular;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How many recent frames the frame-time average/max covers.
const FRAME_WINDOW: usize = 120;
const MEMORY_POLL_INTERVAL: Duration = Duration::from_secs(1);
const RUN_CHOICES: [u32; 3] = [3, 5, 10];
/// Drive benchmark test file sizes, in MB.
const DRIVE_SIZES: [u64; 3] = [64, 256, 1024];

/// A finished timing: `count` items (entries listed, or folders sized) in
/// `elapsed`, for the folder at `path`.
#[derive(Clone, Debug)]
pub struct TimedSample {
    pub path: PathBuf,
    pub count: usize,
    pub elapsed: Duration,
}

impl TimedSample {
    fn per_sec(&self) -> Option<f64> {
        let secs = self.elapsed.as_secs_f64();
        (secs > 0.0).then(|| self.count as f64 / secs)
    }
}

pub struct PerformanceState {
    pub last_listing: Option<TimedSample>,
    pub last_size_scan: Option<TimedSample>,
    /// CPU seconds eframe reports for each recent frame.
    frame_cpu: VecDeque<f32>,
    /// When each recent frame ran, for the actual repaint rate.
    frame_times: VecDeque<Instant>,
    memory: Option<(u64, u64)>,
    peak_working_set: u64,
    memory_polled_at: Option<Instant>,
    benchmark_runs: u32,
    benchmark_rx: Option<Receiver<BenchmarkEvent>>,
    benchmark_progress: (u32, u32),
    benchmark_report: Option<BenchmarkReport>,
    copied_at: Option<Instant>,
    /// Showing the drive benchmark instead of the folder one.
    drive_tab: bool,
    drive_size: usize,
    drive_handle: Option<DriveBenchHandle>,
    drive_progress: (Phase, f32),
    drive_result: Option<Result<DriveReport, String>>,
    drive_copied_at: Option<Instant>,
}

impl Default for PerformanceState {
    fn default() -> Self {
        Self {
            last_listing: None,
            last_size_scan: None,
            frame_cpu: VecDeque::with_capacity(FRAME_WINDOW),
            frame_times: VecDeque::with_capacity(FRAME_WINDOW),
            memory: None,
            peak_working_set: 0,
            memory_polled_at: None,
            benchmark_runs: 5,
            benchmark_rx: None,
            benchmark_progress: (0, 0),
            benchmark_report: None,
            copied_at: None,
            drive_tab: false,
            drive_size: 1,
            drive_handle: None,
            drive_progress: (Phase::SeqWrite, 0.0),
            drive_result: None,
            drive_copied_at: None,
        }
    }
}

impl PerformanceState {
    /// Called once per frame while the panel is open.
    pub fn record_frame(&mut self, cpu_usage: Option<f32>) {
        let now = Instant::now();
        if let Some(cpu) = cpu_usage {
            if self.frame_cpu.len() == FRAME_WINDOW {
                self.frame_cpu.pop_front();
            }
            self.frame_cpu.push_back(cpu);
        }
        self.frame_times.push_back(now);
        while self
            .frame_times
            .front()
            .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(1))
        {
            self.frame_times.pop_front();
        }

        if self
            .memory_polled_at
            .is_none_or(|t| now.duration_since(t) >= MEMORY_POLL_INTERVAL)
        {
            self.memory_polled_at = Some(now);
            self.memory = process_memory();
            if let Some((working_set, _)) = self.memory {
                self.peak_working_set = self.peak_working_set.max(working_set);
            }
        }
    }

    /// (average, max) CPU time per frame over the recent window.
    fn frame_cpu_stats(&self) -> Option<(f32, f32)> {
        if self.frame_cpu.is_empty() {
            return None;
        }
        let sum: f32 = self.frame_cpu.iter().sum();
        let max = self.frame_cpu.iter().cloned().fold(0.0, f32::max);
        Some((sum / self.frame_cpu.len() as f32, max))
    }

    fn poll_drive_benchmark(&mut self) {
        let Some(handle) = &self.drive_handle else {
            return;
        };
        while let Ok(event) = handle.rx.try_recv() {
            match event {
                DriveBenchEvent::Progress(phase, fraction) => self.drive_progress = (phase, fraction),
                DriveBenchEvent::Finished(result) => {
                    // An empty error is a cancel: keep the previous result.
                    if !matches!(&result, Err(e) if e.is_empty()) {
                        self.drive_result = Some(result);
                    }
                    self.drive_handle = None;
                    return;
                }
            }
        }
    }

    fn poll_benchmark(&mut self) {
        let Some(rx) = &self.benchmark_rx else {
            return;
        };
        while let Ok(event) = rx.try_recv() {
            match event {
                BenchmarkEvent::Progress { done, total } => self.benchmark_progress = (done, total),
                BenchmarkEvent::Finished(report) => {
                    self.benchmark_report = Some(report);
                    self.benchmark_rx = None;
                    self.copied_at = None;
                    return;
                }
            }
        }
    }
}

/// What the panel needs to know about the app this frame.
pub struct PanelContext {
    /// The focused view's folder, if it's a real folder that can be
    /// benchmarked (not This PC, a search, a tag view, ...).
    pub current_folder: Option<PathBuf>,
    /// (folders queued so far, time since the scan started) while the
    /// focused view's folder sizes are still being calculated.
    pub size_scan_in_progress: Option<(usize, Duration)>,
    pub folder_scanning_enabled: bool,
}

/// Draws the panel. Returns `true` when its close button was clicked.
pub fn draw_performance_panel(
    ctx: &egui::Context,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut PerformanceState,
    panel: PanelContext,
) -> bool {
    state.poll_benchmark();
    state.poll_drive_benchmark();
    let mut close = false;

    egui::Window::new(i18n.tr("perf_title"))
        .id(egui::Id::new("performance_panel"))
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .frame(modal_frame(&ctx.style_of(ctx.theme()), palette).inner_margin(egui::Margin::same(14)))
        .default_pos(ctx.content_rect().right_top() + egui::vec2(-470.0, 90.0))
        .show(ctx, |ui| {
            ui.set_width(440.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{}  {}", regular::GAUGE, i18n.tr("perf_title")))
                        .strong()
                        .size(palette.text_size + 2.0)
                        .color(palette.text_header_section),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new(regular::X).frame(false))
                        .on_hover_text(i18n.tr("close"))
                        .clicked()
                    {
                        close = true;
                    }
                    ui.label(
                        egui::RichText::new("Ctrl+K")
                            .size(palette.text_size - 1.0)
                            .color(palette.text_normal.gamma_multiply(0.6)),
                    );
                });
            });
            ui.add_space(8.0);

            section_title(ui, palette, &i18n.tr("perf_live_metrics"));
            draw_live_metrics(ui, i18n, palette, state, &panel);

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                for (drive, icon, key) in [
                    (false, regular::FOLDER, "perf_benchmark_title"),
                    (true, regular::HARD_DRIVES, "perf_drive_title"),
                ] {
                    let on = state.drive_tab == drive;
                    let text = egui::RichText::new(format!("{icon} {}", i18n.tr(key)))
                        .strong()
                        .size(palette.text_size)
                        .color(if on { palette.item_viewer_row_text_selected } else { palette.text_normal });
                    if ui.add(egui::Button::selectable(on, text)).clicked() {
                        state.drive_tab = drive;
                    }
                }
            });
            ui.add_space(6.0);
            if state.drive_tab {
                draw_drive_benchmark(ui, i18n, palette, state, &panel);
            } else {
                draw_benchmark(ui, i18n, palette, state, &panel);
            }
        });

    // Keep the live numbers (memory, benchmark progress) moving even when
    // nothing else asks for a repaint.
    let refresh = if state.benchmark_rx.is_some() || state.drive_handle.is_some() {
        Duration::from_millis(100)
    } else {
        Duration::from_millis(500)
    };
    ctx.request_repaint_after(refresh);
    close
}

fn section_title(ui: &mut egui::Ui, palette: &ThemePalette, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .strong()
            .size(palette.text_size)
            .color(palette.text_normal),
    );
    ui.add_space(4.0);
}

fn metric_row(ui: &mut egui::Ui, palette: &ThemePalette, label: &str, value: &str, hint: Option<&str>) {
    let response = ui.label(
        egui::RichText::new(label)
            .size(palette.text_size)
            .color(palette.text_normal.gamma_multiply(0.75)),
    );
    if let Some(hint) = hint {
        response.on_hover_text(hint);
    }
    ui.label(
        egui::RichText::new(value)
            .size(palette.text_size)
            .color(palette.text_normal),
    );
    ui.end_row();
}

fn folder_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn draw_live_metrics(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &PerformanceState,
    panel: &PanelContext,
) {
    egui::Grid::new("perf_live_grid")
        .num_columns(2)
        .spacing([14.0, 6.0])
        .show(ui, |ui| {
            let listing = match &state.last_listing {
                Some(sample) => format!(
                    "{} {} · {} · {} {} · {}",
                    sample.count,
                    i18n.tr("perf_items"),
                    format_duration(sample.elapsed),
                    sample.per_sec().map(format_rate).unwrap_or_else(|| "-".into()),
                    i18n.tr("perf_per_sec_inline"),
                    folder_name(&sample.path),
                ),
                None => i18n.tr("perf_no_data"),
            };
            metric_row(
                ui,
                palette,
                &i18n.tr("perf_folder_load"),
                &listing,
                Some(&i18n.tr("tooltip_perf_folder_load")),
            );

            let size_scan = if !panel.folder_scanning_enabled {
                i18n.tr("perf_size_scan_off")
            } else if let Some((folders, elapsed)) = panel.size_scan_in_progress {
                format!(
                    "{} · {} {} · {}",
                    i18n.tr("perf_scanning"),
                    folders,
                    i18n.tr("perf_folders"),
                    format_duration(elapsed)
                )
            } else if let Some(sample) = state.last_size_scan.as_ref().filter(|s| s.count == 0) {
                format!("{} · {}", i18n.tr("perf_no_subfolders"), folder_name(&sample.path))
            } else if let Some(sample) = &state.last_size_scan {
                format!(
                    "{} {} · {} · {}",
                    sample.count,
                    i18n.tr("perf_folders"),
                    format_duration(sample.elapsed),
                    folder_name(&sample.path),
                )
            } else {
                i18n.tr("perf_no_data")
            };
            metric_row(
                ui,
                palette,
                &i18n.tr("perf_folder_size_scan"),
                &size_scan,
                Some(&i18n.tr("tooltip_perf_folder_size_scan")),
            );

            let frame = match state.frame_cpu_stats() {
                Some((avg, max)) => format!(
                    "{} {} · {} {}",
                    format_duration(Duration::from_secs_f32(avg)),
                    i18n.tr("perf_avg_inline"),
                    format_duration(Duration::from_secs_f32(max)),
                    i18n.tr("perf_max_inline"),
                ),
                None => i18n.tr("perf_no_data"),
            };
            metric_row(
                ui,
                palette,
                &i18n.tr("perf_frame_time"),
                &frame,
                Some(&i18n.tr("tooltip_perf_frame_time")),
            );

            let fps_now = state.frame_times.len();
            let fps = match state.frame_cpu_stats() {
                Some((avg, _)) if avg > 0.0 => format!(
                    "{} {} · {:.0} {}",
                    fps_now,
                    i18n.tr("perf_fps_now"),
                    1.0 / avg,
                    i18n.tr("perf_fps_possible"),
                ),
                _ => format!("{} {}", fps_now, i18n.tr("perf_fps_now")),
            };
            metric_row(ui, palette, &i18n.tr("perf_fps"), &fps, Some(&i18n.tr("tooltip_perf_fps")));

            let memory = match state.memory {
                Some((working_set, private)) => {
                    let mut text = format_size(working_set);
                    // Some systems (e.g. Wine) don't report private bytes.
                    if private > 0 {
                        text.push_str(&format!(" · {} {}", format_size(private), i18n.tr("perf_private")));
                    }
                    text.push_str(&format!(
                        " · {} {}",
                        format_size(state.peak_working_set),
                        i18n.tr("perf_peak")
                    ));
                    text
                }
                None => i18n.tr("perf_no_data"),
            };
            metric_row(
                ui,
                palette,
                &i18n.tr("perf_memory"),
                &memory,
                Some(&i18n.tr("tooltip_perf_memory")),
            );
        });
}

fn draw_benchmark(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut PerformanceState,
    panel: &PanelContext,
) {
    let running = state.benchmark_rx.is_some();

    match &panel.current_folder {
        Some(folder) => {
            ui.label(
                egui::RichText::new(format!("{} {}", regular::FOLDER, folder.display()))
                    .size(palette.text_size - 1.0)
                    .color(palette.text_normal.gamma_multiply(0.75)),
            );
        }
        None => {
            ui.label(
                egui::RichText::new(i18n.tr("perf_benchmark_unavailable"))
                    .size(palette.text_size - 1.0)
                    .color(palette.text_normal.gamma_multiply(0.75)),
            );
        }
    }
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(i18n.tr("perf_runs"))
                .size(palette.text_size)
                .color(palette.text_normal),
        );
        for runs in RUN_CHOICES {
            ui.add_enabled_ui(!running, |ui| {
                if ui
                    .selectable_label(state.benchmark_runs == runs, runs.to_string())
                    .clicked()
                {
                    state.benchmark_runs = runs;
                }
            });
        }
        ui.add_space(8.0);

        if running {
            if eden_button(ui, palette, &format!("{} {}", regular::STOP, i18n.tr("cancel"))).clicked() {
                // Dropping the receiver stops the background run.
                state.benchmark_rx = None;
            }
        } else {
            let enabled = panel.current_folder.is_some();
            let clicked = ui
                .add_enabled_ui(enabled, |ui| {
                    eden_button(
                        ui,
                        palette,
                        &format!("{} {}", regular::PLAY, i18n.tr("perf_benchmark_button")),
                    )
                })
                .inner
                .clicked();
            if clicked && let Some(folder) = panel.current_folder.clone() {
                state.benchmark_progress = (0, 0);
                state.benchmark_rx = Some(start_benchmark(folder, state.benchmark_runs));
            }
        }
    });

    if running {
        let (done, total) = state.benchmark_progress;
        let fraction = if total == 0 { 0.0 } else { done as f32 / total as f32 };
        ui.add_space(6.0);
        ui.add(
            egui::ProgressBar::new(fraction)
                .desired_height(6.0)
                .fill(palette.primary),
        );
        return;
    }

    let Some(report) = &state.benchmark_report else {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(i18n.tr("perf_benchmark_hint"))
                .size(palette.text_size - 1.0)
                .color(palette.text_normal.gamma_multiply(0.6)),
        );
        return;
    };

    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(format!(
            "{} · {} {} · {}",
            folder_name(&report.path),
            report.runs_per_method,
            i18n.tr("perf_runs_each"),
            report.finished_at.format("%H:%M:%S"),
        ))
        .size(palette.text_size - 1.0)
        .color(palette.text_normal.gamma_multiply(0.75)),
    );
    ui.add_space(4.0);

    let fastest = report.fastest();
    egui::Frame::NONE
        .fill(palette.row_bg)
        .corner_radius(egui::CornerRadius::same(palette.medium_radius))
        .stroke(egui::Stroke::new(1.0, palette.borders_default))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Grid::new("perf_benchmark_grid")
                .num_columns(5)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    for header in ["perf_method", "perf_min", "perf_avg", "perf_max", "perf_per_sec"] {
                        ui.label(
                            egui::RichText::new(i18n.tr(header))
                                .strong()
                                .size(palette.text_size - 1.0)
                                .color(palette.text_normal.gamma_multiply(0.75)),
                        );
                    }
                    ui.end_row();

                    for result in &report.results {
                        let is_fastest = Some(result.method) == fastest;
                        let color = if is_fastest {
                            palette.notification_status_success
                        } else {
                            palette.text_normal
                        };
                        let name = if is_fastest {
                            format!("{} {}", regular::LIGHTNING, i18n.tr(result.method.i18n_key()))
                        } else {
                            i18n.tr(result.method.i18n_key())
                        };
                        ui.label(egui::RichText::new(name).size(palette.text_size).color(color))
                            .on_hover_text(format!(
                                "{} · {} {}",
                                result.method.report_name(),
                                result.entries,
                                i18n.tr("perf_items")
                            ));
                        match (result.failed, result.stats()) {
                            (false, Some(stats)) => {
                                for value in [
                                    format_duration(stats.min),
                                    format_duration(stats.avg),
                                    format_duration(stats.max),
                                    result
                                        .items_per_sec()
                                        .map(format_rate)
                                        .unwrap_or_else(|| "-".into()),
                                ] {
                                    ui.label(
                                        egui::RichText::new(value)
                                            .family(egui::FontFamily::Monospace)
                                            .size(palette.text_size - 1.0)
                                            .color(color),
                                    );
                                }
                            }
                            _ => {
                                ui.label(
                                    egui::RichText::new(i18n.tr("perf_failed"))
                                        .size(palette.text_size - 1.0)
                                        .color(palette.notification_status_error),
                                );
                            }
                        }
                        ui.end_row();
                    }
                });
        });

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if eden_button(ui, palette, &format!("{} {}", regular::COPY, i18n.tr("perf_copy_results")))
            .clicked()
        {
            crate::core::utils::clipboard::copy_text_to_clipboard(&report.to_text());
            state.copied_at = Some(Instant::now());
        }
        if state
            .copied_at
            .is_some_and(|t| t.elapsed() < Duration::from_secs(2))
        {
            ui.label(
                egui::RichText::new(format!("{} {}", regular::CHECK, i18n.tr("perf_copied")))
                    .size(palette.text_size - 1.0)
                    .color(palette.notification_status_success),
            );
        }
    });
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(i18n.tr("perf_cache_note"))
            .size(palette.text_size - 2.0)
            .color(palette.text_normal.gamma_multiply(0.55)),
    );
}

fn draw_drive_benchmark(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut PerformanceState,
    panel: &PanelContext,
) {
    let running = state.drive_handle.is_some();
    let muted = |text: String| {
        egui::RichText::new(text)
            .size(palette.text_size - 1.0)
            .color(palette.text_normal.gamma_multiply(0.75))
    };
    match &panel.current_folder {
        Some(folder) => {
            let drive = crate::core::drive_bench::drive_of(folder).unwrap_or_default();
            ui.label(muted(format!("{} {drive}  ·  {}", regular::HARD_DRIVES, folder.display())));
        }
        None => {
            ui.label(muted(i18n.tr("perf_drive_unavailable")));
        }
    }
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(i18n.tr("perf_drive_test_size")).size(palette.text_size).color(palette.text_normal));
        for (i, mb) in DRIVE_SIZES.iter().enumerate() {
            ui.add_enabled_ui(!running, |ui| {
                let label = if *mb >= 1024 { format!("{} GB", mb / 1024) } else { format!("{mb} MB") };
                if ui.selectable_label(state.drive_size == i, label).clicked() {
                    state.drive_size = i;
                }
            });
        }
        ui.add_space(8.0);
        if running {
            if eden_button(ui, palette, &format!("{} {}", regular::STOP, i18n.tr("cancel"))).clicked()
                && let Some(handle) = &state.drive_handle
            {
                handle.cancel();
            }
        } else {
            let clicked = ui
                .add_enabled_ui(panel.current_folder.is_some(), |ui| {
                    eden_button(ui, palette, &format!("{} {}", regular::PLAY, i18n.tr("perf_drive_button")))
                })
                .inner
                .clicked();
            if clicked && let Some(folder) = panel.current_folder.clone() {
                state.drive_progress = (Phase::SeqWrite, 0.0);
                state.drive_handle = Some(start_drive_benchmark(folder, DRIVE_SIZES[state.drive_size] * 1024 * 1024));
            }
        }
    });

    if running {
        let (phase, fraction) = state.drive_progress;
        let index = [Phase::SeqWrite, Phase::SeqRead, Phase::RandomRead, Phase::RandomWrite]
            .iter()
            .position(|p| *p == phase)
            .unwrap_or(0);
        ui.add_space(6.0);
        ui.label(muted(format!("{} ({}/4)", i18n.tr(phase.i18n_key()), index + 1)));
        ui.add(
            egui::ProgressBar::new((index as f32 + fraction) / 4.0)
                .desired_height(6.0)
                .fill(palette.primary),
        );
        return;
    }

    let report = match &state.drive_result {
        None => {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(i18n.tr("perf_drive_hint"))
                    .size(palette.text_size - 1.0)
                    .color(palette.text_normal.gamma_multiply(0.6)),
            );
            return;
        }
        Some(Err(error)) => {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!("{}: {error}", i18n.tr("perf_drive_failed")))
                    .size(palette.text_size - 1.0)
                    .color(palette.notification_status_error),
            );
            return;
        }
        Some(Ok(report)) => report,
    };

    ui.add_space(8.0);
    ui.label(muted(format!(
        "{} · {} MB · {}",
        report.folder.display(),
        report.file_size / (1024 * 1024),
        report.finished_at.format("%H:%M:%S"),
    )));
    ui.add_space(4.0);
    let best = report.results.iter().map(|r| r.mb_per_sec()).fold(0.0f64, f64::max);
    egui::Frame::NONE
        .fill(palette.row_bg)
        .corner_radius(egui::CornerRadius::same(palette.medium_radius))
        .stroke(egui::Stroke::new(1.0, palette.borders_default))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Grid::new("perf_drive_grid")
                .num_columns(4)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    for header in ["perf_drive_test", "perf_drive_speed", "", "perf_drive_iops"] {
                        ui.label(
                            egui::RichText::new(if header.is_empty() { String::new() } else { i18n.tr(header) })
                                .strong()
                                .size(palette.text_size - 1.0)
                                .color(palette.text_normal.gamma_multiply(0.75)),
                        );
                    }
                    ui.end_row();
                    for phase in Phase::ALL {
                        let Some(result) = report.result(phase) else { continue };
                        ui.label(egui::RichText::new(i18n.tr(phase.i18n_key())).size(palette.text_size).color(palette.text_normal));
                        ui.label(
                            egui::RichText::new(format!("{:.1} MB/s", result.mb_per_sec()))
                                .family(egui::FontFamily::Monospace)
                                .size(palette.text_size - 1.0)
                                .color(palette.text_normal),
                        );
                        // A bar relative to the fastest test.
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(80.0, 6.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 3.0, palette.drive_usage_background);
                        let fraction = if best > 0.0 { (result.mb_per_sec() / best) as f32 } else { 0.0 };
                        let mut filled = rect;
                        filled.set_width((rect.width() * fraction).max(2.0));
                        ui.painter().rect_filled(filled, 3.0, palette.primary);
                        ui.label(
                            egui::RichText::new(format!("{:.0}", result.iops()))
                                .family(egui::FontFamily::Monospace)
                                .size(palette.text_size - 1.0)
                                .color(palette.text_normal.gamma_multiply(0.85)),
                        );
                        ui.end_row();
                    }
                });
        });

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if eden_button(ui, palette, &format!("{} {}", regular::COPY, i18n.tr("perf_copy_results"))).clicked() {
            crate::core::utils::clipboard::copy_text_to_clipboard(&report.to_text());
            state.drive_copied_at = Some(Instant::now());
        }
        if state.drive_copied_at.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
            ui.label(
                egui::RichText::new(format!("{} {}", regular::CHECK, i18n.tr("perf_copied")))
                    .size(palette.text_size - 1.0)
                    .color(palette.notification_status_success),
            );
        }
    });
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(i18n.tr("perf_drive_note"))
            .size(palette.text_size - 2.0)
            .color(palette.text_normal.gamma_multiply(0.55)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_stats_track_a_bounded_window() {
        let mut state = PerformanceState::default();
        assert!(state.frame_cpu_stats().is_none());
        for _ in 0..FRAME_WINDOW + 50 {
            state.record_frame(Some(0.002));
        }
        state.record_frame(Some(0.010));
        assert_eq!(state.frame_cpu.len(), FRAME_WINDOW);
        let (avg, max) = state.frame_cpu_stats().unwrap();
        assert!((max - 0.010).abs() < 1e-6);
        assert!(avg > 0.002 && avg < 0.003);
        assert!(state.memory.is_some());
    }

    #[test]
    fn timed_sample_rate() {
        let sample = TimedSample {
            path: PathBuf::from(r"C:\x"),
            count: 500,
            elapsed: Duration::from_millis(250),
        };
        assert_eq!(sample.per_sec(), Some(2000.0));
    }
}
