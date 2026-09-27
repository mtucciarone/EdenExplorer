//! The Disk Usage dashboard's duplicate finder. Works from a finished scan
//! so most files are never opened:
//!
//! 1. files are grouped by size (a file with a unique size has no twin),
//! 2. same-sized files are hashed on their first and last 64 KB,
//! 3. files that still match are hashed in full (SHA-256, the checksum
//!    code's hasher).
//!
//! Hardlinks (one file under several names) aren't duplicates, since
//! deleting one frees nothing, so names of the same file are folded into
//! one before hashing.

use crate::core::disk_usage::DirNode;
use crate::core::disk_usage_stats::{ViewFilter, for_each_file};
use crossbeam_channel::{Receiver, Sender};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Bytes hashed from each end of a file in the quick pass.
const SAMPLE: u64 = 64 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Debug, PartialEq)]
pub struct DupFile {
    pub path: PathBuf,
    /// FILETIME, 0 when unknown.
    pub modified: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DupGroup {
    /// Size of each copy.
    pub size: u64,
    pub files: Vec<DupFile>,
}

impl DupGroup {
    /// Space freed by keeping just one copy.
    pub fn wasted(&self) -> u64 {
        self.size * (self.files.len().saturating_sub(1)) as u64
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DupStage {
    #[default]
    Sizes,
    Quick,
    Full,
}

#[derive(Clone, Debug, Default)]
pub struct DupProgress {
    pub stage: DupStage,
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

pub enum DupEvent {
    Progress(DupProgress),
    /// `None` when cancelled.
    Finished(Option<Vec<DupGroup>>),
}

pub struct DupHandle {
    pub rx: Receiver<DupEvent>,
    cancel: Arc<AtomicBool>,
}

impl DupHandle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for DupHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct Reporter<'a> {
    tx: &'a Sender<DupEvent>,
    progress: DupProgress,
    last: Instant,
    cancel: &'a AtomicBool,
}

impl Reporter<'_> {
    fn tick(&mut self, force: bool) -> bool {
        if force || self.last.elapsed() >= PROGRESS_INTERVAL {
            self.last = Instant::now();
            let _ = self.tx.send(DupEvent::Progress(self.progress.clone()));
        }
        !self.cancel.load(Ordering::Relaxed)
    }
}

/// Every file (passing `filter`, at least `min_size` bytes) whose size is
/// shared with another, grouped by size.
pub fn size_groups(tree: &DirNode, root: &Path, filter: &ViewFilter, min_size: u64) -> Vec<(u64, Vec<DupFile>)> {
    let mut by_size: HashMap<u64, Vec<DupFile>> = HashMap::new();
    for_each_file(tree, root, filter, |folder, _, file| {
        if file.size >= min_size.max(1) {
            by_size.entry(file.size).or_default().push(DupFile {
                path: folder.join(&*file.name),
                modified: file.modified,
            });
        }
    });
    let mut groups: Vec<(u64, Vec<DupFile>)> = by_size.into_iter().filter(|(_, f)| f.len() > 1).collect();
    groups.sort_by(|a, b| b.0.cmp(&a.0));
    groups
}

/// The volume and file index identifying a file whatever its name.
#[cfg(windows)]
fn file_identity(file: &std::fs::File) -> Option<(u32, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }.ok()?;
    Some((info.dwVolumeSerialNumber, ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64))
}

#[cfg(not(windows))]
fn file_identity(_file: &std::fs::File) -> Option<(u32, u64)> {
    None
}

/// Splits each group by the hash of its files (`sample`: first/last bytes
/// only), dropping files that can't be read and groups left with one file.
/// Returns `None` if cancelled.
fn split_by_hash(
    groups: Vec<(u64, Vec<DupFile>)>,
    sample: Option<u64>,
    fold_links: bool,
    report: &mut Reporter,
) -> Option<Vec<(u64, Vec<DupFile>)>> {
    let mut out = Vec::new();
    for (size, files) in groups {
        let mut by_hash: HashMap<[u8; 32], Vec<DupFile>> = HashMap::new();
        let mut seen_ids = std::collections::HashSet::new();
        for file in files {
            let opened = std::fs::File::open(&file.path);
            report.progress.files_done += 1;
            let per_file = match sample {
                Some(s) if size > s * 2 => s * 2,
                _ => size,
            };
            let Ok(mut handle) = opened else {
                report.progress.bytes_done += per_file;
                continue;
            };
            if fold_links
                && let Some(id) = file_identity(&handle)
                && !seen_ids.insert(id)
            {
                report.progress.bytes_done += per_file;
                continue;
            }
            let before = report.progress.bytes_done;
            let hash = crate::core::checksum::sha256_of(&mut handle, size, sample, |n| {
                report.progress.bytes_done += n;
                report.tick(false)
            });
            report.progress.bytes_done = before + per_file;
            match hash {
                Ok(Some(hash)) => by_hash.entry(hash).or_default().push(file),
                Ok(None) => return None,
                Err(_) => {}
            }
            if !report.tick(false) {
                return None;
            }
        }
        out.extend(by_hash.into_values().filter(|f| f.len() > 1).map(|f| (size, f)));
    }
    Some(out)
}

/// Runs the three passes; `None` if cancelled.
pub fn find_duplicates(
    tree: &DirNode,
    root: &Path,
    filter: &ViewFilter,
    min_size: u64,
    tx: &Sender<DupEvent>,
    cancel: &AtomicBool,
) -> Option<Vec<DupGroup>> {
    let mut report = Reporter { tx, progress: DupProgress::default(), last: Instant::now(), cancel };
    report.tick(true);
    let groups = size_groups(tree, root, filter, min_size);

    report.progress = DupProgress {
        stage: DupStage::Quick,
        files_total: groups.iter().map(|g| g.1.len() as u64).sum(),
        bytes_total: groups.iter().map(|(size, f)| (*size).min(SAMPLE * 2) * f.len() as u64).sum(),
        ..Default::default()
    };
    if !report.tick(true) {
        return None;
    }
    let groups = split_by_hash(groups, Some(SAMPLE), true, &mut report)?;

    // Files no bigger than two samples were read whole already.
    let (small, big): (Vec<_>, Vec<_>) = groups.into_iter().partition(|(size, _)| *size <= SAMPLE * 2);
    report.progress = DupProgress {
        stage: DupStage::Full,
        files_total: big.iter().map(|g| g.1.len() as u64).sum(),
        bytes_total: big.iter().map(|(size, f)| size * f.len() as u64).sum(),
        ..Default::default()
    };
    if !report.tick(true) {
        return None;
    }
    let big = split_by_hash(big, None, false, &mut report)?;

    let mut result: Vec<DupGroup> = small
        .into_iter()
        .chain(big)
        .map(|(size, mut files)| {
            files.sort_by(|a, b| a.path.cmp(&b.path));
            DupGroup { size, files }
        })
        .collect();
    result.sort_by(|a, b| b.wasted().cmp(&a.wasted()).then_with(|| a.files[0].path.cmp(&b.files[0].path)));
    Some(result)
}

/// Starts `find_duplicates` on a background thread (below-normal priority,
/// like the scans).
pub fn start_duplicate_search(tree: DirNode, root: PathBuf, filter: ViewFilter, min_size: u64) -> DupHandle {
    let (tx, rx) = crossbeam_channel::unbounded();
    let cancel = Arc::new(AtomicBool::new(false));
    let thread_cancel = cancel.clone();
    std::thread::spawn(move || {
        crate::core::disk_usage::lower_thread_priority();
        let result = find_duplicates(&tree, &root, &filter, min_size, &tx, &thread_cancel);
        let _ = tx.send(DupEvent::Finished(result));
    });
    DupHandle { rx, cancel }
}

/// Which copy of each group to keep when marking the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    Newest,
    Oldest,
}

