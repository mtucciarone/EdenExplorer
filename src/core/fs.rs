use crate::core::network;
use crate::core::portable;
use chrono::{DateTime, Local, TimeZone, Utc};
use crossbeam_channel::Sender;
use ntapi::ntioapi::{FILE_DIRECTORY_INFORMATION, IO_STATUS_BLOCK, NtQueryDirectoryFile};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Foundation::{FILETIME, PROPERTYKEY};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_HIDDEN, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_LIST_DIRECTORY, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetDiskFreeSpaceExW,
    OPEN_EXISTING,
};
use windows::Win32::UI::Shell::PropertiesSystem::PSGetPropertyKeyFromName;
use windows::Win32::UI::Shell::{IShellItem, IShellItem2};
use windows::core::w;
use windows::core::{GUID, Interface, PCWSTR};

const STATUS_NO_MORE_FILES: i32 = 0x80000006u32 as i32;
static RECYCLE_DATE_DELETED: OnceLock<PROPERTYKEY> = OnceLock::new();
static RECYCLE_NAME: OnceLock<PROPERTYKEY> = OnceLock::new();
static RECYCLE_ORIGINAL_DIRECTORY: OnceLock<PROPERTYKEY> = OnceLock::new();
pub const MY_PC_PATH: &str = "::MY_PC::";
pub const MY_RECYCLE_BIN_PATH: &str = "::RECYCLE_BIN::";
pub const SETTINGS_PATH: &str = "::SETTINGS::";
const TAG_VIEW_PREFIX: &str = "::TAG::";

/// The sentinel "path" a tab navigates to in order to show the virtual
/// tagged-items list for one tag group, the same trick used for the
/// This-PC/Recycle-Bin/Settings virtual views.
pub fn tag_view_path(group_id: u64) -> PathBuf {
    PathBuf::from(format!("{TAG_VIEW_PREFIX}{group_id}"))
}

/// If `path` is a tag-view sentinel, returns the tag group id it names.
pub fn parse_tag_view_path(path: &Path) -> Option<u64> {
    path.to_string_lossy()
        .strip_prefix(TAG_VIEW_PREFIX)?
        .parse()
        .ok()
}

const SEARCH_VIEW_PREFIX: &str = "::SEARCH::";

#[derive(Serialize, Deserialize)]
struct SearchViewPayload {
    query: String,
    /// `None` means "search everywhere"; `Some(dir)` scopes the search to
    /// that folder (and its subfolders).
    scope_folder: Option<PathBuf>,
}

/// The sentinel "path" a tab navigates to in order to show Everything search
/// results for `query`/`scope_folder` - same virtual-view trick as
/// `tag_view_path`, but the payload (arbitrary query text, plus an optional
/// scope folder) can't be embedded as plain text the way a tag group's `u64`
/// id can, so it's postcard-serialized and hex-encoded into the path string
/// instead. This is never touched by real filesystem APIs (like every other
/// sentinel here), so there's no need for the encoding to be a filesystem-
/// legal path - just reversible and delimiter-free.
pub fn search_view_path(query: &str, scope_folder: Option<&Path>) -> PathBuf {
    let payload = SearchViewPayload {
        query: query.to_string(),
        scope_folder: scope_folder.map(Path::to_path_buf),
    };
    let bytes = postcard::to_allocvec(&payload).unwrap_or_default();
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    PathBuf::from(format!("{SEARCH_VIEW_PREFIX}{hex}"))
}

