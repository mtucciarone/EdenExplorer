//! Drive benchmark for the Performance panel: sequential and random
//! read/write speed of the drive holding the current folder, measured on a
//! temporary test file. Reads and writes bypass the Windows file cache
//! (`FILE_FLAG_NO_BUFFERING`, plus `FILE_FLAG_WRITE_THROUGH` for writes),
//! so the numbers are the drive's, not RAM's; that needs sector-aligned
//! buffers, sizes and offsets, hence `AlignedBuf` and the 4 KB / 1 MB
//! block sizes.

use chrono::{DateTime, Local};
use crossbeam_channel::{Receiver, Sender};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const FILE_FLAG_NO_BUFFERING: u32 = 0x2000_0000;
const FILE_FLAG_WRITE_THROUGH: u32 = 0x8000_0000;
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
const ALIGN: usize = 4096;
const SEQ_BLOCK: usize = 1024 * 1024;
const RANDOM_BLOCK: usize = 4096;
/// How long each random test runs.
#[cfg(not(test))]
const RANDOM_TIME: Duration = Duration::from_secs(3);
#[cfg(test)]
const RANDOM_TIME: Duration = Duration::from_millis(200);
const TEST_FILE_NAME: &str = ".eden-drive-benchmark.tmp";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    SeqWrite,
    SeqRead,
    RandomRead,
    RandomWrite,
}

impl Phase {
    pub const ALL: [Phase; 4] = [Phase::SeqRead, Phase::SeqWrite, Phase::RandomRead, Phase::RandomWrite];

    pub fn i18n_key(self) -> &'static str {
        match self {
            Phase::SeqRead => "perf_drive_seq_read",
            Phase::SeqWrite => "perf_drive_seq_write",
            Phase::RandomRead => "perf_drive_random_read",
            Phase::RandomWrite => "perf_drive_random_write",
        }
    }

    fn report_name(self) -> &'static str {
        match self {
            Phase::SeqRead => "Sequential read (1 MB)",
            Phase::SeqWrite => "Sequential write (1 MB)",
            Phase::RandomRead => "Random read (4 KB)",
            Phase::RandomWrite => "Random write (4 KB)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhaseResult {
    pub phase: Phase,
    pub bytes: u64,
    pub ops: u64,
    pub elapsed: Duration,
}

impl PhaseResult {
    pub fn mb_per_sec(&self) -> f64 {
        self.bytes as f64 / (1024.0 * 1024.0) / self.elapsed.as_secs_f64().max(1e-9)
    }

    pub fn iops(&self) -> f64 {
        self.ops as f64 / self.elapsed.as_secs_f64().max(1e-9)
    }
}

#[derive(Clone, Debug)]
pub struct DriveReport {
    /// Where the test file was.
    pub folder: PathBuf,
    pub file_size: u64,
    pub results: Vec<PhaseResult>,
    pub finished_at: DateTime<Local>,
}

impl DriveReport {
    pub fn result(&self, phase: Phase) -> Option<&PhaseResult> {
        self.results.iter().find(|r| r.phase == phase)
    }

    pub fn to_text(&self) -> String {
        let mut out = format!(
            "EdenExplorer drive benchmark\nFolder: {}\nTest file: {} MB, uncached\nFinished: {}\n\n",
            self.folder.display(),
            self.file_size / (1024 * 1024),
            self.finished_at.format("%Y-%m-%d %H:%M:%S")
        );
        for phase in Phase::ALL {
            if let Some(r) = self.result(phase) {
                out.push_str(&format!("{:<24} {:>9.1} MB/s {:>10.0} IOPS\n", phase.report_name(), r.mb_per_sec(), r.iops()));
            }
        }
        out
    }
}

pub enum DriveBenchEvent {
    /// The phase running and how far it is (0..1).
    Progress(Phase, f32),
    /// `Err` holds a message; cancelling finishes with `Err("")`.
    Finished(Result<DriveReport, String>),
}

pub struct DriveBenchHandle {
    pub rx: Receiver<DriveBenchEvent>,
    cancel: Arc<AtomicBool>,
}

impl DriveBenchHandle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for DriveBenchHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// A zeroed buffer whose start is `ALIGN`-aligned.
struct AlignedBuf {
    raw: Vec<u8>,
    offset: usize,
    len: usize,
}

impl AlignedBuf {
    fn new(len: usize) -> Self {
        let raw = vec![0u8; len + ALIGN];
        let offset = raw.as_ptr().align_offset(ALIGN);
        Self { raw, offset, len }
    }

