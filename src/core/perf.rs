//! Measurement helpers behind the Performance panel (see
//! `gui::windows::performance_ui`): process memory, simple timing stats, and
//! the "Benchmark This Folder" runner that lists one folder several ways and
//! times each.

use crossbeam_channel::{Receiver, Sender, unbounded};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// One way of listing a folder, as compared by the benchmark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListMethod {
    /// The app's own listing: `NtQueryDirectoryFile` (names, sizes and
    /// timestamps in one call per 64 KB of entries).
    NtQuery,
    /// Rust's standard `std::fs::read_dir` (names only).
    StdReadDir,
    /// `std::fs::read_dir` plus a separate `std::fs::metadata` call per
    /// entry - how a naive listing gets sizes and dates.
    StdReadDirMetadata,
}

impl ListMethod {
    pub const ALL: [ListMethod; 3] = [
        ListMethod::NtQuery,
        ListMethod::StdReadDir,
        ListMethod::StdReadDirMetadata,
    ];

    pub fn i18n_key(self) -> &'static str {
        match self {
            ListMethod::NtQuery => "perf_method_nt",
            ListMethod::StdReadDir => "perf_method_read_dir",
            ListMethod::StdReadDirMetadata => "perf_method_read_dir_metadata",
        }
    }

    /// Plain-English name used in copied reports (which aren't localized,
    /// so they read the same when pasted into an issue).
    pub fn report_name(self) -> &'static str {
        match self {
            ListMethod::NtQuery => "NtQueryDirectoryFile (Eden)",
            ListMethod::StdReadDir => "std::fs::read_dir",
            ListMethod::StdReadDirMetadata => "std::fs::read_dir + metadata()",
        }
    }

    /// Lists `path` once, returning the number of entries found.
    pub fn list_once(self, path: &Path) -> Option<usize> {
        match self {
            ListMethod::NtQuery => crate::core::fs::list_dir_nt_stats(path).map(|(n, _)| n),
            ListMethod::StdReadDir => {
                let entries = std::fs::read_dir(path).ok()?;
                Some(entries.filter_map(Result::ok).count())
            }
            ListMethod::StdReadDirMetadata => {
                let entries = std::fs::read_dir(path).ok()?;
                let mut count = 0usize;
                let mut total = 0u64;
                for entry in entries.filter_map(Result::ok) {
                    count += 1;
                    if let Ok(meta) = std::fs::metadata(entry.path()) {
                        total = total.wrapping_add(meta.len());
                        std::hint::black_box(meta.modified().ok());
                    }
                }
                std::hint::black_box(total);
                Some(count)
            }
        }
    }
}

/// Min / average / max of a set of durations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimingStats {
    pub min: Duration,
    pub avg: Duration,
    pub max: Duration,
}

impl TimingStats {
    pub fn from_samples(samples: &[Duration]) -> Option<Self> {
        let min = *samples.iter().min()?;
        let max = *samples.iter().max()?;
        let total: Duration = samples.iter().sum();
        Some(Self {
            min,
            avg: total / samples.len() as u32,
            max,
        })
    }
}

/// Everything one method produced over all runs.
#[derive(Clone, Debug)]
pub struct MethodResult {
    pub method: ListMethod,
    pub runs: Vec<Duration>,
    pub entries: usize,
    /// Set if the method couldn't list the folder at all.
    pub failed: bool,
}

impl MethodResult {
    pub fn stats(&self) -> Option<TimingStats> {
        TimingStats::from_samples(&self.runs)
    }

    /// Entries per second at the average run time.
    pub fn items_per_sec(&self) -> Option<f64> {
        let avg = self.stats()?.avg.as_secs_f64();
        (avg > 0.0).then(|| self.entries as f64 / avg)
    }
}

#[derive(Clone, Debug)]
pub struct BenchmarkReport {
    pub path: PathBuf,
    pub runs_per_method: u32,
    pub results: Vec<MethodResult>,
    pub finished_at: chrono::DateTime<chrono::Local>,
}

impl BenchmarkReport {
    /// The fastest method by average time, if any succeeded.
    pub fn fastest(&self) -> Option<ListMethod> {
        self.results
            .iter()
            .filter(|r| !r.failed)
            .filter_map(|r| r.stats().map(|s| (r.method, s.avg)))
            .min_by_key(|(_, avg)| *avg)
            .map(|(m, _)| m)
    }

