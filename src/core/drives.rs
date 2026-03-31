use crate::core::fs::get_drive_space;
use crate::core::portable::{list_portable_devices_with_ids, make_portable_path};
use std::ffi::OsString;
use std::mem::size_of;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Devices::DeviceAndDriverInstallation::*;
use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_READ_ATTRIBUTES, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FindFirstVolumeW, FindNextVolumeW, FindVolumeClose, GetDiskFreeSpaceExW,
    GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW, GetVolumePathNamesForVolumeNameW,
    OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::*;
use windows::Win32::System::Ioctl::{
    DISK_GEOMETRY, GUID_DEVINTERFACE_DISK, IOCTL_DISK_GET_DRIVE_GEOMETRY,
};
use windows::Win32::System::WindowsProgramming::{DRIVE_CDROM, DRIVE_REMOVABLE};
use windows::core::PCWSTR;
use windows::core::PWSTR;

// Cache for drive information to avoid expensive enumeration on every call
struct DriveCache {
    drives: Vec<DriveInfo>,
    last_update: Instant,
}

lazy_static::lazy_static! {
    static ref DRIVE_CACHE: Arc<Mutex<Option<DriveCache>>> = Arc::new(Mutex::new(None));
}
static DRIVE_LIST_DIRTY: AtomicBool = AtomicBool::new(true);

// Cache duration - refresh drives every 30 seconds or when explicitly requested
const CACHE_DURATION: Duration = Duration::from_secs(30);

pub fn mark_drive_cache_dirty() {
    DRIVE_LIST_DIRTY.store(true, Ordering::Release);
    if let Ok(mut cache) = DRIVE_CACHE.lock() {
        *cache = None;
    }
}

pub fn consume_drive_list_dirty() -> bool {
    DRIVE_LIST_DIRTY.swap(false, Ordering::AcqRel)
}

#[derive(Clone)]
pub struct DriveInfo {
    pub display: String,
    pub path: PathBuf,
    #[allow(dead_code)]
    pub is_removable: bool,
    pub total_space: Option<u64>,
    pub free_space: Option<u64>,
    #[allow(dead_code)]
    pub device_path: Option<String>, // Optional physical device or WPD device ID
}

/// Represents raw/unmounted physical drives
#[allow(dead_code)]
pub struct RawDriveInfo {
    pub device_path: String, // e.g., "\\.\PhysicalDrive1"
    pub total_bytes: Option<u64>,
    pub is_removable: bool,
    pub bus_type: Option<u32>, // Optional bus type (USB, NVMe, etc.)
}

pub fn list_portable_devices() -> Vec<String> {
    list_portable_devices_with_ids()
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

// Returns all removable physical device paths
fn list_removable_devices() -> Vec<String> {
    let mut devices = Vec::new();

    unsafe {
        let guid = GUID_DEVINTERFACE_DISK;

        // SetupDiGetClassDevsW returns Result<HDEVINFO>
        let device_info_set = match SetupDiGetClassDevsW(
            Some(&guid),
            None,
            None,
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        ) {
            Ok(set) => set,
            Err(_) => return devices,
        };

        if device_info_set.is_invalid() {
            return devices;
        }

        let mut index = 0;
        loop {
            let mut device_interface_data = SP_DEVICE_INTERFACE_DATA::default();
            device_interface_data.cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;

            // SetupDiEnumDeviceInterfaces now returns Result<(), Error>
            if SetupDiEnumDeviceInterfaces(
                device_info_set,
                None,
                &guid,
                index,
                &mut device_interface_data,
            )
            .is_err()
            {
                break; // no more devices
            }

            // Get required buffer size
            let mut required_size: u32 = 0;
            let _ = SetupDiGetDeviceInterfaceDetailW(
                device_info_set,
                &device_interface_data,
                None,
                0,
                Some(&mut required_size),
                None,
            );

            let mut detail_data = vec![0u8; required_size as usize];
            let detail_ptr = detail_data.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail_ptr).cbSize = 6;

            if SetupDiGetDeviceInterfaceDetailW(
                device_info_set,
                &device_interface_data,
                Some(detail_ptr),
                required_size,
                Some(&mut required_size),
                None,
            )
            .is_ok()
            {
                let device_path = std::slice::from_raw_parts(
                    (*detail_ptr).DevicePath.as_ptr(),
                    wcslen((*detail_ptr).DevicePath.as_ptr()),
                );
                let device_str = String::from_utf16_lossy(device_path);
                devices.push(device_str);
            }

            index += 1;
        }

        let _ = SetupDiDestroyDeviceInfoList(device_info_set);
    }

    devices
}

