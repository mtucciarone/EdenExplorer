//! Verify after copy: once a copy finishes, every copied file is compared
//! with its source - size first, then SHA-256 of both (the checksum code's
//! hasher) - so a bad disk, cable or network share can't silently hand
//! back a corrupt copy. Runs on a background thread at low priority.

use crossbeam_channel::{Receiver, Sender};
use std::collections::HashMap;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VerifyResult {
    pub files: u64,
    pub bytes: u64,
    /// Copies whose content differs from the source.
    pub mismatched: Vec<PathBuf>,
    /// Copies that aren't there (or couldn't be read).
    pub missing: Vec<PathBuf>,
}

impl VerifyResult {
    pub fn ok(&self) -> bool {
        self.mismatched.is_empty() && self.missing.is_empty()
    }
}

pub enum VerifyEvent {
    Progress(f32),
    /// `Err("")` = cancelled.
    Finished(Result<VerifyResult, String>),
}

pub struct VerifyHandle {
    pub rx: Receiver<VerifyEvent>,
    cancel: Arc<AtomicBool>,
}

impl VerifyHandle {
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }
}

/// (source file, copy) for every file a copy of `sources` into
/// `target_dir` made; `renames` gives items that were copied under another
/// name. Folders are walked (links and junctions aren't followed, as when
/// copying).
pub fn copy_pairs(sources: &[PathBuf], target_dir: &Path, renames: &HashMap<PathBuf, String>) -> Vec<(PathBuf, PathBuf)> {
    let mut pairs = Vec::new();
    for source in sources {
        let name = match renames.get(source) {
            Some(name) => name.clone(),
            None => match source.file_name() {
                Some(name) => name.to_string_lossy().into_owned(),
                None => continue,
            },
        };
        let dest = target_dir.join(name);
        let Ok(meta) = std::fs::symlink_metadata(source) else { continue };
        if meta.is_dir() {
            let mut stack = vec![(source.clone(), dest)];
            while let Some((src_dir, dst_dir)) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&src_dir) else { continue };
                for entry in entries.flatten() {
                    let Ok(meta) = entry.metadata() else { continue };
                    if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                        continue;
                    }
                    let dst = dst_dir.join(entry.file_name());
                    if meta.is_dir() {
                        stack.push((entry.path(), dst));
                    } else {
                        pairs.push((entry.path(), dst));
                    }
                }
            }
        } else {
            pairs.push((source.clone(), dest));
        }
    }
    pairs
}

fn hash(path: &Path, len: u64, cancel: &AtomicBool, mut on_read: impl FnMut(u64)) -> std::io::Result<Option<[u8; 32]>> {
    let mut file = std::fs::File::open(path)?;
    crate::core::checksum::sha256_of(&mut file, len, None, |n| {
        on_read(n);
        !cancel.load(Ordering::Relaxed)
    })
}

/// Checks every pair; `progress` gets 0..1 now and then.
pub fn verify(pairs: &[(PathBuf, PathBuf)], cancel: &AtomicBool, mut progress: impl FnMut(f32)) -> Result<VerifyResult, String> {
    let sizes: Vec<u64> = pairs.iter().map(|(src, _)| std::fs::metadata(src).map(|m| m.len()).unwrap_or(0)).collect();
    // Each file is read twice (source and copy).
    let total = (sizes.iter().sum::<u64>() * 2).max(1);
    let mut done = 0u64;
    let mut last = Instant::now();
    let mut result = VerifyResult::default();
    for ((src, dst), &size) in pairs.iter().zip(&sizes) {
        if cancel.load(Ordering::Relaxed) {
            return Err(String::new());
        }
        let dst_size = std::fs::metadata(dst).map(|m| m.len());
        match dst_size {
            Err(_) => {
                result.missing.push(dst.clone());
                done += size * 2;
                continue;
            }
            Ok(dst_size) if dst_size != size => {
                result.mismatched.push(dst.clone());
                done += size * 2;
                continue;
            }
            Ok(_) => {}
        }
        let mut tick = |n: u64| {
            done += n;
            if last.elapsed() >= Duration::from_millis(100) {
                last = Instant::now();
                progress(done as f32 / total as f32);
            }
        };
        let a = hash(src, size, cancel, &mut tick);
        let b = hash(dst, size, cancel, &mut tick);
        match (a, b) {
            (Ok(Some(a)), Ok(Some(b))) if a == b => {}
            (Ok(None), _) | (_, Ok(None)) => return Err(String::new()),
            (Ok(Some(_)), Ok(Some(_))) => result.mismatched.push(dst.clone()),
            _ => result.missing.push(dst.clone()),
        }
        result.files += 1;
        result.bytes += size;
    }
    progress(1.0);
    Ok(result)
}

/// Verifies a finished copy of `sources` into `target_dir` on a background
/// thread (the folder walk too, which can take a while for a big tree).
pub fn start_verify(sources: Vec<PathBuf>, target_dir: PathBuf, renames: HashMap<PathBuf, String>) -> VerifyHandle {
    let (tx, rx): (Sender<VerifyEvent>, _) = crossbeam_channel::unbounded();
    let cancel = Arc::new(AtomicBool::new(false));
    let thread_cancel = cancel.clone();
    std::thread::spawn(move || {
        crate::core::disk_usage::lower_thread_priority();
        let wake = crate::gui::windows::windowsoverrides::request_repaint;
        let pairs = copy_pairs(&sources, &target_dir, &renames);
        let result = verify(&pairs, &thread_cancel, |p| {
            let _ = tx.send(VerifyEvent::Progress(p));
            wake();
        });
        let _ = tx.send(VerifyEvent::Finished(result));
        wake();
    });
    VerifyHandle { rx, cancel }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_finds_good_bad_and_missing_copies() {
        let dir = std::env::temp_dir().join(format!("eden-verify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let src = dir.join("src");
        let dst = dir.join("dst");
        std::fs::create_dir_all(src.join("folder/deeper")).unwrap();
        std::fs::create_dir_all(dst.join("folder/deeper")).unwrap();
        std::fs::write(src.join("a.txt"), b"same").unwrap();
        std::fs::write(dst.join("a renamed.txt"), b"same").unwrap();
        std::fs::write(src.join("folder/deeper/b.bin"), b"original").unwrap();
        std::fs::write(dst.join("folder/deeper/b.bin"), b"0riginal").unwrap(); // same size, different bytes
        std::fs::write(src.join("folder/c.txt"), b"ccc").unwrap(); // not copied

        let renames: HashMap<PathBuf, String> = [(src.join("a.txt"), "a renamed.txt".to_string())].into();
        let mut pairs = copy_pairs(&[src.join("a.txt"), src.join("folder")], &dst, &renames);
        pairs.sort();
        assert_eq!(pairs.len(), 3);

        let result = verify(&pairs, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(result.mismatched, vec![dst.join("folder").join("deeper").join("b.bin")]);
        assert_eq!(result.missing, vec![dst.join("folder").join("c.txt")]);
        assert!(!result.ok());

        let good = copy_pairs(&[src.join("a.txt")], &dst, &renames);
        assert!(verify(&good, &AtomicBool::new(false), |_| {}).unwrap().ok());
        assert_eq!(verify(&good, &AtomicBool::new(true), |_| {}), Err(String::new()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
