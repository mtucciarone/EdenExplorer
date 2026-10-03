//! Portable tags: tags are also stored with the files themselves, so they
//! travel when files are copied to another folder, drive, or PC. On NTFS each
//! tagged file or folder gets a small hidden stream, `EdenExplorer.Tags`
//! (an alternate data stream: invisible in Explorer, doesn't change the
//! file's size or contents, and copied along by Windows); the file's dates
//! are kept as they were. Drives without streams (FAT32/exFAT USB sticks,
//! some network shares) get one hidden `.eden-tags.json` per folder instead.
//!
//! The app's own tag list stays the source of truth. Every time it's saved
//! (`on_tags_saved`), only the files whose tags changed are written, on a
//! background thread. When a folder is opened (`read_folder`), files that
//! carry tags this PC doesn't know about yet are imported.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, OnceLock};

use crate::core::indexer::TagsSnapshot;

pub const STREAM_NAME: &str = "EdenExplorer.Tags";
pub const SIDECAR_NAME: &str = ".eden-tags.json";
/// Folders with more entries than this aren't checked for tags to import.
const READ_FOLDER_MAX: usize = 20_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableTag {
    pub name: String,
    pub color: [u8; 4],
}

#[derive(Serialize, Deserialize, Default)]
struct StreamPayload {
    #[serde(default)]
    v: u32,
    tags: Vec<PortableTag>,
}

#[derive(Serialize, Deserialize, Default)]
struct Sidecar {
    #[serde(default)]
    v: u32,
    files: BTreeMap<String, Vec<PortableTag>>,
}

type TagMap = HashMap<PathBuf, Vec<PortableTag>>;
/// Paths and the tags to store with them (empty = remove).
pub type TagChanges = Vec<(PathBuf, Vec<PortableTag>)>;

static ENABLED: AtomicBool = AtomicBool::new(false);
/// The tags last written (or known to be on disk), per path.
static LAST: Mutex<Option<TagMap>> = Mutex::new(None);
static WRITER: OnceLock<Mutex<Sender<TagChanges>>> = OnceLock::new();

/// Every tagged path's tags, in the tag list's order.
fn map_of(snapshot: &TagsSnapshot) -> TagMap {
    let mut map: TagMap = HashMap::new();
    for group in &snapshot.groups {
        for item in &group.items {
            map.entry(item.clone()).or_default().push(PortableTag {
                name: group.name.clone(),
                color: group.color,
            });
        }
    }
    map
}

/// What changed between `old` and `new`: paths with new tags, and paths
/// that lost all theirs (an empty list).
fn diff(old: &TagMap, new: &TagMap) -> TagChanges {
    let mut changes: TagChanges = new
        .iter()
        .filter(|(path, tags)| old.get(*path) != Some(*tags))
        .map(|(path, tags)| (path.clone(), tags.clone()))
        .collect();
    changes.extend(
        old.keys()
            .filter(|p| !new.contains_key(*p))
            .map(|p| (p.clone(), Vec::new())),
    );
    changes
}

/// At startup: remembers what's tagged now (already on disk from earlier
/// sessions), so nothing is rewritten.
pub fn init(snapshot: &TagsSnapshot, enabled: bool) {
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some(map_of(snapshot));
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Turns portable tags on or off. Turning them on writes every tagged
/// file's tags (files already carrying the same tags are just rewritten);
/// turning them off leaves what's on disk alone.
pub fn set_enabled(enabled: bool, snapshot: &TagsSnapshot) {
    if ENABLED.swap(enabled, Ordering::Relaxed) == enabled {
        return;
    }
    if enabled {
        *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some(HashMap::new());
        on_tags_saved(snapshot);
    }
}

/// Called whenever the tag list is saved: writes the changes to the files.
pub fn on_tags_saved(snapshot: &TagsSnapshot) {
    if !is_enabled() {
        return;
    }
    let new = map_of(snapshot);
    let changes = {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        let changes = diff(last.as_ref().unwrap_or(&HashMap::new()), &new);
        *last = Some(new);
        changes
    };
    if changes.is_empty() {
        return;
    }
    let writer = WRITER.get_or_init(|| {
        let (tx, rx) = channel::<TagChanges>();
        std::thread::spawn(move || {
            for batch in rx {
                for (path, tags) in batch {
                    let _ = write_tags(&path, &tags);
                }
            }
        });
        Mutex::new(tx)
    });
    let _ = writer
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .send(changes);
}

/// Records tags just imported from `path`, so they aren't written back.
pub fn mark_synced(path: &Path, tags: &[PortableTag]) {
    if let Some(last) = LAST.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        last.insert(path.to_path_buf(), tags.to_vec());
    }
}