    /// Plain-text summary for "Copy Results".
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("EdenExplorer Folder Benchmark\n");
        out.push_str(&format!("Folder: {}\n", self.path.display()));
        out.push_str(&format!(
            "Date: {}\n",
            self.finished_at.format("%Y-%m-%d %H:%M:%S")
        ));
        out.push_str(&format!(
            "Runs per method: {} (the first run may read from disk; later runs are usually served from the OS cache)\n\n",
            self.runs_per_method
        ));
        out.push_str(&format!(
            "{:<34} {:>8} {:>10} {:>10} {:>10} {:>14}\n",
            "Method", "Items", "Min", "Avg", "Max", "Items/sec"
        ));
        for result in &self.results {
            match (result.failed, result.stats()) {
                (false, Some(stats)) => out.push_str(&format!(
                    "{:<34} {:>8} {:>10} {:>10} {:>10} {:>14}\n",
                    result.method.report_name(),
                    result.entries,
                    format_duration(stats.min),
                    format_duration(stats.avg),
                    format_duration(stats.max),
                    result
                        .items_per_sec()
                        .map(format_rate)
                        .unwrap_or_else(|| "-".into()),
                )),
                _ => out.push_str(&format!(
                    "{:<34} could not list this folder\n",
                    result.method.report_name()
                )),
            }
        }
        if let Some(fastest) = self.fastest() {
            out.push_str(&format!("\nFastest: {}\n", fastest.report_name()));
        }
        out
    }
}

/// Progress messages from a running benchmark.
pub enum BenchmarkEvent {
    /// `done` of `total` timed runs have finished.
    Progress { done: u32, total: u32 },
    Finished(BenchmarkReport),
}

/// Starts benchmarking `path` on a background thread. Each method gets one
/// untimed warm-up run, then `runs` timed runs; methods are interleaved per
/// round so no method always benefits from a cache the others warmed.
/// Dropping the returned receiver stops the run after its current listing.
pub fn start_benchmark(path: PathBuf, runs: u32) -> Receiver<BenchmarkEvent> {
    let (tx, rx) = unbounded();
    std::thread::spawn(move || run_benchmark(path, runs.max(1), tx));
    rx
}

fn run_benchmark(path: PathBuf, runs: u32, tx: Sender<BenchmarkEvent>) {
    let mut results: Vec<MethodResult> = ListMethod::ALL
        .iter()
        .map(|&method| MethodResult {
            method,
            runs: Vec::with_capacity(runs as usize),
            entries: 0,
            failed: false,
        })
        .collect();

    for result in &mut results {
        match result.method.list_once(&path) {
            Some(entries) => result.entries = entries,
            None => result.failed = true,
        }
    }

    let total = runs * results.iter().filter(|r| !r.failed).count() as u32;
    let mut done = 0u32;
    for _ in 0..runs {
        for result in results.iter_mut().filter(|r| !r.failed) {
            let started = Instant::now();
            let listed = result.method.list_once(&path);
            let elapsed = started.elapsed();
            match listed {
                Some(entries) => {
                    result.entries = entries;
                    result.runs.push(elapsed);
                }
                None => result.failed = true,
            }
            done += 1;
            if tx.send(BenchmarkEvent::Progress { done, total }).is_err() {
                return;
            }
        }
    }

    let _ = tx.send(BenchmarkEvent::Finished(BenchmarkReport {
        path,
        runs_per_method: runs,
        results,
        finished_at: chrono::Local::now(),
    }));
}

/// This process's memory use: (working set, private bytes).
pub fn process_memory() -> Option<(u64, u64)> {
    use windows::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows::Win32::System::Threading::GetCurrentProcess;

    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    let ok = unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS,
            counters.cb,
        )
    };
    ok.as_bool()
        .then_some((counters.WorkingSetSize as u64, counters.PrivateUsage as u64))
}

fn performance_panel_path() -> Option<PathBuf> {
    Some(
        crate::core::app_data::data_dir()?
            .join("performance_panel.bin"),
    )
}

