//! The scanning side of "Analyze Disk Usage…" (the dashboard itself is
//! `gui::windows::disk_usage_ui`): builds a size tree for a folder or a
//! whole drive on a background thread, with live progress and Cancel.
//!
//! Two ways to scan:
//! - **Fast MFT scan** (`core::ntfs_mft`) for a whole NTFS drive: reads the
//!   drive's Master File Table in one pass, like WizTree. That needs
//!   administrator rights, so unless the app already has them it asks
//!   Windows once (UAC) and runs just the MFT read in an elevated helper
//!   (`core::mft_helper`).
//! - **Standard scan** everywhere else (a folder, a non-NTFS drive, no
//!   admin rights, or the fast scan failed): lists every folder with the
//!   same `NtQueryDirectoryFile` call the file list uses, many folders at a
//!   time in parallel. Junctions and symbolic links are listed but not
//!   followed, so nothing is counted twice and link loops can't hang it.
//!
//! "Rescan This Branch" re-runs a standard scan on one folder of an
//! existing result and swaps it in with `DirNode::replace_branch`.

use crossbeam_channel::{Receiver, Sender, unbounded};
use rayon::prelude::*;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often a running scan reports progress.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// A file inside a scanned folder.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileEntry {
    pub name: Box<str>,
    /// Logical size (what Explorer shows as "Size").
    pub size: u64,
    /// Space taken on disk ("Size on disk"): rounded up to whole clusters,
    /// smaller for compressed/sparse files, 0 for tiny files NTFS keeps
    /// inside the MFT.
    pub allocated: u64,
}

/// A scanned folder with its totals. `dirs` and `files` are kept sorted
/// largest first.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DirNode {
    /// The folder's name (the full path for the scan's root).
    pub name: Box<str>,
    pub size: u64,
    pub allocated: u64,
    /// Files anywhere below this folder.
    pub file_count: u64,
    /// Folders anywhere below this folder (not counting itself).
    pub dir_count: u64,
    pub dirs: Vec<DirNode>,
    pub files: Vec<FileEntry>,
    /// The folder couldn't be listed (access denied, or it vanished).
    pub unreadable: bool,
    /// A junction or symbolic link, listed but not followed.
    pub is_link: bool,
}

impl DirNode {
    /// Recomputes this folder's totals from its direct children (which must
    /// already have theirs).
    pub fn recompute_totals(&mut self) {
        let mut size = 0u64;
        let mut allocated = 0u64;
        let mut file_count = self.files.len() as u64;
        let mut dir_count = self.dirs.len() as u64;
        for f in &self.files {
            size = size.saturating_add(f.size);
            allocated = allocated.saturating_add(f.allocated);
        }
        for d in &self.dirs {
            size = size.saturating_add(d.size);
            allocated = allocated.saturating_add(d.allocated);
            file_count += d.file_count;
            dir_count += d.dir_count;
        }
        self.size = size;
        self.allocated = allocated;
        self.file_count = file_count;
        self.dir_count = dir_count;
    }

    pub fn sort_children(&mut self) {
        self.dirs
            .sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
        self.files
            .sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
    }

    fn child_index(&self, name: &str) -> Option<usize> {
        self.dirs
            .iter()
            .position(|d| &*d.name == name)
            .or_else(|| self.dirs.iter().position(|d| d.name.eq_ignore_ascii_case(name)))
    }

    /// The folder at `components` below this one (empty = this one).
    pub fn find(&self, components: &[String]) -> Option<&DirNode> {
        match components.split_first() {
            None => Some(self),
            Some((first, rest)) => self.dirs[self.child_index(first)?].find(rest),
        }
    }

    /// Replaces the folder at `components` below this one with `new_node`
    /// (keeping its name), then updates the totals and order of every
    /// folder on the way up. Returns `false` if there's no such folder.
    pub fn replace_branch(&mut self, components: &[String], mut new_node: DirNode) -> bool {
        match components.split_first() {
            None => {
                new_node.name = std::mem::take(&mut self.name);
                *self = new_node;
                true
            }
            Some((first, rest)) => {
                let Some(index) = self.child_index(first) else {
                    return false;
                };
                if !self.dirs[index].replace_branch(rest, new_node) {
                    return false;
                }
                self.recompute_totals();
                self.sort_children();
                true
            }
        }
    }
}