/// If `path` is a search-view sentinel, returns its `(query, scope_folder)`.
pub fn parse_search_view_path(path: &Path) -> Option<(String, Option<PathBuf>)> {
    let hex = path.to_string_lossy().strip_prefix(SEARCH_VIEW_PREFIX)?.to_string();
    if hex.len() % 2 != 0 {
        return None;
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        bytes.push(u8::from_str_radix(&hex[i..i + 2], 16).ok()?);
    }
    let (payload, _): (SearchViewPayload, _) = postcard::take_from_bytes(&bytes).ok()?;
    Some((payload.query, payload.scope_folder))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DateStyle {
    Iso,
    /// Short numeric date. Deliberately dd/mm/yyyy, not the US m/d/yyyy the
    /// variant name suggests - the name is kept as-is since it's persisted
    /// via postcard by variant name and renaming it would need a migration
    /// for no user-visible benefit.
    UsShort,
    Long,
    /// User-supplied `chrono` strftime pattern (see `custom_date_format` in
    /// settings) - falls back to `UsShort`'s format if the pattern is empty
    /// or fails to format, so a bad/incomplete pattern never shows nothing.
    Custom,
}

impl Default for DateStyle {
    fn default() -> Self {
        DateStyle::UsShort
    }
}

const DEFAULT_SHORT_DATE_FMT: &str = "%d/%m/%Y";

pub fn filetime_to_string(
    filetime: i64,
    date_style: DateStyle,
    time_format_24h: bool,
    custom_date_format: &str,
) -> Option<String> {
    if filetime == 0 {
        return None;
    }

    let unix_time = (filetime / 10_000_000) - 11_644_473_600;

    if unix_time <= 0 {
        return None;
    }

    let dt_utc = Utc.timestamp_opt(unix_time, 0).single()?;
    let dt_local: DateTime<Local> = dt_utc.into();

    let time_fmt = if time_format_24h {
        "%H:%M"
    } else {
        "%-I:%M %p"
    };

    if date_style == DateStyle::Custom && !custom_date_format.trim().is_empty() {
        // A custom pattern may already embed its own time portion (or none
        // at all) - honor exactly what the user typed instead of always
        // appending the 12h/24h time format after it.
        let formatted = format_with_pattern(&dt_local, custom_date_format);
        if let Some(formatted) = formatted {
            return Some(formatted);
        }
    }

    let date_fmt = match date_style {
        DateStyle::Iso => "%Y-%m-%d",
        DateStyle::UsShort | DateStyle::Custom => DEFAULT_SHORT_DATE_FMT,
        DateStyle::Long => "%B %-d, %Y",
    };

    Some(
        dt_local
            .format(&format!("{date_fmt} {time_fmt}"))
            .to_string(),
    )
}

/// Renders the current moment with a custom pattern, for a live preview next
/// to the pattern text field in Settings. Returns `None` if the pattern is
/// empty or invalid, so the caller can show "invalid pattern" instead of a
/// silently-wrong preview.
pub fn preview_custom_date_format(pattern: &str) -> Option<String> {
    if pattern.trim().is_empty() {
        return None;
    }
    format_with_pattern(&Local::now(), pattern)
}

/// Applies a user-typed `chrono` strftime pattern, returning `None` if the
/// pattern contains a specifier chrono can't parse (`StrftimeItems` surfaces
/// that as an error item rather than panicking) - the caller falls back to
/// the default short format in that case, so a malformed custom pattern
/// degrades gracefully instead of showing garbled or missing dates.
fn format_with_pattern(dt_local: &DateTime<Local>, pattern: &str) -> Option<String> {
    use chrono::format::{Item, StrftimeItems};

    if StrftimeItems::new(pattern).any(|item| matches!(item, Item::Error)) {
        return None;
    }

    Some(dt_local.format(pattern).to_string())
}

/// Convert PathBuf -> UTF-16
fn path_to_wide(path: &Path) -> Vec<u16> {
    let mut w: Vec<u16> = path.as_os_str().encode_wide().collect();
    w.push(0);
    w
}

/// Open directory handle
fn open_directory_handle(path: &PathBuf) -> Option<HANDLE> {
    let wide = path_to_wide(path);
    let pcw = PCWSTR(wide.as_ptr());

    unsafe {
        match CreateFileW(
            pcw,
            FILE_LIST_DIRECTORY.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        ) {
            Ok(handle) => Some(handle),
            Err(_) => None,
        }
    }
}

/// Get drive space
pub fn get_drive_space(path: &PathBuf) -> Option<(u64, u64)> {
    let wide = path_to_wide(path);
    let pcw = PCWSTR(wide.as_ptr());

    unsafe {
        let mut free: u64 = 0;
        let mut total: u64 = 0;
        let mut _total_free: u64 = 0;

        let res = GetDiskFreeSpaceExW(
            pcw,
            Some(&mut free),
            Some(&mut total),
            Some(&mut _total_free),
        );

        if res.is_ok() {
            Some((total, free))
        } else {
            None
        }
    }
}

/// 🚀 FAST NT-based folder size calculation
#[allow(dead_code)]
pub fn calculate_folder_size_fast(path: PathBuf) -> u64 {
    let mut total_size = 0u64;
    let mut stack = vec![path];

    // Reuse buffer (good)
    let mut buffer = vec![0u8; 256 * 1024];

    while let Some(dir) = stack.pop() {
        let handle = match open_directory_handle(&dir) {
            Some(h) => h,
            None => continue,
        };

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

                let mut offset = 0usize;
                let end = io_status.Information as usize;

                while offset < end {
                    let entry_ptr =
                        buffer.as_ptr().add(offset) as *const FILE_DIRECTORY_INFORMATION;
                    let entry = &*entry_ptr;

                    let name_len = entry.FileNameLength as usize / 2;
                    let name_ptr = entry.FileName.as_ptr();

                    // 🚀 FAST "." and ".." check (no allocation)
                    let is_dot = name_len == 1 && *name_ptr == b'.' as u16;
                    let is_dotdot = name_len == 2
                        && *name_ptr == b'.' as u16
                        && *name_ptr.add(1) == b'.' as u16;

                    if !is_dot && !is_dotdot {
                        let is_dir = (entry.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0) != 0;

                        if is_dir {
                            // Only allocate when needed (directory)
                            let name =
                                OsString::from_wide(std::slice::from_raw_parts(name_ptr, name_len));
                            stack.push(dir.join(name));
                        } else {
                            total_size =
                                total_size.saturating_add(*entry.EndOfFile.QuadPart() as u64);
                        }
                    }

                    if entry.NextEntryOffset == 0 {
                        break;
                    }

                    offset += entry.NextEntryOffset as usize;
                }
            }

            let _ = CloseHandle(handle);
        }
    }

    total_size
}