    fn get(&self) -> &[u8] {
        &self.raw[self.offset..self.offset + self.len]
    }

    fn get_mut(&mut self) -> &mut [u8] {
        &mut self.raw[self.offset..self.offset + self.len]
    }
}

/// Deletes the test file however the run ends.
struct TestFile(PathBuf);

impl Drop for TestFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn open_uncached(path: &Path, write: bool) -> std::io::Result<File> {
    let mut flags = FILE_FLAG_NO_BUFFERING;
    if write {
        flags |= FILE_FLAG_WRITE_THROUGH;
    }
    OpenOptions::new().read(true).write(write).custom_flags(flags).open(path)
}

/// A cheap PRNG (xorshift64*) for offsets and incompressible data.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn fill(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            let v = self.next().to_le_bytes();
            chunk.copy_from_slice(&v[..chunk.len()]);
        }
    }
}

struct Runner<'a> {
    tx: &'a Sender<DriveBenchEvent>,
    cancel: &'a AtomicBool,
    last: Instant,
}

impl Runner<'_> {
    /// Reports progress now and then; false once cancelled.
    fn tick(&mut self, phase: Phase, fraction: f32) -> bool {
        if self.last.elapsed() >= Duration::from_millis(100) {
            self.last = Instant::now();
            let _ = self.tx.send(DriveBenchEvent::Progress(phase, fraction.clamp(0.0, 1.0)));
        }
        !self.cancel.load(Ordering::Relaxed)
    }
}

const CANCELLED: &str = "";

fn sequential(path: &Path, size: u64, write: bool, rng: &mut Rng, run: &mut Runner) -> Result<PhaseResult, String> {
    let phase = if write { Phase::SeqWrite } else { Phase::SeqRead };
    let mut file = open_uncached(path, write).map_err(|e| e.to_string())?;
    let mut buf = AlignedBuf::new(SEQ_BLOCK);
    let blocks = size / SEQ_BLOCK as u64;
    let start = Instant::now();
    for i in 0..blocks {
        if write {
            rng.fill(buf.get_mut());
            file.write_all(buf.get()).map_err(|e| e.to_string())?;
        } else {
            file.read_exact(buf.get_mut()).map_err(|e| e.to_string())?;
        }
        if !run.tick(phase, (i + 1) as f32 / blocks as f32) {
            return Err(CANCELLED.into());
        }
    }
    if write {
        file.sync_all().map_err(|e| e.to_string())?;
    }
    Ok(PhaseResult { phase, bytes: blocks * SEQ_BLOCK as u64, ops: blocks, elapsed: start.elapsed() })
}

fn random(path: &Path, size: u64, write: bool, rng: &mut Rng, run: &mut Runner) -> Result<PhaseResult, String> {
    let phase = if write { Phase::RandomWrite } else { Phase::RandomRead };
    let mut file = open_uncached(path, write).map_err(|e| e.to_string())?;
    let mut buf = AlignedBuf::new(RANDOM_BLOCK);
    rng.fill(buf.get_mut());
    let slots = size / RANDOM_BLOCK as u64;
    let start = Instant::now();
    let mut ops = 0u64;
    while start.elapsed() < RANDOM_TIME {
        let offset = (rng.next() % slots) * RANDOM_BLOCK as u64;
        file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
        if write {
            file.write_all(buf.get()).map_err(|e| e.to_string())?;
        } else {
            file.read_exact(buf.get_mut()).map_err(|e| e.to_string())?;
        }
        ops += 1;
        if ops % 16 == 0 && !run.tick(phase, start.elapsed().as_secs_f32() / RANDOM_TIME.as_secs_f32()) {
            return Err(CANCELLED.into());
        }
    }
    if write {
        file.sync_all().map_err(|e| e.to_string())?;
    }
    Ok(PhaseResult { phase, bytes: ops * RANDOM_BLOCK as u64, ops, elapsed: start.elapsed() })
}

/// Where the test file goes: `folder` if it can be written to, else the
/// temp folder when that's on the same drive.
fn test_file_path(folder: &Path) -> Result<PathBuf, String> {
    let candidate = folder.join(TEST_FILE_NAME);
    let created = OpenOptions::new()
        .write(true)
        .create_new(true)
        .attributes(FILE_ATTRIBUTE_HIDDEN)
        .open(&candidate);
    match created {
        Ok(_) => Ok(candidate),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(candidate),
        Err(e) => {
            let temp = std::env::temp_dir();
            let same_drive = drive_of(folder)
                .zip(drive_of(&temp))
                .is_some_and(|(a, b)| a.eq_ignore_ascii_case(&b));
            if same_drive {
                Ok(temp.join(TEST_FILE_NAME))
            } else {
                Err(e.to_string())
            }
        }
    }
}

