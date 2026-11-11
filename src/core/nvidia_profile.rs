//! Tell the NVIDIA driver that Eden is a desktop app, not a game.
//!
//! With "G-SYNC for windowed mode" on, the driver ties the monitor's refresh
//! rate to whichever focused app presents straight to the screen. Eden only
//! draws when something changes, so an idle window drags the whole display down
//! to the panel's lowest rate (40 Hz on a 240 Hz monitor), which flickers and
//! makes the cursor stutter. Vsync doesn't change this; only the driver's
//! per-application profile does. NVIDIA ships such a profile for Explorer and
//! for browsers ("G-SYNC" forced off). Eden is unknown to the driver, so it
//! registers the same setting for its own exe.
//!
//! This must run before the graphics device is created: the driver reads the
//! profile when the device is made, so a later write only helps the next launch.
//!
//! Everything here is best effort. Without an NVIDIA driver, `nvapi64.dll` is
//! missing and nothing happens.

use std::ffi::c_void;

/// "G-SYNC" application override (`VRR_APP_OVERRIDE_ID` in NvApiDriverSettings.h).
const VRR_APP_OVERRIDE_ID: u32 = 0x10A8_79CF;
/// `VRR_APP_OVERRIDE_FORCE_OFF`, the value NVIDIA's own Explorer profile uses.
const VRR_APP_OVERRIDE_FORCE_OFF: u32 = 1;

const PROFILE_NAME: &str = "EdenExplorer";

const NVAPI_OK: i32 = 0;

/// `NvAPI_UnicodeString`: 2048 UTF-16 units, NUL terminated.
type UnicodeString = [u16; 2048];

/// One value of an `NVDRS_SETTING`. In the SDK this is a union of a DWORD, a
/// string and a binary blob (a length plus 4096 bytes); only the DWORD is used.
#[repr(C)]
struct SettingValue {
    dword: u32,
    rest: [u8; 4096],
}

/// `NVDRS_SETTING`, version 1.
#[repr(C)]
struct Setting {
    version: u32,
    name: UnicodeString,
    id: u32,
    kind: u32,
    location: u32,
    is_current_predefined: u32,
    is_predefined_valid: u32,
    predefined: SettingValue,
    current: SettingValue,
}

/// `NVDRS_PROFILE`, version 1.
#[repr(C)]
struct Profile {
    version: u32,
    name: UnicodeString,
    gpu_support: u32,
    is_predefined: u32,
    num_apps: u32,
    num_settings: u32,
}

/// `NVDRS_APPLICATION`, version 1.
#[repr(C)]
struct Application {
    version: u32,
    is_predefined: u32,
    app_name: UnicodeString,
    friendly_name: UnicodeString,
    launcher: UnicodeString,
}

/// NVAPI versions a struct as its size with the version number in the high word.
const fn struct_version<T>(version: u32) -> u32 {
    size_of::<T>() as u32 | (version << 16)
}

type Handle = *mut c_void;

