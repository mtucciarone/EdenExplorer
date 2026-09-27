//! Storage Sense-style cleanup shortcuts for the Disk Usage dashboard:
//! temporary files, the Windows Update download cache, browser caches, the
//! Recycle Bin, and old files in Downloads. Everything here only measures
//! and deletes; the dashboard runs it on background threads.
//!
//! Temp and cache files are deleted permanently (what Storage Sense does);
//! files in use are skipped. Old Downloads are only *listed* here: the
//! dashboard sends them to the Recycle Bin through the usual delete.

use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
/// Temp files touched this recently are left alone (an installer that's
/// still running, say).
const TEMP_GRACE: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CleanupKind {
    TempFiles,
    WindowsUpdate,
    BrowserCaches,
    RecycleBin,
    OldDownloads,
}

impl CleanupKind {
    pub const ALL: [CleanupKind; 5] = [
        CleanupKind::TempFiles,
        CleanupKind::WindowsUpdate,
        CleanupKind::BrowserCaches,
        CleanupKind::RecycleBin,
        CleanupKind::OldDownloads,
    ];
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Measure {
    pub size: u64,
    pub files: u64,
    /// A folder couldn't be read (usually: needs administrator).
    pub denied: bool,
    /// Old Downloads only: the files that would go to the Recycle Bin.
    pub paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cleaned {
    pub freed: u64,
    pub deleted: u64,
    /// In use or access denied.
    pub skipped: u64,
}

fn windows_dir() -> PathBuf {
    std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
}

/// The folders whose contents `kind` cleans (not Recycle Bin / Downloads).
pub fn folders_for(kind: CleanupKind) -> Vec<PathBuf> {
    match kind {
        CleanupKind::TempFiles => {
            let mut list = vec![std::env::temp_dir()];
            let system = windows_dir().join("Temp");
            if !list.iter().any(|p| same_path(p, &system)) {
                list.push(system);
            }
            list
        }
        CleanupKind::WindowsUpdate => vec![windows_dir().join("SoftwareDistribution").join("Download")],
        CleanupKind::BrowserCaches => browser_cache_folders(),
        CleanupKind::RecycleBin | CleanupKind::OldDownloads => Vec::new(),
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().trim_end_matches('\\').eq_ignore_ascii_case(b.to_string_lossy().trim_end_matches('\\'))
}

/// Cache folders of Chromium browsers (every profile) and Firefox.
pub fn browser_cache_folders() -> Vec<PathBuf> {
    let Some(local) = dirs::data_local_dir() else { return Vec::new() };
    let mut out = Vec::new();
    for user_data in [
        r"Google\Chrome\User Data",
        r"Microsoft\Edge\User Data",
        r"BraveSoftware\Brave-Browser\User Data",
        r"Vivaldi\User Data",
        r"Chromium\User Data",
    ] {
        let Ok(profiles) = std::fs::read_dir(local.join(user_data)) else { continue };
        for profile in profiles.flatten() {
            for cache in ["Cache", "Code Cache", "GPUCache"] {
                let dir = profile.path().join(cache);
                if dir.is_dir() {
                    out.push(dir);
                }
            }
        }
    }
    if let Ok(profiles) = std::fs::read_dir(local.join(r"Mozilla\Firefox\Profiles")) {
        for profile in profiles.flatten() {
            let dir = profile.path().join("cache2");
            if dir.is_dir() {
                out.push(dir);
            }
        }
    }
    out
}

pub fn downloads_folder() -> Option<PathBuf> {
    dirs::download_dir()
}

/// Every file under `dir` (not following junctions or links): `visit(path,
/// size, modified)`. Returns false if `dir` itself couldn't be listed.
fn walk_files(dir: &Path, mut visit: impl FnMut(&Path, u64, SystemTime)) -> bool {
    let mut stack = vec![dir.to_path_buf()];
    let mut first = true;
    while let Some(folder) = stack.pop() {
        let entries = match std::fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(_) if first => return false,
            Err(_) => continue,
        };
        first = false;
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                continue;
            }
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                visit(&entry.path(), meta.len(), meta.modified().unwrap_or(SystemTime::UNIX_EPOCH));
            }
        }
    }
    true
}

fn old_enough(kind: CleanupKind, modified: SystemTime, now: SystemTime) -> bool {
    kind != CleanupKind::TempFiles || now.duration_since(modified).is_ok_and(|age| age >= TEMP_GRACE)
}

