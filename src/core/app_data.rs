//! Where EdenExplorer keeps its settings and data files.
//!
//! Normally that's `%LOCALAPPDATA%\ExplorerEden`. In **portable mode** it's an
//! `EdenExplorerData` folder next to `EdenExplorer.exe` instead, so the app and
//! everything it remembers can live on a USB stick or in a synced folder.
//! Portable mode is on whenever that folder exists - create it by hand before
//! the first launch, or use Settings > Advanced > Portable Mode, which copies
//! the current data across (`enable_portable_mode`) or back
//! (`disable_portable_mode`).

use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// Name of the data folder that switches portable mode on.
pub const PORTABLE_DIR_NAME: &str = "EdenExplorerData";

/// The resolved data folder, cached after the first lookup and replaced when
/// portable mode is switched on or off at runtime.
static DATA_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

/// The folder `EdenExplorer.exe` is in (looked up once; the Settings page
/// asks every frame whether the app is portable).
fn exe_dir() -> Option<PathBuf> {
    static EXE_DIR: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    EXE_DIR
        .get_or_init(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(Path::to_path_buf))
        })
        .clone()
}

/// `EdenExplorerData` next to the exe, whether or not it exists.
pub fn portable_dir() -> Option<PathBuf> {
    exe_dir().map(|dir| dir.join(PORTABLE_DIR_NAME))
}

/// `%LOCALAPPDATA%\ExplorerEden`, the non-portable location.
pub fn installed_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("ExplorerEden"))
}

fn resolve() -> Option<PathBuf> {
    match portable_dir() {
        Some(dir) if dir.is_dir() => Some(dir),
        _ => installed_dir(),
    }
}

/// The folder every settings/data file lives in. Callers create it (or any
/// subfolder) on write, as before.
pub fn data_dir() -> Option<PathBuf> {
    if let Ok(cached) = DATA_DIR.read()
        && let Some(dir) = cached.as_ref()
    {
        return Some(dir.clone());
    }
    let dir = resolve()?;
    if let Ok(mut cached) = DATA_DIR.write() {
        *cached = Some(dir.clone());
    }
    Some(dir)
}

pub fn is_portable() -> bool {
    match (data_dir(), portable_dir()) {
        (Some(data), Some(portable)) => data == portable,
        _ => false,
    }
}

fn set_data_dir(dir: PathBuf) {
    if let Ok(mut cached) = DATA_DIR.write() {
        *cached = Some(dir);
    }
}

/// Copies everything in `from` into `to` (creating it), overwriting files
/// that already exist there.
fn copy_dir_all(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    if !from.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Switches to portable mode: copies the current data into `EdenExplorerData`
/// next to the exe and saves there from now on. Fails (changing nothing) if
/// that folder can't be written - e.g. the exe is in Program Files.
pub fn enable_portable_mode() -> std::io::Result<PathBuf> {
    let portable = portable_dir()
        .ok_or_else(|| std::io::Error::other("Couldn't find the EdenExplorer.exe folder."))?;
    let current = data_dir();
    if let Err(err) = copy_between(current.as_deref(), &portable) {
        // Don't leave a half-copied folder behind: its mere existence would
        // turn portable mode on at the next launch.
        let _ = std::fs::remove_dir_all(&portable);
        return Err(err);
    }
    set_data_dir(portable.clone());
    Ok(portable)
}

/// Switches portable mode off: copies the portable data back to
/// `%LOCALAPPDATA%\ExplorerEden` (replacing what's there) and, once that
/// has succeeded, removes the `EdenExplorerData` folder.
pub fn disable_portable_mode() -> std::io::Result<PathBuf> {
    let installed = installed_dir()
        .ok_or_else(|| std::io::Error::other("Couldn't find the local app data folder."))?;
    let portable = portable_dir();
    copy_between(portable.as_deref(), &installed)?;
    if let Some(portable) = portable
        && portable.is_dir()
    {
        std::fs::remove_dir_all(&portable)?;
    }
    set_data_dir(installed.clone());
    Ok(installed)
}

fn copy_between(from: Option<&Path>, to: &Path) -> std::io::Result<()> {
    match from {
        Some(from) if from != to => copy_dir_all(from, to),
        _ => std::fs::create_dir_all(to),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "eden_app_data_{name}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn copy_dir_all_copies_nested_files_and_overwrites() {
        let from = temp("from");
        let to = temp("to");
        std::fs::create_dir_all(from.join("favorites")).unwrap();
        std::fs::write(from.join("settings.bin"), b"new").unwrap();
        std::fs::write(from.join("favorites").join("drive_C.bin"), b"fav").unwrap();
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(to.join("settings.bin"), b"old").unwrap();

        copy_dir_all(&from, &to).unwrap();

        assert_eq!(std::fs::read(to.join("settings.bin")).unwrap(), b"new");
        assert_eq!(std::fs::read(to.join("favorites").join("drive_C.bin")).unwrap(), b"fav");
        let _ = std::fs::remove_dir_all(&from);
        let _ = std::fs::remove_dir_all(&to);
    }

    #[test]
    fn copy_between_same_folder_is_a_no_op() {
        let dir = temp("same");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.bin"), b"x").unwrap();
        copy_between(Some(&dir), &dir).unwrap();
        assert_eq!(std::fs::read(dir.join("a.bin")).unwrap(), b"x");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
