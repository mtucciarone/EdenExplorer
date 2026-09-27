//! Disk Usage snapshots: a finished scan saved to disk so a later scan of
//! the same folder can be compared against it ("what grew since last
//! week?").
//!
//! Each snapshot is two files in `<data dir>/disk_usage_snapshots/`: a
//! small JSON description (`<id>.json`, read to list snapshots) and the
//! gzipped postcard tree (`<id>.snap`, read only when comparing).

use crate::core::disk_usage::DirNode;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

const FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SnapshotInfo {
    pub id: String,
    pub root: PathBuf,
    /// Unix seconds.
    pub taken: i64,
    pub size: u64,
    pub allocated: u64,
    pub file_count: u64,
    pub dir_count: u64,
    #[serde(default)]
    pub version: u32,
}

pub fn snapshot_dir() -> Option<PathBuf> {
    crate::core::app_data::data_dir().map(|d| d.join("disk_usage_snapshots"))
}

/// Case-insensitive path comparison, the way Windows compares them.
pub fn same_root(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase();
    norm(a) == norm(b)
}

pub fn save_to(dir: &Path, tree: &DirNode, root: &Path, taken: i64) -> io::Result<SnapshotInfo> {
    std::fs::create_dir_all(dir)?;
    let base = chrono::DateTime::from_timestamp(taken, 0)
        .map(|d| d.format("%Y%m%d-%H%M%S").to_string())
        .unwrap_or_else(|| taken.to_string());
    let mut id = base.clone();
    let mut n = 1;
    while dir.join(format!("{id}.json")).exists() || dir.join(format!("{id}.snap")).exists() {
        n += 1;
        id = format!("{base}-{n}");
    }
    let info = SnapshotInfo {
        id: id.clone(),
        root: root.to_path_buf(),
        taken,
        size: tree.size,
        allocated: tree.allocated,
        file_count: tree.file_count,
        dir_count: tree.dir_count,
        version: FORMAT_VERSION,
    };
    let bytes = postcard::to_allocvec(tree).map_err(io::Error::other)?;
    let file = std::fs::File::create(dir.join(format!("{id}.snap")))?;
    let mut gz = flate2::write::GzEncoder::new(io::BufWriter::new(file), flate2::Compression::fast());
    gz.write_all(&bytes)?;
    gz.finish()?.flush()?;
    // The description goes last: a snapshot without one isn't listed, so a
    // half-written tree never shows up.
    std::fs::write(
        dir.join(format!("{id}.json")),
        serde_json::to_vec_pretty(&info).map_err(io::Error::other)?,
    )?;
    Ok(info)
}

pub fn save(tree: &DirNode, root: &Path) -> io::Result<SnapshotInfo> {
    let dir = snapshot_dir().ok_or_else(|| io::Error::other("no data folder"))?;
    save_to(&dir, tree, root, chrono::Utc::now().timestamp())
}

/// Snapshots in `dir`, newest first.
pub fn list_in(dir: &Path) -> Vec<SnapshotInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut list: Vec<SnapshotInfo> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok())
        .filter(|info: &SnapshotInfo| dir.join(format!("{}.snap", info.id)).is_file())
        .collect();
    list.sort_by(|a, b| b.taken.cmp(&a.taken).then_with(|| b.id.cmp(&a.id)));
    list
}

pub fn list() -> Vec<SnapshotInfo> {
    snapshot_dir().map(|d| list_in(&d)).unwrap_or_default()
}

pub fn load_from(dir: &Path, id: &str) -> io::Result<DirNode> {
    let file = std::fs::File::open(dir.join(format!("{id}.snap")))?;
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(io::BufReader::new(file)).read_to_end(&mut bytes)?;
    postcard::from_bytes(&bytes).map_err(io::Error::other)
}

pub fn load(id: &str) -> io::Result<DirNode> {
    let dir = snapshot_dir().ok_or_else(|| io::Error::other("no data folder"))?;
    load_from(&dir, id)
}