fn recycle_bin_totals() -> Option<(u64, u64)> {
    use windows::Win32::UI::Shell::{SHQUERYRBINFO, SHQueryRecycleBinW};
    let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, ..Default::default() };
    unsafe { SHQueryRecycleBinW(windows::core::PCWSTR::null(), &mut info) }.ok()?;
    Some((info.i64Size.max(0) as u64, info.i64NumItems.max(0) as u64))
}

/// How much `kind` would free. `download_age` is how old (unmodified) a
/// file in Downloads must be to count.
pub fn measure(kind: CleanupKind, download_age: Duration) -> Measure {
    let now = SystemTime::now();
    let mut m = Measure::default();
    match kind {
        CleanupKind::RecycleBin => {
            let (size, files) = recycle_bin_totals().unwrap_or_default();
            m.size = size;
            m.files = files;
        }
        CleanupKind::OldDownloads => {
            if let Some(dir) = downloads_folder() {
                m.denied = !walk_files(&dir, |path, size, modified| {
                    if now.duration_since(modified).is_ok_and(|age| age >= download_age) {
                        m.size += size;
                        m.files += 1;
                        m.paths.push(path.to_path_buf());
                    }
                });
                m.paths.sort();
            }
        }
        _ => {
            for dir in folders_for(kind) {
                if !dir.exists() {
                    continue;
                }
                let listed = walk_files(&dir, |_, size, modified| {
                    if old_enough(kind, modified, now) {
                        m.size += size;
                        m.files += 1;
                    }
                });
                m.denied |= !listed;
            }
        }
    }
    m
}

/// Removes the folders under `dir` left empty (not `dir` itself).
fn remove_empty_folders(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() && meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
            remove_empty_folders(&entry.path());
            let _ = std::fs::remove_dir(entry.path()); // fails unless empty
        }
    }
}

/// Deletes what `measure(kind)` counted (not Old Downloads, which go to
/// the Recycle Bin through the dashboard). Files in use are skipped.
pub fn clean(kind: CleanupKind) -> Cleaned {
    let now = SystemTime::now();
    let mut done = Cleaned::default();
    match kind {
        CleanupKind::RecycleBin => {
            use windows::Win32::UI::Shell::{SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHEmptyRecycleBinW};
            let before = recycle_bin_totals().unwrap_or_default();
            let emptied = unsafe {
                SHEmptyRecycleBinW(None, windows::core::PCWSTR::null(), SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND)
            };
            let after = recycle_bin_totals().unwrap_or_default();
            if emptied.is_ok() || after.1 < before.1 {
                done.freed = before.0.saturating_sub(after.0);
                done.deleted = before.1.saturating_sub(after.1);
            }
            done.skipped = after.1;
        }
        CleanupKind::OldDownloads => {}
        _ => {
            for dir in folders_for(kind) {
                walk_files(&dir, |path, size, modified| {
                    if !old_enough(kind, modified, now) {
                        return;
                    }
                    if std::fs::remove_file(path).is_ok() {
                        done.freed += size;
                        done.deleted += 1;
                    } else {
                        done.skipped += 1;
                    }
                });
                remove_empty_folders(&dir);
            }
        }
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_point_at_the_right_places() {
        assert!(folders_for(CleanupKind::TempFiles).iter().any(|p| same_path(p, &std::env::temp_dir())));
        let wu = &folders_for(CleanupKind::WindowsUpdate)[0];
        assert!(wu.ends_with(r"SoftwareDistribution\Download"));
        assert!(folders_for(CleanupKind::RecycleBin).is_empty());
    }

    #[test]
    fn walk_counts_files_and_reports_missing_folders() {
        let dir = std::env::temp_dir().join(format!("eden-cleanup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub/deeper")).unwrap();
        std::fs::write(dir.join("a.tmp"), [0u8; 10]).unwrap();
        std::fs::write(dir.join("sub/deeper/b.tmp"), [0u8; 5]).unwrap();
        let mut total = 0;
        let mut count = 0;
        assert!(walk_files(&dir, |_, size, _| {
            total += size;
            count += 1;
        }));
        assert_eq!((total, count), (15, 2));
        assert!(!walk_files(&dir.join("missing"), |_, _, _| {}));

        std::fs::remove_file(dir.join("sub/deeper/b.tmp")).unwrap();
        remove_empty_folders(&dir);
        assert!(!dir.join("sub").exists());
        assert!(dir.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fresh_temp_files_are_kept() {
        let now = SystemTime::now();
        assert!(!old_enough(CleanupKind::TempFiles, now, now));
        assert!(old_enough(CleanupKind::TempFiles, now - Duration::from_secs(7200), now));
        assert!(old_enough(CleanupKind::BrowserCaches, now, now));
    }
}
