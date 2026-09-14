//! Multi-threaded copy/move engine backed by Windows' own `robocopy.exe`,
//! used for the clipboard paste path (Ctrl+C/Ctrl+X + Ctrl+V) - see
//! `mainwindow_imp.rs::paste_clipboard_native`. Delete stays on
//! `IFileOperation`/Recycle Bin; this module only ever copies or moves.
//!
//! Robocopy is fundamentally a directory-mirroring tool, not a "copy this
//! arbitrary list of files/folders" tool - `build_jobs` bridges that gap by
//! splitting a multi-select paste into one job per selected *folder* (a
//! genuine `robocopy <folder> <dest>/<folder> /E`, i.e. mirroring that one
//! subtree) plus a single job covering every selected *file* that shares a
//! common parent directory (robocopy's own multi-file-list syntax:
//! `robocopy <parent> <dest> file1 file2 ...`), since that's the form
//! robocopy actually supports for copying named files without mirroring
//! their whole parent directory.
//!
//! Robocopy has no real "pause" - `request_pause` kills whichever job's
//! child process is currently running and leaves the remaining job queue
//! (including the interrupted job, which hasn't been removed from the
//! queue yet) untouched; `resume` just spawns a fresh worker thread against
//! that same queue. Because every job is always run with `/Z` (restartable
//! mode), re-running an interrupted job skips whatever it already fully
//! wrote and resumes the in-flight file from its last checkpoint rather
//! than starting over.

use crate::core::fs::calculate_folder_size_fast;
use crossbeam_channel::{Receiver, Sender, unbounded};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const SIGNAL_RUN: u8 = 0;
const SIGNAL_PAUSE: u8 = 1;
const SIGNAL_CANCEL: u8 = 2;

/// How many worker threads robocopy itself is told to use per job
/// (`/MT:N`) - not related to how many jobs this module runs concurrently
/// (jobs always run one at a time, see the module doc comment). Bumped
/// from the previously-safe default of 8 to 16 - still within the range
/// generally considered safe for spinning HDDs, while giving a real
/// improvement for SSD/NVMe and network-share destinations. Revisit if
/// this turns out to hurt throughput on mechanical drives in practice.
const ROBOCOPY_MT_THREADS: u32 = 16;

#[derive(Clone)]
pub struct RobocopyJobSpec {
    source_dir: PathBuf,
    dest_dir: PathBuf,
    file_names: Vec<String>,
    is_folder_job: bool,
    is_move: bool,
    bytes: u64,
    /// Set only for a single-file job whose item needs to end up under a
    /// different name than it has at the source (see `next_available_name`)
    /// - robocopy always copies a file under its source name, so this is
    /// applied as a plain `fs::rename` on `dest_dir.join(file_names[0])`
    /// right after that job's robocopy process exits successfully. Folder
    /// renames don't need this: a folder's dest name is just
    /// `dest_dir`'s own last component, so `build_jobs` bakes the new name
    /// into `dest_dir` directly instead.
    rename_to: Option<String>,
}