/// Detect raw/unmounted drives (ISO sticks, Linux partitions, etc.)
pub fn list_raw_drives() -> Vec<RawDriveInfo> {
    let mut drives = Vec::new();

    for drive_index in 0..32 {
        let path = format!("\\\\.\\PhysicalDrive{}", drive_index);
        let wide_path: Vec<u16> = OsString::from(&path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        unsafe {
            let handle = CreateFileW(
                PCWSTR(wide_path.as_ptr()),
                FILE_READ_ATTRIBUTES.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            );

            let handle = match handle {
                Ok(h) => h,
                Err(_) => continue,
            };

            // Allocate buffer for descriptor
            let mut buffer = vec![0u8; 1024];
            let mut bytes_returned: u32 = 0;
            let query = STORAGE_PROPERTY_QUERY {
                PropertyId: StorageDeviceProperty,
                QueryType: PropertyStandardQuery,
                AdditionalParameters: [0; 1],
            };

            let _ = DeviceIoControl(
                handle,
                IOCTL_STORAGE_QUERY_PROPERTY,
                Some(&query as *const _ as *const _),
                size_of::<STORAGE_PROPERTY_QUERY>() as u32,
                Some(buffer.as_mut_ptr() as *mut _),
                buffer.len() as u32,
                Some(&mut bytes_returned as *mut _),
                None,
            );

            let storage_desc = &*(buffer.as_ptr() as *const STORAGE_DEVICE_DESCRIPTOR);
            let bus_type = Some(storage_desc.BusType.0 as u32);
            let is_removable = storage_desc.RemovableMedia || bus_type == Some(7); // 7 = USB

            // Disk geometry (optional)
            let mut geometry: DISK_GEOMETRY = std::mem::zeroed();
            let total_bytes = if DeviceIoControl(
                handle,
                IOCTL_DISK_GET_DRIVE_GEOMETRY,
                None,
                0,
                Some(&mut geometry as *mut _ as *mut _),
                size_of::<DISK_GEOMETRY>() as u32,
                Some(&mut bytes_returned as *mut _),
                None,
            )
            .is_ok()
            {
                Some(
                    geometry.Cylinders as u64
                        * geometry.TracksPerCylinder as u64
                        * geometry.SectorsPerTrack as u64
                        * geometry.BytesPerSector as u64,
                )
            } else {
                None
            };

            drives.push(RawDriveInfo {
                device_path: path,
                total_bytes,
                is_removable,
                bus_type,
            });

            let _ = CloseHandle(handle);
        }
    }

    drives
}

fn list_unmounted_volumes() -> Vec<RawDriveInfo> {
    let mut volumes = Vec::new();
    let mut name_buf = vec![0u16; 1024];

    unsafe {
        let handle = match FindFirstVolumeW(&mut name_buf) {
            Ok(h) => h,
            Err(_) => return volumes,
        };

        loop {
            let len = name_buf.iter().position(|&c| c == 0).unwrap_or(0);
            let volume_name = if len > 0 {
                String::from_utf16_lossy(&name_buf[..len])
            } else {
                String::new()
            };

            if !volume_name.is_empty() {
                let mut paths_buf = vec![0u16; 2048];
                let mut needed: u32 = 0;
                let mut has_mount_point = false;

                let _ = GetVolumePathNamesForVolumeNameW(
                    PCWSTR(name_buf.as_ptr()),
                    Some(&mut paths_buf),
                    &mut needed,
                );
                has_mount_point = paths_buf[0] != 0;

                if !has_mount_point {
                    let mut free_bytes: u64 = 0;
                    let mut total_bytes: u64 = 0;
                    let _ = GetDiskFreeSpaceExW(
                        PCWSTR(name_buf.as_ptr()),
                        None,
                        Some(&mut total_bytes),
                        Some(&mut free_bytes),
                    );

                    volumes.push(RawDriveInfo {
                        device_path: volume_name,
                        total_bytes: if total_bytes > 0 {
                            Some(total_bytes)
                        } else {
                            None
                        },
                        is_removable: true,
                        bus_type: None,
                    });
                }
            }

            if FindNextVolumeW(handle, &mut name_buf).is_err() {
                break;
            }
        }

        let _ = FindVolumeClose(handle);
    }

    volumes
}

pub fn get_drive_infos() -> Vec<DriveInfo> {
    let mut cache = DRIVE_CACHE.lock().unwrap();

    // Check if cache is valid (not expired)
    if let Some(ref cached) = *cache {
        if cached.last_update.elapsed() < CACHE_DURATION {
            return cached.drives.clone();
        }
    }

    // Cache is expired or doesn't exist, regenerate drive info
    let drives = get_drive_infos_internal();

    // Update cache
    *cache = Some(DriveCache {
        drives: drives.clone(),
        last_update: std::time::Instant::now(),
    });

    drives
}

// Internal function that does the actual drive enumeration (expensive operations)
fn get_drive_infos_internal() -> Vec<DriveInfo> {
    let mut drives = Vec::new();
    let mut drive_labels: Vec<String> = Vec::new();
    let removable_devices = list_removable_devices();

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
                let mut is_removable = matches!(drive_type, DRIVE_REMOVABLE | DRIVE_CDROM);
                let mut device_path = None;

                // Associate with a removable physical device if possible
                for dev in &removable_devices {
                    if dev
                        .to_lowercase()
                        .contains(&letter.to_lowercase().to_string())
                    {
                        is_removable = true;
                        device_path = Some(dev.clone());
                        break;
                    }
                }

                // Try to get total/free space, but don't skip drives if unavailable
                let (total_space, free_space) = get_drive_space(&PathBuf::from(&drive_path))
                    .map(|(t, f)| (Some(t), Some(f)))
                    .unwrap_or((None, None));

                // Volume label
                let mut volume_name_buffer = [0u16; 256];
                let mut file_system_flags = 0u32;
                let display = if GetVolumeInformationW(
                    pcw_path,
                    Some(&mut volume_name_buffer),
                    None,
                    None,
                    Some(&mut file_system_flags),
                    None,
                )
                .is_ok()
                {
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

                if let Some(label) = drive_label_from_display(&display) {
                    drive_labels.push(label);
                }

                drives.push(DriveInfo {
                    display,
                    path: PathBuf::from(&drive_path),
                    is_removable,
                    total_space,
                    free_space,
                    device_path,
                });
            }
        }
    }

    // Add portable devices (iPhone, Android, etc.)
    let drive_labels: Vec<String> = drive_labels
        .into_iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();

    drives.extend(
        list_portable_devices_with_ids()
            .into_iter()
            .filter(|(name, _)| {
                let name_l = name.to_ascii_lowercase();
                !drive_labels.iter().any(|label| label == &name_l)
            })
            .map(|(name, device_id)| DriveInfo {
                display: name.clone(),
                path: make_portable_path(&device_id, "DEVICE"),
                is_removable: true,
                total_space: None,
                free_space: None,
                device_path: Some(device_id),
            }),
    );

    // println!("Portable Devices: {:#?}", list_portable_devices());

    // Unmounted volumes (no drive letter), e.g. Linux/ext partitions on USB
    drives.extend(list_unmounted_volumes().into_iter().filter_map(|raw| {
        let fs_name = volume_filesystem_name(&raw.device_path).unwrap_or_default();
        let fs_upper = fs_name.to_ascii_uppercase();
        if fs_upper == "NTFS" || fs_upper == "FAT32" {
            return None;
        }

        let display = format!("{} (unmounted)", volume_display_name(&raw.device_path));
        Some(DriveInfo {
            display,
            path: PathBuf::from(&raw.device_path),
            is_removable: raw.is_removable,
            total_space: raw.total_bytes,
            free_space: None,
            device_path: Some(raw.device_path),
        })
    }));

    // Removable raw physical disks (e.g. USB devices with non-Windows partitions)
    let mut existing_paths: std::collections::HashSet<String> = drives
        .iter()
        .map(|d| d.path.to_string_lossy().to_string().to_ascii_lowercase())
        .collect();

    for raw in list_raw_drives().into_iter().filter(|r| r.is_removable) {
        let path_str = raw.device_path.to_ascii_lowercase();
        if existing_paths.contains(&path_str) {
            continue;
        }
        existing_paths.insert(path_str);

        let display = format!("Non-NTFS Drive ({})", raw.device_path);
        drives.push(DriveInfo {
            display,
            path: PathBuf::from(&raw.device_path),
            is_removable: true,
            total_space: raw.total_bytes,
            free_space: None,
            device_path: Some(raw.device_path),
        });
    }

    // println!("Raw Devices: {:#?}", list_raw_drives().iter().map(|r| r.device_path.clone()).collect::<Vec<_>>());

    drives
}

