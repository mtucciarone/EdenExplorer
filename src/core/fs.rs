use crate::core::portable;
use chrono::{DateTime, Local, TimeZone, Utc};
use crossbeam_channel::Sender;
use ntapi::ntioapi::{FILE_DIRECTORY_INFORMATION, IO_STATUS_BLOCK, NtQueryDirectoryFile};
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetDiskFreeSpaceExW, OPEN_EXISTING,
};
use windows::core::PCWSTR;

const STATUS_NO_MORE_FILES: i32 = 0x80000006u32 as i32;
pub const MY_PC_PATH: &str = "::MY_PC::";

pub fn filetime_to_string(filetime: i64, time_format_24h: bool) -> Option<String> {
    if filetime == 0 {
        return None;
    }

    let unix_time = (filetime / 10_000_000) - 11_644_473_600;

    if unix_time <= 0 {
        return None;
    }

    let dt_utc = Utc.timestamp_opt(unix_time, 0).single()?;
    let dt_local: DateTime<Local> = dt_utc.into();

    let format_string = if time_format_24h {
        "%Y-%m-%d %H:%M"
    } else {
        "%Y-%m-%d %I:%M %p"
    };

    Some(dt_local.format(format_string).to_string())
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
pub fn scan_dir_async(path: PathBuf, tx: Sender<FileItem>, time_format_24h: bool) {
    thread::spawn(move || {
        if path.to_string_lossy() == MY_PC_PATH {
            return;
        }

        if portable::is_portable_path(&path) {
            portable::scan_portable_async(path, tx, time_format_24h);
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

                    let file_size = if is_dir {
                        None
                    } else {
                        Some(*entry.EndOfFile.QuadPart() as u64)
                    };

                    let modified_time =
                        filetime_to_string(*entry.LastWriteTime.QuadPart(), time_format_24h);
                    let created_time =
                        filetime_to_string(*entry.CreationTime.QuadPart(), time_format_24h);

                    let item = FileItem::new(
                        name,
                        full_path.clone(),
                        is_dir,
                        file_size,
                        modified_time,
                        created_time,
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
    pub file_size: Option<u64>,
    pub modified_time: Option<String>,
    pub created_time: Option<String>,

    // Optional drive info (only populated for drive roots)
    pub total_space: Option<u64>,
    pub free_space: Option<u64>,
}

impl FileItem {
    pub fn new(
        name: String,
        path: PathBuf,
        is_dir: bool,
        file_size: Option<u64>,
        modified_time: Option<String>,
        created_time: Option<String>,
    ) -> Self {
        Self {
            name,
            path,
            is_dir,
            file_size,
            modified_time,
            created_time,
            total_space: None,
            free_space: None,
        }
    }

    pub fn with_drive_info(
        name: String,
        path: PathBuf,
        is_dir: bool,
        file_size: Option<u64>,
        modified_time: Option<String>,
        created_time: Option<String>,
        total: u64,
        free: u64,
    ) -> Self {
        Self {
            name,
            path,
            is_dir,
            file_size,
            modified_time,
            created_time,
            total_space: Some(total),
            free_space: Some(free),
        }
    }
}