/// 🚀 Async directory scan
pub fn scan_dir_async(
    path: PathBuf,
    tx: Sender<FileItem>,
    date_style: DateStyle,
    time_format_24h: bool,
    custom_date_format: String,
) {
    thread::spawn(move || {
        if path.to_string_lossy() == MY_PC_PATH {
            return;
        }

        if portable::is_portable_path(&path) {
            portable::scan_portable_async(path, tx);
            return;
        }

        if let Some(host) = network::unc_host_only(&path) {
            for share in network::list_shares(&host) {
                let is_hidden = share.name.ends_with('$');
                let item = FileItem::new(
                    share.name, share.path, true, is_hidden, None, None, None, None, None, None,
                    None, None, None,
                );
                let _ = tx.send(item);
            }
            return;
        }

        let handle = match open_directory_handle(&path) {
            Some(h) => h,
            None => return,
        };

        unsafe {
            let mut buffer = vec![0u8; 64 * 1024];
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

                let mut offset = 0;

                while offset < io_status.Information as usize {
                    let entry_ptr =
                        buffer.as_ptr().add(offset) as *const FILE_DIRECTORY_INFORMATION;
                    let entry = &*entry_ptr;

                    let name_len = entry.FileNameLength as usize / 2;

                    let name_os = OsString::from_wide(std::slice::from_raw_parts(
                        entry.FileName.as_ptr(),
                        name_len,
                    ));

                    let name = name_os.to_string_lossy().to_string();

                    if name == "." || name == ".." {
                        if entry.NextEntryOffset == 0 {
                            break;
                        }
                        offset += entry.NextEntryOffset as usize;
                        continue;
                    }

                    let full_path = path.join(&name_os);

                    let is_dir = (entry.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0) != 0;
                    let is_hidden = (entry.FileAttributes & FILE_ATTRIBUTE_HIDDEN.0) != 0;

                    let file_size = if is_dir {
                        None
                    } else {
                        Some(*entry.EndOfFile.QuadPart() as u64)
                    };

                    let modified_time_filetime = *entry.LastWriteTime.QuadPart();
                    let created_time_filetime = *entry.CreationTime.QuadPart();

                    let modified_time = filetime_to_string(
                        modified_time_filetime,
                        date_style,
                        time_format_24h,
                        &custom_date_format,
                    );
                    let created_time = filetime_to_string(
                        created_time_filetime,
                        date_style,
                        time_format_24h,
                        &custom_date_format,
                    );

                    let modified_time_raw = if modified_time_filetime == 0 {
                        None
                    } else {
                        Some(modified_time_filetime)
                    };
                    let created_time_raw = if created_time_filetime == 0 {
                        None
                    } else {
                        Some(created_time_filetime)
                    };

                    let item = FileItem::new(
                        name,
                        full_path.clone(),
                        is_dir,
                        is_hidden,
                        None,
                        file_size,
                        modified_time,
                        created_time,
                        None,
                        modified_time_raw,
                        created_time_raw,
                        None,
                        None,
                    );

                    let _ = tx.send(item);

                    if entry.NextEntryOffset == 0 {
                        break;
                    }

                    offset += entry.NextEntryOffset as usize;
                }
            }

            let _ = CloseHandle(handle);
        }
    });
}

// ⚡ Fast, accurate folder size calculation with progress updates
pub fn parallel_directory_scan(path: PathBuf, tx: Sender<(PathBuf, u64, bool)>) {
    let mut total_size = 0u64;
    let mut stack = vec![path.clone()];
    let mut last_emit = Instant::now();

    while let Some(dir) = stack.pop() {
        if let Some(handle) = open_directory_handle(&dir) {
            unsafe {
                let mut buffer = vec![0u8; 64 * 1024];
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
                        1, // FileDirectoryInformation
                        0,
                        std::ptr::null_mut(),
                        0,
                    );

                    if status == STATUS_NO_MORE_FILES || status < 0 {
                        break;
                    }

                    let mut offset = 0;
                    while offset < io_status.Information as usize {
                        let entry_ptr =
                            buffer.as_ptr().add(offset) as *const FILE_DIRECTORY_INFORMATION;
                        let entry = &*entry_ptr;

                        let name_len = entry.FileNameLength as usize / 2;
                        let name = OsString::from_wide(std::slice::from_raw_parts(
                            entry.FileName.as_ptr(),
                            name_len,
                        ));

                        if name != "." && name != ".." {
                            let full_path = dir.join(&name);
                            let is_dir = (entry.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0) != 0;
                            let is_reparse =
                                (entry.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0) != 0;

                            // Only traverse real directories (skip symlinks/junctions)
                            if is_dir && !is_reparse {
                                stack.push(full_path);
                            } else if !is_dir {
                                total_size =
                                    total_size.saturating_add(*entry.EndOfFile.QuadPart() as u64);
                            }
                        }

                        if entry.NextEntryOffset == 0 {
                            break;
                        }
                        offset += entry.NextEntryOffset as usize;
                    }

                    // Emit progress every 100ms
                    if last_emit.elapsed() > Duration::from_millis(100) {
                        let _ = tx.send((path.clone(), total_size, false));
                        last_emit = Instant::now();
                    }
                }

                let _ = CloseHandle(handle);
            }
        }
    }

    // Final emit
    let _ = tx.send((path, total_size, true));
}