/// The drive (`C:`, or `\\server\share`) a path is on.
pub fn drive_of(path: &Path) -> Option<String> {
    match path.components().next()? {
        std::path::Component::Prefix(prefix) => Some(prefix.as_os_str().to_string_lossy().into_owned()),
        _ => None,
    }
}

pub fn run_drive_benchmark(folder: &Path, size: u64, tx: &Sender<DriveBenchEvent>, cancel: &AtomicBool) -> Result<DriveReport, String> {
    let size = (size / SEQ_BLOCK as u64).max(1) * SEQ_BLOCK as u64;
    let path = test_file_path(folder)?;
    let guard = TestFile(path.clone());
    // Room for the test file, with a margin.
    if let Some((_, free)) = crate::core::fs::get_drive_space(&folder.to_path_buf())
        && free < size + 256 * 1024 * 1024
    {
        return Err("not enough free space".into());
    }
    let mut run = Runner { tx, cancel, last: Instant::now() };
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
    let mut rng = Rng(seed | 1);
    // Pre-size the file so the write test doesn't also time extending it.
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .and_then(|f| f.set_len(size))
        .map_err(|e| e.to_string())?;

    let results = vec![
        sequential(&path, size, true, &mut rng, &mut run)?,
        sequential(&path, size, false, &mut rng, &mut run)?,
        random(&path, size, false, &mut rng, &mut run)?,
        random(&path, size, true, &mut rng, &mut run)?,
    ];
    drop(guard);
    Ok(DriveReport {
        folder: path.parent().map(Path::to_path_buf).unwrap_or_default(),
        file_size: size,
        results,
        finished_at: Local::now(),
    })
}

/// Runs the benchmark on a background thread.
pub fn start_drive_benchmark(folder: PathBuf, size: u64) -> DriveBenchHandle {
    let (tx, rx) = crossbeam_channel::unbounded();
    let cancel = Arc::new(AtomicBool::new(false));
    let thread_cancel = cancel.clone();
    std::thread::spawn(move || {
        let result = run_drive_benchmark(&folder, size, &tx, &thread_cancel);
        let _ = tx.send(DriveBenchEvent::Finished(result));
    });
    DriveBenchHandle { rx, cancel }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_runs_every_phase_and_cleans_up() {
        let dir = std::env::temp_dir().join(format!("eden-drive-bench-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, _rx) = crossbeam_channel::unbounded();
        let cancel = AtomicBool::new(false);
        let report = run_drive_benchmark(&dir, 4 * 1024 * 1024, &tx, &cancel).unwrap();
        assert_eq!(report.results.len(), 4);
        let seq = report.result(Phase::SeqRead).unwrap();
        assert_eq!(seq.bytes, 4 * 1024 * 1024);
        assert!(seq.mb_per_sec() > 0.0);
        assert!(report.result(Phase::RandomWrite).unwrap().ops > 0);
        assert!(report.to_text().contains("Random read (4 KB)"));
        assert!(!dir.join(TEST_FILE_NAME).exists(), "test file removed");

        cancel.store(true, Ordering::Relaxed);
        assert_eq!(run_drive_benchmark(&dir, 4 * 1024 * 1024, &tx, &cancel).unwrap_err(), CANCELLED);
        assert!(!dir.join(TEST_FILE_NAME).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn drives_of_paths() {
        assert_eq!(drive_of(Path::new(r"C:\Users\me")).as_deref(), Some("C:"));
        assert_eq!(drive_of(Path::new(r"d:\")).as_deref(), Some("d:"));
        assert_eq!(drive_of(Path::new(r"\\srv\share\x")).as_deref(), Some(r"\\srv\share"));
        assert_eq!(drive_of(Path::new("relative")), None);
    }

    #[test]
    fn buffers_are_aligned() {
        for len in [4096, SEQ_BLOCK] {
            let buf = AlignedBuf::new(len);
            assert_eq!(buf.get().as_ptr() as usize % ALIGN, 0);
            assert_eq!(buf.get().len(), len);
        }
    }
}
