use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{copy, create_dir_all, read_dir};
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::UI::Shell::{
    FO_DELETE, FOF_ALLOWUNDO, SHFILEINFOW, SHFILEOPSTRUCTW, SHFileOperationW, SHGFI_TYPENAME,
    SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW,
};
use windows::core::PCWSTR;

pub fn shell_delete_to_recycle_bin(path: &PathBuf) -> bool {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    wide.push(0);

    let mut op = SHFILEOPSTRUCTW::default();
    op.wFunc = FO_DELETE;
    op.pFrom = PCWSTR(wide.as_ptr());
    op.fFlags = FOF_ALLOWUNDO.0 as u16;

    unsafe { SHFileOperationW(&mut op) == 0 }
}

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
