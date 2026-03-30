use crate::core::drives::DriveInfo;
use crate::core::indexer::WindowSizeMode;
use crate::core::networkdevices::NetworkDevicesState;
use crate::gui::theme::{ThemeMode, ThemePalette};
use crate::gui::windows::containers::structs::FavoriteItem;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct AboutWindow {
    pub open: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustomTheme {
    pub name: String,
    pub mode: ThemeMode,
    pub palette: ThemePalette,
}

#[derive(Default)]
pub struct ThemeCustomizer {
    pub open: bool,
    pub current_theme: CustomTheme,
    pub selected_mode: ThemeMode,
    pub has_unsaved_changes: bool,
}

#[derive(Debug, Clone)]
pub struct Navigation {
    pub current: PathBuf,
    pub back: Vec<PathBuf>,
    pub forward: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppSettings {
    pub folder_scanning_enabled: bool,
    pub start_path: Option<PathBuf>,
    pub window_size_mode: WindowSizeMode,
}

#[derive(Default)]
pub struct SettingsWindow {
    pub open: bool,
    pub current_settings: AppSettings,
    pub has_unsaved_changes: bool,
    pub show_reset_favorites_confirmation: bool,
}

pub struct SidebarState {
    pub favorites: Vec<FavoriteItem>,
    pub item_clicked: Option<PathBuf>,
    pub dragging_favorite: Option<usize>,
    pub sidebar_default_width: f32,
    pub network_state: NetworkDevicesState,
    pub cached_drives: Vec<DriveInfo>,
    pub last_drive_refresh: Instant,
}

impl Default for SidebarState {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            favorites: vec![],
            dragging_favorite: None,
            item_clicked: None,
            sidebar_default_width: 250.0,
            network_state: NetworkDevicesState::default(),
            cached_drives: Vec::new(),
            last_drive_refresh: now
                .checked_sub(Duration::from_secs(60))
                .unwrap_or(now),
        }
    }
}
