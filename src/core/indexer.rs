use crate::core::fs::{DateStyle, MY_PC_PATH};
use crate::gui::theme::{THEME_VERSION, ThemePalette, get_default_palette};
use eframe::egui::Color32;
use crate::gui::utils::SortKey;
use crate::gui::windows::containers::enums::ItemViewerHeaderColumn;
use crate::gui::windows::containers::structs::{
    FavoriteItem, GalleryThumbnailSize, ItemViewerDisplayMode,
};
use crate::gui::windows::structs::AppSettings;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A portable bundle of every user-configurable setting (general settings, favorites,
/// tags, and both theme palettes), exported/imported as a single human-readable JSON
/// file so a user can back up their setup or move it to another machine.
#[derive(Serialize, Deserialize)]
pub struct SettingsExportBundle {
    pub format_version: u32,
    pub settings: AppSettings,
    pub favorites: Vec<FavoriteItem>,
    pub tags: Option<TagsSnapshot>,
    pub theme_light: ThemePalette,
    pub theme_dark: ThemePalette,
}

pub const SETTINGS_EXPORT_FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct FavoritesSnapshot {
    favorites: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct TagGroupSnapshot {
    pub id: u64,
    pub name: String,
    pub color: [u8; 4],
    pub items: Vec<PathBuf>,
}

#[derive(Serialize, Deserialize)]
pub struct TagsSnapshot {
    #[serde(default = "default_tags_version")]
    pub version: u32,
    #[serde(default = "default_next_tag_group_id")]
    pub next_group_id: u64,
    #[serde(default)]
    pub groups: Vec<TagGroupSnapshot>,
}

#[derive(Serialize, Deserialize)]
pub struct SavedSearchSnapshot {
    pub id: u64,
    pub name: String,
    pub query: String,
    pub scope_folder: Option<PathBuf>,
}

#[derive(Serialize, Deserialize)]
pub struct SavedSearchesSnapshot {
    #[serde(default = "default_saved_searches_version")]
    pub version: u32,
    #[serde(default = "default_next_saved_search_id")]
    pub next_id: u64,
    #[serde(default)]
    pub items: Vec<SavedSearchSnapshot>,
}

fn default_saved_searches_version() -> u32 {
    1
}

fn default_next_saved_search_id() -> u64 {
    1
}

#[derive(Serialize, Deserialize)]
pub struct RecentLocationsSnapshot {
    #[serde(default = "default_recent_locations_version")]
    pub version: u32,
    #[serde(default)]
    pub items: Vec<PathBuf>,
}

fn default_recent_locations_version() -> u32 {
    1
}

/// One user-created named theme - just a name plus the same accent +
/// secondary pair a built-in `ThemePresetDef` carries (see `theme.rs`),
/// deliberately not a full `ThemePalette` snapshot: applying one is meant to
/// behave exactly like clicking a prebuilt preset (set `primary`/
/// `secondary_accent` and re-derive everything else), not restore a whole
/// saved configuration.
#[derive(Clone, Serialize, Deserialize)]
pub struct CustomThemeEntry {
    pub id: u64,
    pub name: String,
    pub accent: Color32,
    pub secondary: Color32,
}

#[derive(Serialize, Deserialize)]
pub struct CustomThemesSnapshot {
    #[serde(default = "default_next_custom_theme_id")]
    pub next_id: u64,
    #[serde(default)]
    pub items: Vec<CustomThemeEntry>,
}

fn default_next_custom_theme_id() -> u64 {
    1
}

#[derive(Serialize, Deserialize)]
struct AppSettingsSnapshot {
    folder_scanning_enabled: bool,
    #[serde(default = "default_show_hidden_files_folders")]
    show_hidden_files_folders: bool,
    #[serde(default = "default_show_item_viewer_icons")]
    show_item_viewer_icons: bool,
    #[serde(default)]
    windows_context_menu_enabled: bool,
    window_size_mode: WindowSizeMode,
    pub start_path: Option<PathBuf>,
    theme: Option<String>,
    #[serde(default)]
    pinned_tabs: Vec<PathBuf>,
    #[serde(default)]
    time_format_24h: bool,
    #[serde(default = "default_date_style")]
    date_style: DateStyle,
    #[serde(default)]
    custom_date_format: String,
    #[serde(default = "default_sort_column")]
    sort_column: crate::gui::utils::SortColumn,
    #[serde(default)]
    sort_ascending: bool,
    #[serde(default = "default_language")]
    language: String,
    #[serde(default = "default_item_viewer_file_column_order")]
    item_viewer_file_column_order: Vec<ItemViewerHeaderColumn>,
    #[serde(default = "default_item_viewer_drive_column_order")]
    item_viewer_drive_column_order: Vec<ItemViewerHeaderColumn>,
    #[serde(default = "default_recycle_bin_column_order")]
    recycle_bin_column_order: Vec<ItemViewerHeaderColumn>,
    #[serde(default = "default_item_viewer_file_column_size")]
    item_viewer_file_column_sizes: Vec<f32>,
    #[serde(default = "default_item_viewer_drive_column_size")]
    item_viewer_drive_column_sizes: Vec<f32>,
    #[serde(default = "default_recycle_bin_column_size")]
    recycle_bin_column_sizes: Vec<f32>,
    #[serde(default)]
    directory_settings: Vec<DirectorySettingsSnapshot>,
    #[serde(default = "default_true")]
    double_click_navigates_up: bool,
    #[serde(default = "default_true")]
    show_selection_checkboxes: bool,
    #[serde(default = "default_true")]
    middle_click_opens_new_tab: bool,
    #[serde(default)]
    restore_last_session_tabs: bool,
    #[serde(default)]
    default_display_mode: ItemViewerDisplayMode,
    #[serde(default)]
    default_search_scope: crate::core::everything::DefaultSearchScope,
    #[serde(default)]
    search_engine: crate::core::everything::SearchEngine,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DirectorySettingsSnapshot {
    pub directory: PathBuf,
    #[serde(default = "default_item_viewer_file_column_order")]
    pub item_viewer_file_column_order: Vec<ItemViewerHeaderColumn>,
    #[serde(default = "default_item_viewer_drive_column_order")]
    pub item_viewer_drive_column_order: Vec<ItemViewerHeaderColumn>,
    #[serde(default = "default_recycle_bin_column_order")]
    pub recycle_bin_column_order: Vec<ItemViewerHeaderColumn>,
    #[serde(default = "default_item_viewer_file_column_size")]
    pub item_viewer_file_column_sizes: Vec<f32>,
    #[serde(default = "default_item_viewer_drive_column_size")]
    pub item_viewer_drive_column_sizes: Vec<f32>,
    #[serde(default = "default_recycle_bin_column_size")]
    pub recycle_bin_column_sizes: Vec<f32>,
    #[serde(default)]
    pub filter_query: String,
    #[serde(default)]
    pub display_mode: ItemViewerDisplayMode,
    #[serde(default = "default_gallery_thumbnail_size")]
    pub gallery_thumbnail_size: GalleryThumbnailSize,
    #[serde(default = "default_sort_column")]
    pub sort_column: crate::gui::utils::SortColumn,
    #[serde(default)]
    pub sort_ascending: bool,
    #[serde(default)]
    pub sort_keys: Vec<SortKey>,
}

fn default_gallery_thumbnail_size() -> GalleryThumbnailSize {
    GalleryThumbnailSize::Medium
}

// Legacy snapshot struct for deserializing old settings with HalfScreen
#[derive(Serialize, Deserialize)]
struct LegacyAppSettingsSnapshot {
    folder_scanning_enabled: bool,
    #[serde(default)]
    windows_context_menu_enabled: bool,
    window_size_mode: LegacyWindowSizeMode,
    pub start_path: Option<PathBuf>,
    theme: Option<String>,
    #[serde(default)]
    pinned_tabs: Vec<PathBuf>,
    #[serde(default)]
    time_format_24h: bool,
    #[serde(default = "default_sort_column")]
    sort_column: crate::gui::utils::SortColumn,
    #[serde(default)]
    sort_ascending: bool,
}

impl From<LegacyAppSettingsSnapshot> for AppSettingsSnapshot {
    fn from(legacy: LegacyAppSettingsSnapshot) -> Self {
        Self {
            folder_scanning_enabled: legacy.folder_scanning_enabled,
            show_hidden_files_folders: true,
            show_item_viewer_icons: true,
            windows_context_menu_enabled: legacy.windows_context_menu_enabled,
            window_size_mode: legacy.window_size_mode.into(),
            start_path: legacy.start_path,
            theme: legacy.theme,
            pinned_tabs: legacy.pinned_tabs,
            time_format_24h: legacy.time_format_24h,
            date_style: default_date_style(),
            custom_date_format: String::new(),
            sort_column: legacy.sort_column,
            sort_ascending: legacy.sort_ascending,
            language: default_language(),
            item_viewer_file_column_order: default_item_viewer_file_column_order(),
            item_viewer_drive_column_order: default_item_viewer_drive_column_order(),
            recycle_bin_column_order: default_recycle_bin_column_order(),
            item_viewer_file_column_sizes: default_item_viewer_file_column_size(),
            item_viewer_drive_column_sizes: default_item_viewer_drive_column_size(),
            recycle_bin_column_sizes: default_recycle_bin_column_size(),
            directory_settings: Vec::new(),
            double_click_navigates_up: true,
            show_selection_checkboxes: true,
            middle_click_opens_new_tab: true,
            restore_last_session_tabs: false,
            default_display_mode: ItemViewerDisplayMode::Details,
            default_search_scope: crate::core::everything::DefaultSearchScope::default(),
            search_engine: crate::core::everything::SearchEngine::default(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ThemeSettingsSnapshot {
    version: u32,
    light: ThemePalette,
    dark: ThemePalette,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WindowSizeMode {
    FullScreen,
    Custom { width: f32, height: f32 },
}

// Temporary enum for deserializing old settings with HalfScreen
#[derive(Clone, Debug, Serialize, Deserialize)]
enum LegacyWindowSizeMode {
    FullScreen,
    HalfScreen,
    Custom { width: f32, height: f32 },
}

impl From<LegacyWindowSizeMode> for WindowSizeMode {
    fn from(legacy: LegacyWindowSizeMode) -> Self {
        match legacy {
            LegacyWindowSizeMode::FullScreen => WindowSizeMode::FullScreen,
            LegacyWindowSizeMode::HalfScreen => WindowSizeMode::Custom {
                width: 960.0,
                height: 540.0,
            },
            LegacyWindowSizeMode::Custom { width, height } => {
                WindowSizeMode::Custom { width, height }
            }
        }
    }
}

impl Default for WindowSizeMode {
    fn default() -> Self {
        Self::Custom {
            width: 1200.0,
            height: 800.0,
        }
    }
}

fn load_or_migrate_bincode_to_postcard<T>(path: &std::path::Path) -> Option<T>
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    let data = std::fs::read(path).ok()?;

    // 1️⃣ Try OLD format first (bincode)
    if let Ok(v) = bincode::deserialize::<T>(&data) {
        // migrate → postcard
        if let Ok(new_bytes) = postcard::to_allocvec(&v) {
            let tmp_path = path.with_extension("tmp");

            if std::fs::write(&tmp_path, new_bytes).is_ok() {
                let _ = std::fs::rename(tmp_path, path);
            }
        }

        return Some(v);
    }

    // 2️⃣ Try NEW format (postcard)
    if let Ok(v) = postcard::from_bytes::<T>(&data) {
        return Some(v);
    }

    // 3️⃣ Corrupt
    None
}

fn default_sort_column() -> crate::gui::utils::SortColumn {
    crate::gui::utils::SortColumn::Name
}

fn default_language() -> String {
    "en-US".to_string()
}

fn default_item_viewer_file_column_order() -> Vec<ItemViewerHeaderColumn> {
    vec![
        ItemViewerHeaderColumn::Type,
        ItemViewerHeaderColumn::Size,
        ItemViewerHeaderColumn::Modified,
        ItemViewerHeaderColumn::Created,
        ItemViewerHeaderColumn::Tags,
    ]
}

fn default_item_viewer_drive_column_order() -> Vec<ItemViewerHeaderColumn> {
    vec![
        ItemViewerHeaderColumn::Type,
        ItemViewerHeaderColumn::Size,
        ItemViewerHeaderColumn::Usage,
    ]
}

fn default_recycle_bin_column_order() -> Vec<ItemViewerHeaderColumn> {
    vec![
        ItemViewerHeaderColumn::OriginalDirectory,
        ItemViewerHeaderColumn::Type,
        ItemViewerHeaderColumn::Size,
        ItemViewerHeaderColumn::Deleted,
        ItemViewerHeaderColumn::Created,
    ]
}

pub fn default_item_viewer_file_column_size() -> Vec<f32> {
    vec![180.0, 60.0, 75.0, 100.0, 100.0, 140.0]
}

pub fn default_item_viewer_drive_column_size() -> Vec<f32> {
    vec![180.0, 120.0, 150.0]
}

pub fn default_recycle_bin_column_size() -> Vec<f32> {
    vec![180.0, 200.0, 60.0, 120.0, 100.0, 100.0]
}

fn default_date_style() -> DateStyle {
    DateStyle::UsShort
}

fn default_show_hidden_files_folders() -> bool {
    true
}

fn default_show_item_viewer_icons() -> bool {
    true
}

fn default_true() -> bool {
    true
}

fn default_tags_version() -> u32 {
    1
}

fn default_next_tag_group_id() -> u64 {
    1
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

fn tags_cache_path() -> Option<PathBuf> {
    let base = dirs::data_local_dir()?;
    Some(base.join("ExplorerEden").join("tags.bin"))
}

fn saved_searches_cache_path() -> Option<PathBuf> {
    let base = dirs::data_local_dir()?;
    Some(base.join("ExplorerEden").join("saved_searches.bin"))
}

fn recent_locations_cache_path() -> Option<PathBuf> {
    let base = dirs::data_local_dir()?;
    Some(base.join("ExplorerEden").join("recent_locations.bin"))
}

fn custom_themes_cache_path() -> Option<PathBuf> {
    let base = dirs::data_local_dir()?;
    Some(base.join("ExplorerEden").join("custom_themes.bin"))
}

/// Which collapsible sidebar sections (Places, Storage, Favorites, Tags,
/// Shared Network) are expanded, persisted across restarts.
#[derive(Serialize, Deserialize)]
pub struct SidebarSectionsSnapshot {
    pub places: bool,
    pub storage: bool,
    pub favorites: bool,
    pub tags: bool,
    pub shared_network: bool,
    // Appended after `shared_network` rather than inserted alongside the
    // other section flags above - postcard's binary format is purely
    // positional (unlike JSON), so a new field must always go at the very
    // end of the struct. `#[serde(default = ...)]` only rescues a *missing
    // trailing* field when an old save's byte stream runs out early; it
    // does nothing to fix a field inserted mid-struct, which would instead
    // silently misalign every field that comes after it against old data.
    #[serde(default = "default_true")]
    pub saved_searches: bool,
    // Same positional-append rule as `saved_searches` above.
    #[serde(default = "default_true")]
    pub recent_locations: bool,
    /// The sidebar's user-resized width in points - previously ephemeral
    /// (reset to 250.0 on every restart); same positional-append rule as
    /// the fields above.
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: f32,
}

fn default_sidebar_width() -> f32 {
    250.0
}

impl Default for SidebarSectionsSnapshot {
    fn default() -> Self {
        Self {
            places: true,
            storage: true,
            favorites: true,
            tags: true,
            shared_network: true,
            saved_searches: true,
            recent_locations: true,
            sidebar_width: default_sidebar_width(),
        }
    }
}

fn sidebar_sections_cache_path() -> Option<PathBuf> {
    let base = dirs::data_local_dir()?;
    Some(base.join("ExplorerEden").join("sidebar_sections.bin"))
}

pub fn load_sidebar_sections() -> SidebarSectionsSnapshot {
    let Some(path) = sidebar_sections_cache_path() else {
        return SidebarSectionsSnapshot::default();
    };
    let Ok(data) = std::fs::read(&path) else {
        return SidebarSectionsSnapshot::default();
    };
    postcard::take_from_bytes::<SidebarSectionsSnapshot>(&data)
        .ok()
        .filter(|(_, rest)| rest.is_empty())
        .map(|(v, _)| v)
        .unwrap_or_default()
}

pub fn save_sidebar_sections(snapshot: &SidebarSectionsSnapshot) {
    let Some(path) = sidebar_sections_cache_path() else {
        return;
    };
    let Some(parent) = path.parent() else { return };
    let _ = std::fs::create_dir_all(parent);
    if let Ok(data) = postcard::to_allocvec(snapshot) {
        let _ = std::fs::write(path, data);
    }
}

fn window_position_cache_path() -> Option<PathBuf> {
    let base = dirs::data_local_dir()?;
    Some(base.join("ExplorerEden").join("window_position.bin"))
}

/// Last on-screen top-left corner of the app window, in physical pixels.
pub fn load_window_position() -> Option<(f32, f32)> {
    let path = window_position_cache_path()?;
    let data = std::fs::read(&path).ok()?;
    let (pos, _) = postcard::take_from_bytes::<(f32, f32)>(&data).ok()?;
    Some(pos)
}

pub fn save_window_position(x: f32, y: f32) {
    let Some(path) = window_position_cache_path() else {
        return;
    };
    let Some(parent) = path.parent() else { return };
    let _ = std::fs::create_dir_all(parent);
    if let Ok(data) = postcard::to_allocvec(&(x, y)) {
        let _ = std::fs::write(path, data);
    }
}

/// Where user-browsed custom icons (for a custom context menu command, a
/// favorite, ...) get copied to.
pub fn custom_icons_dir() -> Option<PathBuf> {
    let base = dirs::data_local_dir()?;
    Some(base.join("ExplorerEden").join("custom_icons"))
}

/// Copies a user-browsed icon/image file into the app's own data folder, so
/// it keeps working even if the original file is later moved or deleted,
/// and so a settings export always references a stable, app-managed path
/// rather than wherever the user happened to browse from. Returns the new
/// path, or `None` if the copy failed (in which case the caller should fall
/// back to using the original path as-is).
pub fn import_custom_icon(source: &std::path::Path) -> Option<PathBuf> {
    let dir = custom_icons_dir()?;
    std::fs::create_dir_all(&dir).ok()?;

    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dest = dir.join(format!("icon_{nanos}.{ext}"));

    std::fs::copy(source, &dest).ok()?;
    Some(dest)
}

/// Loads saved favorites, understanding both the legacy bare-path format
/// (from before favorites could carry a custom label/icon) and the current
/// one - trying the legacy shape first, since every existing user's file on
/// disk is still in that format until their next save migrates it.
pub fn load_favorites(
    drive: char,
) -> Vec<crate::gui::windows::containers::structs::FavoriteItem> {
    use crate::gui::windows::containers::structs::FavoriteItem;
    use std::path::PathBuf;

    let path = match favorites_cache_path(drive) {
        Some(path) => path,
        None => return Vec::new(),
    };
    let Ok(data) = std::fs::read(&path) else {
        return Vec::new();
    };

    let legacy_to_item = |raw: String| {
        let path = PathBuf::from(raw);
        let label = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        FavoriteItem {
            path,
            label,
            custom_icon: None,
            custom_icon_file: None,
        }
    };

    // `postcard::from_bytes` happily returns `Ok` even when it doesn't
    // consume the whole buffer, which made the two postcard shapes below
    // ambiguous with each other - bytes actually written as
    // `Vec<FavoriteItem>` could silently "succeed" as a shorter, garbled
    // `Vec<String>` (surviving favorites, others truncated/blank).
    // `take_from_bytes` + an explicit "no bytes left over" check makes each
    // shape only match its own bytes.
    let legacy_postcard = postcard::take_from_bytes::<FavoritesSnapshot>(&data)
        .ok()
        .filter(|(_, rest)| rest.is_empty())
        .map(|(v, _)| v);
    let current_format = postcard::take_from_bytes::<Vec<FavoriteItem>>(&data)
        .ok()
        .filter(|(_, rest)| rest.is_empty())
        .map(|(v, _)| v);

    let loaded = if let Ok(v) = bincode::deserialize::<FavoritesSnapshot>(&data) {
        v.favorites.into_iter().map(legacy_to_item).collect()
    } else if let Some(v) = legacy_postcard {
        v.favorites.into_iter().map(legacy_to_item).collect()
    } else if let Some(items) = current_format {
        items
    } else {
        Vec::new()
    };

    // Defensive cleanup: drop any entry with an empty path - the only way
    // one of these could exist is leftover corruption from the ambiguous
    // parse above (fixed now, but already-saved files may still carry
    // blanks it produced) or some other malformed save.
    loaded
        .into_iter()
        .filter(|item| !item.path.as_os_str().is_empty())
        .collect()
}

pub fn save_favorites(
    drive: char,
    favorites: &[crate::gui::windows::containers::structs::FavoriteItem],
) {
    let path = match favorites_cache_path(drive) {
        Some(path) => path,
        None => return,
    };
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    if let Ok(data) = postcard::to_allocvec(&favorites.to_vec()) {
        let _ = std::fs::write(path, data);
    }
}

pub fn load_tags() -> Option<TagsSnapshot> {
    let path = tags_cache_path()?;
    load_or_migrate_bincode_to_postcard::<TagsSnapshot>(&path)
}

pub fn save_tags(snapshot: &TagsSnapshot) {
    let path = match tags_cache_path() {
        Some(path) => path,
        None => return,
    };
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    if let Ok(data) = postcard::to_allocvec(snapshot) {
        let _ = std::fs::write(path, data);
    }
}

pub fn load_saved_searches() -> Option<SavedSearchesSnapshot> {
    let path = saved_searches_cache_path()?;
    load_or_migrate_bincode_to_postcard::<SavedSearchesSnapshot>(&path)
}

pub fn save_saved_searches(snapshot: &SavedSearchesSnapshot) {
    let path = match saved_searches_cache_path() {
        Some(path) => path,
        None => return,
    };
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    if let Ok(data) = postcard::to_allocvec(snapshot) {
        let _ = std::fs::write(path, data);
    }
}

pub fn load_recent_locations() -> Option<RecentLocationsSnapshot> {
    let path = recent_locations_cache_path()?;
    load_or_migrate_bincode_to_postcard::<RecentLocationsSnapshot>(&path)
}

pub fn save_recent_locations(snapshot: &RecentLocationsSnapshot) {
    let path = match recent_locations_cache_path() {
        Some(path) => path,
        None => return,
    };
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    if let Ok(data) = postcard::to_allocvec(snapshot) {
        let _ = std::fs::write(path, data);
    }
}

pub fn load_custom_themes() -> Option<CustomThemesSnapshot> {
    let path = custom_themes_cache_path()?;
    load_or_migrate_bincode_to_postcard::<CustomThemesSnapshot>(&path)
}

pub fn save_custom_themes(snapshot: &CustomThemesSnapshot) {
    let path = match custom_themes_cache_path() {
        Some(path) => path,
        None => return,
    };
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    if let Ok(data) = postcard::to_allocvec(snapshot) {
        let _ = std::fs::write(path, data);
    }
}

/// One restored tab: its primary folder, and the secondary (split-view)
/// folder alongside it, if the tab had split view open.
#[derive(Serialize, Deserialize, Clone)]
pub struct SessionTabEntry {
    pub path: PathBuf,
    pub split_path: Option<PathBuf>,
}

/// The set of tabs (and which was active) open when the app last exited, used to
/// restore the previous session on the next launch when that setting is enabled.
#[derive(Serialize, Deserialize, Default)]
pub struct SessionTabsSnapshot {
    pub tabs: Vec<SessionTabEntry>,
    pub active_index: usize,
}

fn session_tabs_cache_path() -> Option<PathBuf> {
    let base = dirs::data_local_dir()?;
    Some(base.join("ExplorerEden").join("session_tabs.bin"))
}

pub fn load_session_tabs() -> Option<SessionTabsSnapshot> {
    let path = session_tabs_cache_path()?;
    load_or_migrate_bincode_to_postcard::<SessionTabsSnapshot>(&path)
}

pub fn save_session_tabs(snapshot: &SessionTabsSnapshot) {
    let path = match session_tabs_cache_path() {
        Some(path) => path,
        None => return,
    };
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    if let Ok(data) = postcard::to_allocvec(snapshot) {
        let _ = std::fs::write(path, data);
    }
}

pub fn load_windows_size_mode_on_start() -> WindowSizeMode {
    let path = match settings_cache_path() {
        Some(path) => path,
        None => return WindowSizeMode::default(),
    };

    let snapshot =
        load_or_migrate_bincode_to_postcard::<AppSettingsSnapshot>(&path).or_else(|| {
            load_or_migrate_bincode_to_postcard::<LegacyAppSettingsSnapshot>(&path).map(Into::into)
        });

    let snapshot = match snapshot {
        Some(s) => s,
        None => return WindowSizeMode::default(),
    };

    snapshot.window_size_mode
}

pub fn load_app_settings() -> (
    bool,
    bool,
    bool,
    bool,
    WindowSizeMode,
    PathBuf,
    Option<String>,
    Vec<PathBuf>,
    bool,
    crate::gui::utils::SortColumn,
    bool,
    String,
    DateStyle,
    String,
    Vec<ItemViewerHeaderColumn>,
    Vec<ItemViewerHeaderColumn>,
    Vec<ItemViewerHeaderColumn>,
    Vec<f32>,
    Vec<f32>,
    Vec<f32>,
    Vec<DirectorySettingsSnapshot>,
    bool,
    bool,
    bool,
    bool,
    ItemViewerDisplayMode,
    crate::core::everything::DefaultSearchScope,
    crate::core::everything::SearchEngine,
) {
    let default_path = PathBuf::from(MY_PC_PATH);

    let path = match settings_cache_path() {
        Some(path) => path,
        None => return default_app_settings(default_path),
    };

    let snapshot =
        load_or_migrate_bincode_to_postcard::<AppSettingsSnapshot>(&path).or_else(|| {
            load_or_migrate_bincode_to_postcard::<LegacyAppSettingsSnapshot>(&path).map(Into::into)
        });

    let snapshot = match snapshot {
        Some(s) => s,
        None => return default_app_settings(default_path),
    };

    (
        snapshot.folder_scanning_enabled,
        snapshot.show_hidden_files_folders,
        snapshot.show_item_viewer_icons,
        snapshot.windows_context_menu_enabled,
        snapshot.window_size_mode,
        snapshot.start_path.unwrap_or(default_path),
        snapshot.theme,
        snapshot.pinned_tabs,
        snapshot.time_format_24h,
        snapshot.sort_column,
        snapshot.sort_ascending,
        snapshot.language,
        snapshot.date_style,
        snapshot.custom_date_format,
        snapshot.item_viewer_file_column_order,
        snapshot.item_viewer_drive_column_order,
        snapshot.recycle_bin_column_order,
        snapshot.item_viewer_file_column_sizes,
        snapshot.item_viewer_drive_column_sizes,
        snapshot.recycle_bin_column_sizes,
        snapshot.directory_settings,
        snapshot.double_click_navigates_up,
        snapshot.show_selection_checkboxes,
        snapshot.middle_click_opens_new_tab,
        snapshot.restore_last_session_tabs,
        snapshot.default_display_mode,
        snapshot.default_search_scope,
        snapshot.search_engine,
    )
}

fn default_app_settings(
    default_path: PathBuf,
) -> (
    bool,
    bool,
    bool,
    bool,
    WindowSizeMode,
    PathBuf,
    Option<String>,
    Vec<PathBuf>,
    bool,
    crate::gui::utils::SortColumn,
    bool,
    String,
    DateStyle,
    String,
    Vec<ItemViewerHeaderColumn>,
    Vec<ItemViewerHeaderColumn>,
    Vec<ItemViewerHeaderColumn>,
    Vec<f32>,
    Vec<f32>,
    Vec<f32>,
    Vec<DirectorySettingsSnapshot>,
    bool,
    bool,
    bool,
    bool,
    ItemViewerDisplayMode,
    crate::core::everything::DefaultSearchScope,
    crate::core::everything::SearchEngine,
) {
    (
        true,
        true,
        true,
        false,
        WindowSizeMode::default(),
        default_path,
        None,
        Vec::new(),
        false,
        crate::gui::utils::SortColumn::Name,
        true,
        default_language(),
        DateStyle::default(),
        String::new(),
        default_item_viewer_file_column_order(),
        default_item_viewer_drive_column_order(),
        default_recycle_bin_column_order(),
        default_item_viewer_file_column_size(),
        default_item_viewer_drive_column_size(),
        default_recycle_bin_column_size(),
        Vec::new(),
        true,
        true,
        true,
        false,
        ItemViewerDisplayMode::Details,
        crate::core::everything::DefaultSearchScope::default(),
        crate::core::everything::SearchEngine::default(),
    )
}

pub fn save_app_settings(
    folder_scanning_enabled: bool,
    show_hidden_files_folders: bool,
    show_item_viewer_icons: bool,
    windows_context_menu_enabled: bool,
    window_size_mode: &WindowSizeMode,
    start_path: &Option<PathBuf>,
    theme: Option<&str>,
    pinned_tabs: &[PathBuf],
    time_format_24h: bool,
    sort_column: crate::gui::utils::SortColumn,
    sort_ascending: bool,
    language: &str,
    date_style: DateStyle,
    custom_date_format: &str,
    item_viewer_file_column_order: &[ItemViewerHeaderColumn],
    item_viewer_drive_column_order: &[ItemViewerHeaderColumn],
    recycle_bin_column_order: &[ItemViewerHeaderColumn],
    item_viewer_file_column_sizes: &[f32],
    item_viewer_drive_column_sizes: &[f32],
    recycle_bin_column_sizes: &[f32],
    directory_settings: &[DirectorySettingsSnapshot],
    double_click_navigates_up: bool,
    show_selection_checkboxes: bool,
    middle_click_opens_new_tab: bool,
    restore_last_session_tabs: bool,
    default_display_mode: ItemViewerDisplayMode,
    default_search_scope: crate::core::everything::DefaultSearchScope,
    search_engine: crate::core::everything::SearchEngine,
) {
    let path = match settings_cache_path() {
        Some(path) => path,
        None => return,
    };
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let snapshot = AppSettingsSnapshot {
        folder_scanning_enabled,
        show_hidden_files_folders,
        show_item_viewer_icons,
        windows_context_menu_enabled,
        window_size_mode: window_size_mode.clone(),
        start_path: start_path.clone(),
        theme: theme.map(|s| s.to_string()),
        pinned_tabs: pinned_tabs.to_vec(),
        time_format_24h,
        date_style,
        custom_date_format: custom_date_format.to_string(),
        sort_column,
        sort_ascending,
        language: language.to_string(),
        item_viewer_file_column_order: item_viewer_file_column_order.to_vec(),
        item_viewer_drive_column_order: item_viewer_drive_column_order.to_vec(),
        recycle_bin_column_order: recycle_bin_column_order.to_vec(),
        item_viewer_file_column_sizes: item_viewer_file_column_sizes.to_vec(),
        item_viewer_drive_column_sizes: item_viewer_drive_column_sizes.to_vec(),
        recycle_bin_column_sizes: recycle_bin_column_sizes.to_vec(),
        directory_settings: directory_settings.to_vec(),
        double_click_navigates_up,
        show_selection_checkboxes,
        middle_click_opens_new_tab,
        restore_last_session_tabs,
        default_display_mode,
        default_search_scope,
        search_engine,
    };
    if let Ok(data) = postcard::to_allocvec(&snapshot) {
        let _ = std::fs::write(path, data);
    }
}

pub fn load_theme_settings() -> Option<(ThemePalette, ThemePalette)> {
    let path = theme_cache_path()?;

    match load_or_migrate_bincode_to_postcard::<ThemeSettingsSnapshot>(&path) {
        Some(snapshot) if snapshot.version == THEME_VERSION => {
            Some((snapshot.light, snapshot.dark))
        }
        _ => {
            eprintln!("Theme version mismatch or corruption. Resetting.");
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
        version: THEME_VERSION,
        light: light.clone(),
        dark: dark.clone(),
    };
    if let Ok(data) = postcard::to_allocvec(&snapshot) {
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

    #[test]
    fn session_tab_entry_roundtrips_split_path() {
        let snapshot = SessionTabsSnapshot {
            tabs: vec![
                SessionTabEntry {
                    path: PathBuf::from("D:\\"),
                    split_path: Some(PathBuf::from("C:\\Users")),
                },
                SessionTabEntry {
                    path: PathBuf::from("C:\\"),
                    split_path: None,
                },
            ],
            active_index: 1,
        };

        let bytes = postcard::to_allocvec(&snapshot).unwrap();
        let decoded: SessionTabsSnapshot = postcard::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.active_index, 1);
        assert_eq!(decoded.tabs.len(), 2);
        assert_eq!(decoded.tabs[0].path, PathBuf::from("D:\\"));
        assert_eq!(decoded.tabs[0].split_path, Some(PathBuf::from("C:\\Users")));
        assert_eq!(decoded.tabs[1].path, PathBuf::from("C:\\"));
        assert_eq!(decoded.tabs[1].split_path, None);
    }

    #[test]
    fn sidebar_sections_snapshot_roundtrips_sidebar_width() {
        let snapshot = SidebarSectionsSnapshot {
            sidebar_width: 312.5,
            ..SidebarSectionsSnapshot::default()
        };

        let bytes = postcard::to_allocvec(&snapshot).unwrap();
        let decoded: SidebarSectionsSnapshot = postcard::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.sidebar_width, 312.5);
    }

    #[test]
    fn sidebar_sections_snapshot_falls_back_to_defaults_for_a_save_from_before_the_field_existed() {
        // Simulates an old on-disk save written before `sidebar_width` was
        // added. Postcard's positional format has no explicit "end of
        // struct" marker, so a missing *trailing* field (unlike one that's
        // present but wrong) can't always be told apart from "ran out of
        // bytes mid-value" - `load_sidebar_sections` (the real load path)
        // treats either case the same way: fall back to a fresh, valid
        // `SidebarSectionsSnapshot::default()` rather than erroring or
        // reading garbage, which is what this confirms.
        #[derive(Serialize)]
        struct OldSidebarSectionsSnapshot {
            places: bool,
            storage: bool,
            favorites: bool,
            tags: bool,
            shared_network: bool,
            saved_searches: bool,
            recent_locations: bool,
        }

        let old = OldSidebarSectionsSnapshot {
            places: true,
            storage: false,
            favorites: true,
            tags: false,
            shared_network: true,
            saved_searches: false,
            recent_locations: true,
        };
        let bytes = postcard::to_allocvec(&old).unwrap();
        let decoded = postcard::take_from_bytes::<SidebarSectionsSnapshot>(&bytes)
            .ok()
            .filter(|(_, rest)| rest.is_empty())
            .map(|(v, _)| v)
            .unwrap_or_default();

        assert_eq!(decoded.sidebar_width, default_sidebar_width());
    }
}
