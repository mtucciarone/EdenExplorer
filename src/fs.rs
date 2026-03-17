use std::path::PathBuf;
use std::thread;

use crossbeam_channel::Sender;

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HANDLE, CloseHandle};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetDiskFreeSpaceExW,
    FILE_ATTRIBUTE_DIRECTORY,
    FILE_FLAG_BACKUP_SEMANTICS,
    FILE_LIST_DIRECTORY,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};

use ntapi::ntioapi::{NtQueryDirectoryFile, IO_STATUS_BLOCK, FILE_DIRECTORY_INFORMATION};

use crate::state::FileItem;

const STATUS_NO_MORE_FILES: i32 = 0x80000006u32 as i32;

/// Convert PathBuf -> UTF-16
fn path_to_wide(path: &PathBuf) -> Vec<u16> {
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
pub fn calculate_folder_size_fast(path: PathBuf) -> u64 {
    let mut total_size = 0u64;
    let mut stack = vec![path];

    while let Some(dir) = stack.pop() {
        let handle = match open_directory_handle(&dir) {
            Some(h) => h,
            None => continue,
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

                    let name = OsString::from_wide(
                        std::slice::from_raw_parts(entry.FileName.as_ptr(), name_len),
                    );

                    if name != "." && name != ".." {
                        let full = dir.join(&name);

                        let is_dir =
                            (entry.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0) != 0;

                        if is_dir {
                            stack.push(full);
                        } else {
                            total_size = total_size
                                .saturating_add(*entry.EndOfFile.QuadPart() as u64);
                        }
                    }

                    if entry.NextEntryOffset == 0 {
                        break;
                    }

                    offset += entry.NextEntryOffset as usize;
                }
            }

            let _ = CloseHandle(handle); // 🔥 IMPORTANT
        }
    }

    total_size
}

/// 🚀 Async directory scan
pub fn scan_dir_async(path: PathBuf, tx: Sender<FileItem>) {
    thread::spawn(move || {
        if path.to_string_lossy() == "::MY_PC::" {
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

                    let name_os = OsString::from_wide(
                        std::slice::from_raw_parts(entry.FileName.as_ptr(), name_len),
                    );

                    let name = name_os.to_string_lossy().to_string();

                    if name == "." || name == ".." {
                        if entry.NextEntryOffset == 0 {
                            break;
                        }
                        offset += entry.NextEntryOffset as usize;
                        continue;
                    }

                    let full_path = path.join(&name_os);

                    let is_dir =
                        (entry.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0) != 0;

                    let file_size = if is_dir {
                        None
                    } else {
                        Some(*entry.EndOfFile.QuadPart() as u64)
                    };

                    let modified_time = if entry.LastWriteTime.QuadPart() != &0 {
                        let filetime = *entry.LastWriteTime.QuadPart() as i64;
                        let unix_time = (filetime / 10000000) - 11644473600;

                        if unix_time > 0 {
                            let secs = unix_time as u64;
                            let days = secs / 86400;
                            let years = 1970 + (days / 365);
                            let remaining_days = days % 365;
                            let months = (remaining_days / 30) + 1;
                            let hours = (secs % 86400) / 3600;
                            let minutes = (secs % 3600) / 60;

                            Some(format!(
                                "{:04}-{:02}-{:02} {:02}:{:02}",
                                years,
                                months,
                                remaining_days % 30 + 1,
                                hours,
                                minutes
                            ))
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    let item = FileItem::new(
                        name,
                        full_path.clone(),
                        is_dir,
                        file_size,
                        modified_time,
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
