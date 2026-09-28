//! Browsing into an archive like a folder: a path such as
//! `C:\Downloads\photos.zip\2024\June` names a folder inside `photos.zip`.
//! The listing comes from `core::extract::list` (read once per archive and
//! kept, see `listing`), opening a file extracts just that file to a temp
//! folder, and the view is read-only (extract to change anything).

use crate::core::extract::{ArchiveItem, ArchiveKind};
use crate::core::fs::{DateStyle, FileItem, filetime_to_string};
use crossbeam_channel::Sender;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Shown instead of playing a video or song that's inside an archive.
pub const MEDIA_IN_ARCHIVE: &str = "This file is inside an archive - open it (double-click) or extract it to play it.";

/// Tar and 7z archives up to this size are extracted whole the first time
/// a file inside is opened, previewed or thumbnailed (see
/// `extract_for_open`).
const WHOLE_EXTRACT_LIMIT: u64 = 1024 * 1024 * 1024;

/// Archives whose listing is kept in memory.
const CACHED_ARCHIVES: usize = 8;

/// Whether `name` looks like an archive that can be browsed (not one only
/// 7-Zip reads, and not a single compressed file).
fn browsable_name(name: &str) -> bool {
    ArchiveKind::of(name).is_some_and(|(kind, _)| kind != ArchiveKind::External && kind.is_multi())
}

/// A browsable archive file (for "double-click opens it like a folder").
pub fn is_browsable_archive(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(browsable_name) && path.is_file()
}

/// Splits a path inside an archive into the archive file and the parts
/// below it (empty for the archive's top level). Cheap for ordinary
/// paths: the file system is only asked when a component is named like an
/// archive.
pub fn split(path: &Path) -> Option<(PathBuf, Vec<String>)> {
    let names_archive = path.components().any(|c| c.as_os_str().to_str().is_some_and(browsable_name));
    if !names_archive {
        return None;
    }
    for ancestor in path.ancestors() {
        let Some(name) = ancestor.file_name().and_then(|n| n.to_str()) else { continue };
        if browsable_name(name) && ancestor.is_file() {
            let inner = path
                .strip_prefix(ancestor)
                .ok()?
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            return Some((ancestor.to_path_buf(), inner));
        }
    }
    None
}

/// True for a path inside an archive (not the archive file itself).
pub fn is_inside_archive(path: &Path) -> bool {
    split(path).is_some_and(|(archive, _)| archive != path)
}

type Cache = HashMap<PathBuf, (SystemTime, u64, Arc<Vec<ArchiveItem>>, u64)>;
static CACHE: Mutex<Option<(Cache, u64)>> = Mutex::new(None);

/// The archive's entries, read once and kept until the file changes (at
/// most `CACHED_ARCHIVES` archives, least recently used dropped first).
pub fn listing(archive: &Path) -> Result<Arc<Vec<ArchiveItem>>, String> {
    let meta = std::fs::metadata(archive).map_err(|e| e.to_string())?;
    let stamp = (meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), meta.len());
    if let Ok(mut guard) = CACHE.lock() {
        let (cache, tick) = guard.get_or_insert_with(|| (HashMap::new(), 0));
        *tick += 1;
        let now = *tick;
        if let Some(entry) = cache.get_mut(archive)
            && (entry.0, entry.1) == stamp
        {
            entry.3 = now;
            return Ok(entry.2.clone());
        }
    }
    let items = Arc::new(crate::core::extract::list(archive)?);
    if let Ok(mut guard) = CACHE.lock() {
        let (cache, tick) = guard.get_or_insert_with(|| (HashMap::new(), 0));
        if cache.len() >= CACHED_ARCHIVES
            && let Some(oldest) = cache.iter().min_by_key(|(_, v)| v.3).map(|(k, _)| k.clone())
        {
            cache.remove(&oldest);
        }
        cache.insert(archive.to_path_buf(), (stamp.0, stamp.1, items.clone(), *tick));
    }
    Ok(items)
}

/// The entries directly inside `inner` (a folder in the archive; empty =
/// the top level).
pub fn children<'a>(items: &'a [ArchiveItem], inner: &[String]) -> Vec<&'a ArchiveItem> {
    let prefix = inner.join("/");
    items
        .iter()
        .filter(|item| {
            let rest = if prefix.is_empty() {
                Some(item.path.as_str())
            } else {
                item.path.strip_prefix(&prefix).and_then(|r| r.strip_prefix('/'))
            };
            rest.is_some_and(|r| !r.is_empty() && !r.contains('/'))
        })
        .collect()
}

