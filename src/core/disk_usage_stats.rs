//! Summaries of a Disk Usage scan (`core::disk_usage::DirNode`): space by
//! file type, space by age, and the filters the dashboard's lists share
//! (only files over a size, only one type, skip some folders).
//!
//! Everything here is a single pass over the tree already in memory - no
//! disk access - so it's cheap enough to redo whenever the results or the
//! filters change.

use crate::core::disk_usage::{DirNode, FileEntry};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Broad kinds of files, for the category filter and the colors shared by
/// the File Types list, the treemap, and the sunburst.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Video,
    Images,
    Audio,
    Documents,
    Archives,
    Programs,
    Code,
    System,
    Other,
}

impl Category {
    pub const ALL: [Category; 9] = [
        Category::Video,
        Category::Images,
        Category::Audio,
        Category::Documents,
        Category::Archives,
        Category::Programs,
        Category::Code,
        Category::System,
        Category::Other,
    ];

    pub fn i18n_key(self) -> &'static str {
        match self {
            Category::Video => "disk_usage_cat_video",
            Category::Images => "disk_usage_cat_images",
            Category::Audio => "disk_usage_cat_audio",
            Category::Documents => "disk_usage_cat_documents",
            Category::Archives => "disk_usage_cat_archives",
            Category::Programs => "disk_usage_cat_programs",
            Category::Code => "disk_usage_cat_code",
            Category::System => "disk_usage_cat_system",
            Category::Other => "disk_usage_cat_other",
        }
    }
}

/// A file's extension, lowercased, without the dot; empty for none. A name
/// that only starts with a dot (`.gitignore`) counts as having none.
pub fn extension_of(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() && !ext.contains(' ') => {
            ext.to_lowercase()
        }
        _ => String::new(),
    }
}

/// The category an extension (lowercase, no dot) belongs to.
pub fn category_of(ext: &str) -> Category {
    match ext {
        "mp4" | "mkv" | "mov" | "avi" | "wmv" | "webm" | "m4v" | "mpg" | "mpeg" | "3gp" | "flv"
        | "ts" | "m2ts" | "vob" => Category::Video,
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "tif" | "tiff" | "heic" | "heif"
        | "avif" | "raw" | "cr2" | "cr3" | "nef" | "arw" | "dng" | "psd" | "svg" | "ico"
        | "xcf" => Category::Images,
        "mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg" | "opus" | "wma" | "aiff" | "mid"
        | "midi" => Category::Audio,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp"
        | "txt" | "md" | "rtf" | "csv" | "epub" | "one" | "pst" | "ost" => Category::Documents,
        "zip" | "7z" | "rar" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "iso" | "img"
        | "cab" | "vhd" | "vhdx" | "wim" | "esd" | "bak" => Category::Archives,
        "exe" | "msi" | "msix" | "appx" | "dll" | "sys" | "ocx" | "com" | "scr" | "apk"
        | "jar" => Category::Programs,
        "rs" | "c" | "h" | "cpp" | "hpp" | "cs" | "java" | "py" | "js" | "mjs" | "jsx" | "tsx"
        | "go" | "rb" | "php" | "html" | "htm" | "css" | "scss" | "json" | "xml" | "yaml"
        | "yml" | "toml" | "sql" | "sh" | "ps1" | "bat" | "cmd" | "pdb" | "obj" | "o" | "lib"
        | "rlib" | "rmeta" | "class" | "pyc" => Category::Code,
        "tmp" | "log" | "etl" | "dmp" | "mdmp" | "cache" | "db" | "dat" | "bin" | "pf"
        | "evtx" | "cat" | "mui" | "manifest" | "blf" | "regtrans-ms" | "pak" => Category::System,
        _ => Category::Other,
    }
}

/// What the dashboard's lists include. The default includes everything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewFilter {
    /// Only files at least this big.
    pub min_size: u64,
    pub category: Option<Category>,
    /// Only files with this extension (lowercase, no dot; "" = none).
    pub extension: Option<String>,
    /// Folder names (wildcards allowed, `;`-separated like Select By
    /// Pattern) whose contents are skipped entirely.
    pub excluded_folders: String,
}

impl ViewFilter {
    pub fn is_active(&self) -> bool {
        *self != ViewFilter::default()
    }

