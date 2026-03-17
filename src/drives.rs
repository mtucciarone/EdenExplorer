use windows::Win32::Storage::FileSystem::{
    GetLogicalDrives, GetVolumeInformationW,
};
use windows::core::PCWSTR;
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};

pub fn get_drives() -> Vec<String> {
    let mut drives = vec![];

    unsafe {
        let mask = GetLogicalDrives();

        for i in 0..26 {
            if (mask >> i) & 1 == 1 {
                let letter = (b'A' + i as u8) as char;
                let drive_path = format!("{}:\\", letter);
                
                // Get drive label
                let mut volume_name_buffer = [0u16; 256];
                let mut file_system_flags = 0u32;
                
                let wide_path: Vec<u16> = OsString::from(&drive_path)
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                let pcw_path = PCWSTR(wide_path.as_ptr());
                
                let result = GetVolumeInformationW(
                    pcw_path,
                    Some(&mut volume_name_buffer),
                    None,
                    None,
                    Some(&mut file_system_flags),
                    None,
                );
                
                if result.is_ok() {
                    // Find null terminator
                    let label_len = volume_name_buffer
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(volume_name_buffer.len());
                    
                    if label_len > 0 {
                        let label = OsString::from_wide(&volume_name_buffer[..label_len])
                            .to_string_lossy()
                            .to_string();
                        drives.push(format!("{} ({})", drive_path, label));
                    } else {
                        drives.push(drive_path);
                    }
                } else {
                    drives.push(drive_path);
                }
            }
        }
    }

    drives
}