//! Remembers calculated folder sizes between sessions (`folder_sizes.bin`),
//! so a large folder's subfolder sizes show immediately on the next visit
//! instead of counting up from zero. The cached size is shown as provisional
//! while the usual background scan re-checks it, and replaced by the fresh
//! result once that finishes.
//!
//! The file can hold tens of thousands of folders, so it's read and written
//! on background threads rather than on the UI thread (where a periodic
//! save used to cause a visible hitch).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Most folders kept; the least recently calculated are dropped first.
const MAX_ENTRIES: usize = 50_000;
/// How often (at most) a changed cache is written to disk while running.
const SAVE_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Entry {
    bytes: u64,
    /// Seconds since the Unix epoch when this size was calculated.
    calculated_at: u64,
}

#[derive(Serialize, Deserialize)]
struct Snapshot {
    entries: Vec<(String, Entry)>,
}

pub struct FolderSizeCache {
    /// Keyed by lowercased path (Windows paths are case-insensitive).
    entries: HashMap<String, Entry>,
    dirty: bool,
    last_saved: Instant,
    /// The saved file, while it's still being read (see `load`).
    loading: Option<std::sync::mpsc::Receiver<HashMap<String, Entry>>>,
    /// A background save still writing.
    saving: Option<std::thread::JoinHandle<()>>,
}

impl Default for FolderSizeCache {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            dirty: false,
            last_saved: Instant::now(),
            loading: None,
            saving: None,
        }
    }
}

fn key(path: &Path) -> String {
    path.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_lowercase()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_path() -> Option<PathBuf> {
    Some(crate::core::app_data::data_dir()?.join("folder_sizes.bin"))
}

fn read_file() -> HashMap<String, Entry> {
    cache_path()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|data| postcard::from_bytes::<Snapshot>(&data).ok())
        .map(|snapshot| snapshot.entries.into_iter().collect())
        .unwrap_or_default()
}

fn write_file(entries: Vec<(String, Entry)>) {
    let Some(path) = cache_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = postcard::to_allocvec(&Snapshot { entries }) {
        let _ = std::fs::write(path, data);
    }
}

