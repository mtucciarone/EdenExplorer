use crate::core::fs::MY_PC_PATH;
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

pub fn load_app_settings() -> (bool, WindowSizeMode, PathBuf) {
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
            );
        }
    };
    match bincode::deserialize::<AppSettingsSnapshot>(&data) {
        Ok(snapshot) => (
            snapshot.folder_scanning_enabled,
            snapshot.window_size_mode,
            snapshot.start_path.unwrap_or(default_path),
        ),
        Err(_) => (
            true,
            WindowSizeMode::Custom {
                width: 1200.0,
                height: 800.0,
            },
            default_path,
        ),
    }
}

pub fn save_app_settings(
    folder_scanning_enabled: bool,
    window_size_mode: &WindowSizeMode,
    start_path: &Option<PathBuf>,
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
    };
    if let Ok(data) = bincode::serialize(&snapshot) {
        let _ = std::fs::write(path, data);
    }
}