/// The folder names leading from `root` down to `branch` (empty when
/// they're the same folder), or `None` if `branch` isn't inside `root`.
pub fn branch_components(root: &Path, branch: &Path) -> Option<Vec<String>> {
    let root: Vec<Component> = root.components().collect();
    let branch: Vec<Component> = branch.components().collect();
    if branch.len() < root.len() {
        return None;
    }
    // Windows paths match regardless of letter case.
    let same = root.iter().zip(&branch).all(|(a, b)| {
        a.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
    });
    same.then(|| {
        branch[root.len()..]
            .iter()
            .filter_map(|c| match c {
                Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect()
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanMethod {
    /// Read the NTFS Master File Table directly.
    Mft,
    /// Listed folder by folder with `NtQueryDirectoryFile`.
    Standard,
}

/// Whether a whole-drive scan may use the fast MFT scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FastScan {
    /// Always list folder by folder (branch rescans).
    Off,
    /// Only when the app already runs as administrator.
    IfElevated,
    /// Also when it doesn't: ask Windows for permission and run the MFT
    /// read in an elevated helper.
    AskForAdmin,
}

/// Why a drive scan didn't use the fast MFT scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FastScanNote {
    /// The app isn't running as administrator and asking is turned off.
    NeedsAdmin,
    /// Windows asked for administrator permission and it was refused.
    Declined,
    /// The drive isn't NTFS (FAT32, exFAT, a network drive, ...).
    NotNtfs,
    /// It was tried and failed; the message says why.
    Failed(String),
}

#[derive(Clone, Debug, Default)]
pub struct ScanProgress {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    /// 0..1 when the total is known (an MFT scan, or a whole drive's used
    /// space for a standard scan).
    pub fraction: Option<f32>,
    /// A folder being listed right now (standard scan only).
    pub current: Option<PathBuf>,
    pub method: Option<ScanMethod>,
    /// Windows is asking for administrator permission for the fast scan.
    pub waiting_for_permission: bool,
}

#[derive(Debug)]
pub struct ScanOutcome {
    /// `None` if the scan was cancelled.
    pub tree: Option<DirNode>,
    pub method: ScanMethod,
    pub fast_scan_note: Option<FastScanNote>,
    pub elapsed: Duration,
}

#[derive(Debug)]
pub enum ScanEvent {
    Progress(ScanProgress),
    Finished(ScanOutcome),
}

/// A running scan. Dropping it (or `cancel`) stops the scan.
pub struct ScanHandle {
    pub rx: Receiver<ScanEvent>,
    cancel: Arc<AtomicBool>,
}

impl ScanHandle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for ScanHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Default)]
struct Counters {
    files: AtomicU64,
    dirs: AtomicU64,
    bytes: AtomicU64,
    allocated: AtomicU64,
    /// f32 bits of the MFT scan's fraction done.
    fraction_bits: AtomicU32,
    has_fraction: AtomicBool,
    /// Set once the standard scan starts (right away, or after the MFT
    /// scan fell back).
    standard: AtomicBool,
    waiting_for_permission: AtomicBool,
    current: Mutex<Option<PathBuf>>,
}

/// The drive letter if `path` is a drive's root folder (`C:\`).
pub fn drive_root_letter(path: &Path) -> Option<char> {
    let s = path.to_string_lossy();
    let s = s.trim_end_matches(['\\', '/']);
    let mut chars = s.chars();
    let letter = chars.next()?;
    (letter.is_ascii_alphabetic() && chars.next() == Some(':') && chars.next().is_none())
        .then(|| letter.to_ascii_uppercase())
}

/// Whether the app is running elevated (as administrator).
pub fn is_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut core::ffi::c_void),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

/// The file system name of the drive holding `root` ("NTFS", "FAT32", ...).
fn file_system_name(root: &Path) -> Option<String> {
    use windows::Win32::Storage::FileSystem::GetVolumeInformationW;
    let mut root_s = root.to_string_lossy().into_owned();
    if !root_s.ends_with('\\') {
        root_s.push('\\');
    }
    let wide = crate::core::fs::path_to_wide(Path::new(&root_s));
    let mut name = [0u16; 64];
    unsafe {
        GetVolumeInformationW(
            windows::core::PCWSTR(wide.as_ptr()),
            None,
            None,
            None,
            None,
            Some(&mut name),
        )
        .ok()?;
    }
    let len = name.iter().position(|c| *c == 0).unwrap_or(name.len());
    Some(String::from_utf16_lossy(&name[..len]))
}