fn stream_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(":");
    s.push(STREAM_NAME);
    PathBuf::from(s)
}

/// Whether the drive holding `path` supports alternate data streams
/// (NTFS, ReFS), cached per volume.
pub fn supports_streams(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{GetVolumeInformationW, GetVolumePathNameW};
    use windows::core::PCWSTR;
    const FILE_NAMED_STREAMS: u32 = 0x0004_0000;
    static CACHE: Mutex<Option<HashMap<Vec<u16>, bool>>> = Mutex::new(None);

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut root = [0u16; 512];
    if unsafe { GetVolumePathNameW(PCWSTR(wide.as_ptr()), &mut root) }.is_err() {
        return false;
    }
    let len = root.iter().position(|&c| c == 0).unwrap_or(root.len());
    let key = root[..=len.min(root.len() - 1)].to_vec();
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(known) = cache.get_or_insert_with(HashMap::new).get(&key) {
        return *known;
    }
    let mut flags = 0u32;
    let ok = unsafe {
        GetVolumeInformationW(
            PCWSTR(key.as_ptr()),
            None,
            None,
            None,
            Some(&mut flags),
            None,
        )
    }
    .is_ok();
    let supported = ok && flags & FILE_NAMED_STREAMS != 0;
    cache
        .get_or_insert_with(HashMap::new)
        .insert(key, supported);
    supported
}

/// Keeps a file's created/accessed/modified dates across a stream write
/// (writing a stream otherwise counts as modifying the file).
struct KeepTimes(
    Option<(
        windows::Win32::Foundation::HANDLE,
        [windows::Win32::Foundation::FILETIME; 3],
    )>,
);

impl KeepTimes {
    fn capture(path: &Path) -> Self {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Foundation::FILETIME;
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
            FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES, GetFileTime, OPEN_EXISTING,
        };
        use windows::core::PCWSTR;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let handle = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                (FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES).0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        };
        let Ok(handle) = handle else {
            return KeepTimes(None);
        };
        let mut times = [FILETIME::default(); 3];
        let [created, accessed, written] = &mut times;
        if unsafe { GetFileTime(handle, Some(created), Some(accessed), Some(written)) }.is_err() {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
            }
            return KeepTimes(None);
        }
        KeepTimes(Some((handle, times)))
    }
}

impl Drop for KeepTimes {
    fn drop(&mut self) {
        if let Some((handle, [created, accessed, written])) = self.0.take() {
            unsafe {
                let _ = windows::Win32::Storage::FileSystem::SetFileTime(
                    handle,
                    Some(&created),
                    Some(&accessed),
                    Some(&written),
                );
                let _ = windows::Win32::Foundation::CloseHandle(handle);
            }
        }
    }
}

/// Stores `tags` with `path` (an empty list removes them).
pub fn write_tags(path: &Path, tags: &[PortableTag]) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if supports_streams(path) {
        let _times = KeepTimes::capture(path);
        let stream = stream_path(path);
        if tags.is_empty() {
            match std::fs::remove_file(&stream) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            }
        } else {
            let payload = StreamPayload {
                v: 1,
                tags: tags.to_vec(),
            };
            std::fs::write(&stream, serde_json::to_vec(&payload).unwrap_or_default())
        }
    } else {
        write_sidecar(path, tags)
    }
}

fn read_sidecar(dir: &Path) -> Sidecar {
    std::fs::read(dir.join(SIDECAR_NAME))
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

fn write_sidecar(path: &Path, tags: &[PortableTag]) -> std::io::Result<()> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Ok(());
    };
    let mut sidecar = read_sidecar(dir);
    let name = name.to_string_lossy().to_string();
    if tags.is_empty() {
        sidecar.files.remove(&name);
    } else {
        sidecar.files.insert(name, tags.to_vec());
    }
    let file = dir.join(SIDECAR_NAME);
    if sidecar.files.is_empty() {
        return match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    sidecar.v = 1;
    // A hidden file can't be overwritten by name with normal attributes:
    // clear the attribute first, then hide it again.
    set_hidden(&file, false);
    std::fs::write(
        &file,
        serde_json::to_vec_pretty(&sidecar).unwrap_or_default(),
    )?;
    set_hidden(&file, true);
    Ok(())
}

