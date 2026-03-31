use crate::core::fs::FileItem;
use crossbeam_channel::Sender;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::RwLock;
use windows::Win32::Devices::PortableDevices::*;
use windows::Win32::Foundation::{PROPERTYKEY, RPC_E_CHANGED_MODE};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::core::{GUID, HSTRING, PCWSTR, PWSTR};

pub const PORTABLE_PREFIX: &str = "portable://";
const PORTABLE_ROOT_OBJECT_ID: &str = "DEVICE";

#[derive(Clone)]
struct PortableObjectInfo {
    name: String,
    parent_id: Option<String>,
}

lazy_static::lazy_static! {
    static ref DEVICE_NAMES: RwLock<HashMap<String, String>> = RwLock::new(HashMap::new());
    static ref OBJECT_CACHE: RwLock<HashMap<(String, String), PortableObjectInfo>> =
        RwLock::new(HashMap::new());
}

pub fn make_portable_path(device_id: &str, object_id: &str) -> PathBuf {
    let encoded_device = encode_component(device_id);
    let encoded_object = encode_component(object_id);
    PathBuf::from(format!(
        "{PORTABLE_PREFIX}{encoded_device}/{encoded_object}"
    ))
}

pub fn is_portable_path(path: &PathBuf) -> bool {
    normalize_portable_str(&path.to_string_lossy()).is_some()
}

pub fn parse_portable_path(path: &PathBuf) -> Option<(String, String)> {
    let s = path.to_string_lossy();
    let rest = normalize_portable_str(&s)?;
    let mut parts = rest.splitn(2, '/');
    let device_enc = parts.next()?;
    let object_enc = parts.next().unwrap_or(PORTABLE_ROOT_OBJECT_ID);
    let device_id = decode_component(device_enc)?;
    if device_id.is_empty() {
        return None;
    }
    let object_id = decode_component(object_enc)?;
    Some((device_id, object_id))
}

pub fn is_portable_device_path(path: &PathBuf) -> bool {
    parse_portable_path(path)
        .map(|(_, object_id)| object_id == PORTABLE_ROOT_OBJECT_ID)
        .unwrap_or(false)
}

pub fn cache_device_name(device_id: &str, name: &str) {
    if let Ok(mut map) = DEVICE_NAMES.write() {
        map.insert(device_id.to_string(), name.to_string());
    }
    cache_object_info(device_id, PORTABLE_ROOT_OBJECT_ID, "DEVICE", None);
}

pub fn cache_object_info(device_id: &str, object_id: &str, name: &str, parent_id: Option<String>) {
    if let Ok(mut cache) = OBJECT_CACHE.write() {
        cache.insert(
            (device_id.to_string(), object_id.to_string()),
            PortableObjectInfo {
                name: name.to_string(),
                parent_id,
            },
        );
    }
}

pub fn build_breadcrumb_segments(path: &PathBuf) -> Option<Vec<(String, PathBuf)>> {
    let (device_id, object_id) = parse_portable_path(path)?;

    let device_name = DEVICE_NAMES
        .read()
        .ok()
        .and_then(|m| m.get(&device_id).cloned())
        .unwrap_or_else(|| device_id.clone());

    let mut chain: Vec<(String, String)> = Vec::new(); // (object_id, label)
    let mut current = object_id.clone();

    loop {
        let info = OBJECT_CACHE
            .read()
            .ok()
            .and_then(|m| m.get(&(device_id.clone(), current.clone())).cloned());

        let label = info
            .as_ref()
            .map(|i| i.name.clone())
            .unwrap_or_else(|| current.clone());

        chain.push((current.clone(), label));

        if current == PORTABLE_ROOT_OBJECT_ID {
            break;
        }

        let parent = info.and_then(|i| i.parent_id);
        match parent {
            Some(p) => current = p,
            None => {
                // fallback to DEVICE if parent unknown
                if current != PORTABLE_ROOT_OBJECT_ID {
                    chain.push((PORTABLE_ROOT_OBJECT_ID.to_string(), "DEVICE".to_string()));
                }
                break;
            }
        }
    }

    chain.reverse();

    let mut segments: Vec<(String, PathBuf)> = Vec::new();
    // Device label at root, points to DEVICE
    segments.push((
        device_name,
        make_portable_path(&device_id, PORTABLE_ROOT_OBJECT_ID),
    ));

    for (obj_id, label) in chain {
        segments.push((label, make_portable_path(&device_id, &obj_id)));
    }

    Some(segments)
}