/// The drive letter if `root` is a whole NTFS drive (the fast scan's
/// only requirement besides administrator rights).
pub fn fast_scan_availability(root: &Path) -> Result<char, FastScanNote> {
    let letter = drive_root_letter(root).ok_or(FastScanNote::NotNtfs)?;
    if !file_system_name(root).is_some_and(|fs| fs.eq_ignore_ascii_case("NTFS")) {
        return Err(FastScanNote::NotNtfs);
    }
    Ok(letter)
}

/// Starts scanning `root` in the background; `fast` says whether a whole
/// NTFS drive may use the MFT scan. Call it from the UI thread: the window
/// in front at that moment owns the permission prompt, if there is one.
pub fn start_scan(root: PathBuf, fast: FastScan) -> ScanHandle {
    let owner = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }.0 as isize;
    let (tx, rx) = unbounded();
    let cancel = Arc::new(AtomicBool::new(false));
    let handle = ScanHandle {
        rx,
        cancel: cancel.clone(),
    };
    let spawned = std::thread::Builder::new()
        .name("disk-usage-scan".into())
        .spawn(move || run_scan(root, fast, owner, tx, cancel));
    if spawned.is_err() {
        handle.cancel();
    }
    handle
}

fn run_scan(
    root: PathBuf,
    mode: FastScan,
    owner: isize,
    tx: Sender<ScanEvent>,
    cancel: Arc<AtomicBool>,
) {
    let started = Instant::now();
    let counters = Arc::new(Counters::default());

    let fast = (mode != FastScan::Off && drive_root_letter(&root).is_some())
        .then(|| fast_scan_availability(&root));

    // Used space is what a whole-drive standard scan works towards.
    let expected_bytes = drive_root_letter(&root)
        .and_then(|_| crate::core::fs::get_drive_space(&root))
        .map(|(total, free)| total.saturating_sub(free))
        .filter(|used| *used > 0);

    let method = match fast {
        Some(Ok(_)) => ScanMethod::Mft,
        _ => ScanMethod::Standard,
    };
    counters
        .standard
        .store(method == ScanMethod::Standard, Ordering::Relaxed);
    let worker = {
        let root = root.clone();
        let counters = counters.clone();
        let cancel = cancel.clone();
        // Deep folder trees recurse deeply; give the walk room.
        std::thread::Builder::new()
            .name("disk-usage-walk".into())
            .stack_size(64 * 1024 * 1024)
            .spawn(move || scan_tree(&root, fast, mode, owner, &counters, &cancel))
    };
    let Ok(worker) = worker else {
        let _ = tx.send(ScanEvent::Finished(ScanOutcome {
            tree: None,
            method,
            fast_scan_note: None,
            elapsed: started.elapsed(),
        }));
        return;
    };

    while !worker.is_finished() {
        std::thread::sleep(PROGRESS_INTERVAL);
        let progress = snapshot(&counters, expected_bytes);
        if tx.send(ScanEvent::Progress(progress)).is_err() {
            // Nobody's listening any more.
            cancel.store(true, Ordering::Relaxed);
        }
    }

    let (tree, method, note) = worker
        .join()
        .unwrap_or((None, ScanMethod::Standard, None));
    let tree = tree.filter(|_| !cancel.load(Ordering::Relaxed));
    let _ = tx.send(ScanEvent::Finished(ScanOutcome {
        tree,
        method,
        fast_scan_note: note,
        elapsed: started.elapsed(),
    }));
}

fn snapshot(counters: &Counters, expected_bytes: Option<u64>) -> ScanProgress {
    let method = if counters.standard.load(Ordering::Relaxed) {
        ScanMethod::Standard
    } else {
        ScanMethod::Mft
    };
    let mft_fraction = (method == ScanMethod::Mft)
        .then(|| {
            counters
                .has_fraction
                .load(Ordering::Relaxed)
                .then(|| f32::from_bits(counters.fraction_bits.load(Ordering::Relaxed)))
        })
        .flatten();
    let fraction = mft_fraction.or_else(|| {
        (method == ScanMethod::Standard)
            .then_some(expected_bytes)
            .flatten()
            .map(|expected| {
                (counters.allocated.load(Ordering::Relaxed) as f64 / expected as f64).min(0.99) as f32
            })
    });
    ScanProgress {
        files: counters.files.load(Ordering::Relaxed),
        dirs: counters.dirs.load(Ordering::Relaxed),
        bytes: counters.bytes.load(Ordering::Relaxed),
        fraction,
        current: counters.current.lock().ok().and_then(|c| c.clone()),
        method: Some(method),
        waiting_for_permission: counters.waiting_for_permission.load(Ordering::Relaxed),
    }
}