/// Every file except the one to keep in each group (ties go to the first
/// path in order).
pub fn all_but_one(groups: &[DupGroup], keep: Keep) -> Vec<PathBuf> {
    let mut marked = Vec::new();
    for group in groups {
        let kept = match keep {
            Keep::Newest => group.files.iter().enumerate().max_by(|a, b| a.1.modified.cmp(&b.1.modified).then(b.0.cmp(&a.0))),
            Keep::Oldest => group.files.iter().enumerate().min_by(|a, b| a.1.modified.cmp(&b.1.modified).then(a.0.cmp(&b.0))),
        }
        .map(|(i, _)| i);
        marked.extend(group.files.iter().enumerate().filter(|(i, _)| Some(*i) != kept).map(|(_, f)| f.path.clone()));
    }
    marked
}

/// True when `marked` leaves at least one copy of every group.
pub fn keeps_a_copy(groups: &[DupGroup], marked: &std::collections::HashSet<PathBuf>) -> bool {
    groups.iter().all(|g| g.files.iter().any(|f| !marked.contains(&f.path)))
}

/// Drops files that no longer exist (`gone`), and groups left with one.
pub fn forget(groups: &mut Vec<DupGroup>, gone: impl Fn(&Path) -> bool) {
    for group in groups.iter_mut() {
        group.files.retain(|f| !gone(&f.path));
    }
    groups.retain(|g| g.files.len() > 1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::disk_usage::FileEntry;
    use std::io::Write;

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::File::create(path).unwrap().write_all(bytes).unwrap();
    }

    /// A DirNode for what's on disk under `dir` (one level of folders is
    /// enough here).
    fn tree_of(dir: &Path) -> DirNode {
        fn walk(dir: &Path) -> DirNode {
            let mut node = DirNode { name: dir.file_name().unwrap().to_string_lossy().into(), ..Default::default() };
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let meta = entry.metadata().unwrap();
                if meta.is_dir() {
                    node.dirs.push(walk(&entry.path()));
                } else {
                    node.files.push(FileEntry {
                        name: entry.file_name().to_string_lossy().into(),
                        size: meta.len(),
                        allocated: meta.len(),
                        modified: 0,
                    });
                }
            }
            node.recompute_totals();
            node
        }
        walk(dir)
    }

    #[test]
    fn finds_true_duplicates_only() {
        let dir = std::env::temp_dir().join(format!("eden-dups-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let big: Vec<u8> = (0..400_000u32).map(|i| (i % 251) as u8).collect();
        let mut big_changed_middle = big.clone();
        big_changed_middle[200_000] ^= 1; // same size, same ends: only the full hash tells
        write(&dir.join("a/one.bin"), &big);
        write(&dir.join("b/two.bin"), &big);
        write(&dir.join("c/three.bin"), &big_changed_middle);
        write(&dir.join("a/small.txt"), b"hello duplicate");
        write(&dir.join("b/small copy.txt"), b"hello duplicate");
        write(&dir.join("b/other.txt"), b"hello DUPLICATE"); // same size, different bytes
        write(&dir.join("c/tiny"), b"x");
        write(&dir.join("c/tiny2"), b"x");

        let tree = tree_of(&dir);
        let (tx, _rx) = crossbeam_channel::unbounded();
        let cancel = AtomicBool::new(false);
        let groups = find_duplicates(&tree, &dir, &ViewFilter::default(), 2, &tx, &cancel).unwrap();
        assert_eq!(groups.len(), 2, "{groups:?}");
        assert_eq!(groups[0].size, 400_000);
        let names: Vec<_> = groups[0].files.iter().map(|f| f.path.strip_prefix(&dir).unwrap().to_path_buf()).collect();
        assert_eq!(names, vec![PathBuf::from("a/one.bin"), PathBuf::from("b/two.bin")]);
        assert_eq!(groups[0].wasted(), 400_000);
        assert_eq!(groups[1].files.len(), 2);
        assert!(groups[1].files.iter().all(|f| f.path.to_string_lossy().contains("small")));

        // Cancelling stops it.
        cancel.store(true, Ordering::Relaxed);
        assert!(find_duplicates(&tree, &dir, &ViewFilter::default(), 2, &tx, &cancel).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keep_newest_or_oldest_marks_the_rest() {
        let f = |p: &str, m: i64| DupFile { path: PathBuf::from(p), modified: m };
        let groups = vec![
            DupGroup { size: 10, files: vec![f("a", 5), f("b", 9), f("c", 1)] },
            DupGroup { size: 5, files: vec![f("d", 3), f("e", 3)] },
        ];
        let newest = all_but_one(&groups, Keep::Newest);
        assert_eq!(newest, vec![PathBuf::from("a"), PathBuf::from("c"), PathBuf::from("e")]);
        let oldest = all_but_one(&groups, Keep::Oldest);
        assert_eq!(oldest, vec![PathBuf::from("a"), PathBuf::from("b"), PathBuf::from("e")]);

        let marked: std::collections::HashSet<PathBuf> = newest.into_iter().collect();
        assert!(keeps_a_copy(&groups, &marked));
        let mut all = marked.clone();
        all.insert(PathBuf::from("d"));
        assert!(!keeps_a_copy(&groups, &all));

        let mut groups = groups;
        forget(&mut groups, |p| p == Path::new("d"));
        assert_eq!(groups.len(), 1);
    }
}