impl FolderSizeCache {
    /// Starts reading the saved sizes on a background thread; they're
    /// merged in by `poll_loaded` once read.
    pub fn load() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("folder-size-cache-load".into())
            .spawn(move || {
                let _ = tx.send(read_file());
            });
        Self {
            loading: spawned.is_ok().then_some(rx),
            ..Default::default()
        }
    }

    /// Merges the saved sizes in once they've been read (sizes calculated
    /// since startup win). Call once per frame.
    pub fn poll_loaded(&mut self) {
        let Some(rx) = &self.loading else {
            return;
        };
        match rx.try_recv() {
            Ok(saved) => {
                self.loading = None;
                self.merge(saved);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.loading = None,
        }
    }

    /// Waits for the saved sizes (before writing the file, so a save
    /// can't drop what hasn't been read yet).
    fn finish_loading(&mut self) {
        if let Some(rx) = self.loading.take()
            && let Ok(saved) = rx.recv_timeout(Duration::from_secs(5))
        {
            self.merge(saved);
        }
    }

    fn merge(&mut self, saved: HashMap<String, Entry>) {
        for (key, entry) in saved {
            self.entries.entry(key).or_insert(entry);
        }
    }

    fn finish_saving(&mut self) {
        if let Some(handle) = self.saving.take() {
            let _ = handle.join();
        }
    }

    pub fn get(&self, path: &Path) -> Option<u64> {
        self.entries.get(&key(path)).map(|entry| entry.bytes)
    }

    pub fn insert(&mut self, path: &Path, bytes: u64) {
        let entry = Entry {
            bytes,
            calculated_at: now_secs(),
        };
        if self.entries.insert(key(path), entry).map(|old| old.bytes) != Some(bytes) {
            self.dirty = true;
        }
    }

    pub fn clear(&mut self) {
        // Don't let a pending read bring the old sizes back, or a pending
        // write recreate the file.
        self.loading = None;
        self.finish_saving();
        self.entries.clear();
        self.dirty = false;
        if let Some(path) = cache_path() {
            let _ = std::fs::remove_file(path);
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }

    /// Drops the oldest entries past `MAX_ENTRIES`.
    fn trim(&mut self) {
        if self.entries.len() <= MAX_ENTRIES {
            return;
        }
        let mut by_age: Vec<(u64, String)> = self
            .entries
            .iter()
            .map(|(k, e)| (e.calculated_at, k.clone()))
            .collect();
        by_age.sort_unstable();
        let excess = self.entries.len() - MAX_ENTRIES;
        for (_, k) in by_age.into_iter().take(excess) {
            self.entries.remove(&k);
        }
    }

    fn snapshot(&mut self) -> Vec<(String, Entry)> {
        self.trim();
        self.dirty = false;
        self.last_saved = Instant::now();
        self.entries.iter().map(|(k, e)| (k.clone(), *e)).collect()
    }

    /// Writes the cache now if it changed (on exit).
    pub fn save(&mut self) {
        self.finish_loading();
        self.finish_saving();
        if !self.dirty {
            return;
        }
        let entries = self.snapshot();
        write_file(entries);
    }

    /// Saves in the background if it changed and the last save was a while
    /// ago - call once per frame.
    pub fn save_if_due(&mut self) {
        if !self.dirty
            || self.loading.is_some()
            || self.last_saved.elapsed() < SAVE_INTERVAL
            || self.saving.as_ref().is_some_and(|h| !h.is_finished())
        {
            return;
        }
        self.finish_saving();
        let entries = self.snapshot();
        self.saving = std::thread::Builder::new()
            .name("folder-size-cache-save".into())
            .spawn(move || write_file(entries))
            .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_ignores_case_and_trailing_separator() {
        let mut cache = FolderSizeCache::default();
        cache.insert(Path::new(r"C:\Games\"), 42);
        assert_eq!(cache.get(Path::new(r"c:\games")), Some(42));
        assert_eq!(cache.get(Path::new(r"C:\Other")), None);
    }

    #[test]
    fn saved_sizes_load_in_the_background_without_overriding_fresh_ones() {
        let mut cache = FolderSizeCache::default();
        cache.insert(Path::new(r"C:\A"), 5);
        let saved: HashMap<String, Entry> = [
            (key(Path::new(r"C:\A")), Entry { bytes: 1, calculated_at: 0 }),
            (key(Path::new(r"C:\B")), Entry { bytes: 2, calculated_at: 0 }),
        ]
        .into();
        let (tx, rx) = std::sync::mpsc::channel();
        cache.loading = Some(rx);
        cache.poll_loaded();
        assert!(cache.loading.is_some(), "still waiting");
        tx.send(saved).unwrap();
        cache.poll_loaded();
        assert!(cache.loading.is_none());
        assert_eq!(cache.get(Path::new(r"C:\A")), Some(5));
        assert_eq!(cache.get(Path::new(r"C:\B")), Some(2));
    }

    #[test]
    fn only_real_changes_mark_it_dirty() {
        let mut cache = FolderSizeCache::default();
        cache.insert(Path::new(r"C:\A"), 1);
        assert!(cache.dirty);
        cache.dirty = false;
        cache.insert(Path::new(r"C:\A"), 1);
        assert!(!cache.dirty);
        cache.insert(Path::new(r"C:\A"), 2);
        assert!(cache.dirty);
    }

    #[test]
    fn trim_drops_the_oldest() {
        let mut cache = FolderSizeCache::default();
        for i in 0..MAX_ENTRIES + 2 {
            cache.entries.insert(
                format!("c:\\f{i}"),
                Entry {
                    bytes: 1,
                    calculated_at: i as u64,
                },
            );
        }
        cache.trim();
        assert_eq!(cache.len(), MAX_ENTRIES);
        assert!(cache.get(Path::new(r"C:\f0")).is_none());
        assert!(cache.get(Path::new(r"C:\f1")).is_none());
        assert!(cache.get(Path::new(r"C:\f2")).is_some());
    }

    #[test]
    fn snapshot_round_trips() {
        let snapshot = Snapshot {
            entries: vec![("c:\\a".into(), Entry { bytes: 7, calculated_at: 9 })],
        };
        let data = postcard::to_allocvec(&snapshot).unwrap();
        let back: Snapshot = postcard::from_bytes(&data).unwrap();
        assert_eq!(back.entries, snapshot.entries);
    }
}