fn set_hidden(path: &Path, hidden: bool) {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL, SetFileAttributesW,
    };
    use windows::core::PCWSTR;
    if !path.exists() {
        return;
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let attributes = if hidden {
        FILE_ATTRIBUTE_HIDDEN
    } else {
        FILE_ATTRIBUTE_NORMAL
    };
    unsafe {
        let _ = SetFileAttributesW(PCWSTR(wide.as_ptr()), attributes);
    }
}

/// The tags stored with `path`, if any.
pub fn read_tags(path: &Path) -> Option<Vec<PortableTag>> {
    let data = std::fs::read(stream_path(path)).ok()?;
    let payload: StreamPayload = serde_json::from_slice(&data).ok()?;
    (!payload.tags.is_empty()).then_some(payload.tags)
}

/// Every item in `dir` that carries tags, except the paths in `known`
/// (already tagged on this PC). Run it off the UI thread.
pub fn read_folder(dir: &Path, known: &HashSet<PathBuf>) -> TagChanges {
    if supports_streams(dir) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .take(READ_FOLDER_MAX)
            .map(|e| e.path())
            .filter(|p| !known.contains(p))
            .filter_map(|p| read_tags(&p).map(|tags| (p, tags)))
            .collect()
    } else {
        read_sidecar(dir)
            .files
            .into_iter()
            .map(|(name, tags)| (dir.join(name), tags))
            .filter(|(p, tags)| !tags.is_empty() && !known.contains(p) && p.exists())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::indexer::TagGroupSnapshot;

    fn snapshot(groups: &[(&str, &[&str])]) -> TagsSnapshot {
        TagsSnapshot {
            version: 1,
            next_group_id: 10,
            groups: groups
                .iter()
                .enumerate()
                .map(|(i, (name, items))| TagGroupSnapshot {
                    id: i as u64 + 1,
                    name: name.to_string(),
                    color: [i as u8, 0, 0, 255],
                    items: items.iter().map(PathBuf::from).collect(),
                })
                .collect(),
        }
    }

    #[test]
    fn diff_lists_only_changed_paths() {
        let old = map_of(&snapshot(&[
            ("Work", &[r"C:\a.txt", r"C:\b.txt"]),
            ("Home", &[r"C:\b.txt"]),
        ]));
        let new = map_of(&snapshot(&[
            ("Work", &[r"C:\a.txt", r"C:\c.txt"]),
            ("Home", &[r"C:\b.txt"]),
        ]));
        let mut changes = diff(&old, &new);
        changes.sort_by(|a, b| a.0.cmp(&b.0));
        let names: Vec<(&str, Vec<&str>)> = changes
            .iter()
            .map(|(p, t)| {
                (
                    p.to_str().unwrap(),
                    t.iter().map(|t| t.name.as_str()).collect(),
                )
            })
            .collect();
        assert_eq!(
            names,
            [(r"C:\b.txt", vec!["Home"]), (r"C:\c.txt", vec!["Work"])]
        );
        let removed = diff(&new, &map_of(&snapshot(&[])));
        assert!(removed.iter().all(|(_, t)| t.is_empty()) && removed.len() == 3);
    }

    #[test]
    fn tags_round_trip_through_a_file() {
        let dir = std::env::temp_dir().join(format!("eden-portable-tags-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("photo.jpg");
        std::fs::write(&file, b"data").unwrap();
        let tags = vec![PortableTag {
            name: "Work".into(),
            color: [1, 2, 3, 255],
        }];
        write_tags(&file, &tags).unwrap();
        let found = read_folder(&dir, &HashSet::new());
        assert_eq!(found, vec![(file.clone(), tags)]);
        assert_eq!(std::fs::read(&file).unwrap(), b"data");
        assert!(read_folder(&dir, &HashSet::from([file.clone()])).is_empty());
        write_tags(&file, &[]).unwrap();
        assert!(read_folder(&dir, &HashSet::new()).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