/// The DRS functions Eden needs, looked up by their ids from nvapi_interface.h.
struct NvApi {
    create_session: unsafe extern "C" fn(*mut Handle) -> i32,
    destroy_session: unsafe extern "C" fn(Handle) -> i32,
    load_settings: unsafe extern "C" fn(Handle) -> i32,
    save_settings: unsafe extern "C" fn(Handle) -> i32,
    find_application:
        unsafe extern "C" fn(Handle, *const u16, *mut Handle, *mut Application) -> i32,
    find_profile: unsafe extern "C" fn(Handle, *const u16, *mut Handle) -> i32,
    create_profile: unsafe extern "C" fn(Handle, *mut Profile, *mut Handle) -> i32,
    create_application: unsafe extern "C" fn(Handle, Handle, *mut Application) -> i32,
    get_setting: unsafe extern "C" fn(Handle, Handle, u32, *mut Setting) -> i32,
    set_setting: unsafe extern "C" fn(Handle, Handle, *mut Setting) -> i32,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

/// Only look in System32, where the driver installs nvapi64.dll.
const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x0000_0800;

impl NvApi {
    /// Load nvapi64.dll and initialise it. `None` when there is no NVIDIA driver.
    // Each transmute targets the type of the field it fills, which is written
    // out once, in the struct.
    #[allow(clippy::missing_transmute_annotations)]
    fn load() -> Option<Self> {
        let dll: Vec<u16> = "nvapi64.dll\0".encode_utf16().collect();
        // SAFETY: plain Win32 calls with NUL-terminated names. The library is
        // left loaded for the life of the process.
        let query = unsafe {
            let module = LoadLibraryExW(
                dll.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            );
            if module.is_null() {
                return None;
            }
            let query = GetProcAddress(module, c"nvapi_QueryInterface".as_ptr().cast());
            if query.is_null() {
                return None;
            }
            std::mem::transmute::<*mut c_void, unsafe extern "C" fn(u32) -> *mut c_void>(query)
        };

        // Every NVAPI function is fetched by id; a null means this driver
        // doesn't have it.
        macro_rules! function {
            ($id:expr) => {{
                // SAFETY: `query` is nvapi_QueryInterface, and the id belongs
                // to a function with the signature of the field it is stored in.
                let pointer = unsafe { query($id) };
                if pointer.is_null() {
                    return None;
                }
                unsafe { std::mem::transmute::<*mut c_void, _>(pointer) }
            }};
        }

        let initialize: unsafe extern "C" fn() -> i32 = function!(0x0150_E828);
        let api = Self {
            create_session: function!(0x0694_D52E),
            destroy_session: function!(0xDAD9_CFF8),
            load_settings: function!(0x375D_BD6B),
            save_settings: function!(0xFCBC_7E14),
            find_application: function!(0xEEE5_66B2),
            find_profile: function!(0x7E4A_9A0B),
            create_profile: function!(0xCC17_6068),
            create_application: function!(0x4347_A9DE),
            get_setting: function!(0x73BF_8338),
            set_setting: function!(0x577D_D202),
        };
        // SAFETY: NvAPI_Initialize takes no arguments.
        (unsafe { initialize() } == NVAPI_OK).then_some(api)
    }
}

/// What the driver already knows about this exe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Existing {
    /// No profile lists the exe.
    NoProfile,
    /// A profile lists the exe but says nothing about G-SYNC.
    ProfileWithoutSetting,
    /// A profile lists the exe and already has a G-SYNC value.
    ProfileWithSetting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Plan {
    Nothing,
    AddSetting,
    CreateProfileAndAddSetting,
}

/// A G-SYNC value that is already there is left alone, whatever it is, so
/// someone who turns G-SYNC back on for Eden in the NVIDIA settings keeps it.
fn plan(existing: Existing) -> Plan {
    match existing {
        Existing::NoProfile => Plan::CreateProfileAndAddSetting,
        Existing::ProfileWithoutSetting => Plan::AddSetting,
        Existing::ProfileWithSetting => Plan::Nothing,
    }
}

fn unicode_string(text: &str) -> Option<UnicodeString> {
    let mut buffer = [0u16; 2048];
    for (index, unit) in text.encode_utf16().enumerate() {
        // Keep the last unit as the terminating NUL.
        if index + 1 >= buffer.len() {
            return None;
        }
        buffer[index] = unit;
    }
    Some(buffer)
}

/// The driver matches applications by lower-case file name.
fn exe_name() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.file_name()?.to_str()?.to_lowercase())
}

/// What `disable_gsync_for_this_exe` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// No NVIDIA driver, or it refused; nothing was changed.
    Unavailable,
    /// The exe already had a G-SYNC value.
    AlreadySet,
    /// The setting was written.
    Written,
}

/// Make the driver keep G-SYNC off for this exe. Call before creating the
/// graphics device.
///
/// Asking the driver takes over 100 ms, nearly all of it the driver loading its
/// profile database, so Eden only waits for the answer when it can't tell that
/// the setting is in place: on the first launch, and after the driver changes
/// (a clean driver install wipes profiles). Otherwise the same check runs in
/// the background, where it repairs a profile someone deleted in time for the
/// next launch.
pub fn ensure_gsync_disabled() {
    let Some(stamp) = driver_stamp() else {
        return;
    };
    let marker = marker_path();
    let confirmed = marker
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .is_some_and(|saved| saved == stamp);
    if confirmed {
        std::thread::spawn(disable_gsync_for_this_exe);
        return;
    }
    if disable_gsync_for_this_exe() != Outcome::Unavailable
        && let Some(path) = marker
    {
        if let Some(folder) = path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let _ = std::fs::write(path, stamp);
    }
}