    pub fn includes_file(&self, file: &FileEntry) -> bool {
        if file.size < self.min_size {
            return false;
        }
        if self.category.is_none() && self.extension.is_none() {
            return true;
        }
        let ext = extension_of(&file.name);
        self.extension.as_ref().is_none_or(|want| *want == ext)
            && self.category.is_none_or(|want| category_of(&ext) == want)
    }

    pub fn includes_folder(&self, name: &str) -> bool {
        self.excluded_folders.trim().is_empty()
            || !crate::core::pattern::matches_any_exact(&self.excluded_folders, name)
    }
}

/// Calls `visit(folder_path, folder, file)` for every file the filter
/// includes, skipping excluded folders (the root itself is never skipped).
pub fn for_each_file<'a>(
    root: &'a DirNode,
    root_path: &Path,
    filter: &ViewFilter,
    mut visit: impl FnMut(&Path, &'a DirNode, &'a FileEntry),
) {
    let mut stack: Vec<(&DirNode, PathBuf)> = vec![(root, root_path.to_path_buf())];
    while let Some((dir, path)) = stack.pop() {
        for file in &dir.files {
            if filter.includes_file(file) {
                visit(&path, dir, file);
            }
        }
        for child in &dir.dirs {
            if filter.includes_folder(&child.name) {
                stack.push((child, path.join(&*child.name)));
            }
        }
    }
}

/// Space taken by one extension.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeStat {
    /// Lowercase, no dot; empty for files without an extension.
    pub extension: String,
    pub category: Category,
    pub size: u64,
    pub allocated: u64,
    pub count: u64,
    /// The biggest file of this type.
    pub largest: Option<(PathBuf, u64)>,
}

/// Size and count per extension, largest first.
pub fn type_breakdown(root: &DirNode, root_path: &Path, filter: &ViewFilter) -> Vec<TypeStat> {
    let mut by_ext: HashMap<String, TypeStat> = HashMap::new();
    for_each_file(root, root_path, filter, |dir_path, _, file| {
        let ext = extension_of(&file.name);
        let stat = by_ext.entry(ext.clone()).or_insert_with(|| TypeStat {
            category: category_of(&ext),
            extension: ext,
            size: 0,
            allocated: 0,
            count: 0,
            largest: None,
        });
        stat.size += file.size;
        stat.allocated += file.allocated;
        stat.count += 1;
        if stat.largest.as_ref().is_none_or(|(_, size)| file.size > *size) {
            stat.largest = Some((dir_path.join(&*file.name), file.size));
        }
    });
    let mut stats: Vec<TypeStat> = by_ext.into_values().collect();
    stats.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.extension.cmp(&b.extension)));
    stats
}

/// Totals per category, in `Category::ALL` order (empty ones included).
pub fn category_totals(types: &[TypeStat]) -> Vec<(Category, u64, u64)> {
    Category::ALL
        .iter()
        .map(|&cat| {
            let (size, count) = types
                .iter()
                .filter(|t| t.category == cat)
                .fold((0, 0), |(s, c), t| (s + t.size, c + t.count));
            (cat, size, count)
        })
        .collect()
}

/// How long ago files were last modified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgeBucket {
    UnderMonth,
    Months1To6,
    Months6To12,
    Years1To3,
    Over3Years,
    Unknown,
}

impl AgeBucket {
    pub const ALL: [AgeBucket; 6] = [
        AgeBucket::UnderMonth,
        AgeBucket::Months1To6,
        AgeBucket::Months6To12,
        AgeBucket::Years1To3,
        AgeBucket::Over3Years,
        AgeBucket::Unknown,
    ];

    pub fn i18n_key(self) -> &'static str {
        match self {
            AgeBucket::UnderMonth => "disk_usage_age_month",
            AgeBucket::Months1To6 => "disk_usage_age_6_months",
            AgeBucket::Months6To12 => "disk_usage_age_year",
            AgeBucket::Years1To3 => "disk_usage_age_3_years",
            AgeBucket::Over3Years => "disk_usage_age_older",
            AgeBucket::Unknown => "disk_usage_age_unknown",
        }
    }

    /// The bucket for a file last modified at `modified` (FILETIME) when
    /// it's now `now` (FILETIME).
    pub fn of(modified: i64, now: i64) -> AgeBucket {
        const DAY: i64 = 24 * 60 * 60 * 10_000_000;
        if modified <= 0 {
            return AgeBucket::Unknown;
        }
        let age_days = (now - modified).max(0) / DAY;
        match age_days {
            0..30 => AgeBucket::UnderMonth,
            30..182 => AgeBucket::Months1To6,
            182..365 => AgeBucket::Months6To12,
            365..1096 => AgeBucket::Years1To3,
            _ => AgeBucket::Over3Years,
        }
    }
}

