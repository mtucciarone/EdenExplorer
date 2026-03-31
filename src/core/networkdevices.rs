use crossbeam_channel::{Receiver, Sender, unbounded};
use std::thread;
use windows::Win32::NetworkManagement::NetManagement::{
    MAX_PREFERRED_LENGTH, NetApiBufferFree, NetServerEnum, SERVER_INFO_101, SV_TYPE_ALL,
};
use windows::core::PWSTR;

#[derive(Default)]
pub struct NetworkDevicesState {
    pub devices: Vec<NetworkDevice>,
    pub loading: bool,
    pub sender: Option<Sender<NetworkDevice>>,
    pub receiver: Option<Receiver<NetworkDevice>>,
}

#[derive(Clone)]
pub struct NetworkDevice {
    pub name: String,
    pub ip: Option<String>,
}

/// Get length of a PWSTR (like `wcslen`)
fn pwstr_len(p: PWSTR) -> usize {
    if p.is_null() {
        return 0;
    }
    unsafe {
        let mut len = 0;
        while *p.0.add(len) != 0 {
            len += 1;
        }
        len
    }
}

pub fn enumerate_lan_devices() -> Vec<NetworkDevice> {
    let mut devices = Vec::new();

    unsafe {
        let mut bufptr: *mut u8 = std::ptr::null_mut();
        let mut entries_read: u32 = 0;
        let mut total_entries: u32 = 0;
        let mut resume_handle: u32 = 0;

        let res = NetServerEnum(
            PWSTR::null(),            // server name
            101,                      // level
            &mut bufptr,              // buffer
            MAX_PREFERRED_LENGTH,     // preferred max
            &mut entries_read,        // entries read
            &mut total_entries,       // total entries
            SV_TYPE_ALL,              // server type
            PWSTR::null(),            // domain
            Some(&mut resume_handle), // resume handle (Option<*mut u32>)
        );

        const NERR_SUCCESS: u32 = 0;

        if res == NERR_SUCCESS && !bufptr.is_null() {
            let entries =
                std::slice::from_raw_parts(bufptr as *const SERVER_INFO_101, entries_read as usize);

            for entry in entries {
                let name = if !entry.sv101_name.is_null() {
                    let pwstr = entry.sv101_name;
                    String::from_utf16_lossy(std::slice::from_raw_parts(pwstr.0, pwstr_len(pwstr)))
                } else {
                    "Unknown".to_string()
                };

                devices.push(NetworkDevice { name, ip: None });
            }

            // Correct wrapper for NetApiBufferFree
            let _ = NetApiBufferFree(Some(bufptr as *const std::ffi::c_void));
        }
    }

    devices
}

impl NetworkDevicesState {
    pub fn start_loading(&mut self) {
        if self.loading {
            return; // Already loading
        }

        let (tx, rx) = unbounded();
        self.sender = Some(tx.clone());
        self.receiver = Some(rx);
        self.loading = true;

        // Spawn background thread
        thread::spawn(move || {
            for device in crate::core::networkdevices::enumerate_lan_devices() {
                let _ = tx.send(device); // Send each device as it arrives
            }
        });
    }

    pub fn update(&mut self) {
        if let Some(rx) = &self.receiver {
            while let Ok(device) = rx.try_recv() {
                self.devices.push(device);
            }

            // Stop loading if channel is empty
            if self.loading && self.devices.len() > 0 && rx.is_empty() {
                self.loading = false;
            }
        }
    }
}