pub fn scan_portable_async(path: PathBuf, tx: Sender<FileItem>) {
    std::thread::spawn(move || {
        let mut should_uninit = false;
        unsafe {
            let init = CoInitializeEx(None, COINIT_MULTITHREADED);
            if init.is_ok() {
                should_uninit = true;
            } else if init == RPC_E_CHANGED_MODE.into() {
                let init_sta = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                if init_sta.is_ok() {
                    should_uninit = true;
                }
            }
        }

        let Some((device_id, object_id)) = parse_portable_path(&path) else {
            if should_uninit {
                unsafe { CoUninitialize() };
            }
            return;
        };

        let Some(device) = open_device(&device_id) else {
            if should_uninit {
                unsafe { CoUninitialize() };
            }
            return;
        };

        let (content, props) = unsafe {
            let Ok(content) = device.Content() else {
                if should_uninit {
                    CoUninitialize();
                }
                return;
            };

            let Ok(props) = content.Properties() else {
                if should_uninit {
                    CoUninitialize();
                }
                return;
            };

            (content, props)
        };

        if object_id == PORTABLE_ROOT_OBJECT_ID {
            cache_object_info(&device_id, &object_id, "DEVICE", None);
        }

        let mut storage_object_id: Option<String> = None;
        if object_id != PORTABLE_ROOT_OBJECT_ID {
            let object_w = wide_z(&object_id);
            if let Ok(values) = unsafe { props.GetValues(PCWSTR(object_w.as_ptr()), None) } {
                storage_object_id = get_string_value(&values, &WPD_PROPERTY_STORAGE_OBJECT_ID);
            }
        }

        let mut enumerate_id = object_id.clone();
        if let Some(storage_id) = storage_object_id.as_ref() {
            if !storage_id.is_empty() && storage_id != &object_id {
                enumerate_id = storage_id.clone();
            }
        }

        let mut items = enumerate_portable_children(
            &content,
            &props,
            &device_id,
            &enumerate_id,
            object_id == PORTABLE_ROOT_OBJECT_ID,
            if enumerate_id != object_id {
                Some(&object_id)
            } else {
                None
            },
        );

        if items.is_empty() && enumerate_id != object_id {
            items = enumerate_portable_children(
                &content,
                &props,
                &device_id,
                &object_id,
                object_id == PORTABLE_ROOT_OBJECT_ID,
                None,
            );
        }

        for item in items {
            let _ = tx.send(item);
        }

        if should_uninit {
            unsafe { CoUninitialize() };
        }
    });
}

pub fn list_portable_devices_with_ids() -> Vec<(String, String)> {
    let mut devices = Vec::new();

    unsafe {
        let device_manager: IPortableDeviceManager =
            match CoCreateInstance(&PortableDeviceManager, None, CLSCTX_INPROC_SERVER) {
                Ok(dm) => dm,
                Err(_) => return devices,
            };

        let mut count: u32 = 0;
        if device_manager
            .GetDevices(std::ptr::null_mut(), &mut count)
            .is_err()
            || count == 0
        {
            return devices;
        }

        let mut device_ids: Vec<PWSTR> = Vec::with_capacity(count as usize);
        device_ids.set_len(count as usize);

        if device_manager
            .GetDevices(device_ids.as_mut_ptr(), &mut count)
            .is_err()
        {
            return devices;
        }

        for device_id_ptr in &device_ids {
            if device_id_ptr.is_null() {
                continue;
            }

            let device_id = pwstr_to_string(*device_id_ptr);

            let mut name_len: u32 = 0;
            let friendly_name = if device_manager
                .GetDeviceFriendlyName(*device_id_ptr, PWSTR::null(), &mut name_len)
                .is_ok()
                && name_len > 0
            {
                let mut buffer: Vec<u16> = vec![0; name_len as usize];
                if device_manager
                    .GetDeviceFriendlyName(
                        *device_id_ptr,
                        PWSTR(buffer.as_mut_ptr()),
                        &mut name_len,
                    )
                    .is_ok()
                {
                    String::from_utf16_lossy(&buffer[..(name_len as usize - 1)])
                } else {
                    device_id.clone()
                }
            } else {
                device_id.clone()
            };

            cache_device_name(&device_id, &friendly_name);

            devices.push((friendly_name, device_id));

            CoTaskMemFree(Some(device_id_ptr.0 as _));
        }
    }

    devices
}

fn open_device(device_id: &str) -> Option<IPortableDevice> {
    unsafe {
        let device: IPortableDevice =
            CoCreateInstance(&PortableDevice, None, CLSCTX_INPROC_SERVER).ok()?;
        let client_info: IPortableDeviceValues =
            CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER).ok()?;

        let client_name = HSTRING::from("EdenExplorer");
        let _ = client_info.SetStringValue(&WPD_CLIENT_NAME, PCWSTR(client_name.as_ptr()));
        let _ = client_info.SetUnsignedIntegerValue(&WPD_CLIENT_MAJOR_VERSION, 1);
        let _ = client_info.SetUnsignedIntegerValue(&WPD_CLIENT_MINOR_VERSION, 0);
        let _ = client_info.SetUnsignedIntegerValue(&WPD_CLIENT_REVISION, 0);

        let id_w = wide_z(device_id);
        device.Open(PCWSTR(id_w.as_ptr()), &client_info).ok()?;
        Some(device)
    }
}