#[derive(Clone, Debug)]
pub struct FileItem {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_hidden: bool,
    pub recycle_bin_pidl: Option<Vec<u8>>,
    pub file_size: Option<u64>,
    pub modified_time: Option<String>,
    pub created_time: Option<String>,
    pub deleted_time: Option<String>,
    pub modified_time_raw: Option<i64>,
    pub created_time_raw: Option<i64>,
    pub deleted_time_raw: Option<i64>,
    pub original_directory: Option<String>,

    // Optional drive info (only populated for drive roots)
    pub total_space: Option<u64>,
    pub free_space: Option<u64>,
}

impl FileItem {
    pub fn new(
        name: String,
        path: PathBuf,
        is_dir: bool,
        is_hidden: bool,
        recycle_bin_pidl: Option<Vec<u8>>,
        file_size: Option<u64>,
        modified_time: Option<String>,
        created_time: Option<String>,
        deleted_time: Option<String>,
        modified_time_raw: Option<i64>,
        created_time_raw: Option<i64>,
        deleted_time_raw: Option<i64>,
        original_directory: Option<String>,
    ) -> Self {
        Self {
            name,
            path,
            is_dir,
            is_hidden,
            recycle_bin_pidl,
            file_size,
            modified_time,
            created_time,
            deleted_time,
            modified_time_raw,
            created_time_raw,
            deleted_time_raw,
            original_directory,
            total_space: None,
            free_space: None,
        }
    }

    pub fn with_drive_info(
        name: String,
        path: PathBuf,
        is_dir: bool,
        is_hidden: bool,
        recycle_bin_pidl: Option<Vec<u8>>,
        file_size: Option<u64>,
        modified_time: Option<String>,
        created_time: Option<String>,
        deleted_time: Option<String>,
        modified_time_raw: Option<i64>,
        created_time_raw: Option<i64>,
        deleted_time_raw: Option<i64>,
        original_directory: Option<String>,
        total: u64,
        free: u64,
    ) -> Self {
        Self {
            name,
            path,
            is_dir,
            is_hidden,
            recycle_bin_pidl,
            file_size,
            modified_time,
            created_time,
            deleted_time,
            modified_time_raw,
            created_time_raw,
            deleted_time_raw,
            original_directory,
            total_space: Some(total),
            free_space: Some(free),
        }
    }
}

#[inline]
pub fn filetime_struct_to_i64(ft: FILETIME) -> Option<i64> {
    let value = ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64);

    if value == 0 { None } else { Some(value as i64) }
}

/// Converts a Rust `SystemTime` (as returned by `Metadata::modified()`/
/// `created()`) to the same raw Windows FILETIME tick count (100ns
/// intervals since 1601) that `filetime_to_string`/`filetime_struct_to_i64`
/// already work with elsewhere - so the built-in search's `std::fs`-based
/// walk produces `FileItem`s with dates formatted identically to every
/// other listing, despite not going through the NT/IPC APIs that hand back
/// a `FILETIME` directly.
fn system_time_to_filetime_raw(t: std::time::SystemTime) -> Option<i64> {
    let dur = t.duration_since(std::time::UNIX_EPOCH).ok()?;
    let ticks = (dur.as_secs() as i64 + 11_644_473_600) * 10_000_000 + dur.subsec_nanos() as i64 / 100;
    Some(ticks)
}

/// Maximum number of results the built-in (non-Everything) search collects
/// before stopping - it walks the real filesystem live rather than
/// consulting a pre-built index, so an unbounded "Everywhere" search could
/// otherwise run for a very long time over a large disk.
pub const BUILTIN_SEARCH_MAX_RESULTS: usize = 5_000;

/// A single structured filter parsed out of a search query's `key:value`
/// tokens (`ext:png`, `size:>100mb`, `modified:today`) - see
/// `parse_search_filters`. Everything's own query parser already
/// understands these same keywords natively, so this only matters for the
/// built-in fallback engine (`search_builtin_async`), which otherwise only
/// ever matched against the filename.
enum SearchFilter {
    Ext(String),
    Size(SizeCmp, u64),
    Modified(ModifiedRange),
    /// `content:<term>` - handled separately from the other filters (see
    /// `search_builtin_async`) since matching it requires reading the
    /// candidate file's bytes, not just its already-fetched metadata; kept
    /// as a `SearchFilter` variant anyway so it parses/tokenizes through
    /// the exact same `key:value` path as everything else.
    Content(String),
}

enum SizeCmp {
    Gt,
    Lt,
    Eq,
}

#[derive(Clone, Copy)]
enum ModifiedRange {
    Today,
    Yesterday,
    ThisWeek,
}