/// The total size of the files under each folder directly inside `inner`,
/// by folder name.
pub fn subfolder_sizes<'a>(items: &'a [ArchiveItem], inner: &[String]) -> HashMap<&'a str, u64> {
    let prefix = inner.join("/");
    let mut sizes: HashMap<&str, u64> = HashMap::new();
    for item in items.iter().filter(|i| !i.is_dir) {
        let rest = if prefix.is_empty() {
            Some(item.path.as_str())
        } else {
            item.path.strip_prefix(prefix.as_str()).and_then(|r| r.strip_prefix('/'))
        };
        if let Some((folder, _)) = rest.and_then(|r| r.split_once('/')) {
            *sizes.entry(folder).or_default() += item.size;
        }
    }
    sizes
}

fn to_filetime(time: SystemTime) -> Option<i64> {
    let since = time.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some(since.as_secs() as i64 * 10_000_000 + since.subsec_nanos() as i64 / 100 + 116_444_736_000_000_000)
}

/// Lists the folder `path` (inside an archive) on a background thread,
/// sending a `FileItem` per entry like a normal folder scan. A failure is
/// sent back as the returned message.
pub fn list_folder_async(
    path: PathBuf,
    tx: Sender<FileItem>,
    error: Arc<Mutex<Option<String>>>,
    date_style: DateStyle,
    time_format_24h: bool,
    custom_date_format: String,
) {
    std::thread::spawn(move || {
        let Some((archive, inner)) = split(&path) else { return };
        let items = match listing(&archive) {
            Ok(items) => items,
            Err(e) => {
                if let Ok(mut slot) = error.lock() {
                    *slot = Some(e);
                }
                return;
            }
        };
        // Each subfolder's size (everything under it), in one pass.
        let folder_sizes = subfolder_sizes(&items, &inner);
        for item in children(&items, &inner) {
            let name = item.path.rsplit('/').next().unwrap_or(&item.path).to_string();
            let size = if item.is_dir { folder_sizes.get(name.as_str()).copied().unwrap_or(0) } else { item.size };
            let raw = item.modified.and_then(to_filetime);
            let text = raw.and_then(|ft| filetime_to_string(ft, date_style, time_format_24h, &custom_date_format));
            let file = FileItem::new(
                name.clone(),
                path.join(&name),
                item.is_dir,
                false,
                None,
                Some(size),
                text,
                None,
                None,
                raw,
                None,
                None,
                None,
            );
            if tx.send(file).is_err() {
                return;
            }
        }
        drop(tx);
        crate::gui::windows::windowsoverrides::request_repaint();
    });
}

/// Where files opened from inside archives are extracted to.
pub fn temp_root() -> PathBuf {
    std::env::temp_dir().join("EdenExplorer Archives")
}

/// Extracts one file from inside an archive to a temporary folder (one per
/// archive) and returns its path there, reusing an earlier extraction of
/// the same, unchanged archive.
pub fn extract_for_open(path: &Path) -> Result<PathBuf, String> {
    let (archive, inner) = split(path).ok_or("not inside an archive")?;
    if inner.is_empty() {
        return Err("not inside an archive".into());
    }
    let meta = std::fs::metadata(&archive).map_err(|e| e.to_string())?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&archive, &mut hasher);
    std::hash::Hash::hash(&meta.len(), &mut hasher);
    std::hash::Hash::hash(&meta.modified().ok(), &mut hasher);
    let folder = temp_root().join(format!("{:016x}", std::hash::Hasher::finish(&hasher)));
    let mut dir = folder.clone();
    for part in &inner[..inner.len() - 1] {
        dir.push(part);
    }
    let target = dir.join(&inner[inner.len() - 1]);
    if target.exists() {
        return Ok(target);
    }
    // One extraction at a time: thumbnails and previews ask from several
    // threads, often for the same archive.
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock();
    if target.exists() {
        return Ok(target);
    }
    let (tx, _rx) = crossbeam_channel::unbounded();
    // Tar and 7z can't jump to one entry: getting any file means
    // decompressing everything before it, so a Gallery of N files would
    // decompress the archive N times. Those are extracted whole, once
    // (when not huge), and every later file is already there.
    let name = archive.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let random_access = ArchiveKind::of(name).is_some_and(|(kind, _)| kind == ArchiveKind::Zip);
    let total: u64 = listing(&archive).map(|items| items.iter().map(|i| i.size).sum()).unwrap_or(u64::MAX);
    if !random_access && total <= WHOLE_EXTRACT_LIMIT {
        let marker = folder.join(".eden-complete");
        if !marker.exists() {
            std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            crate::core::extract::extract(
                &archive,
                &crate::core::extract::Target::Into(folder.clone()),
                None,
                &tx,
                &std::sync::atomic::AtomicBool::new(false),
            )?;
            let _ = std::fs::write(&marker, b"");
        }
        return if target.exists() { Ok(target) } else { Err("the file wasn't found in the archive".into()) };
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let parts = crate::core::extract::safe_parts(&inner.join("/")).ok_or("unsafe name")?;
    crate::core::extract::extract(
        &archive,
        &crate::core::extract::Target::Into(dir),
        Some(vec![parts]),
        &tx,
        &std::sync::atomic::AtomicBool::new(false),
    )?;
    if target.exists() { Ok(target) } else { Err("the file wasn't found in the archive".into()) }
}

