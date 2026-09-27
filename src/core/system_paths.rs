//! Absolute paths for the Windows programs this app launches.
//!
//! Starting a program by bare name (`Command::new("robocopy")`) makes Windows
//! search the app's own directory before System32. EdenExplorer is a portable
//! .exe that is often run straight from Downloads, so a malicious
//! `robocopy.exe` (or `cmd.exe`, ...) saved next to it would be run instead
//! of the real one. Resolving these to their real install locations closes
//! that hole.

use std::path::PathBuf;

/// `C:\Windows\System32`, from `%SystemRoot%` (falling back to the default
/// install location if the variable is somehow missing).
pub fn system32_dir() -> PathBuf {
    let root = std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("windir"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join("System32")
}

pub fn robocopy_exe() -> PathBuf {
    system32_dir().join("robocopy.exe")
}

pub fn cmd_exe() -> PathBuf {
    system32_dir().join("cmd.exe")
}

pub fn powershell_exe() -> PathBuf {
    system32_dir()
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe")
}

/// Windows Terminal: the Microsoft Store install's per-user app execution
/// alias (`%LOCALAPPDATA%\Microsoft\WindowsApps\wt.exe`) if present,
/// otherwise the first `wt.exe` on `PATH` (winget/scoop installs) - skipping
/// the directory EdenExplorer itself runs from, which is exactly the
/// location this module exists to avoid trusting.
pub fn windows_terminal_exe() -> Option<PathBuf> {
    let store_alias = dirs::data_local_dir()?
        .join("Microsoft")
        .join("WindowsApps")
        .join("wt.exe");
    // App execution aliases are reparse points that `Path::exists`/`is_file`
    // can't follow, so check the link itself instead.
    if store_alias.symlink_metadata().is_ok() {
        return Some(store_alias);
    }

    let own_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()));
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .filter(|dir| dir.is_absolute() && Some(dir) != own_dir.as_ref())
        .map(|dir| dir.join("wt.exe"))
        .find(|candidate| candidate.symlink_metadata().is_ok())
}