/// Whether the Performance panel was left open. Kept in its own file (like
/// `sidebar_visibility.bin`) so older settings files keep decoding.
pub fn load_performance_panel_visible() -> bool {
    performance_panel_path()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|data| postcard::from_bytes::<bool>(&data).ok())
        .unwrap_or(false)
}

pub fn save_performance_panel_visible(visible: bool) {
    let Some(path) = performance_panel_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = postcard::to_allocvec(&visible) {
        let _ = std::fs::write(path, data);
    }
}

/// "0.42 ms", "12.3 ms", "1.25 s".
pub fn format_duration(d: Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    if ms < 10.0 {
        format!("{ms:.2} ms")
    } else if ms < 1000.0 {
        format!("{ms:.1} ms")
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

/// "850", "12.4K", "1.20M".
pub fn format_rate(per_sec: f64) -> String {
    if per_sec >= 1_000_000.0 {
        format!("{:.2}M", per_sec / 1_000_000.0)
    } else if per_sec >= 1_000.0 {
        format!("{:.1}K", per_sec / 1_000.0)
    } else {
        format!("{per_sec:.0}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir_with_files(n: usize) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "eden_perf_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        for i in 0..n {
            std::fs::write(dir.join(format!("f{i}.txt")), vec![b'x'; i]).unwrap();
        }
        dir
    }

    #[test]
    fn stats_are_min_avg_max() {
        let stats = TimingStats::from_samples(&[
            Duration::from_millis(4),
            Duration::from_millis(2),
            Duration::from_millis(6),
        ])
        .unwrap();
        assert_eq!(stats.min, Duration::from_millis(2));
        assert_eq!(stats.avg, Duration::from_millis(4));
        assert_eq!(stats.max, Duration::from_millis(6));
        assert!(TimingStats::from_samples(&[]).is_none());
    }

    #[test]
    fn every_method_counts_the_same_entries() {
        let dir = temp_dir_with_files(25);
        for method in ListMethod::ALL {
            // 25 files + 1 subfolder, never "." or "..".
            assert_eq!(method.list_once(&dir), Some(26), "{method:?}");
        }
        let (_, bytes) = crate::core::fs::list_dir_nt_stats(&dir).unwrap();
        assert_eq!(bytes, (0..25u64).sum::<u64>());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn benchmark_reports_every_method_and_copyable_text() {
        let dir = temp_dir_with_files(5);
        let rx = start_benchmark(dir.clone(), 2);
        let report = loop {
            match rx.recv_timeout(Duration::from_secs(30)).unwrap() {
                BenchmarkEvent::Progress { done, total } => assert!(done <= total),
                BenchmarkEvent::Finished(report) => break report,
            }
        };
        assert_eq!(report.results.len(), 3);
        for result in &report.results {
            assert!(!result.failed);
            assert_eq!(result.runs.len(), 2);
            assert_eq!(result.entries, 6);
        }
        assert!(report.fastest().is_some());
        let text = report.to_text();
        assert!(text.contains("NtQueryDirectoryFile"));
        assert!(text.contains("Fastest:"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_folder_is_reported_as_failed() {
        let rx = start_benchmark(PathBuf::from(r"C:\definitely\not\here\eden"), 2);
        let report = loop {
            if let BenchmarkEvent::Finished(report) = rx.recv_timeout(Duration::from_secs(30)).unwrap() {
                break report;
            }
        };
        assert!(report.results.iter().all(|r| r.failed));
        assert!(report.fastest().is_none());
        assert!(report.to_text().contains("could not list this folder"));
    }

    #[test]
    fn formats_are_readable() {
        assert_eq!(format_duration(Duration::from_micros(420)), "0.42 ms");
        assert_eq!(format_duration(Duration::from_micros(12_340)), "12.3 ms");
        assert_eq!(format_duration(Duration::from_millis(1250)), "1.25 s");
        assert_eq!(format_rate(850.0), "850");
        assert_eq!(format_rate(12_400.0), "12.4K");
        assert_eq!(format_rate(1_200_000.0), "1.20M");
    }

    #[test]
    fn process_memory_is_available() {
        let (working_set, _private) = process_memory().unwrap();
        assert!(working_set > 0);
    }
}