fn scan_tree(
    root: &Path,
    fast: Option<Result<char, FastScanNote>>,
    mode: FastScan,
    owner: isize,
    counters: &Arc<Counters>,
    cancel: &Arc<AtomicBool>,
) -> (Option<DirNode>, ScanMethod, Option<FastScanNote>) {
    let mut note = None;
    match fast {
        Some(Ok(letter)) => {
            let progress = |done: u64, total: u64, bytes: u64| {
                let fraction = if total == 0 { 0.0 } else { done as f32 / total as f32 };
                counters.fraction_bits.store(fraction.to_bits(), Ordering::Relaxed);
                counters.has_fraction.store(true, Ordering::Relaxed);
                counters.files.store(done, Ordering::Relaxed);
                counters.bytes.store(bytes, Ordering::Relaxed);
            };
            let result = if is_elevated() {
                crate::core::ntfs_mft::scan_volume(letter, cancel, &progress)
                    .map_err(FastScanNote::Failed)
            } else if mode == FastScan::AskForAdmin {
                use crate::core::mft_helper::{ElevatedScanError, scan_elevated};
                let waiting = |on: bool| counters.waiting_for_permission.store(on, Ordering::Relaxed);
                scan_elevated(letter, owner, cancel, &waiting, &progress).map_err(|e| match e {
                    ElevatedScanError::Declined => FastScanNote::Declined,
                    ElevatedScanError::Failed(reason) => FastScanNote::Failed(reason),
                })
            } else {
                Err(FastScanNote::NeedsAdmin)
            };
            match result {
                Ok(Some(tree)) => return (Some(tree), ScanMethod::Mft, None),
                Ok(None) => return (None, ScanMethod::Mft, None),
                Err(reason) => {
                    if let FastScanNote::Failed(detail) = &reason {
                        eprintln!("MFT scan of {letter}: failed ({detail}); using the standard scan");
                    }
                    note = Some(reason);
                    counters.has_fraction.store(false, Ordering::Relaxed);
                    counters.files.store(0, Ordering::Relaxed);
                    counters.bytes.store(0, Ordering::Relaxed);
                }
            }
        }
        Some(Err(reason)) => note = Some(reason),
        None => {}
    }

    counters.standard.store(true, Ordering::Relaxed);
    let tree = scan_standard(root, counters, cancel);
    (tree, ScanMethod::Standard, note)
}

/// Scans `root` folder by folder on a thread pool. `None` if cancelled.
fn scan_standard(root: &Path, counters: &Counters, cancel: &AtomicBool) -> Option<DirNode> {
    let threads = num_cpus::get().clamp(2, 8);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .stack_size(16 * 1024 * 1024)
        .thread_name(|i| format!("disk-usage-{i}"))
        .build()
        .ok()?;
    let name: Box<str> = root.to_string_lossy().into();
    let tree = pool.install(|| scan_dir(root, name, counters, cancel));
    (!cancel.load(Ordering::Relaxed)).then_some(tree)
}

struct RawEntry {
    name: Box<str>,
    is_dir: bool,
    is_link: bool,
    size: u64,
    allocated: u64,
}