/// Splits a paste (or cut-move) into robocopy jobs - see the module doc
/// comment for why folders and files need different job shapes. `renames`
/// maps a source path to the name it should end up with in `target_dir`
/// instead of its own name (see `next_available_name`) - entries not in
/// `renames` keep their original name and, if they're plain files, still
/// get grouped into one shared multi-file job with every other
/// non-renamed file from the same parent directory; a renamed file always
/// gets its own job, since robocopy's multi-file-list syntax has no way to
/// give one of the listed files a different destination name than the
/// others. Returns the jobs plus the combined byte total (used to turn
/// per-job progress into one overall fraction).
pub fn build_jobs(
    paths: &[PathBuf],
    target_dir: &Path,
    is_cut: bool,
    renames: &HashMap<PathBuf, String>,
) -> (Vec<RobocopyJobSpec>, u64) {
    let mut jobs = Vec::new();
    let mut total_bytes = 0u64;
    let mut files_by_parent: HashMap<PathBuf, Vec<String>> = HashMap::new();

    for path in paths {
        let rename_to = renames.get(path).cloned();

        if path.is_dir() {
            let name = rename_to.unwrap_or_else(|| {
                path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            });
            let bytes = calculate_folder_size_fast(path.clone());
            total_bytes += bytes;
            jobs.push(RobocopyJobSpec {
                source_dir: path.clone(),
                dest_dir: target_dir.join(&name),
                file_names: Vec::new(),
                is_folder_job: true,
                is_move: is_cut,
                bytes,
                rename_to: None,
            });
        } else {
            let parent = path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| target_dir.to_path_buf());
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            total_bytes += bytes;

            if let Some(new_name) = rename_to {
                // Robocopy always writes a file under its SOURCE name - it
                // has no way to write directly under `new_name`. Naively
                // pointing `dest_dir` straight at `target_dir` would have
                // robocopy write the copy under `name` first, which is
                // exactly the path the existing, colliding file already
                // occupies - silently overwriting the very file "Rename"
                // was supposed to keep, before the rename-in-place below
                // ever runs. Instead, stage the copy in a fresh (and
                // therefore guaranteed collision-free) subdirectory of
                // `target_dir`, and only move it to its final `new_name`
                // - and only then - after the copy has fully and safely
                // landed there; see the completion handling in `run_jobs`.
                let stage_name = format!(
                    ".eden-paste-stage-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos())
                        .unwrap_or_default()
                );
                jobs.push(RobocopyJobSpec {
                    source_dir: parent,
                    dest_dir: target_dir.join(stage_name),
                    file_names: vec![name],
                    is_folder_job: false,
                    is_move: is_cut,
                    bytes,
                    rename_to: Some(new_name),
                });
            } else {
                files_by_parent.entry(parent).or_default().push(name);
            }
        }
    }

    for (parent, file_names) in files_by_parent {
        let bytes = file_names
            .iter()
            .map(|n| std::fs::metadata(parent.join(n)).map(|m| m.len()).unwrap_or(0))
            .sum();
        jobs.push(RobocopyJobSpec {
            source_dir: parent,
            dest_dir: target_dir.to_path_buf(),
            file_names,
            is_folder_job: false,
            is_move: is_cut,
            bytes,
            rename_to: None,
        });
    }

    (jobs, total_bytes)
}

/// Finds the first `<stem>-NNN<ext>` (files) or `<name>-NNN` (folders) that
/// doesn't already exist in `target_dir`, starting at `-001` - the format
/// the user asked for. Falls back to a timestamp-based suffix past `-999`,
/// which should never actually happen in practice.
pub fn next_available_name(target_dir: &Path, name: &str) -> String {
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()));

    for n in 1..=999u32 {
        let candidate = match &ext {
            Some(ext) => format!("{stem}-{n:03}{ext}"),
            None => format!("{stem}-{n:03}"),
        };
        if !target_dir.join(&candidate).exists() {
            return candidate;
        }
    }

    let fallback_suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    match &ext {
        Some(ext) => format!("{stem}-{fallback_suffix}{ext}"),
        None => format!("{stem}-{fallback_suffix}"),
    }
}

fn spawn_job(job: &RobocopyJobSpec) -> std::io::Result<Child> {
    let mut cmd = Command::new("robocopy");
    cmd.arg(&job.source_dir).arg(&job.dest_dir);

    if job.is_folder_job {
        cmd.arg("/E");
    } else {
        for name in &job.file_names {
            cmd.arg(name);
        }
    }

    cmd.arg(format!("/MT:{ROBOCOPY_MT_THREADS}"))
        .arg("/COPY:DAT")
        .arg("/R:1")
        .arg("/W:1")
        // Restartable mode on every run (not just resumes) - a plain first
        // attempt costs a bit of extra checkpointing overhead, but it means
        // `resume` never needs to know whether this is a fresh job or a
        // continuation, and an interrupted file resumes instead of
        // restarting from zero either way.
        .arg("/Z")
        .arg("/NP")
        .arg("/NFL")
        .arg("/NDL")
        .arg("/NJH")
        .arg("/NJS");

    if job.is_folder_job {
        cmd.arg("/DCOPY:DAT");
    }

    if job.is_move {
        cmd.arg(if job.is_folder_job { "/MOVE" } else { "/MOV" });
    }

    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    cmd.spawn()
}

