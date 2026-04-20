use crate::core::drives::DriveInfo;
use crate::core::indexer::WindowSizeMode;
use crate::core::networkdevices::NetworkDevicesState;
use crate::gui::theme::{ThemeMode, ThemePalette};
use crate::gui::utils::SortColumn;
use crate::gui::windows::containers::structs::FavoriteItem;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct AboutWindow {
    pub open: bool,
}

pub struct ThemeCustomizer {
    pub open: bool,
    pub selected_mode: ThemeMode,
    pub light_palette: ThemePalette,
    pub dark_palette: ThemePalette,
}

impl Default for ThemeCustomizer {
    fn default() -> Self {
        Self {
            open: false,
            selected_mode: ThemeMode::Dark,
            light_palette: crate::gui::theme::get_palette(ThemeMode::Light),
            dark_palette: crate::gui::theme::get_palette(ThemeMode::Dark),
        }
    }
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
    pub windows_context_menu_enabled: bool,
    pub start_path: Option<PathBuf>,
    pub window_size_mode: WindowSizeMode,
    pub pinned_tabs: Vec<PathBuf>,
    pub time_format_24h: bool,
    pub sort_column: SortColumn,
    pub sort_ascending: bool,
}

#[derive(Default)]
pub struct SettingsWindow {
    pub open: bool,
    pub current_settings: AppSettings,
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
    pub non_ntfs_popup_path: Option<PathBuf>,
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
            last_drive_refresh: now.checked_sub(Duration::from_secs(60)).unwrap_or(now),
            non_ntfs_popup_path: None,
        }
    }
}