unsafe fn wcslen(mut ptr: *const u16) -> usize {
    unsafe {
        let mut len = 0;
        while *ptr != 0 {
            len += 1;
            ptr = ptr.add(1);
        }
        len
    }
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

pub fn is_raw_physical_drive_path(path: &PathBuf) -> bool {
    let s = path.to_string_lossy();
    s.starts_with(r"\\.\PhysicalDrive")
}

fn drive_label_from_display(display: &str) -> Option<String> {
    if let Some(open) = display.rfind(" (") {
        if display.ends_with(')') {
            let label = display[..open].trim();
            if !label.is_empty() {
                return Some(label.to_string());
            }
        }
    }
    None
}

fn volume_display_name(volume_name: &str) -> String {
    let normalized = normalize_volume_name(volume_name);
    let wide: Vec<u16> = OsString::from(&normalized)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut volume_name_buffer = [0u16; 256];
    let mut fs_name_buffer = [0u16; 256];

    unsafe {
        if GetVolumeInformationW(
            PCWSTR(wide.as_ptr()),
            Some(&mut volume_name_buffer),
            None,
            None,
            None,
            Some(&mut fs_name_buffer),
        )
        .is_ok()
        {
            let label_len = volume_name_buffer
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(volume_name_buffer.len());
            if label_len > 0 {
                return OsString::from_wide(&volume_name_buffer[..label_len])
                    .to_string_lossy()
                    .to_string();
            }

            let fs_len = fs_name_buffer
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(fs_name_buffer.len());
            if fs_len > 0 {
                let fs = OsString::from_wide(&fs_name_buffer[..fs_len])
                    .to_string_lossy()
                    .to_string();
                return format!("{fs} Volume");
            }
        }
    }

    "Raw Volume".to_string()
}

fn volume_filesystem_name(volume_name: &str) -> Option<String> {
    let normalized = normalize_volume_name(volume_name);
    let wide: Vec<u16> = OsString::from(&normalized)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut fs_name_buffer = [0u16; 256];

    unsafe {
        if GetVolumeInformationW(
            PCWSTR(wide.as_ptr()),
            None,
            None,
            None,
            None,
            Some(&mut fs_name_buffer),
        )
        .is_ok()
        {
            let fs_len = fs_name_buffer
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(fs_name_buffer.len());
            if fs_len > 0 {
                return Some(
                    OsString::from_wide(&fs_name_buffer[..fs_len])
                        .to_string_lossy()
                        .to_string(),
                );
            }
        }
    }

    None
}

fn normalize_volume_name(volume_name: &str) -> String {
    if volume_name.ends_with('\\') {
        volume_name.to_string()
    } else {
        format!("{volume_name}\\")
    }
}