pub fn delete_in(dir: &Path, id: &str) -> io::Result<()> {
    std::fs::remove_file(dir.join(format!("{id}.json")))?;
    let _ = std::fs::remove_file(dir.join(format!("{id}.snap")));
    Ok(())
}

pub fn delete(id: &str) -> io::Result<()> {
    let dir = snapshot_dir().ok_or_else(|| io::Error::other("no data folder"))?;
    delete_in(&dir, id)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Removed,
    Grew,
    Shrank,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub path: PathBuf,
    pub is_dir: bool,
    pub kind: ChangeKind,
    pub before: u64,
    pub after: u64,
}

impl Change {
    pub fn delta(&self) -> i64 {
        self.after as i64 - self.before as i64
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Comparison {
    pub before_size: u64,
    pub after_size: u64,
    pub before_files: u64,
    pub after_files: u64,
    /// Every folder whose total changed, biggest change first.
    pub folders: Vec<Change>,
    /// Every file that appeared, vanished or changed size, biggest change
    /// first.
    pub files: Vec<Change>,
}

fn kind(before: Option<u64>, after: Option<u64>) -> Option<ChangeKind> {
    match (before, after) {
        (None, Some(_)) => Some(ChangeKind::Added),
        (Some(_), None) => Some(ChangeKind::Removed),
        (Some(b), Some(a)) if a > b => Some(ChangeKind::Grew),
        (Some(b), Some(a)) if a < b => Some(ChangeKind::Shrank),
        _ => None,
    }
}

fn key(name: &str) -> String {
    name.to_lowercase()
}

fn walk(before: Option<&DirNode>, after: Option<&DirNode>, path: &Path, out: &mut Comparison) {
    if let Some(k) = kind(before.map(|d| d.size), after.map(|d| d.size)) {
        out.folders.push(Change {
            path: path.to_path_buf(),
            is_dir: true,
            kind: k,
            before: before.map_or(0, |d| d.size),
            after: after.map_or(0, |d| d.size),
        });
    } else {
        // Same total: nothing below can have changed size in a way worth
        // reporting... except files swapped for equal-sized ones, which
        // aren't worth the walk.
        return;
    }

    let mut files: HashMap<String, (Option<u64>, Option<u64>, &str)> = HashMap::new();
    for f in before.map(|d| d.files.as_slice()).unwrap_or_default() {
        files.entry(key(&f.name)).or_insert((None, None, &f.name)).0 = Some(f.size);
    }
    for f in after.map(|d| d.files.as_slice()).unwrap_or_default() {
        let e = files.entry(key(&f.name)).or_insert((None, None, &f.name));
        e.1 = Some(f.size);
        e.2 = &f.name;
    }
    for (b, a, name) in files.into_values() {
        if let Some(k) = kind(b, a) {
            out.files.push(Change {
                path: path.join(name),
                is_dir: false,
                kind: k,
                before: b.unwrap_or(0),
                after: a.unwrap_or(0),
            });
        }
    }

    let mut dirs: HashMap<String, (Option<&DirNode>, Option<&DirNode>)> = HashMap::new();
    for d in before.map(|d| d.dirs.as_slice()).unwrap_or_default() {
        dirs.entry(key(&d.name)).or_default().0 = Some(d);
    }
    for d in after.map(|d| d.dirs.as_slice()).unwrap_or_default() {
        dirs.entry(key(&d.name)).or_default().1 = Some(d);
    }
    for (b, a) in dirs.into_values() {
        let name = a.or(b).map(|d| &*d.name).unwrap_or_default();
        walk(b, a, &path.join(name), out);
    }
}

/// What changed between an older scan and a newer one of the same folder.
pub fn compare(before: &DirNode, after: &DirNode, root: &Path) -> Comparison {
    let mut out = Comparison {
        before_size: before.size,
        after_size: after.size,
        before_files: before.file_count,
        after_files: after.file_count,
        ..Default::default()
    };
    walk(Some(before), Some(after), root, &mut out);
    let by_delta = |a: &Change, b: &Change| {
        b.delta().unsigned_abs().cmp(&a.delta().unsigned_abs()).then_with(|| a.path.cmp(&b.path))
    };
    out.folders.sort_by(by_delta);
    out.files.sort_by(by_delta);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::disk_usage::FileEntry;

    fn file(name: &str, size: u64) -> FileEntry {
        FileEntry { name: name.into(), size, allocated: size, modified: 0 }
    }

    fn dir(name: &str, dirs: Vec<DirNode>, files: Vec<FileEntry>) -> DirNode {
        let mut d = DirNode { name: name.into(), dirs, files, ..Default::default() };
        d.recompute_totals();
        d.sort_children();
        d
    }

    #[test]
    fn compare_finds_added_removed_and_resized() {
        let before = dir(
            "R",
            vec![dir("Keep", vec![], vec![file("same", 5)]), dir("Old", vec![], vec![file("x", 50)]), dir("Grow", vec![], vec![file("g", 10), file("gone", 3)])],
            vec![file("top", 1)],
        );
        let after = dir(
            "R",
            vec![dir("keep", vec![], vec![file("same", 5)]), dir("New", vec![], vec![file("y", 70)]), dir("Grow", vec![], vec![file("g", 100)])],
            vec![file("top", 1)],
        );
        let cmp = compare(&before, &after, Path::new(r"C:\R"));
        assert_eq!(cmp.before_size, 69);
        assert_eq!(cmp.after_size, 176);

        let folders: Vec<(String, ChangeKind, i64)> =
            cmp.folders.iter().map(|c| (c.path.display().to_string(), c.kind, c.delta())).collect();
        assert_eq!(folders[0], (r"C:\R".to_string(), ChangeKind::Grew, 107));
        assert!(folders.contains(&(r"C:\R\Grow".to_string(), ChangeKind::Grew, 87)));
        assert!(folders.contains(&(r"C:\R\New".to_string(), ChangeKind::Added, 70)));
        assert!(folders.contains(&(r"C:\R\Old".to_string(), ChangeKind::Removed, -50)));
        assert!(!folders.iter().any(|f| f.0.to_lowercase().ends_with("keep")), "case-insensitive match");

        let files: Vec<(String, ChangeKind)> =
            cmp.files.iter().map(|c| (c.path.display().to_string(), c.kind)).collect();
        assert_eq!(files[0], (r"C:\R\Grow\g".to_string(), ChangeKind::Grew));
        assert!(files.contains(&(r"C:\R\Grow\gone".to_string(), ChangeKind::Removed)));
        assert!(files.contains(&(r"C:\R\New\y".to_string(), ChangeKind::Added)));
        assert!(files.contains(&(r"C:\R\Old\x".to_string(), ChangeKind::Removed)));
        assert_eq!(files.len(), 4);
    }

    #[test]
    fn snapshots_save_list_load_and_delete() {
        let tmp = std::env::temp_dir().join(format!("eden-snap-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let tree = dir("R", vec![dir("A", vec![], vec![file("a", 7)])], vec![file("b", 3)]);
        let first = save_to(&tmp, &tree, Path::new(r"C:\R"), 1_700_000_000).unwrap();
        let second = save_to(&tmp, &tree, Path::new(r"C:\R"), 1_700_000_000).unwrap();
        assert_ne!(first.id, second.id, "same second gets a distinct id");
        let third = save_to(&tmp, &tree, Path::new(r"C:\R"), 1_800_000_000).unwrap();

        let listed = list_in(&tmp);
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0], third);
        assert_eq!(listed[0].size, 10);
        assert_eq!(load_from(&tmp, &first.id).unwrap(), tree);

        delete_in(&tmp, &first.id).unwrap();
        assert_eq!(list_in(&tmp).len(), 2);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn roots_compare_case_insensitively() {
        assert!(same_root(Path::new(r"C:\Users\"), Path::new(r"c:\users")));
        assert!(!same_root(Path::new(r"C:\Users"), Path::new(r"C:\Users2")));
    }
}