fn get_string_value(values: &IPortableDeviceValues, key: &PROPERTYKEY) -> Option<String> {
    if let Ok(value) = unsafe { values.GetStringValue(key) } {
        if value.is_null() {
            return None;
        }
        let s = unsafe { pwstr_to_string(value) };
        unsafe {
            CoTaskMemFree(Some(value.0 as _));
        }
        Some(s)
    } else {
        None
    }
}

fn enumerate_portable_children(
    content: &IPortableDeviceContent,
    props: &IPortableDeviceProperties,
    device_id: &str,
    parent_id: &str,
    is_root: bool,
    parent_override: Option<&str>,
) -> Vec<FileItem> {
    let parent_w = wide_z(parent_id);
    let enum_ids = match unsafe { content.EnumObjects(0, PCWSTR(parent_w.as_ptr()), None) } {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut fetched = 0u32;
    let mut pending: Vec<FileItem> = Vec::new();
    let mut storage_only: Vec<FileItem> = Vec::new();

    loop {
        let mut object_ids = [PWSTR::null()];
        let next_result = unsafe { enum_ids.Next(&mut object_ids, &mut fetched as *mut u32) };
        if next_result.is_err() || fetched == 0 {
            break;
        }

        let object_id_ptr = object_ids[0];
        let object_id = unsafe { pwstr_to_string(object_id_ptr) };
        unsafe { CoTaskMemFree(Some(object_id_ptr.0 as _)) };

        let object_w = wide_z(&object_id);
        let Ok(values) = (unsafe { props.GetValues(PCWSTR(object_w.as_ptr()), None) }) else {
            continue;
        };

        let name = get_string_value(&values, &WPD_OBJECT_NAME)
            .or_else(|| get_string_value(&values, &WPD_OBJECT_ORIGINAL_FILE_NAME))
            .unwrap_or_else(|| object_id.clone());

        let parent_id = get_string_value(&values, &WPD_OBJECT_PARENT_ID);

        let content_type =
            get_guid_value(&values, &WPD_OBJECT_CONTENT_TYPE).unwrap_or(GUID::zeroed());
        let functional_category =
            get_guid_value(&values, &WPD_FUNCTIONAL_OBJECT_CATEGORY).unwrap_or(GUID::zeroed());

        let is_dir = content_type == WPD_CONTENT_TYPE_FOLDER
            || content_type == WPD_CONTENT_TYPE_FUNCTIONAL_OBJECT
            || functional_category == WPD_FUNCTIONAL_CATEGORY_STORAGE;

        let file_size = if is_dir {
            None
        } else {
            get_u64_value(&values, &WPD_OBJECT_SIZE)
        };

        let path_object_id = object_id.clone();
        let virtual_path = make_portable_path(device_id, &path_object_id);

        let parent_override = if let Some(override_id) = parent_override {
            Some(override_id.to_string())
        } else if is_root {
            Some(PORTABLE_ROOT_OBJECT_ID.to_string())
        } else {
            parent_id
        };
        cache_object_info(device_id, &path_object_id, &name, parent_override);

        let item = FileItem::new(name, virtual_path, is_dir, file_size, None, None);
        pending.push(item.clone());

        if is_root {
            if functional_category == WPD_FUNCTIONAL_CATEGORY_STORAGE {
                storage_only.push(item);
            }
        }
    }

    if is_root && !storage_only.is_empty() {
        storage_only
    } else {
        pending
    }
}

fn get_guid_value(values: &IPortableDeviceValues, key: &PROPERTYKEY) -> Option<GUID> {
    unsafe { values.GetGuidValue(key).ok() }
}

fn get_u64_value(values: &IPortableDeviceValues, key: &PROPERTYKEY) -> Option<u64> {
    unsafe { values.GetUnsignedLargeIntegerValue(key).ok() }
}

unsafe fn pwstr_to_string(pw: PWSTR) -> String {
    let mut len = 0usize;
    let mut ptr = pw.0;
    while !ptr.is_null() && *ptr != 0 {
        len += 1;
        ptr = ptr.add(1);
    }
    let slice = std::slice::from_raw_parts(pw.0, len);
    String::from_utf16_lossy(slice)
}

fn encode_component(input: &str) -> String {
    let mut out = String::new();
    for b in input.as_bytes() {
        let c = *b as char;
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' {
            out.push(c);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

fn decode_component(input: &str) -> Option<String> {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hex = &input[i + 1..i + 3];
            let val = u8::from_str_radix(hex, 16).ok()?;
            out.push(val);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn wide_z(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn normalize_portable_str(input: &str) -> Option<String> {
    let lower = input.to_ascii_lowercase();
    if lower.starts_with("portable://") {
        let rest = &input["portable://".len()..];
        return Some(rest.replace('\\', "/").trim_start_matches('/').to_string());
    }
    if lower.starts_with("portable:") {
        let rest = &input["portable:".len()..];
        return Some(rest.replace('\\', "/").trim_start_matches('/').to_string());
    }
    None
}
