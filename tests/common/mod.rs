use std::path::PathBuf;

pub fn missing_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("eden-test-missing-{}-{name}", std::process::id()))
}

pub fn init_com() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
    }
}