/// (bucket, size, count) for every bucket, in `AgeBucket::ALL` order.
pub fn age_breakdown(root: &DirNode, root_path: &Path, filter: &ViewFilter, now: i64) -> Vec<(AgeBucket, u64, u64)> {
    let mut totals = [(0u64, 0u64); 6];
    for_each_file(root, root_path, filter, |_, _, file| {
        let index = AgeBucket::ALL
            .iter()
            .position(|b| *b == AgeBucket::of(file.modified, now))
            .unwrap_or(5);
        totals[index].0 += file.size;
        totals[index].1 += 1;
    });
    AgeBucket::ALL
        .iter()
        .zip(totals)
        .map(|(bucket, (size, count))| (*bucket, size, count))
        .collect()
}

/// The current time as a FILETIME.
pub fn filetime_now() -> i64 {
    // FILETIME counts from 1601; Unix time from 1970.
    const UNIX_EPOCH_AS_FILETIME: i64 = 116_444_736_000_000_000;
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64 / 100)
        .unwrap_or(0);
    UNIX_EPOCH_AS_FILETIME + unix
}

/// The drive facts behind the dashboard's Drive Summary.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DriveInfo {
    pub total: u64,
    pub free: u64,
    pub cluster_size: u64,
    pub file_system: String,
    pub label: String,
}

/// Looks up the drive holding `root`.
pub fn drive_info(root: &Path) -> Option<DriveInfo> {
    use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceW, GetVolumeInformationW};
    let mut drive = root.ancestors().last()?.to_string_lossy().into_owned();
    if !drive.ends_with('\\') {
        drive.push('\\');
    }
    let wide = crate::core::fs::path_to_wide(Path::new(&drive));
    let pcw = windows::core::PCWSTR(wide.as_ptr());
    let (total, free) = crate::core::fs::get_drive_space(&PathBuf::from(&drive))?;
    let mut sectors_per_cluster = 0u32;
    let mut bytes_per_sector = 0u32;
    let mut free_clusters = 0u32;
    let mut total_clusters = 0u32;
    let mut label = [0u16; 128];
    let mut fs_name = [0u16; 64];
    unsafe {
        let _ = GetDiskFreeSpaceW(
            pcw,
            Some(&mut sectors_per_cluster),
            Some(&mut bytes_per_sector),
            Some(&mut free_clusters),
            Some(&mut total_clusters),
        );
        let _ = GetVolumeInformationW(pcw, Some(&mut label), None, None, None, Some(&mut fs_name));
    }
    let text = |buf: &[u16]| {
        let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    };
    Some(DriveInfo {
        total,
        free,
        cluster_size: sectors_per_cluster as u64 * bytes_per_sector as u64,
        file_system: text(&fs_name),
        label: text(&label),
    })
}