/// Splits a search query into its plain free-text portion and any
/// recognized `key:value` filter tokens, so the built-in search engine can
/// support the same `ext:`/`size:`/`modified:` syntax Everything's own
/// query parser already accepts for free (see `build_query_string` in
/// `core::everything`, which passes the query through unmodified for
/// exactly this reason). An unrecognized or malformed `key:value` token is
/// left in the free-text portion rather than rejected outright - this is a
/// convenience feature for whole-word tokens, not a strict query language,
/// so a token like `size:` (no value) should just be treated as literal
/// text rather than showing a validation error.
fn parse_search_filters(query: &str) -> (String, Vec<SearchFilter>) {
    let mut filters = Vec::new();
    let mut free_terms = Vec::new();

    for token in query.split_whitespace() {
        let recognized = if let Some(rest) = token.strip_prefix("ext:") {
            let ext = rest.trim_start_matches('.').to_lowercase();
            (!ext.is_empty()).then(|| SearchFilter::Ext(ext))
        } else if let Some(rest) = token.strip_prefix("size:") {
            parse_size_filter(rest)
        } else if let Some(rest) = token.strip_prefix("modified:") {
            parse_modified_filter(rest)
        } else if let Some(rest) = token.strip_prefix("content:") {
            // Single-token term only (no quoted multi-word phrases yet) -
            // matches how `ext:`/`size:`/`modified:` are all one token too.
            (!rest.is_empty()).then(|| SearchFilter::Content(rest.to_lowercase()))
        } else {
            None
        };

        match recognized {
            Some(filter) => filters.push(filter),
            None => free_terms.push(token),
        }
    }

    (free_terms.join(" "), filters)
}

fn parse_size_filter(rest: &str) -> Option<SearchFilter> {
    let (cmp, amount) = if let Some(r) = rest.strip_prefix('>') {
        (SizeCmp::Gt, r)
    } else if let Some(r) = rest.strip_prefix('<') {
        (SizeCmp::Lt, r)
    } else {
        (SizeCmp::Eq, rest)
    };
    parse_size_to_bytes(amount).map(|bytes| SearchFilter::Size(cmp, bytes))
}

fn parse_size_to_bytes(text: &str) -> Option<u64> {
    let lower = text.trim().to_lowercase();
    let (number, multiplier) = if let Some(n) = lower.strip_suffix("gb") {
        (n, 1024u64 * 1024 * 1024)
    } else if let Some(n) = lower.strip_suffix("mb") {
        (n, 1024u64 * 1024)
    } else if let Some(n) = lower.strip_suffix("kb") {
        (n, 1024u64)
    } else if let Some(n) = lower.strip_suffix('b') {
        (n, 1u64)
    } else {
        (lower.as_str(), 1u64)
    };

    let value: f64 = number.trim().parse().ok()?;
    if value < 0.0 {
        return None;
    }
    Some((value * multiplier as f64) as u64)
}

fn parse_modified_filter(rest: &str) -> Option<SearchFilter> {
    let range = match rest.to_lowercase().as_str() {
        "today" => ModifiedRange::Today,
        "yesterday" => ModifiedRange::Yesterday,
        "thisweek" => ModifiedRange::ThisWeek,
        _ => return None,
    };
    Some(SearchFilter::Modified(range))
}

/// Checks a candidate entry against every parsed filter (AND semantics -
/// all must pass). `modified_raw` is the same raw Windows FILETIME tick
/// count (100ns intervals since 1601) that `filetime_to_string` already
/// works with, so callers can pass it straight through without a separate
/// conversion.
fn matches_search_filters(
    filters: &[SearchFilter],
    name_lower: &str,
    is_dir: bool,
    file_size: Option<u64>,
    modified_raw: Option<i64>,
) -> bool {
    for filter in filters {
        let ok = match filter {
            SearchFilter::Ext(ext) => {
                !is_dir && name_lower.rsplit_once('.').is_some_and(|(_, e)| e == ext)
            }
            SearchFilter::Size(cmp, bytes) => file_size.is_some_and(|size| match cmp {
                SizeCmp::Gt => size > *bytes,
                SizeCmp::Lt => size < *bytes,
                SizeCmp::Eq => size == *bytes,
            }),
            SearchFilter::Modified(range) => {
                modified_raw.is_some_and(|raw| filetime_in_modified_range(raw, *range))
            }
            // Handled by the caller (`search_builtin_async`), which strips
            // `Content` filters out before calling this function - matching
            // it needs a file read, not just the metadata already in scope
            // here. Always `true` so this match stays exhaustive.
            SearchFilter::Content(_) => true,
        };
        if !ok {
            return false;
        }
    }
    true
}

