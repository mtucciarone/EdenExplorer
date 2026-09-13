//! Global file search backed by the voidtools "Everything" search engine,
//! via the `everything-ipc` crate's window-message IPC client. Everything
//! itself must be running (installed and launched by the user) - this
//! module never bundles or requires its SDK DLL, it only talks to whatever
//! instance is already running.

use crate::core::fs::{DateStyle, FileItem, filetime_struct_to_i64, filetime_to_string};
use crossbeam_channel::Sender;
use everything_ipc::wm::{EverythingClient, QueryItem, RequestFlags, Sort};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::thread;
use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_HIDDEN};

/// How far an Everything search should look.
#[derive(Clone, Debug, PartialEq)]
pub enum SearchScope {
    /// Restrict to this folder and its subfolders.
    CurrentFolder(PathBuf),
    Everywhere,
}

/// The user's persisted preference for what a freshly-opened search box
/// defaults its scope to - see `AppSettings::default_search_scope`. Distinct
/// from `SearchScope` itself since a *setting* can't carry a specific
/// folder, only a preference for whether to use "wherever I am" or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DefaultSearchScope {
    CurrentFolder,
    Everywhere,
}

impl Default for DefaultSearchScope {
    fn default() -> Self {
        Self::CurrentFolder
    }
}

/// Which engine powers search. Everything is the default (near-instant,
/// index-backed), but it requires the user to have voidtools Everything
/// installed and running - `BuiltIn` is a real filesystem walk that always
/// works, just slower, and is what a fresh/no-Everything install falls
/// back to automatically (see `check_everything_available` at the call
/// site in `load_path_with_fallback`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchEngine {
    Everything,
    BuiltIn,
}

impl Default for SearchEngine {
    fn default() -> Self {
        Self::Everything
    }
}

/// Cap on how many results a single search pulls across the IPC boundary -
/// keeps a maximally broad query (e.g. a single letter against `Everywhere`)
/// from stalling the IPC round-trip or flooding the UI with an unusable
/// number of rows. Everything's own `total_len()` on the response still
/// reports the real (uncapped) match count, so a "results capped" message
/// can compare that against this constant.
pub const MAX_SEARCH_RESULTS: u32 = 5_000;

/// Cheap probe: is Everything.exe running and reachable over IPC right now?
/// Checked once, up front, so the caller can distinguish "Everything isn't
/// running" from "the query just had zero hits" - both look like an empty
/// `FileItem` stream from `search_everything_async` alone.
pub fn check_everything_available() -> bool {
    EverythingClient::new().is_ok()
}

fn build_query_string(query: &str, scope: &SearchScope) -> String {
    match scope {
        SearchScope::Everywhere => query.to_string(),
        // Everything's `path:"<folder>"` modifier restricts results to that
        // folder and everything under it.
        SearchScope::CurrentFolder(dir) => format!("path:\"{}\" {}", dir.display(), query),
    }
}

/// Runs an Everything search on a background thread and streams results
/// into `tx` as `FileItem`s, one at a time. This deliberately matches
/// `fs::scan_dir_async`'s contract exactly - just `tx.send(item)` per
/// result, then let `tx` drop when done, no explicit "finished" message -
/// so the existing progressive-fill polling in
/// `handle_directory_batch_recieve_for` (mainwindow_imp.rs) needs no
/// changes at all to pick this up as just another file listing.
pub fn search_everything_async(
    query: String,
    scope: SearchScope,
    tx: Sender<FileItem>,
    date_style: DateStyle,
    time_format_24h: bool,
    custom_date_format: String,
) {
    thread::spawn(move || {
        let Ok(client) = EverythingClient::new() else {
            // Everything isn't running/reachable - `tx` drops here, which
            // the polling side already treats as "load finished with zero
            // items", no special-casing needed there. The caller checks
            // `check_everything_available()` up front to tell this apart
            // from a query that genuinely had zero hits.
            return;
        };

        let search_string = build_query_string(&query, &scope);
        let request_flags = RequestFlags::FileName
            | RequestFlags::FullPathAndFileName
            | RequestFlags::Size
            | RequestFlags::DateModified
            | RequestFlags::DateCreated
            | RequestFlags::Attributes;

        let Ok(results) = client
            .query_wait(&search_string)
            .request_flags(request_flags)
            .sort(Sort::NameAscending)
            .max_results(MAX_SEARCH_RESULTS)
            .call()
        else {
            return;
        };

        for item in results.iter() {
            if let Some(file_item) =
                everything_item_to_file_item(&item, date_style, time_format_24h, &custom_date_format)
                && tx.send(file_item).is_err()
            {
                break;
            }
        }
    });
}

fn everything_item_to_file_item(
    item: &QueryItem<'_>,
    date_style: DateStyle,
    time_format_24h: bool,
    custom_date_format: &str,
) -> Option<FileItem> {
    let name = item.get_string(RequestFlags::FileName)?;
    let path = PathBuf::from(item.get_string(RequestFlags::FullPathAndFileName)?);
    let original_directory = path
        .parent()
        .map(|p| p.to_string_lossy().to_string());

    let attributes = item.get_u32(RequestFlags::Attributes).unwrap_or(0);
    let is_dir = attributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
    let is_hidden = attributes & FILE_ATTRIBUTE_HIDDEN.0 != 0;

    // Everything's index doesn't track folder sizes (that's a separate,
    // opt-in recursive scan the app already does for real directories via
    // `parallel_directory_scan`) - leave it unset for folders, same as a
    // normal directory listing does until that background scan fills it in.
    let file_size = if is_dir {
        None
    } else {
        item.get_size(RequestFlags::Size)
    };

    let modified_raw = item
        .get_time(RequestFlags::DateModified)
        .and_then(filetime_struct_to_i64);
    let created_raw = item
        .get_time(RequestFlags::DateCreated)
        .and_then(filetime_struct_to_i64);

    let modified_time = modified_raw
        .and_then(|raw| filetime_to_string(raw, date_style, time_format_24h, custom_date_format));
    let created_time = created_raw
        .and_then(|raw| filetime_to_string(raw, date_style, time_format_24h, custom_date_format));

    Some(FileItem::new(
        name,
        path,
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
    ))
}
