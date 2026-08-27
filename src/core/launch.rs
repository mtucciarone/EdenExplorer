use crate::core::fs::{MY_PC_PATH, MY_RECYCLE_BIN_PATH};
use fluent_bundle::{FluentArgs, FluentBundle, FluentResource};
use rust_embed::RustEmbed;
use std::borrow::Cow;
use std::ffi::c_void;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use unic_langid::LanguageIdentifier;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Foundation::ERROR_ALREADY_EXISTS;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Foundation::{GetLastError, HWND, LPARAM, WPARAM};
use windows::Win32::Globalization::GetUserDefaultUILanguage;
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, SW_RESTORE, SendMessageW, SetForegroundWindow, ShowWindow, WM_COPYDATA,
};
use windows::core::PCWSTR;

#[derive(RustEmbed)]
#[folder = "locales/"]
struct LaunchLocalizations;

const INSTANCE_MUTEX: &str = "Local\\EdenExplorer.SingleInstance.v1";
const WINDOW_TITLE: &str = "EdenExplorer";
const COPYDATA_ID: usize = 0x4544_454e;
static FORWARDED_PATHS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
const LANGUAGE_MAP: &[(u16, &str)] = &[
    // English
    (0x0409, "en-US"),
    (0x0809, "en-US"),
    (0x0C09, "en-US"),
    (0x1009, "en-US"),
    (0x1409, "en-US"),
    (0x1809, "en-US"),
    (0x1C09, "en-US"),
    (0x2009, "en-US"),
    (0x2409, "en-US"),
    (0x2809, "en-US"),
    (0x2C09, "en-US"),
    (0x3009, "en-US"),
    (0x3409, "en-US"),
    // Indonesian
    (0x0421, "id-ID"),
    // Japanese
    (0x0411, "ja-JP"),
    // Chinese
    (0x0404, "zh-TW"),
    (0x0804, "zh-CN"),
    (0x0C04, "zh-HK"),
];

pub struct InstanceGuard {
    handle: HANDLE,
}

#[derive(Debug, Default)]
pub struct LaunchOptions {
    pub paths: Vec<PathBuf>,
    pub new_window: bool,
}

#[derive(Debug)]
pub enum LaunchError {
    UnknownOption(String),
    InvalidDirectory(PathBuf),
    MutexCreate(String),
    WindowNotReady,
    WindowRejected,
}

impl LaunchError {
    pub fn message(&self) -> String {
        let i18n = LaunchI18n::new();

        match self {
            Self::UnknownOption(option) => {
                let mut args = FluentArgs::new();
                args.set("option", option.as_str());
                i18n.tr_args("launch-unknown-option", &args)
            }

            Self::InvalidDirectory(path) => {
                let mut args = FluentArgs::new();
                args.set("path", path.display().to_string());
                i18n.tr_args("launch-invalid-directory", &args)
            }

            Self::MutexCreate(error) => {
                let mut args = FluentArgs::new();
                args.set("error", error.as_str());
                i18n.tr_args("launch-mutex-create", &args)
            }

            Self::WindowNotReady => i18n.tr("launch-window-not-ready"),

            Self::WindowRejected => i18n.tr("launch-window-rejected"),
        }
    }
}

struct LaunchI18n {
    primary: FluentBundle<FluentResource>,
    fallback: FluentBundle<FluentResource>,
}

impl LaunchI18n {
    fn new() -> Self {
        let locale = system_locale();
        println!("Windows locale: {}", locale);

        let langid = LanguageIdentifier::from_str(&locale)
            .unwrap_or_else(|_| LanguageIdentifier::from_str("en-US").unwrap());

        let mut primary = FluentBundle::new(vec![langid]);
        primary.set_use_isolating(false);
        load_locale(&mut primary, &locale);

        let mut fallback = FluentBundle::new(vec![LanguageIdentifier::from_str("en-US").unwrap()]);
        fallback.set_use_isolating(false);
        load_locale(&mut fallback, "en-US");

        Self { primary, fallback }
    }

    fn format_bundle(
        bundle: &FluentBundle<FluentResource>,
        key: &str,
        args: Option<&FluentArgs>,
    ) -> Option<String> {
        let message = bundle.get_message(key)?;
        let pattern = message.value()?;

        let mut errors = Vec::new();

        Some(
            bundle
                .format_pattern(pattern, args, &mut errors)
                .to_string(),
        )
    }

    fn tr(&self, key: &str) -> String {
        Self::format_bundle(&self.primary, key, None)
            .or_else(|| Self::format_bundle(&self.fallback, key, None))
            .unwrap_or_else(|| key.to_string())
    }

    fn tr_args(&self, key: &str, args: &FluentArgs) -> String {
        Self::format_bundle(&self.primary, key, Some(args))
            .or_else(|| Self::format_bundle(&self.fallback, key, Some(args)))
            .unwrap_or_else(|| key.to_string())
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.handle);
            let _ = CloseHandle(self.handle);
        }
    }
}

#[cfg(windows)]
pub fn system_locale() -> String {
    let langid = unsafe { GetUserDefaultUILanguage() };

    LANGUAGE_MAP
        .iter()
        .find(|(id, _)| *id == langid)
        .map(|(_, locale)| (*locale).to_string())
        .unwrap_or_else(|| "en-US".to_string())
}