/// Maximum file size a `content:` filter will actually read - reading a
/// multi-gigabyte log file line-by-line for every candidate would make a
/// broad content search pathologically slow, so anything bigger is treated
/// as a non-match rather than scanned.
const CONTENT_SEARCH_MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// Checks whether `path` (a file, not a directory, of at most
/// `CONTENT_SEARCH_MAX_FILE_BYTES`) contains `term_lower` (already
/// lowercased) on any line, case-insensitively. Skips anything that isn't
/// plausibly a text file - reusing the exact same "is this safe to read as
/// text" checks the preview pane already uses - so a content search never
/// tries to line-scan a binary.
fn matches_content_filter(path: &Path, is_dir: bool, file_size: Option<u64>, term_lower: &str) -> bool {
    use std::io::BufRead;

    if is_dir {
        return false;
    }
    let Some(size) = file_size else { return false };
    if size > CONTENT_SEARCH_MAX_FILE_BYTES {
        return false;
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !crate::core::preview::is_known_text_extension(&ext) && !crate::core::preview::sniff_is_text(path) {
        return false;
    }

    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    std::io::BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .any(|line| line.to_lowercase().contains(term_lower))
}

/// Compares a raw FILETIME's local calendar date against `today`/
/// `yesterday`/`thisweek`, matching how a user thinks about these ranges
/// (calendar days in their own timezone), not a rolling 24h/48h window.
fn filetime_in_modified_range(raw: i64, range: ModifiedRange) -> bool {
    let unix_time = (raw / 10_000_000) - 11_644_473_600;
    let Some(dt_utc) = Utc.timestamp_opt(unix_time, 0).single() else {
        return false;
    };
    let entry_date = DateTime::<Local>::from(dt_utc).date_naive();
    let today = Local::now().date_naive();

    match range {
        ModifiedRange::Today => entry_date == today,
        ModifiedRange::Yesterday => entry_date == today - chrono::Duration::days(1),
        ModifiedRange::ThisWeek => entry_date > today - chrono::Duration::days(7) && entry_date <= today,
    }
}

/// Matches a (already-lowercased) file/folder name against a
/// (already-lowercased) query the same way a plain-text query would read
/// to a user: if the query contains no wildcard characters, it matches
/// anywhere in the name (`"loctest"` matches `"zzzloctest1.txt"`) - the
/// same substring behavior the existing per-folder type-to-filter box
/// uses. If it contains `*` (any run of characters) or `?` (any single
/// character), it's matched as a whole-name glob instead (`"*.png"`
/// matches names *ending* in `.png`, not names that literally contain the
/// three characters `*`, `.`, `p`...). Everything's own query syntax
/// already treats `*`/`?` as wildcards, so without this the built-in
/// engine would return zero results for the exact same query Everything
/// handles correctly - `*` doesn't appear in real filenames, so a bare
/// substring search for it can never match anything.
fn matches_search_query(name_lower: &str, query_lower: &str) -> bool {
    if !query_lower.contains(['*', '?']) {
        return name_lower.contains(query_lower);
    }

    let name: Vec<char> = name_lower.chars().collect();
    let pattern: Vec<char> = query_lower.chars().collect();

    // Classic two-pointer wildcard matcher: `star_idx`/`match_idx` remember
    // the most recent `*` and how far into `name` we'd matched up to it, so
    // a later mismatch can backtrack to "the `*` consumes one more
    // character" instead of failing outright.
    let (mut n, mut p) = (0usize, 0usize);
    let (mut star_idx, mut match_idx) = (None, 0usize);

    while n < name.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == name[n]) {
            n += 1;
            p += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star_idx = Some(p);
            match_idx = n;
            p += 1;
        } else if let Some(s) = star_idx {
            p = s + 1;
            match_idx += 1;
            n = match_idx;
        } else {
            return false;
        }
    }

    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }

    p == pattern.len()
}

/// Fallback search used when the user has no Everything install (or
/// prefers not to use it, per the "Search engine" setting): walks the real
/// filesystem from `scope_folder` (or every drive, if `None` - "Everywhere")
/// looking for names matching `query` (case-insensitive; see
/// `matches_search_query` for the exact matching rules), streaming matches
/// into `tx` as they're found. Matches `scan_dir_async`'s contract exactly
/// (send-per-item, let `tx` drop when done) so it drops into the same
/// progressive-fill polling as everything else.
pub fn search_builtin_async(
    query: String,
    scope_folder: Option<PathBuf>,
    tx: Sender<FileItem>,
    date_style: DateStyle,
    time_format_24h: bool,
    custom_date_format: String,
) {
    use std::os::windows::fs::MetadataExt;

    thread::spawn(move || {
        let (free_text, all_filters) = parse_search_filters(&query);
        let query_lower = free_text.to_lowercase();
        if query_lower.is_empty() && all_filters.is_empty() {
            return;
        }

        // `content:` needs a real file read per candidate (not just the
        // metadata `read_dir` already hands back), so it's split out of the
        // cheap metadata filters and checked separately, only once
        // everything else has already matched. It's also only honored with
        // a specific folder scope - scanning file contents across every
        // drive on an "Everywhere" search would be far slower than users
        // expect from what looks like a normal filename search, so that
        // combination just runs as if `content:` weren't there rather than
        // silently taking minutes or refusing the query outright.
        let content_term = all_filters.iter().find_map(|f| match f {
            SearchFilter::Content(term) if scope_folder.is_some() => Some(term.clone()),
            _ => None,
        });
        let filters: Vec<SearchFilter> = all_filters
            .into_iter()
            .filter(|f| !matches!(f, SearchFilter::Content(_)))
            .collect();

        let roots: Vec<PathBuf> = match scope_folder {
            Some(dir) => vec![dir],
            None => crate::core::drives::get_drive_infos()
                .into_iter()
                .map(|d| d.path)
                .collect(),
        };

        let mut sent = 0usize;
        let mut stack = roots;

        'walk: while let Some(dir) = stack.pop() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for entry in entries.flatten() {
                let Ok(metadata) = entry.metadata() else {
                    continue;
                };

                let name = entry.file_name().to_string_lossy().to_string();
                let attributes = metadata.file_attributes();
                let is_dir = attributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
                let is_reparse = attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0;

                // Only traverse real directories - skip symlinks/junctions,
                // the same way parallel_directory_scan does, so a reparse
                // point pointing back up the tree can't cause an infinite
                // loop.
                if is_dir && !is_reparse {
                    stack.push(entry.path());
                }

                let name_lower = name.to_lowercase();
                if !query_lower.is_empty() && !matches_search_query(&name_lower, &query_lower) {
                    continue;
                }

                let is_hidden = attributes & FILE_ATTRIBUTE_HIDDEN.0 != 0;
                let file_size = if is_dir { None } else { Some(metadata.len()) };
                let modified_raw = metadata.modified().ok().and_then(system_time_to_filetime_raw);
                let created_raw = metadata.created().ok().and_then(system_time_to_filetime_raw);

                if !matches_search_filters(&filters, &name_lower, is_dir, file_size, modified_raw) {
                    continue;
                }

                if let Some(term) = &content_term {
                    let entry_path = entry.path();
                    if !matches_content_filter(&entry_path, is_dir, file_size, term) {
                        continue;
                    }
                }

                let modified_time = modified_raw
                    .and_then(|raw| filetime_to_string(raw, date_style, time_format_24h, &custom_date_format));
                let created_time = created_raw
                    .and_then(|raw| filetime_to_string(raw, date_style, time_format_24h, &custom_date_format));
                let original_directory = Some(dir.to_string_lossy().to_string());

                let item = FileItem::new(
                    name,
                    entry.path(),
                    is_dir,
                    is_hidden,
                    None,
                    file_size,
                    modified_time,
                    created_time,
                    None,
                    modified_raw,
                    created_raw,
                    None,
                    original_directory,
                );

                if tx.send(item).is_err() {
                    break 'walk;
                }
                sent += 1;
                if sent >= BUILTIN_SEARCH_MAX_RESULTS {
                    break 'walk;
                }
            }
        }
    });
}