/// Bytes robocopy has written for one job so far, by measuring the
/// destination directly rather than parsing robocopy's own progress output
/// (which updates via `\r` mid-line and isn't reliably readable through a
/// piped, line-buffered `Stdio`). Works the same way whether the job just
/// started, is resuming, or is nearly done.
fn measure_job_progress_bytes(job: &RobocopyJobSpec) -> u64 {
    if job.is_folder_job {
        calculate_folder_size_fast(job.dest_dir.clone())
    } else {
        job.file_names
            .iter()
            .map(|n| std::fs::metadata(job.dest_dir.join(n)).map(|m| m.len()).unwrap_or(0))
            .sum()
    }
}

pub enum RobocopyUpdate {
    /// 0.0..=1.0, estimated from destination byte counts.
    Progress(f32),
    Finished(RoboFinish),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoboFinish {
    Completed,
    Failed,
    /// Stopped by `request_pause` - the job queue is intact and `resume`
    /// picks up where this left off.
    Paused,
    /// Stopped by `request_cancel` - terminal, no resume.
    Cancelled,
}

fn run_jobs(
    remaining: Arc<Mutex<Vec<RobocopyJobSpec>>>,
    signal: Arc<AtomicU8>,
    tx: Sender<RobocopyUpdate>,
    total_bytes: u64,
    completed_bytes_base: Arc<Mutex<u64>>,
) {
    let wake = || crate::gui::windows::windowsoverrides::request_repaint();

    loop {
        match signal.load(Ordering::SeqCst) {
            SIGNAL_CANCEL => {
                let _ = tx.send(RobocopyUpdate::Finished(RoboFinish::Cancelled));
                wake();
                return;
            }
            SIGNAL_PAUSE => {
                let _ = tx.send(RobocopyUpdate::Finished(RoboFinish::Paused));
                wake();
                return;
            }
            _ => {}
        }

        let job = { remaining.lock().unwrap().first().cloned() };
        let Some(job) = job else {
            let _ = tx.send(RobocopyUpdate::Finished(RoboFinish::Completed));
            wake();
            return;
        };

        let mut child = match spawn_job(&job) {
            Ok(c) => c,
            Err(_) => {
                let _ = tx.send(RobocopyUpdate::Finished(RoboFinish::Failed));
                wake();
                return;
            }
        };

        let mut last_poll = Instant::now() - Duration::from_secs(1);
        loop {
            match signal.load(Ordering::SeqCst) {
                SIGNAL_CANCEL => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = tx.send(RobocopyUpdate::Finished(RoboFinish::Cancelled));
                    wake();
                    return;
                }
                SIGNAL_PAUSE => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = tx.send(RobocopyUpdate::Finished(RoboFinish::Paused));
                    wake();
                    return;
                }
                _ => {}
            }

            match child.try_wait() {
                Ok(Some(status)) => {
                    // Robocopy's own success/failure convention: exit codes
                    // 0-7 are informational (files copied, extra files,
                    // mismatches) and still a success; 8+ means at least
                    // one file/dir failed, or a fatal error.
                    let ok = status.code().map(|c| c < 8).unwrap_or(false);
                    if !ok {
                        let _ = tx.send(RobocopyUpdate::Finished(RoboFinish::Failed));
                        wake();
                        return;
                    }
                    // A rename job copied into an isolated staging
                    // subdirectory (see `build_jobs`) precisely so this
                    // step never has to touch - let alone overwrite -
                    // whatever pre-existing file the user chose "Rename"
                    // specifically to keep. Move the finished copy out to
                    // its real name in the actual target directory, then
                    // remove the now-empty staging folder.
                    if let Some(new_name) = &job.rename_to {
                        if let Some(original_name) = job.file_names.first() {
                            let final_dir = job
                                .dest_dir
                                .parent()
                                .map(Path::to_path_buf)
                                .unwrap_or_else(|| job.dest_dir.clone());
                            if std::fs::rename(
                                job.dest_dir.join(original_name),
                                final_dir.join(new_name),
                            )
                            .is_err()
                            {
                                let _ = tx.send(RobocopyUpdate::Finished(RoboFinish::Failed));
                                wake();
                                return;
                            }
                            let _ = std::fs::remove_dir(&job.dest_dir);
                        }
                    }
                    *completed_bytes_base.lock().unwrap() += job.bytes;
                    {
                        let mut g = remaining.lock().unwrap();
                        if !g.is_empty() {
                            g.remove(0);
                        }
                    }
                    if total_bytes > 0 {
                        let done = *completed_bytes_base.lock().unwrap();
                        let _ = tx.send(RobocopyUpdate::Progress(
                            (done as f32 / total_bytes as f32).min(1.0),
                        ));
                        wake();
                    }
                    break;
                }
                Ok(None) => {
                    if last_poll.elapsed() >= Duration::from_millis(250) {
                        if total_bytes > 0 {
                            let base = *completed_bytes_base.lock().unwrap();
                            let done = base + measure_job_progress_bytes(&job);
                            let _ = tx.send(RobocopyUpdate::Progress(
                                (done as f32 / total_bytes as f32).min(0.99),
                            ));
                            wake();
                        }
                        last_poll = Instant::now();
                    }
                    thread::sleep(Duration::from_millis(60));
                }
                Err(_) => {
                    let _ = tx.send(RobocopyUpdate::Finished(RoboFinish::Failed));
                    wake();
                    return;
                }
            }
        }
    }
}

