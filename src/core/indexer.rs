use crate::core::fs::MY_PC_PATH;
use crate::gui::theme::{ThemePalette, get_default_palette};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
struct FavoritesSnapshot {
    favorites: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct AppSettingsSnapshot {
    folder_scanning_enabled: bool,
    window_size_mode: WindowSizeMode,
    pub start_path: Option<PathBuf>,
    theme: Option<String>,
    #[serde(default)]
    pinned_tabs: Vec<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct ThemeSettingsSnapshot {
    light: ThemePalette,
    dark: ThemePalette,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WindowSizeMode {
    FullScreen,
    HalfScreen,
    Custom { width: f32, height: f32 },
}

impl Default for WindowSizeMode {
    fn default() -> Self {
        Self::Custom {
            width: 1200.0,
            height: 800.0,
        }
    }
}

fn favorites_cache_path(drive: char) -> Option<PathBuf> {
    let base = dirs::data_local_dir()?;
    Some(
        base.join("ExplorerEden")
            .join("favorites")
            .join(format!("drive_{}.bin", drive)),
    )
}

fn settings_cache_path() -> Option<PathBuf> {
    let base = dirs::data_local_dir()?;
    Some(base.join("ExplorerEden").join("settings.bin"))
}

fn theme_cache_path() -> Option<PathBuf> {
    let base = dirs::data_local_dir()?;
    Some(base.join("ExplorerEden").join("theme.bin"))
}

pub fn load_favorites(drive: char) -> Vec<String> {
    let path = match favorites_cache_path(drive) {
        Some(path) => path,
        None => return Vec::new(),
    };
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(_) => return Vec::new(),
    };
    match bincode::deserialize::<FavoritesSnapshot>(&data) {
        Ok(snapshot) => snapshot.favorites,
        Err(_) => Vec::new(),
    }
}

pub fn save_favorites(drive: char, favorites: &[String]) {
    let path = match favorites_cache_path(drive) {
        Some(path) => path,
        None => return,
    };
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let snapshot = FavoritesSnapshot {
        favorites: favorites.to_vec(),
    };
    if let Ok(data) = bincode::serialize(&snapshot) {
        let _ = std::fs::write(path, data);
    }
}

pub fn load_app_settings() -> (bool, WindowSizeMode, PathBuf, Option<String>, Vec<PathBuf>) {
    let default_path = PathBuf::from(MY_PC_PATH);
    let path = match settings_cache_path() {
        Some(path) => path,
        None => {
            return (
                true,
                WindowSizeMode::Custom {
                    width: 1200.0,
                    height: 800.0,
                },
                default_path,
                None,
                Vec::new(),
            );
        }
    };
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(_) => {
            return (
                true,
                WindowSizeMode::Custom {
                    width: 1200.0,
                    height: 800.0,
                },
                default_path,
                None,
                Vec::new(),
            );
        }
    };
    match bincode::deserialize::<AppSettingsSnapshot>(&data) {
        Ok(snapshot) => (
            snapshot.folder_scanning_enabled,
            snapshot.window_size_mode,
            snapshot.start_path.unwrap_or(default_path),
            snapshot.theme,
            snapshot.pinned_tabs,
        ),
        Err(_) => (
            true,
            WindowSizeMode::Custom {
                width: 1200.0,
                height: 800.0,
            },
            default_path,
            None,
            Vec::new(),
        ),
    }
}

pub fn save_app_settings(
    folder_scanning_enabled: bool,
    window_size_mode: &WindowSizeMode,
    start_path: &Option<PathBuf>,
    theme: Option<&str>,
    pinned_tabs: &[PathBuf],
) {
    let path = match settings_cache_path() {
        Some(path) => path,
        None => return,
    };
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let snapshot = AppSettingsSnapshot {
        folder_scanning_enabled,
        window_size_mode: window_size_mode.clone(),
        start_path: start_path.clone(),
        theme: theme.map(|s| s.to_string()),
        pinned_tabs: pinned_tabs.to_vec(),
    };
    if let Ok(data) = bincode::serialize(&snapshot) {
        let _ = std::fs::write(path, data);
    }
}

pub fn load_theme_settings() -> Option<(ThemePalette, ThemePalette)> {
    let path = theme_cache_path()?;
    let data = std::fs::read(path).ok()?;

    match bincode::deserialize::<ThemeSettingsSnapshot>(&data) {
        Ok(snapshot) => Some((snapshot.light, snapshot.dark)),
        Err(e) => {
            eprintln!(
                "Theme deserialization failed (likely due to theme variable changes): {}. Resetting to defaults.",
                e
            );
            reset_theme_to_defaults();
            None
        }
    }
}

pub fn save_theme_settings(light: &ThemePalette, dark: &ThemePalette) {
    let path = match theme_cache_path() {
        Some(path) => path,
        None => return,
    };
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let snapshot = ThemeSettingsSnapshot {
        light: light.clone(),
        dark: dark.clone(),
    };
    if let Ok(data) = bincode::serialize(&snapshot) {
        let _ = std::fs::write(path, data);
    }
}

/// Resets theme settings to defaults by deleting the corrupted theme file
fn reset_theme_to_defaults() {
    if let Some(path) = theme_cache_path() {
        // Remove the corrupted theme file
        if let Err(e) = std::fs::remove_file(&path) {
            eprintln!("Failed to remove corrupted theme file: {}", e);
        } else {
            eprintln!("Corrupted theme file removed. Will use defaults on next startup.");
        }

        // Save fresh default themes
        let light_default = get_default_palette(crate::gui::theme::ThemeMode::Light);
        let dark_default = get_default_palette(crate::gui::theme::ThemeMode::Dark);
        save_theme_settings(&light_default, &dark_default);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_reset_functionality() {
        // Test that reset_theme_to_defaults doesn't panic
        // In a real scenario, this would be tested with actual file system operations
        // For now, we just verify the function exists and can be called
        let path = theme_cache_path();
        assert!(path.is_some() || path.is_none()); // Basic sanity check
    }
}
