//! Creates Windows `.lnk` shortcut files via the shell's `IShellLinkW`/
//! `IPersistFile` COM interfaces - the same mechanism Explorer's own
//! "Create shortcut" uses. Runs synchronously on the calling (UI) thread,
//! same as every other native COM call in this app - `main.rs` already
//! calls `CoInitializeEx` once at startup, so no per-call init is needed
//! here.

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::core::{Interface, PCWSTR};

fn to_wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// Creates a `.lnk` shortcut at `shortcut_path` pointing to `target`.
pub fn create_shortcut(target: &Path, shortcut_path: &Path) -> windows::core::Result<()> {
    unsafe {
        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_ALL)?;

        let target_wide = to_wide(target);
        shell_link.SetPath(PCWSTR(target_wide.as_ptr()))?;

        // Matches Explorer's own "Create shortcut": a shortcut to a file
        // (not a folder) also gets a working directory, so launching it
        // starts the target app in a sensible location instead of none at
        // all. Best-effort - a failure here shouldn't stop the shortcut
        // itself from being created.
        if target.is_file()
            && let Some(parent) = target.parent()
        {
            let dir_wide = to_wide(parent);
            let _ = shell_link.SetWorkingDirectory(PCWSTR(dir_wide.as_ptr()));
        }

        let persist_file: windows::Win32::System::Com::IPersistFile = shell_link.cast()?;
        let path_wide = to_wide(shortcut_path);
        persist_file.Save(PCWSTR(path_wide.as_ptr()), true)?;
    }
    Ok(())
}

/// Picks a non-colliding `"<name> - Shortcut.lnk"` (then `"(2)"`, `"(3)"`,
/// ...) inside `dir` for `target`'s own file/folder name - matches
/// Explorer's own naming convention for a same-folder shortcut.
pub fn shortcut_file_name(target: &Path, dir: &Path) -> PathBuf {
    let base = target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Shortcut");

    let mut candidate = dir.join(format!("{base} - Shortcut.lnk"));
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{base} - Shortcut ({n}).lnk"));
        n += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_file_name_avoids_existing_files() {
        let dir = std::env::temp_dir().join(format!(
            "eden_shortcut_name_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let target = dir.join("report.pdf");
        let first = shortcut_file_name(&target, &dir);
        assert_eq!(first, dir.join("report.pdf - Shortcut.lnk"));

        std::fs::write(&first, b"").unwrap();
        let second = shortcut_file_name(&target, &dir);
        assert_eq!(second, dir.join("report.pdf - Shortcut (2).lnk"));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