fn scan_dir(path: &Path, name: Box<str>, counters: &Counters, cancel: &AtomicBool) -> DirNode {
    let mut node = DirNode {
        name,
        ..Default::default()
    };
    if cancel.load(Ordering::Relaxed) {
        return node;
    }
    if let Ok(mut current) = counters.current.try_lock() {
        *current = Some(path.to_path_buf());
    }
    let Some(entries) = list_dir(path) else {
        node.unreadable = true;
        return node;
    };
    counters.dirs.fetch_add(1, Ordering::Relaxed);

    let mut subdirs = Vec::new();
    for entry in entries {
        if entry.is_dir {
            if entry.is_link {
                node.dirs.push(DirNode {
                    name: entry.name,
                    is_link: true,
                    ..Default::default()
                });
            } else {
                subdirs.push(entry.name);
            }
        } else {
            counters.files.fetch_add(1, Ordering::Relaxed);
            counters.bytes.fetch_add(entry.size, Ordering::Relaxed);
            counters.allocated.fetch_add(entry.allocated, Ordering::Relaxed);
            node.files.push(FileEntry {
                name: entry.name,
                size: entry.size,
                allocated: entry.allocated,
            });
        }
    }

    let children: Vec<DirNode> = subdirs
        .into_par_iter()
        .map(|child| {
            let child_path = path.join(&*child);
            scan_dir(&child_path, child, counters, cancel)
        })
        .collect();
    node.dirs.extend(children);
    node.recompute_totals();
    node.sort_children();
    node
}

