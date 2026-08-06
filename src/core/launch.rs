use std::path::PathBuf;

#[derive(Debug, Default)]
pub struct LaunchOptions {
    pub paths: Vec<PathBuf>,
    pub new_window: bool,
}

pub fn parse_args<I, S>(args: I) -> Result<LaunchOptions, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut options = LaunchOptions::default();
    let mut after_double_dash = false;

    for raw in args.into_iter().skip(1) {
        let arg = raw.into();
        if !after_double_dash && arg == "--" {
            after_double_dash = true;
        } else if !after_double_dash && arg == "--new-window" {
            options.new_window = true;
        } else if !after_double_dash && arg.starts_with('-') {
            return Err(format!("Unknown option: {arg}"));
        } else if !arg.is_empty() {
            options.paths.push(PathBuf::from(arg));
        }
    }

    Ok(options)
}

pub fn existing_directories(options: &LaunchOptions) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::with_capacity(options.paths.len());
    for path in &options.paths {
        if !path.is_dir() {
            return Err(format!("Not an existing directory: {}", path.display()));
        }
        paths.push(path.clone());
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_paths_and_new_window() {
        let options = parse_args([
            "EdenExplorer.exe",
            "--new-window",
            "C:\\Projects",
            "D:\\Archive",
        ])
        .unwrap();

        assert!(options.new_window);
        assert_eq!(
            options.paths,
            vec![PathBuf::from("C:\\Projects"), PathBuf::from("D:\\Archive")]
        );
    }

    #[test]
    fn allows_dash_prefixed_paths_after_double_dash() {
        let options = parse_args(["EdenExplorer.exe", "--", "-folder"]).unwrap();
        assert_eq!(options.paths, vec![PathBuf::from("-folder")]);
    }

    #[test]
    fn rejects_unknown_options() {
        let error = parse_args(["EdenExplorer.exe", "--unknown"]).unwrap_err();
        assert!(error.contains("--unknown"));
    }
}

mod windows_instance {
    use super::*;
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::Mutex;
    use std::thread;
    use std::time::Duration;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Foundation::ERROR_ALREADY_EXISTS;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Foundation::{GetLastError, HWND, LPARAM, WPARAM};
    use windows::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SW_RESTORE, SendMessageW, SetForegroundWindow, ShowWindow, WM_COPYDATA,
    };
    use windows::core::PCWSTR;

    const INSTANCE_MUTEX: &str = "Local\\EdenExplorer.SingleInstance.v1";
    const WINDOW_TITLE: &str = "EdenExplorer";
    const COPYDATA_ID: usize = 0x4544_454e;

    static FORWARDED_PATHS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    fn wide(value: &str) -> Vec<u16> {
        std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub struct InstanceGuard {
        handle: HANDLE,
    }

    impl Drop for InstanceGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = ReleaseMutex(self.handle);
                let _ = CloseHandle(self.handle);
            }
        }
    }

    pub fn acquire_or_forward(paths: &[PathBuf]) -> Result<Option<InstanceGuard>, String> {
        let name = wide(INSTANCE_MUTEX);
        let handle = unsafe { CreateMutexW(None, true, PCWSTR(name.as_ptr())) }
            .map_err(|e| format!("Could not create the instance mutex: {e}"))?;

        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            let result = forward_paths(paths);
            unsafe {
                let _ = CloseHandle(handle);
            }
            result.map(|_| None)
        } else {
            Ok(Some(InstanceGuard { handle }))
        }
    }

    fn forward_paths(paths: &[PathBuf]) -> Result<(), String> {
        let mut payload: Vec<u16> = Vec::new();
        for path in paths {
            payload.extend(path.as_os_str().encode_wide());
            payload.push(0);
        }
        payload.push(0);

        let title = wide(WINDOW_TITLE);
        let mut hwnd = HWND::default();
        for _ in 0..30 {
            hwnd = unsafe { FindWindowW(None, PCWSTR(title.as_ptr())) }.unwrap_or_default();
            if !hwnd.is_invalid() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        if hwnd.is_invalid() {
            return Err("The running EdenExplorer window was not ready.".to_string());
        }

        let data = COPYDATASTRUCT {
            dwData: COPYDATA_ID,
            cbData: (payload.len() * size_of::<u16>()) as u32,
            lpData: payload.as_ptr() as *mut c_void,
        };
        let mut delivered = false;
        for _ in 0..20 {
            let result = unsafe {
                SendMessageW(
                    hwnd,
                    WM_COPYDATA,
                    Some(WPARAM(0)),
                    Some(LPARAM(&data as *const COPYDATASTRUCT as isize)),
                )
            };
            if result.0 != 0 {
                delivered = true;
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        if !delivered {
            return Err("The running EdenExplorer window did not accept the request.".to_string());
        }
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
        Ok(())
    }

    pub fn receive_copydata(lparam: LPARAM) -> bool {
        let data = unsafe { (lparam.0 as *const COPYDATASTRUCT).as_ref() };
        let Some(data) = data else { return false };
        if data.dwData != COPYDATA_ID || data.cbData == 0 || data.lpData.is_null() {
            return false;
        }

        let units = data.cbData as usize / size_of::<u16>();
        let words = unsafe { std::slice::from_raw_parts(data.lpData as *const u16, units) };
        let paths = words
            .split(|unit| *unit == 0)
            .take_while(|part| !part.is_empty())
            .map(String::from_utf16_lossy)
            .map(PathBuf::from)
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        if let Ok(mut pending) = FORWARDED_PATHS.lock() {
            pending.extend(paths);
        }
        true
    }

    pub fn take_forwarded_paths() -> Vec<PathBuf> {
        FORWARDED_PATHS
            .lock()
            .map(|mut paths| std::mem::take(&mut *paths))
            .unwrap_or_default()
    }
}

pub use windows_instance::{
    InstanceGuard, acquire_or_forward, receive_copydata, take_forwarded_paths,
};

#[cfg(not(windows))]
pub fn take_forwarded_paths() -> Vec<PathBuf> {
    Vec::new()
}