/// Source: Windows SDK propkey.h
pub const PKEY_SIZE: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xB725F130_47EF_101A_A5F1_02608C9EEBAC),
    pid: 12,
};

/// Source: Windows SDK propkey.h
pub const PKEY_DATE_MODIFIED: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xB725F130_47EF_101A_A5F1_02608C9EEBAC),
    pid: 14,
};

/// Source: Windows SDK propkey.h
pub const PKEY_DATE_CREATED: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xB725F130_47EF_101A_A5F1_02608C9EEBAC),
    pid: 15,
};

pub fn get_shell_item_metadata(
    item: &IShellItem,
    date_style: DateStyle,
    time_format_24h: bool,
    custom_date_format: &str,
) -> (
    Option<u64>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<String>,
) {
    let Ok(item2) = item.cast::<IShellItem2>() else {
        return (None, None, None, None, None, None, None, None, None);
    };

    let (
        file_size,
        modified_time_raw,
        created_time_raw,
        deleted_time_raw,
        original_name,
        original_directory,
    ) = unsafe {
        (
            item2.GetUInt64(&PKEY_SIZE).ok(),
            item2
                .GetFileTime(&PKEY_DATE_MODIFIED)
                .ok()
                .and_then(filetime_struct_to_i64),
            item2
                .GetFileTime(&PKEY_DATE_CREATED)
                .ok()
                .and_then(filetime_struct_to_i64),
            item2
                .GetFileTime(recycle_date_deleted_key())
                .ok()
                .and_then(filetime_struct_to_i64),
            item2.GetString(recycle_name_key()).ok(),
            item2.GetString(recycle_original_directory_key()).ok(),
        )
    };

    let modified_time = modified_time_raw
        .and_then(|ft| filetime_to_string(ft, date_style, time_format_24h, custom_date_format));

    let created_time = created_time_raw
        .and_then(|ft| filetime_to_string(ft, date_style, time_format_24h, custom_date_format));

    let deleted_time = deleted_time_raw
        .and_then(|ft| filetime_to_string(ft, date_style, time_format_24h, custom_date_format));

    let original_name = unsafe { original_name.and_then(|name| name.to_string().ok()) };

    let original_directory = unsafe { original_directory.and_then(|dir| dir.to_string().ok()) };

    (
        file_size,
        modified_time,
        created_time,
        deleted_time,
        modified_time_raw,
        created_time_raw,
        deleted_time_raw,
        original_name,
        original_directory,
    )
}

fn recycle_date_deleted_key() -> &'static PROPERTYKEY {
    RECYCLE_DATE_DELETED.get_or_init(|| {
        let mut key = PROPERTYKEY::default();
        unsafe {
            PSGetPropertyKeyFromName(w!("System.Recycle.DateDeleted"), &mut key)
                .expect("System.Recycle.DateDeleted");
        }
        key
    })
}

fn recycle_name_key() -> &'static PROPERTYKEY {
    RECYCLE_NAME.get_or_init(|| {
        let mut key = PROPERTYKEY::default();
        unsafe {
            PSGetPropertyKeyFromName(w!("System.ItemNameDisplay"), &mut key)
                .expect("PKEY_ItemNameDisplay");
        }
        key
    })
}

fn recycle_original_directory_key() -> &'static PROPERTYKEY {
    RECYCLE_ORIGINAL_DIRECTORY.get_or_init(|| {
        let mut key = PROPERTYKEY::default();

        unsafe {
            PSGetPropertyKeyFromName(w!("System.Recycle.DeletedFrom"), &mut key)
                .expect("System.Recycle.DeletedFrom");
        }

        key
    })
}

#[cfg(test)]
mod search_query_tests {
    use super::matches_search_query;