/// Lists one folder with `NtQueryDirectoryFile`, or `None` if it can't be
/// opened.
fn list_dir(path: &Path) -> Option<Vec<RawEntry>> {
    use ntapi::ntioapi::{FILE_DIRECTORY_INFORMATION, IO_STATUS_BLOCK, NtQueryDirectoryFile};
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    };
    const STATUS_NO_MORE_FILES: i32 = 0x80000006u32 as i32;

    let handle = crate::core::fs::open_directory_handle(&path.to_path_buf())?;
    let mut entries = Vec::new();
    let mut buffer = vec![0u8; 64 * 1024];
    unsafe {
        let mut io_status: IO_STATUS_BLOCK = std::mem::zeroed();
        loop {
            let status = NtQueryDirectoryFile(
                handle.0 as *mut _,
                std::ptr::null_mut(),
                None,
                std::ptr::null_mut(),
                &mut io_status,
                buffer.as_mut_ptr() as *mut _,
                buffer.len() as u32,
                1,
                0,
                std::ptr::null_mut(),
                0,
            );
            if status == STATUS_NO_MORE_FILES || status < 0 {
                break;
            }
            let end = io_status.Information as usize;
            let mut offset = 0usize;
            while offset < end {
                let entry = &*(buffer.as_ptr().add(offset) as *const FILE_DIRECTORY_INFORMATION);
                let name = std::slice::from_raw_parts(
                    entry.FileName.as_ptr(),
                    entry.FileNameLength as usize / 2,
                );
                let is_dot = name == [b'.' as u16] || name == [b'.' as u16, b'.' as u16];
                if !is_dot {
                    let attrs = entry.FileAttributes;
                    entries.push(RawEntry {
                        name: std::ffi::OsString::from_wide(name).to_string_lossy().into(),
                        is_dir: attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0,
                        is_link: attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0,
                        size: (*entry.EndOfFile.QuadPart()).max(0) as u64,
                        allocated: (*entry.AllocationSize.QuadPart()).max(0) as u64,
                    });
                }
                if entry.NextEntryOffset == 0 {
                    break;
                }
                offset += entry.NextEntryOffset as usize;
            }
        }
        let _ = CloseHandle(handle);
    }
    Some(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, size: u64) -> FileEntry {
        FileEntry {
            name: name.into(),
            size,
            allocated: size.div_ceil(4096) * 4096,
        }
    }

    fn dir(name: &str, dirs: Vec<DirNode>, files: Vec<FileEntry>) -> DirNode {
        let mut node = DirNode {
            name: name.into(),
            dirs,
            files,
            ..Default::default()
        };
        node.recompute_totals();
        node.sort_children();
        node
    }

    fn sample() -> DirNode {
        dir(
            r"C:\data",
            vec![
                dir("small", vec![], vec![file("a", 10)]),
                dir("Big", vec![dir("inner", vec![], vec![file("x", 5000)])], vec![file("b", 100)]),
            ],
            vec![file("top.txt", 1)],
        )
    }

    #[test]
    fn totals_and_order_roll_up() {
        let root = sample();
        assert_eq!(root.size, 5111);
        assert_eq!(root.file_count, 4);
        assert_eq!(root.dir_count, 3);
        assert_eq!(&*root.dirs[0].name, "Big", "largest folder first");
        assert_eq!(root.allocated, 4096 * 3 + 8192);
    }

    #[test]
    fn replace_branch_updates_every_ancestor() {
        let mut root = sample();
        let path = vec!["Big".to_string(), "inner".to_string()];
        let rescanned = dir("whatever", vec![], vec![file("x", 1), file("y", 2)]);
        assert!(root.replace_branch(&path, rescanned));

        let inner = root.find(&path).unwrap();
        assert_eq!(&*inner.name, "inner", "keeps its own name");
        assert_eq!(inner.size, 3);
        assert_eq!(root.size, 1 + 10 + 100 + 3);
        assert_eq!(root.file_count, 5);
        assert_eq!(&*root.dirs[0].name, "Big");
        assert_eq!(root.dirs[0].size, 103);

        // Folder names match regardless of case.
        assert!(root.find(&["big".to_string()]).is_some());
        assert!(!root.replace_branch(&["missing".to_string()], DirNode::default()));
    }

    #[test]
    fn branch_components_are_relative_to_the_root() {
        let root = Path::new(r"C:\data");
        assert_eq!(
            branch_components(root, Path::new(r"C:\data\Big\inner")),
            Some(vec!["Big".to_string(), "inner".to_string()])
        );
        assert_eq!(branch_components(root, root), Some(vec![]));
        assert_eq!(
            branch_components(root, Path::new(r"c:\DATA\Big")),
            Some(vec!["Big".to_string()])
        );
        assert_eq!(branch_components(root, Path::new(r"D:\other")), None);
    }

    #[test]
    fn drive_roots_are_recognized() {
        assert_eq!(drive_root_letter(Path::new(r"C:\")), Some('C'));
        assert_eq!(drive_root_letter(Path::new("d:")), Some('D'));
        assert_eq!(drive_root_letter(Path::new(r"C:\Windows")), None);
        assert_eq!(drive_root_letter(Path::new(r"\\server\share")), None);
    }

    fn wait(handle: ScanHandle) -> (Vec<ScanProgress>, ScanOutcome) {
        let mut progress = Vec::new();
        loop {
            match handle.rx.recv_timeout(Duration::from_secs(60)).expect("scan finishes") {
                ScanEvent::Progress(p) => progress.push(p),
                ScanEvent::Finished(outcome) => return (progress, outcome),
            }
        }
    }

    #[test]
    fn standard_scan_builds_the_tree_and_skips_links() {
        let root = std::env::temp_dir().join(format!("eden_du_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub/deeper")).unwrap();
        std::fs::write(root.join("a.bin"), vec![0u8; 3000]).unwrap();
        std::fs::write(root.join("sub/b.bin"), vec![0u8; 700]).unwrap();
        std::fs::write(root.join("sub/deeper/c.bin"), vec![0u8; 50]).unwrap();

        let (_, outcome) = wait(start_scan(root.clone(), FastScan::AskForAdmin));
        assert_eq!(outcome.method, ScanMethod::Standard);
        assert_eq!(outcome.fast_scan_note, None, "a folder never tries the MFT scan");
        let tree = outcome.tree.expect("finished");
        assert_eq!(tree.size, 3750);
        assert_eq!(tree.file_count, 3);
        assert_eq!(tree.dir_count, 2);
        assert_eq!(&*tree.dirs[0].name, "sub");
        assert_eq!(tree.dirs[0].dirs[0].size, 50);

        // Rescan one branch after it changed.
        std::fs::write(root.join("sub/deeper/d.bin"), vec![0u8; 25]).unwrap();
        let (_, branch) = wait(start_scan(root.join("sub").join("deeper"), FastScan::Off));
        let mut tree = tree;
        let components = branch_components(&root, &root.join("sub").join("deeper")).unwrap();
        assert!(tree.replace_branch(&components, branch.tree.unwrap()));
        assert_eq!(tree.size, 3775);
        assert_eq!(tree.file_count, 4);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cancelled_scan_reports_no_tree() {
        let handle = start_scan(std::env::temp_dir(), FastScan::Off);
        handle.cancel();
        let (_, outcome) = wait(handle);
        assert!(outcome.tree.is_none());
    }

    #[test]
    fn unreadable_folder_is_marked() {
        let (_, outcome) = wait(start_scan(PathBuf::from(r"C:\definitely\not\here"), FastScan::Off));
        let tree = outcome.tree.unwrap();
        assert!(tree.unreadable);
        assert_eq!(tree.size, 0);
    }
}
