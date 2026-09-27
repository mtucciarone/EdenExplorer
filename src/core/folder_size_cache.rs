//! Remembers calculated folder sizes between sessions (`folder_sizes.bin`),
//! so a large folder's subfolder sizes show immediately on the next visit
//! instead of counting up from zero. The cached size is shown as provisional
//! while the usual background scan re-checks it, and replaced by the fresh
//! result once that finishes.

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
}

impl Default for FolderSizeCache {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            dirty: false,
            last_saved: Instant::now(),
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

impl FolderSizeCache {
    pub fn load() -> Self {
        let entries = cache_path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|data| postcard::from_bytes::<Snapshot>(&data).ok())
            .map(|snapshot| snapshot.entries.into_iter().collect())
            .unwrap_or_default();
        Self {
            entries,
            ..Default::default()
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

    /// Writes the cache if it changed.
    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        self.trim();
        let Some(path) = cache_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let snapshot = Snapshot {
            entries: self.entries.iter().map(|(k, e)| (k.clone(), *e)).collect(),
        };
        if let Ok(data) = postcard::to_allocvec(&snapshot) {
            let _ = std::fs::write(path, data);
        }
        self.dirty = false;
        self.last_saved = Instant::now();
    }

    /// Saves if it changed and the last save was a while ago - call once
    /// per frame.
    pub fn save_if_due(&mut self) {
        if self.dirty && self.last_saved.elapsed() >= SAVE_INTERVAL {
            self.save();
        }
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
