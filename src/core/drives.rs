use crate::core::fs::get_drive_space;
use std::ffi::OsString;
use std::mem::size_of;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Devices::DeviceAndDriverInstallation::*;
use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_READ_ATTRIBUTES, FILE_SHARE_READ,
    FILE_SHARE_WRITE, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::*;
use windows::Win32::System::Ioctl::{
    DISK_GEOMETRY, GUID_DEVINTERFACE_DISK, IOCTL_DISK_GET_DRIVE_GEOMETRY,
};
use windows::Win32::System::WindowsProgramming::{DRIVE_CDROM, DRIVE_REMOVABLE};
use windows::core::PCWSTR;
use windows::{Win32::Devices::PortableDevices::*, Win32::System::Com::*, core::PWSTR};

// Cache for drive information to avoid expensive enumeration on every call
struct DriveCache {
    drives: Vec<DriveInfo>,
    last_update: Instant,
}

lazy_static::lazy_static! {
    static ref DRIVE_CACHE: Arc<Mutex<Option<DriveCache>>> = Arc::new(Mutex::new(None));
}

// Cache duration - refresh drives every 30 seconds or when explicitly requested
const CACHE_DURATION: Duration = Duration::from_secs(30);

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
    let mut devices = Vec::new();

    unsafe {
        // Initialize COM
        if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
            return devices;
        }

        // Create IPortableDeviceManager
        let device_manager: IPortableDeviceManager =
            match CoCreateInstance(&PortableDeviceManager, None, CLSCTX_INPROC_SERVER) {
                Ok(dm) => dm,
                Err(_) => {
                    CoUninitialize();
                    return devices;
                }
            };

        // Get device count
        let mut count: u32 = 0;
        if device_manager
            .GetDevices(std::ptr::null_mut(), &mut count)
            .is_err()
            || count == 0
        {
            CoUninitialize();
            return devices;
        }

        // Allocate array for device IDs
        let mut device_ids: Vec<PWSTR> = Vec::with_capacity(count as usize);
        device_ids.set_len(count as usize);

        // Get the device IDs
        if device_manager
            .GetDevices(device_ids.as_mut_ptr(), &mut count)
            .is_err()
        {
            CoUninitialize();
            return devices;
        }

        // Get friendly names
        for device_id in &device_ids {
            let mut name_len: u32 = 0;
            if device_manager
                .GetDeviceFriendlyName(*device_id, PWSTR::null(), &mut name_len)
                .is_ok()
                && name_len > 0
            {
                let mut buffer: Vec<u16> = vec![0; name_len as usize];
                if device_manager
                    .GetDeviceFriendlyName(*device_id, PWSTR(buffer.as_mut_ptr()), &mut name_len)
                    .is_ok()
                {
                    let name = String::from_utf16_lossy(&buffer[..(name_len as usize - 1)]);
                    devices.push(name);
                }
            }
        }

        CoUninitialize();
    }

    devices
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
                Err(_) => {
                    // Could not open, still include as unknown removable
                    drives.push(RawDriveInfo {
                        device_path: path.clone(),
                        total_bytes: None,
                        is_removable: true,
                        bus_type: None,
                    });
                    continue;
                }
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
    drives.extend(list_portable_devices().into_iter().map(|name| DriveInfo {
        display: name.clone(),
        path: PathBuf::from(&name),
        is_removable: true,
        total_space: None,
        free_space: None,
        device_path: Some(name),
    }));

    // println!("Portable Devices: {:#?}", list_portable_devices());

    // Merge raw/unmounted drives
    // drives.extend(list_raw_drives().into_iter().map(|raw| DriveInfo {
    //     display: raw.device_path.clone(),
    //     path: PathBuf::from(&raw.device_path),
    //     is_removable: raw.is_removable,
    //     total_space: raw.total_bytes,
    //     free_space: None,
    //     device_path: Some(raw.device_path),
    // }));

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