    #[test]
    fn wildcard_and_substring_matching() {
        assert!(matches_search_query("photo.png", "*.png"));
        assert!(!matches_search_query("photo.jpg", "*.png"));
        assert!(matches_search_query("photo.png", "photo.*"));
        assert!(matches_search_query("a.png", "?.png"));
        assert!(!matches_search_query("ab.png", "?.png"));
        assert!(matches_search_query("zzzloctest1.txt", "loctest"));
        assert!(!matches_search_query("zzzloctest1.txt", "notthere"));
        assert!(matches_search_query(&"ANYTHING.PNG".to_lowercase(), "*.png"));
        assert!(matches_search_query("archive.tar.gz", "*.gz"));
        assert!(matches_search_query("noext", "*"));
    }
}

#[cfg(test)]
mod search_filter_tests {
    use super::{matches_search_filters, parse_search_filters};

    #[test]
    fn ext_filter_parses_and_matches() {
        let (free_text, filters) = parse_search_filters("report ext:pdf");
        assert_eq!(free_text, "report");
        assert_eq!(filters.len(), 1);
        assert!(matches_search_filters(&filters, "report.pdf", false, None, None));
        assert!(!matches_search_filters(&filters, "report.docx", false, None, None));
        assert!(!matches_search_filters(&filters, "somefolder.pdf", true, None, None));
    }

    #[test]
    fn ext_filter_strips_leading_dot_and_is_case_insensitive() {
        let (_, filters) = parse_search_filters("ext:.PNG");
        assert!(matches_search_filters(&filters, "photo.png", false, None, None));
    }

    #[test]
    fn size_filter_parses_units_and_compares() {
        let (_, filters) = parse_search_filters("size:>10mb");
        assert!(matches_search_filters(&filters, "big.bin", false, Some(11 * 1024 * 1024), None));
        assert!(!matches_search_filters(&filters, "small.bin", false, Some(5 * 1024 * 1024), None));

        let (_, filters) = parse_search_filters("size:<1kb");
        assert!(matches_search_filters(&filters, "tiny.txt", false, Some(500), None));
        assert!(!matches_search_filters(&filters, "notsotiny.txt", false, Some(2000), None));
    }

    #[test]
    fn size_filter_missing_size_never_matches() {
        let (_, filters) = parse_search_filters("size:>1mb");
        assert!(!matches_search_filters(&filters, "unknown.bin", false, None, None));
    }

    #[test]
    fn modified_filter_parses_recognized_keywords_only() {
        let (free_text, filters) = parse_search_filters("modified:today");
        assert_eq!(free_text, "");
        assert_eq!(filters.len(), 1);

        // An unrecognized modified:<value> falls back to free text instead
        // of being silently dropped or erroring.
        let (free_text, filters) = parse_search_filters("modified:lastmonth");
        assert_eq!(free_text, "modified:lastmonth");
        assert!(filters.is_empty());
    }

    #[test]
    fn combined_filters_and_free_text() {
        let (free_text, filters) = parse_search_filters("invoice ext:pdf size:>100kb");
        assert_eq!(free_text, "invoice");
        assert_eq!(filters.len(), 2);
    }

    #[test]
    fn content_filter_parses_as_single_lowercased_token() {
        let (free_text, filters) = parse_search_filters("content:TODO ext:rs");
        assert_eq!(free_text, "");
        assert_eq!(filters.len(), 2);
        assert!(matches!(filters[0], super::SearchFilter::Content(ref t) if t == "todo"));
    }

    #[test]
    fn content_filter_rejects_empty_term() {
        let (free_text, filters) = parse_search_filters("content:");
        assert_eq!(free_text, "content:");
        assert!(filters.is_empty());
    }
}

#[cfg(test)]
mod content_search_tests {
    use super::matches_content_filter;
    use std::io::Write;

    fn write_temp_file(name: &str, contents: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("eden_content_search_test_{name}"));
        let mut file = std::fs::File::create(&path).expect("create temp file");
        file.write_all(contents).expect("write temp file");
        path
    }

    #[test]
    fn matches_a_text_file_containing_the_term_case_insensitively() {
        let path = write_temp_file("match.txt", b"first line\nsecond LINE has Needle\nthird");
        let size = std::fs::metadata(&path).unwrap().len();

        assert!(matches_content_filter(&path, false, Some(size), "needle"));
        assert!(!matches_content_filter(&path, false, Some(size), "absent"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn skips_directories_and_oversized_or_binary_files() {
        let dir = std::env::temp_dir();
        assert!(!matches_content_filter(&dir, true, Some(0), "anything"));

        let binary_path = write_temp_file("binary.bin", &[0u8, 1, 2, 3, 0, 5]);
        let size = std::fs::metadata(&binary_path).unwrap().len();
        assert!(!matches_content_filter(&binary_path, false, Some(size), "anything"));
        let _ = std::fs::remove_file(&binary_path);

        let text_path = write_temp_file("toobig.txt", b"needle");
        assert!(!matches_content_filter(
            &text_path,
            false,
            Some(super::CONTENT_SEARCH_MAX_FILE_BYTES + 1),
            "needle"
        ));
        let _ = std::fs::remove_file(&text_path);
    }
}