/// Handle to a running (or paused) robocopy-backed operation - owns enough
/// state to pause, cancel, or resume it, and to drain its progress/result
/// updates once per frame. Lives on `NotificationsState` for as long as the
/// operation is active; dropped once it reaches a terminal state
/// (Completed/Failed/Cancelled).
pub struct RobocopyHandle {
    signal: Arc<AtomicU8>,
    remaining: Arc<Mutex<Vec<RobocopyJobSpec>>>,
    completed_bytes_base: Arc<Mutex<u64>>,
    total_bytes: u64,
    pub rx: Receiver<RobocopyUpdate>,
}

impl RobocopyHandle {
    pub fn start(jobs: Vec<RobocopyJobSpec>, total_bytes: u64) -> Self {
        let signal = Arc::new(AtomicU8::new(SIGNAL_RUN));
        let remaining = Arc::new(Mutex::new(jobs));
        let completed_bytes_base = Arc::new(Mutex::new(0u64));
        let (tx, rx) = unbounded();

        let worker_signal = signal.clone();
        let worker_remaining = remaining.clone();
        let worker_base = completed_bytes_base.clone();
        thread::spawn(move || {
            run_jobs(worker_remaining, worker_signal, tx, total_bytes, worker_base);
        });

        Self {
            signal,
            remaining,
            completed_bytes_base,
            total_bytes,
            rx,
        }
    }

    /// Re-launches the worker thread against the same remaining job queue
    /// and byte counter, continuing from wherever `request_pause` stopped
    /// it. No-op-ish if called on an operation that isn't actually paused
    /// (the fresh worker will just find nothing left to do and report
    /// Completed immediately).
    pub fn resume(&mut self) {
        self.signal.store(SIGNAL_RUN, Ordering::SeqCst);
        let (tx, rx) = unbounded();
        self.rx = rx;

        let worker_signal = self.signal.clone();
        let worker_remaining = self.remaining.clone();
        let worker_base = self.completed_bytes_base.clone();
        let total_bytes = self.total_bytes;
        thread::spawn(move || {
            run_jobs(worker_remaining, worker_signal, tx, total_bytes, worker_base);
        });
    }

    pub fn request_pause(&self) {
        self.signal.store(SIGNAL_PAUSE, Ordering::SeqCst);
    }

    pub fn request_cancel(&self) {
        self.signal.store(SIGNAL_CANCEL, Ordering::SeqCst);
    }
}
