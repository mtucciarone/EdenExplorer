use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs::{copy, create_dir_all, read_dir};
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::HWND;
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    BHID_SFUIObject, CMINVOKECOMMANDINFO, CMINVOKECOMMANDINFOEX, IContextMenu, IShellItem,
    SHCreateItemFromIDList, SHFILEINFOW, SHGFI_TYPENAME, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{Error, HRESULT, PCSTR, PCWSTR};

pub fn get_file_type_name<'a>(ext: &str, cache: &'a mut HashMap<String, String>) -> &'a str {
    use std::collections::hash_map::Entry;

    match cache.entry(ext.to_string()) {
        Entry::Occupied(entry) => entry.into_mut().as_str(),
        Entry::Vacant(entry) => {
            // Ensure extension starts with "."
            let ext_formatted = if ext.starts_with('.') {
                ext.to_string()
            } else {
                format!(".{}", ext)
            };

            let wide: Vec<u16> = OsStr::new(&ext_formatted)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();

            let mut info = SHFILEINFOW::default();

            let _result = unsafe {
                SHGetFileInfoW(
                    PCWSTR(wide.as_ptr()),
                    FILE_ATTRIBUTE_NORMAL,
                    Some(&mut info),
                    size_of::<SHFILEINFOW>() as u32,
                    SHGFI_TYPENAME | SHGFI_USEFILEATTRIBUTES,
                )
            };

            let len = info.szTypeName.iter().position(|&c| c == 0).unwrap_or(0);
            let type_name = String::from_utf16_lossy(&info.szTypeName[..len]);

            entry.insert(type_name).as_str()
        }
    }
}

pub fn format_size(size: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size_f = size as f64;
    let mut unit_index = 0;

    while size_f >= 1024.0 && unit_index < UNITS.len() - 1 {
        size_f /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", size, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size_f, UNITS[unit_index])
    }
}

#[allow(dead_code)]
pub fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    create_dir_all(dest)?;

    for entry in read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let new_path = dest.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &new_path)?;
        } else {
            copy(entry.path(), new_path)?;
        }
    }

    Ok(())
}

/// Checks if a filename contains valid characters for real-time validation
/// Used during typing to immediately filter invalid characters
pub fn filename_has_valid_characters_realtime(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }

    // Check maximum length
    if name.len() > 255 {
        return false;
    }

    // Windows reserved characters that cannot be used in filenames
    let invalid_chars = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

    // Check for invalid characters
    for ch in name.chars() {
        if invalid_chars.contains(&ch) {
            return false;
        }
    }

    // Windows reserved names (case-insensitive)
    let reserved_names = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];

    let name_upper = name.to_uppercase();
    for reserved in &reserved_names {
        if name_upper == *reserved {
            return false;
        }
    }

    true
}

pub fn delete_paths_native(paths: Vec<PathBuf>, allow_undo: bool) -> windows::core::Result<()> {
    use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
    use windows::Win32::UI::Shell::{
        FILEOPERATION_FLAGS, FOF_ALLOWUNDO, FileOperation, IFileOperation, IShellItem,
        SHCreateItemFromParsingName,
    };
    use windows::core::HSTRING;

    unsafe {
        let file_op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;

        // Normal deletion:
        //   - send items to the Recycle Bin
        //   - allow the operation to be undone
        //
        // Recycle Bin deletion:
        //   - permanently delete the items
        let flags = if allow_undo {
            FOF_ALLOWUNDO
        } else {
            FILEOPERATION_FLAGS(0)
        };

        file_op.SetOperationFlags(flags)?;

        for path in paths {
            let path = HSTRING::from(path.to_string_lossy().as_ref());

            let item: IShellItem = SHCreateItemFromParsingName(&path, None)?;

            file_op.DeleteItem(&item, None)?;
        }

        // This is intentionally synchronous.
        //
        // Windows owns the progress UI while the operation is running,
        // and we don't update ItemViewer state until the operation has
        // completely finished.
        file_op.PerformOperations()?;
    }

    Ok(())
}

pub fn restore_paths_native(pidls: Vec<Vec<u8>>) -> windows::core::Result<()> {
    use windows::Win32::System::Com::{CoTaskMemAlloc, CoTaskMemFree};

    unsafe {
        for pidl_bytes in pidls {
            let pidl = CoTaskMemAlloc(pidl_bytes.len()) as *mut u8;
            if pidl.is_null() {
                return Err(Error::from(HRESULT(0x80004005u32 as i32)));
            }

            let result = (|| -> windows::core::Result<()> {
                std::ptr::copy_nonoverlapping(pidl_bytes.as_ptr(), pidl, pidl_bytes.len());

                let shell_item: IShellItem = SHCreateItemFromIDList(pidl as *const ITEMIDLIST)?;
                let context_menu: IContextMenu =
                    shell_item.BindToHandler(None, &BHID_SFUIObject)?;

                let restore_verb = PCSTR(b"undelete\0".as_ptr());
                let info = CMINVOKECOMMANDINFOEX {
                    cbSize: std::mem::size_of::<CMINVOKECOMMANDINFOEX>() as u32,
                    fMask: 0,
                    hwnd: HWND(std::ptr::null_mut()),
                    lpVerb: restore_verb,
                    lpVerbW: PCWSTR::null(),
                    nShow: SW_SHOWNORMAL.0,
                    ..Default::default()
                };

                context_menu.InvokeCommand(&info as *const _ as *const CMINVOKECOMMANDINFO)
            })();

            CoTaskMemFree(Some(pidl as _));
            result?;
        }
    }

    Ok(())
}

pub fn directory_child_paths(dir: &Path) -> HashSet<PathBuf> {
    read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .collect()
        })
        .unwrap_or_default()
}

pub fn selection_paths_after_paste(
    target_dir: &Path,
    before_entries: &HashSet<PathBuf>,
    sources: &[PathBuf],
) -> Vec<PathBuf> {
    let mut pasted_paths: Vec<PathBuf> = directory_child_paths(target_dir)
        .difference(before_entries)
        .cloned()
        .collect();

    if pasted_paths.is_empty() {
        for source in sources {
            if let Some(name) = source.file_name() {
                let candidate = target_dir.join(name);
                if candidate.exists() {
                    pasted_paths.push(candidate);
                }
            }
        }
    }

    pasted_paths
}