/// Identifies this exe name and this driver install. `None` without an NVIDIA
/// driver, which is also the cheapest way to find that out.
fn driver_stamp() -> Option<String> {
    let system_root = std::env::var_os("SystemRoot")?;
    let dll = std::path::Path::new(&system_root)
        .join("System32")
        .join("nvapi64.dll");
    let metadata = std::fs::metadata(dll).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(stamp_text(&exe_name()?, modified, metadata.len()))
}

fn stamp_text(exe: &str, driver_modified: u64, driver_size: u64) -> String {
    format!("{exe}|{driver_modified}|{driver_size}")
}

fn marker_path() -> Option<std::path::PathBuf> {
    Some(
        dirs::data_local_dir()?
            .join("ExplorerEden")
            .join("nvidia_profile.txt"),
    )
}

fn disable_gsync_for_this_exe() -> Outcome {
    let (Some(api), Some(exe)) = (NvApi::load(), exe_name()) else {
        return Outcome::Unavailable;
    };
    let mut session: Handle = std::ptr::null_mut();
    // SAFETY: `session` is a valid out pointer.
    if unsafe { (api.create_session)(&mut session) } != NVAPI_OK {
        return Outcome::Unavailable;
    }
    let outcome = write_setting(&api, session, &exe).unwrap_or(Outcome::Unavailable);
    // SAFETY: `session` came from create_session and is not used afterwards.
    unsafe { (api.destroy_session)(session) };
    outcome
}

fn write_setting(api: &NvApi, session: Handle, exe: &str) -> Option<Outcome> {
    let exe_wide = unicode_string(exe)?;
    let profile_wide = unicode_string(PROFILE_NAME)?;
    let ok = |status: i32| (status == NVAPI_OK).then_some(());

    // SAFETY: every pointer passed below is to a live, correctly versioned
    // struct or a NUL-terminated NvAPI_UnicodeString, and `session` is open.
    unsafe {
        ok((api.load_settings)(session))?;

        let mut profile: Handle = std::ptr::null_mut();
        let mut application: Box<Application> = Box::new(std::mem::zeroed());
        application.version = struct_version::<Application>(1);
        let listed =
            (api.find_application)(session, exe_wide.as_ptr(), &mut profile, &mut *application)
                == NVAPI_OK;

        let existing = if !listed {
            Existing::NoProfile
        } else {
            let mut setting: Box<Setting> = Box::new(std::mem::zeroed());
            setting.version = struct_version::<Setting>(1);
            if (api.get_setting)(session, profile, VRR_APP_OVERRIDE_ID, &mut *setting) == NVAPI_OK {
                Existing::ProfileWithSetting
            } else {
                Existing::ProfileWithoutSetting
            }
        };

        match plan(existing) {
            Plan::Nothing => return Some(Outcome::AlreadySet),
            Plan::AddSetting => {}
            Plan::CreateProfileAndAddSetting => {
                // A profile of ours may be left from a differently named exe.
                if (api.find_profile)(session, profile_wide.as_ptr(), &mut profile) != NVAPI_OK {
                    let mut new_profile: Box<Profile> = Box::new(std::mem::zeroed());
                    new_profile.version = struct_version::<Profile>(1);
                    new_profile.name = profile_wide;
                    ok((api.create_profile)(
                        session,
                        &mut *new_profile,
                        &mut profile,
                    ))?;
                }
                let mut new_application: Box<Application> = Box::new(std::mem::zeroed());
                new_application.version = struct_version::<Application>(1);
                new_application.app_name = exe_wide;
                new_application.friendly_name = profile_wide;
                ok((api.create_application)(
                    session,
                    profile,
                    &mut *new_application,
                ))?;
            }
        }

        let mut setting: Box<Setting> = Box::new(std::mem::zeroed());
        setting.version = struct_version::<Setting>(1);
        setting.id = VRR_APP_OVERRIDE_ID;
        setting.current.dword = VRR_APP_OVERRIDE_FORCE_OFF;
        ok((api.set_setting)(session, profile, &mut *setting))?;
        ok((api.save_settings)(session))?;
    }
    Some(Outcome::Written)
}

#[cfg(test)]
#[path = "../../tests/unit/nvidia_profile.rs"]
mod tests;