/// A path the file system can read for `path`: itself, or for a file
/// inside an archive, a temporary extracted copy.
pub fn readable_path(path: &Path) -> Result<PathBuf, String> {
    if is_inside_archive(path) {
        extract_for_open(path)
    } else {
        Ok(path.to_path_buf())
    }
}

/// Removes files extracted for opening (at startup; ones still open are
/// skipped).
pub fn clean_temp() {
    let _ = std::fs::remove_dir_all(temp_root());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(path: &Path) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, data) in [("top.txt", "t"), ("docs/a.txt", "a"), ("docs/deep/b.txt", "b")] {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(data.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn paths_inside_archives_split_and_list() {
        let dir = std::env::temp_dir().join(format!("eden-archive-view-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("pack.zip");
        make_zip(&archive);

        assert_eq!(split(&dir.join("plain")), None);
        assert_eq!(split(&archive), Some((archive.clone(), vec![])));
        assert_eq!(split(&archive.join("docs").join("deep")), Some((archive.clone(), vec!["docs".into(), "deep".into()])));
        assert!(!is_inside_archive(&archive));
        assert!(is_inside_archive(&archive.join("docs")));
        assert!(is_browsable_archive(&archive));

        let items = listing(&archive).unwrap();
        let top: Vec<&str> = children(&items, &[]).iter().map(|i| i.path.as_str()).collect();
        assert_eq!(top, vec!["docs", "top.txt"]);
        let docs: Vec<&str> = children(&items, &["docs".into()]).iter().map(|i| i.path.as_str()).collect();
        assert_eq!(docs, vec!["docs/a.txt", "docs/deep"]);
        let sizes = subfolder_sizes(&items, &[]);
        assert_eq!(sizes.get("docs"), Some(&2), "a.txt + deep/b.txt");
        assert_eq!(subfolder_sizes(&items, &["docs".into()]).get("deep"), Some(&1));
        // Cached: the same list comes back.
        assert!(Arc::ptr_eq(&items, &listing(&archive).unwrap()));

        let opened = extract_for_open(&archive.join("docs").join("deep").join("b.txt")).unwrap();
        assert_eq!(std::fs::read_to_string(&opened).unwrap(), "b");
        assert!(opened.starts_with(temp_root()));

        // A tar.gz: the first file opened extracts the whole archive once.
        let tar = crate::core::extract::tests::make_tar(&[("src/", b""), ("src/one.rs", b"1"), ("src/two.rs", b"2")]);
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar).unwrap();
        let tgz = dir.join("code.tar.gz");
        std::fs::write(&tgz, gz.finish().unwrap()).unwrap();
        let one = extract_for_open(&tgz.join("src").join("one.rs")).unwrap();
        assert_eq!(std::fs::read_to_string(&one).unwrap(), "1");
        let two_path = one.with_file_name("two.rs");
        assert!(two_path.exists(), "the rest came out with it");
        assert!(one.parent().unwrap().parent().unwrap().join(".eden-complete").exists());
        assert_eq!(extract_for_open(&tgz.join("src").join("two.rs")).unwrap(), two_path);

        let (tx, rx) = crossbeam_channel::unbounded();
        let error = Arc::new(Mutex::new(None));
        list_folder_async(archive.join("docs"), tx, error, DateStyle::default(), true, String::new());
        let mut names: Vec<(String, bool)> = rx.iter().map(|f| (f.name, f.is_dir)).collect();
        names.sort();
        assert_eq!(names, vec![("a.txt".into(), false), ("deep".into(), true)]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