pub fn parse_args<I, S>(args: I) -> Result<LaunchOptions, LaunchError>
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
            return Err(LaunchError::UnknownOption(arg));
        } else if !arg.is_empty() {
            options.paths.push(normalize_path(PathBuf::from(arg)));
        }
    }

    Ok(options)
}

pub fn existing_directories(options: &LaunchOptions) -> Result<Vec<PathBuf>, LaunchError> {
    let mut paths = Vec::with_capacity(options.paths.len());

    for path in &options.paths {
        if !is_virtual_path(path) && !path.is_dir() {
            return Err(LaunchError::InvalidDirectory(path.clone()));
        }

        paths.push(path.clone());
    }

    Ok(paths)
}

pub fn acquire_or_forward(paths: &[PathBuf]) -> Result<Option<InstanceGuard>, LaunchError> {
    let name = wide(INSTANCE_MUTEX);
    let handle = unsafe { CreateMutexW(None, true, PCWSTR(name.as_ptr())) }
        .map_err(|e| LaunchError::MutexCreate(e.to_string()))?;

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
        .filter(|path| is_virtual_path(path) || path.is_dir())
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

fn wide(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn load_locale(bundle: &mut FluentBundle<FluentResource>, locale: &str) {
    let path = format!("{}/main.ftl", locale);
    let file = match LaunchLocalizations::get(&path) {
        Some(file) => file,
        None => {
            println!("Missing locale file: {}", path);
            return;
        }
    };

    let source = match file.data {
        Cow::Borrowed(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) => text.to_owned(),
            Err(e) => {
                println!("UTF-8 error: {:?}", e);
                return;
            }
        },
        Cow::Owned(bytes) => match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(e) => {
                println!("UTF-8 error: {:?}", e);
                return;
            }
        },
    };

    let resource = match FluentResource::try_new(source) {
        Ok(resource) => resource,
        Err((_, errors)) => {
            println!("Fluent parse errors: {:?}", errors);
            return;
        }
    };

    if let Err(errors) = bundle.add_resource(resource) {
        println!("add_resource errors: {:?}", errors);
    }
}

fn forward_paths(paths: &[PathBuf]) -> Result<(), LaunchError> {
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
        return Err(LaunchError::WindowNotReady);
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
        return Err(LaunchError::WindowRejected);
    }
    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
    }
    Ok(())
}

pub fn normalize_path(path: PathBuf) -> PathBuf {
    let value = path.to_string_lossy();

    if value.eq_ignore_ascii_case(MY_PC_PATH)
        || value.eq_ignore_ascii_case("file:\\")
        || value.eq_ignore_ascii_case("shell:MyComputerFolder")
    {
        return PathBuf::from(MY_PC_PATH);
    }

    if value.eq_ignore_ascii_case(MY_RECYCLE_BIN_PATH)
        || value.eq_ignore_ascii_case("shell:RecycleBinFolder")
    {
        return PathBuf::from(MY_RECYCLE_BIN_PATH);
    }

    path
}

pub fn is_virtual_path(path: &Path) -> bool {
    matches!(
        path.to_string_lossy().as_ref(),
        MY_PC_PATH | MY_RECYCLE_BIN_PATH
    )
}

#[cfg(not(windows))]
pub fn take_forwarded_paths() -> Vec<PathBuf> {
    Vec::new()
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
        match error {
            LaunchError::UnknownOption(option) => {
                assert_eq!(option, "--unknown");
            }
            _ => panic!("unexpected error"),
        }
    }

    #[test]
    fn startup_i18n_loads() {
        let i18n = LaunchI18n::new();

        let value = i18n.tr("launch-window-not-ready");

        assert_ne!(value, "");
    }

    #[test]
    fn normalizes_my_pc_aliases() {
        let aliases = [
            "::MY_PC::",
            "file:\\",
            "shell:MyComputerFolder",
            "SHELL:MYCOMPUTERFOLDER",
        ];

        for alias in aliases {
            let options = parse_args(["EdenExplorer.exe", alias]).unwrap();

            assert_eq!(
                options.paths,
                vec![PathBuf::from(MY_PC_PATH)],
                "failed to normalize {alias:?}"
            );
        }
    }

    #[test]
    fn normalizes_recycle_bin_aliases() {
        let aliases = [
            "::RECYCLE_BIN::",
            "shell:RecycleBinFolder",
            "SHELL:RECYCLEBINFOLDER",
        ];

        for alias in aliases {
            let options = parse_args(["EdenExplorer.exe", alias]).unwrap();

            assert_eq!(
                options.paths,
                vec![PathBuf::from(MY_RECYCLE_BIN_PATH)],
                "failed to normalize {alias:?}"
            );
        }
    }

    #[test]
    fn preserves_normal_directories() {
        let options = parse_args(["EdenExplorer.exe", "C:\\Projects", "D:\\Archive"]).unwrap();

        assert_eq!(
            options.paths,
            vec![PathBuf::from("C:\\Projects"), PathBuf::from("D:\\Archive"),]
        );
    }

    #[test]
    fn normalizes_virtual_paths_before_directory_validation() {
        let options =
            parse_args(["EdenExplorer.exe", "file:\\", "shell:RecycleBinFolder"]).unwrap();

        let paths = existing_directories(&options).unwrap();

        assert_eq!(
            paths,
            vec![
                PathBuf::from(MY_PC_PATH),
                PathBuf::from(MY_RECYCLE_BIN_PATH),
            ]
        );
    }
}
