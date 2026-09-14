use crate::core::drives::DriveInfo;
use crate::core::indexer::{DirectorySettingsSnapshot, WindowSizeMode};
use crate::gui::theme::{ThemeMode, ThemePalette};
use crate::gui::utils::SortColumn;
use crate::gui::windows::containers::enums::ItemViewerHeaderColumn;
use crate::gui::windows::containers::structs::{FavoriteItem, ItemViewerDisplayMode};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct AboutWindow {
    pub open: bool,
}

pub struct ThemeCustomizer {
    pub selected_mode: ThemeMode,
    pub light_palette: ThemePalette,
    pub dark_palette: ThemePalette,
    /// Draft copy of the sidebar's persisted width (see
    /// `core::indexer::SidebarSectionsSnapshot::sidebar_width`) - not a
    /// theme-mode-specific value like the palettes above, but edited from
    /// the same Appearance page's new Layout section.
    pub sidebar_width: f32,
    /// User-created named themes (name + accent + secondary, like a
    /// built-in `ThemePresetDef`) - persisted in their own file via
    /// `core::indexer::{load_custom_themes, save_custom_themes}`, not part
    /// of either palette above.
    pub custom_themes: Vec<crate::core::indexer::CustomThemeEntry>,
    pub custom_themes_next_id: u64,
    /// Draft text for the "save current colors as a new theme" input.
    pub new_custom_theme_name: String,
    /// Draft secondary color for the theme about to be saved - `secondary_
    /// accent` itself has no direct Core Colors picker (it's normally only
    /// set as a side effect of clicking a whole prebuilt preset), so saving
    /// a custom theme needs its own explicit, editable secondary swatch
    /// rather than silently capturing whatever `secondary_accent` currently
    /// happens to be.
    pub new_custom_theme_secondary: eframe::egui::Color32,
    /// Id of the custom theme pending a delete confirmation, if any.
    pub custom_theme_delete_confirm: Option<u64>,
}

impl Default for ThemeCustomizer {
    fn default() -> Self {
        let custom_themes_snapshot = crate::core::indexer::load_custom_themes();
        let dark_palette = crate::gui::theme::get_palette(ThemeMode::Dark);
        let light_palette = crate::gui::theme::get_palette(ThemeMode::Light);
        Self {
            // Starting point for the draft secondary swatch below - the
            // mode selected first (Dark) own current `secondary_accent`,
            // so it isn't an arbitrary unrelated color on first open.
            new_custom_theme_secondary: dark_palette.secondary_accent,
            selected_mode: ThemeMode::Dark,
            light_palette,
            dark_palette,
            sidebar_width: crate::core::indexer::load_sidebar_sections().sidebar_width,
            custom_themes: custom_themes_snapshot
                .as_ref()
                .map(|s| s.items.clone())
                .unwrap_or_default(),
            custom_themes_next_id: custom_themes_snapshot.map(|s| s.next_id).unwrap_or(1),
            new_custom_theme_name: String::new(),
            custom_theme_delete_confirm: None,
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
    pub show_hidden_files_folders: bool,
    pub show_item_viewer_icons: bool,
    pub windows_context_menu_enabled: bool,
    pub start_path: Option<PathBuf>,
    pub window_size_mode: WindowSizeMode,
    pub pinned_tabs: Vec<PathBuf>,
    pub time_format_24h: bool,
    pub date_style: crate::core::fs::DateStyle,
    /// `chrono` strftime pattern used when `date_style ==
    /// DateStyle::Custom`. Ignored for every other style.
    #[serde(default)]
    pub custom_date_format: String,
    pub sort_column: SortColumn,
    pub sort_ascending: bool,
    pub language: String,
    pub item_viewer_file_column_order: Vec<ItemViewerHeaderColumn>,
    pub item_viewer_drive_column_order: Vec<ItemViewerHeaderColumn>,
    pub recycle_bin_column_order: Vec<ItemViewerHeaderColumn>,
    pub item_viewer_file_column_sizes: Vec<f32>,
    pub item_viewer_drive_column_sizes: Vec<f32>,
    pub recycle_bin_column_sizes: Vec<f32>,
    pub directory_settings: Vec<DirectorySettingsSnapshot>,
    #[serde(default = "default_true")]
    pub double_click_navigates_up: bool,
    #[serde(default = "default_true")]
    pub show_selection_checkboxes: bool,
    #[serde(default = "default_true")]
    pub middle_click_opens_new_tab: bool,
    #[serde(default)]
    pub restore_last_session_tabs: bool,
    #[serde(default)]
    pub default_display_mode: ItemViewerDisplayMode,
    #[serde(default)]
    pub default_search_scope: crate::core::everything::DefaultSearchScope,
    #[serde(default)]
    pub search_engine: crate::core::everything::SearchEngine,
    /// Loaded/saved separately from the rest of these fields - see
    /// `core::context_menu_settings`.
    #[serde(default)]
    pub custom_context_menu: Vec<crate::core::context_menu_settings::CustomContextMenuEntry>,
    /// Loaded/saved separately from the rest of these fields - see
    /// `core::tab_groups`.
    #[serde(default)]
    pub tab_groups: Vec<crate::core::tab_groups::TabGroup>,
}

fn default_true() -> bool {
    true
}

#[derive(Default)]
pub struct SettingsWindow {
    pub current_settings: AppSettings,
    pub show_reset_favorites_confirmation: bool,
    /// An action produced while drawing the Settings tab's content, picked up and
    /// handled once per frame after all tabs have been drawn.
    pub pending_action: Option<crate::gui::windows::enums::SettingsAction>,
    /// Which category the Settings page's sidebar currently has selected.
    pub selected_category: crate::gui::windows::settings::SettingsCategory,
    /// Shared search text for the icon picker used by Custom Context Menu
    /// and Favorites settings (only one picker is open at a time in
    /// practice, so a single field is enough).
    pub icon_picker_search: String,
}

pub struct SidebarState {
    pub favorites: Vec<FavoriteItem>,
    pub item_clicked: Option<PathBuf>,
    pub dragging_favorite: Option<usize>,
    pub sidebar_default_width: f32,
    pub cached_drives: Vec<DriveInfo>,
    pub last_drive_refresh: Instant,
    pub non_ntfs_popup_path: Option<PathBuf>,
    /// Whether each collapsible sidebar section (Places, Storage, Favorites,
    /// Tags, Shared Network) is currently expanded. All expanded by default,
    /// matching the previous always-expanded behavior.
    pub places_expanded: bool,
    pub storage_expanded: bool,
    pub favorites_expanded: bool,
    pub tags_expanded: bool,
    pub shared_network_expanded: bool,
    pub saved_searches_expanded: bool,
    pub recent_locations_expanded: bool,
}

impl Default for SidebarState {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            favorites: vec![],
            dragging_favorite: None,
            places_expanded: true,
            storage_expanded: true,
            favorites_expanded: true,
            tags_expanded: true,
            shared_network_expanded: true,
            saved_searches_expanded: true,
            recent_locations_expanded: true,
            item_clicked: None,
            sidebar_default_width: 250.0,
            cached_drives: Vec::new(),
            last_drive_refresh: now.checked_sub(Duration::from_secs(60)).unwrap_or(now),
            non_ntfs_popup_path: None,
        }
    }
}
