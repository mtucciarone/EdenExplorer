use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
use windows::Win32::System::WindowsProgramming::{DRIVE_CDROM, DRIVE_REMOVABLE};

use crate::fs::get_drive_space;

pub struct DriveInfo {
    pub display: String,
    pub path: PathBuf,
    #[allow(dead_code)]
    pub is_removable: bool,
    pub total_space: Option<u64>,
    pub free_space: Option<u64>,
}

pub fn get_drive_infos() -> Vec<DriveInfo> {
    let mut drives = Vec::new();

    unsafe {
        let mask = GetLogicalDrives();

        for i in 0..26 {
            if (mask >> i) & 1 == 1 {
                let letter = (b'A' + i as u8) as char;
                let drive_path = format!("{}:\\", letter);

                let wide_path: Vec<u16> = OsString::from(&drive_path)
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                let pcw_path = PCWSTR(wide_path.as_ptr());

                let drive_type = GetDriveTypeW(pcw_path);
                let is_removable = drive_type == DRIVE_REMOVABLE || drive_type == DRIVE_CDROM;

                let mut volume_name_buffer = [0u16; 256];
                let mut file_system_flags = 0u32;

                let result = GetVolumeInformationW(
                    pcw_path,
                    Some(&mut volume_name_buffer),
                    None,
                    None,
                    Some(&mut file_system_flags),
                    None,
                );

                let display = if result.is_ok() {
                    let label_len = volume_name_buffer
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(volume_name_buffer.len());

                    if label_len > 0 {
                        let label = OsString::from_wide(&volume_name_buffer[..label_len])
                            .to_string_lossy()
                            .to_string();
                        format!("{} ({})", label, drive_path)
                    } else {
                        drive_path.clone()
                    }
                } else {
                    drive_path.clone()
                };

                let path = PathBuf::from(&drive_path);
                let (total_space, free_space) = match get_drive_space(&path) {
                    Some((total, free)) => (Some(total), Some(free)),
                    None => (None, None),
                };

                drives.push(DriveInfo {
                    display,
                    path,
                    is_removable,
                    total_space,
                    free_space,
                });
            }
        }
    }

    drives
}

pub fn get_drives() -> Vec<String> {
    get_drive_infos().into_iter().map(|d| d.display).collect()
}

pub fn parse_drive_display(display: &str) -> (String, PathBuf) {
    if let Some(open) = display.rfind('(') {
        if display.ends_with(')') {
            let inside = &display[open + 1..display.len() - 1];
            return (display.to_string(), PathBuf::from(inside));
        }
    }

    (display.to_string(), PathBuf::from(display))
}