/// Space lost to "slack": the unused end of each file's last cluster.
/// Only files whose on-disk size is at least their logical size count
/// (compressed and sparse files take less than their size).
pub fn slack_space(root: &DirNode) -> u64 {
    let mut slack = 0u64;
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for file in &dir.files {
            slack += file.allocated.saturating_sub(file.size);
        }
        stack.extend(dir.dirs.iter());
    }
    slack
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, size: u64, modified: i64) -> FileEntry {
        FileEntry {
            name: name.into(),
            size,
            allocated: size.div_ceil(4096) * 4096,
            modified,
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

    const DAY: i64 = 24 * 60 * 60 * 10_000_000;
    const NOW: i64 = 134_000_000_000_000_000;

    fn sample() -> DirNode {
        dir(
            r"C:\x",
            vec![
                dir(
                    "node_modules",
                    vec![],
                    vec![file("big.js", 9000, NOW - 5 * DAY), file("pkg.JSON", 100, NOW)],
                ),
                dir(
                    "Videos",
                    vec![],
                    vec![file("a.MP4", 5000, NOW - 400 * DAY), file("b.mkv", 3000, NOW - 2000 * DAY)],
                ),
            ],
            vec![file("notes.txt", 10, NOW - 60 * DAY), file("Makefile", 20, 0), file(".gitignore", 5, NOW)],
        )
    }

    #[test]
    fn extensions_and_categories() {
        assert_eq!(extension_of("Photo.JPG"), "jpg");
        assert_eq!(extension_of("archive.tar.gz"), "gz");
        assert_eq!(extension_of("Makefile"), "");
        assert_eq!(extension_of(".gitignore"), "");
        assert_eq!(extension_of("trailing."), "");
        assert_eq!(extension_of("v1.2 final"), "");
        assert_eq!(category_of("mp4"), Category::Video);
        assert_eq!(category_of("xlsx"), Category::Documents);
        assert_eq!(category_of("weird"), Category::Other);
    }

    #[test]
    fn breakdown_groups_by_extension_ignoring_case() {
        let root = sample();
        let types = type_breakdown(&root, Path::new(r"C:\x"), &ViewFilter::default());
        let names: Vec<&str> = types.iter().map(|t| t.extension.as_str()).collect();
        assert_eq!(names, vec!["js", "mp4", "mkv", "json", "", "txt"]);
        let none = types.iter().find(|t| t.extension.is_empty()).unwrap();
        assert_eq!((none.count, none.size), (2, 25));
        assert_eq!(types[1].largest, Some((PathBuf::from(r"C:\x\Videos\a.MP4"), 5000)));
        let cats = category_totals(&types);
        assert_eq!(cats.iter().find(|c| c.0 == Category::Video).unwrap().1, 8000);
        assert_eq!(cats.len(), Category::ALL.len());
    }

    #[test]
    fn filters_skip_folders_small_files_and_other_types() {
        let root = sample();
        let path = Path::new(r"C:\x");
        let skip_modules = ViewFilter {
            excluded_folders: "node_*; .git".into(),
            ..Default::default()
        };
        let types = type_breakdown(&root, path, &skip_modules);
        assert!(types.iter().all(|t| t.extension != "js"));
        let big_video = ViewFilter {
            min_size: 4000,
            category: Some(Category::Video),
            ..Default::default()
        };
        let types = type_breakdown(&root, path, &big_video);
        assert_eq!(types.len(), 1);
        assert_eq!(types[0].extension, "mp4");
        let only_none = ViewFilter {
            extension: Some(String::new()),
            ..Default::default()
        };
        assert_eq!(type_breakdown(&root, path, &only_none)[0].count, 2);
        assert!(!ViewFilter::default().is_active());
        assert!(big_video.is_active());
    }

    #[test]
    fn ages_fall_into_buckets() {
        assert_eq!(AgeBucket::of(NOW - 3 * DAY, NOW), AgeBucket::UnderMonth);
        assert_eq!(AgeBucket::of(NOW - 90 * DAY, NOW), AgeBucket::Months1To6);
        assert_eq!(AgeBucket::of(NOW - 200 * DAY, NOW), AgeBucket::Months6To12);
        assert_eq!(AgeBucket::of(NOW - 800 * DAY, NOW), AgeBucket::Years1To3);
        assert_eq!(AgeBucket::of(NOW - 1200 * DAY, NOW), AgeBucket::Over3Years);
        assert_eq!(AgeBucket::of(0, NOW), AgeBucket::Unknown);
        assert_eq!(AgeBucket::of(NOW + DAY, NOW), AgeBucket::UnderMonth, "future dates");

        let ages = age_breakdown(&sample(), Path::new(r"C:\x"), &ViewFilter::default(), NOW);
        let size_of = |b: AgeBucket| ages.iter().find(|a| a.0 == b).unwrap().1;
        assert_eq!(size_of(AgeBucket::UnderMonth), 9000 + 100 + 5);
        assert_eq!(size_of(AgeBucket::Months1To6), 10);
        assert_eq!(size_of(AgeBucket::Years1To3), 5000);
        assert_eq!(size_of(AgeBucket::Over3Years), 3000);
        assert_eq!(size_of(AgeBucket::Unknown), 20);
    }

    #[test]
    fn slack_is_the_unused_end_of_clusters() {
        let root = dir("r", vec![], vec![file("a", 1, 0), file("b", 4096, 0)]);
        assert_eq!(slack_space(&root), 4095);
    }

    #[test]
    fn now_is_after_2020() {
        assert!(filetime_now() > 132_000_000_000_000_000);
    }
}
