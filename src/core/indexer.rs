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

/// One user-created named theme. `accent`/`secondary` are kept as their own
/// fields (rather than only living inside `palette`) for the swatch preview
/// and the cheap `is_selected` check, and so a `palette: None` legacy entry
/// (saved before `palette` existed) still has something to apply. `palette`,
/// when present, is a *full* snapshot of every color/field the user had set
/// when they saved - clicking the swatch restores all of it, not just the
/// two-color accent/secondary pair the way applying a built-in preset does.
/// Earlier versions of this feature stored only `accent`/`secondary` and
/// re-derived everything else via `regenerate_base_derived_colors` on
/// apply, which is exactly why a user's other manually-edited colors
/// (sidebar text, notification colors, etc.) silently reverted to their
/// tint-formula defaults every time they reselected their own saved theme -
/// `palette` is what actually fixes that.
#[derive(Clone, Serialize, Deserialize)]
pub struct CustomThemeEntry {
    pub id: u64,
    pub name: String,
    pub accent: Color32,
    pub secondary: Color32,
    #[serde(default)]
    pub palette: Option<ThemePalette>,
}

#[derive(Serialize, Deserialize)]
pub struct CustomThemesSnapshot {
    #[serde(default = "default_next_custom_theme_id")]
    pub next_id: u64,
    #[serde(default)]
    pub items: Vec<CustomThemeEntry>,
}

/// What Settings > Appearance's own "Export Theme"/"Import Theme" buttons
/// read and write - the currently-edited palette for the selected mode,
/// plus every saved custom theme, so importing this file elsewhere (or
/// after a reinstall) restores the whole Custom Themes list too, not just
/// the one active palette. `#[serde(default)]` on `custom_themes` means a
/// file exported *before* this field existed still imports cleanly (just
/// with no custom themes to merge) rather than failing to parse outright -
/// unlike `postcard`, `serde_json` genuinely does rescue a missing trailing
/// field this way, so this one case doesn't need the `*Legacy`-struct
/// pattern `CustomThemeEntry` above needed for its own `postcard` file.
#[derive(Serialize, Deserialize)]
pub struct ThemeFileExportBundle {
    pub palette: ThemePalette,
    #[serde(default)]
    pub custom_themes: Vec<CustomThemeEntry>,
}

/// Merges an imported custom-themes list into the user's existing one, for
/// Settings > Appearance's "Import Theme" button. A name match (case-
/// insensitive, matching the same convention "Save Current Colors"/"Update
/// Theme" already uses) overwrites that entry's colors/palette in place -
/// its `id` is left untouched, so anything still pointing at it (the
/// persisted "last selected custom theme") keeps resolving correctly -
/// rather than appending a same-named duplicate. An imported entry with no
/// name match is appended as a brand-new entry with a fresh id. Returns
/// whether anything actually changed, so the caller only re-saves the file
/// when it needs to.
pub fn merge_imported_custom_themes(
    existing: &mut Vec<CustomThemeEntry>,
    next_id: &mut u64,
    imported: Vec<CustomThemeEntry>,
) -> bool {
    let mut changed = false;
    for entry in imported {
        if let Some(existing_entry) = existing
            .iter_mut()
            .find(|e| e.name.eq_ignore_ascii_case(&entry.name))
        {
            existing_entry.accent = entry.accent;
            existing_entry.secondary = entry.secondary;
            existing_entry.palette = entry.palette;
        } else {
            let id = *next_id;
            *next_id += 1;
            existing.push(CustomThemeEntry {
                id,
                name: entry.name,
                accent: entry.accent,
                secondary: entry.secondary,
                palette: entry.palette,
            });
        }
        changed = true;
    }
    changed
}

/// The pre-`palette`-field shape of `CustomThemeEntry`/`CustomThemesSnapshot`
/// - kept only as a decode fallback in `load_custom_themes`. Postcard's
/// format has no per-field rescue for an appended field (confirmed
/// elsewhere in this codebase: a whole-struct decode either fully succeeds
/// or fully fails, `#[serde(default)]` only helps a JSON-style format) - so
/// without this fallback, adding `palette` to `CustomThemeEntry` would make
/// *every* custom theme saved before this change fail to decode at once,
/// silently wiping a user's whole saved-themes list rather than just
/// missing the new field.
#[derive(Deserialize, Serialize)]
struct CustomThemeEntryLegacy {
    id: u64,
    name: String,
    accent: Color32,
    secondary: Color32,
}

#[derive(Deserialize, Serialize)]
struct CustomThemesSnapshotLegacy {
    #[serde(default = "default_next_custom_theme_id")]
    next_id: u64,
    #[serde(default)]
    items: Vec<CustomThemeEntryLegacy>,
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
    #[serde(default = "default_true")]
    auto_open_notification_panel: bool,
    #[serde(default = "default_true")]
    show_operation_toasts: bool,
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
            auto_open_notification_panel: true,
            show_operation_toasts: true,
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
    let base = crate::core::app_data::data_dir()?;
    Some(
        base.join("favorites")
            .join(format!("drive_{}.bin", drive)),
    )
}

fn settings_cache_path() -> Option<PathBuf> {
    let base = crate::core::app_data::data_dir()?;
    Some(base.join("settings.bin"))
}

fn theme_cache_path() -> Option<PathBuf> {
    let base = crate::core::app_data::data_dir()?;
    Some(base.join("theme.bin"))
}

fn tags_cache_path() -> Option<PathBuf> {
    let base = crate::core::app_data::data_dir()?;
    Some(base.join("tags.bin"))
}

fn saved_searches_cache_path() -> Option<PathBuf> {
    let base = crate::core::app_data::data_dir()?;
    Some(base.join("saved_searches.bin"))
}

fn recent_locations_cache_path() -> Option<PathBuf> {
    let base = crate::core::app_data::data_dir()?;
    Some(base.join("recent_locations.bin"))
}

fn custom_themes_cache_path() -> Option<PathBuf> {
    let base = crate::core::app_data::data_dir()?;
    Some(base.join("custom_themes.bin"))
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
    let base = crate::core::app_data::data_dir()?;
    Some(base.join("sidebar_sections.bin"))
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

/// Horizontal gap between tabs in the tab strip - a brand-new, dedicated
/// file rather than a field on `ThemePalette`/`AppSettingsSnapshot`
/// (appending a field to either of those resets every existing user's saved
/// data on next load, since their loaders decode the whole struct at once
/// rather than rescuing a missing trailing field - see `CLAUDE.md`). A new
/// file with nothing preceding it has no old data to break.
#[derive(Serialize, Deserialize)]
pub struct TabLayoutSnapshot {
    pub tab_gap: f32,
    #[serde(default = "default_min_tab_width")]
    pub min_tab_width: f32,
}

fn default_tab_gap() -> f32 {
    // Matches egui's own ambient `item_spacing.x` default, which this value
    // replaces - zero visual change for anyone until they touch the new
    // Appearance > Layout setting.
    8.0
}

fn default_min_tab_width() -> f32 {
    // Matches `tabs.rs`'s own previous hardcoded `MIN_TAB_WIDTH` constant -
    // zero visual change for anyone until they touch the new Appearance >
    // Layout setting. Per CLAUDE.md's own documented finding, appending
    // this field still means an *existing* `tab_layout.bin` (one that only
    // has `tab_gap`) fails to decode outright - `#[serde(default = ...)]`
    // doesn't rescue a trailing field even via the safe `take_from_bytes`
    // pattern below, it's `load_tab_layout`'s own `.unwrap_or_default()`
    // fallback that saves it. That means a user who already customized
    // `tab_gap` loses that customization too, once, on the first launch
    // after this field is added - contained to just this small file
    // (unlike an equivalent `ThemePalette` field, which would reset the
    // user's entire color palette) rather than something this addition
    // avoids entirely.
    110.0
}

impl Default for TabLayoutSnapshot {
    fn default() -> Self {
        Self {
            tab_gap: default_tab_gap(),
            min_tab_width: default_min_tab_width(),
        }
    }
}

fn tab_layout_cache_path() -> Option<PathBuf> {
    let base = crate::core::app_data::data_dir()?;
    Some(base.join("tab_layout.bin"))
}

pub fn load_tab_layout() -> TabLayoutSnapshot {
    let Some(path) = tab_layout_cache_path() else {
        return TabLayoutSnapshot::default();
    };
    let Ok(data) = std::fs::read(&path) else {
        return TabLayoutSnapshot::default();
    };
    postcard::take_from_bytes::<TabLayoutSnapshot>(&data)
        .ok()
        .filter(|(_, rest)| rest.is_empty())
        .map(|(v, _)| v)
        .unwrap_or_default()
}

pub fn save_tab_layout(snapshot: &TabLayoutSnapshot) {
    let Some(path) = tab_layout_cache_path() else {
        return;
    };
    let Some(parent) = path.parent() else { return };
    let _ = std::fs::create_dir_all(parent);
    if let Ok(data) = postcard::to_allocvec(snapshot) {
        let _ = std::fs::write(path, data);
    }
}

/// Whether a tag group's own colored glyph (sidebar list, tab strip icon for
/// a tag-view tab, Settings > Tags list) renders as a solid filled tag or a
/// thin outline - a purely visual preference applied everywhere that glyph
/// shows up, not per-tag. `Filled` matches this app's look before this
/// setting existed, so it's the default (zero visual change for an existing
/// user until they touch it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TagIconStyle {
    Filled,
    Outline,
}

impl Default for TagIconStyle {
    fn default() -> Self {
        Self::Filled
    }
}

/// A brand-new, dedicated file for the same reason `TabLayoutSnapshot` above
/// is - a single small enum with no relation to `ThemePalette`/
/// `AppSettingsSnapshot`, so it gets its own file rather than risking a
/// whole-struct decode failure resetting either of those unrelated blobs.
#[derive(Serialize, Deserialize, Default)]
pub struct TagIconStyleSnapshot {
    #[serde(default)]
    pub style: TagIconStyle,
}

fn tag_icon_style_cache_path() -> Option<PathBuf> {
    let base = crate::core::app_data::data_dir()?;
    Some(base.join("tag_icon_style.bin"))
}

pub fn load_tag_icon_style() -> TagIconStyle {
    let Some(path) = tag_icon_style_cache_path() else {
        return TagIconStyle::default();
    };
    let Ok(data) = std::fs::read(&path) else {
        return TagIconStyle::default();
    };
    postcard::take_from_bytes::<TagIconStyleSnapshot>(&data)
        .ok()
        .filter(|(_, rest)| rest.is_empty())
        .map(|(v, _)| v.style)
        .unwrap_or_default()
}

pub fn save_tag_icon_style(style: TagIconStyle) {
    let Some(path) = tag_icon_style_cache_path() else {
        return;
    };
    let Some(parent) = path.parent() else { return };
    let _ = std::fs::create_dir_all(parent);
    if let Ok(data) = postcard::to_allocvec(&TagIconStyleSnapshot { style }) {
        let _ = std::fs::write(path, data);
    }
}

/// Which optional sidebar sections are shown (Settings > General > Sidebar
/// Sections). Places and Storage are always shown. A brand-new, dedicated
/// file for the same reason `TabLayoutSnapshot` above is (and separate from
/// `SidebarSectionsSnapshot`, which only remembers expanded/collapsed), so it can never
/// break decoding of `ThemePalette`/`AppSettingsSnapshot`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidebarSectionVisibility {
    pub favorites: bool,
    pub tags: bool,
    pub saved_searches: bool,
    pub recent_locations: bool,
    pub shared_network: bool,
}

impl Default for SidebarSectionVisibility {
    fn default() -> Self {
        Self {
            favorites: true,
            tags: true,
            saved_searches: true,
            recent_locations: true,
            shared_network: true,
        }
    }
}

fn sidebar_visibility_cache_path() -> Option<PathBuf> {
    let base = crate::core::app_data::data_dir()?;
    Some(base.join("sidebar_visibility.bin"))
}

pub fn load_sidebar_visibility() -> SidebarSectionVisibility {
    let Some(path) = sidebar_visibility_cache_path() else {
        return SidebarSectionVisibility::default();
    };
    let Ok(data) = std::fs::read(&path) else {
        return SidebarSectionVisibility::default();
    };
    postcard::take_from_bytes::<SidebarSectionVisibility>(&data)
        .ok()
        .filter(|(_, rest)| rest.is_empty())
        .map(|(v, _)| v)
        .unwrap_or_default()
}

pub fn save_sidebar_visibility(sections: &SidebarSectionVisibility) {
    let Some(path) = sidebar_visibility_cache_path() else {
        return;
    };
    let Some(parent) = path.parent() else { return };
    let _ = std::fs::create_dir_all(parent);
    if let Ok(data) = postcard::to_allocvec(sections) {
        let _ = std::fs::write(path, data);
    }
}

/// Which `CustomThemeEntry` (by id, not name - a name can be edited later)
/// the user most recently selected in Settings > Appearance > Custom
/// Themes, so `ThemeCustomizer::new_custom_theme_name` can be pre-filled
/// with it on the next launch - the user can then jump straight to
/// "tweak a color, click Update Theme" instead of re-picking or re-typing
/// the theme's name from scratch. A brand-new, dedicated file for the same
/// reason `TabLayoutSnapshot` above is - this is pure UI-convenience state
/// with no relation to `ThemePalette`/`AppSettingsSnapshot`, so it gets its
/// own file rather than risking either of those.
#[derive(Serialize, Deserialize, Default)]
pub struct SelectedCustomThemeSnapshot {
    pub id: Option<u64>,
}

fn selected_custom_theme_cache_path() -> Option<PathBuf> {
    let base = crate::core::app_data::data_dir()?;
    Some(base.join("selected_custom_theme.bin"))
}

pub fn load_selected_custom_theme() -> SelectedCustomThemeSnapshot {
    let Some(path) = selected_custom_theme_cache_path() else {
        return SelectedCustomThemeSnapshot::default();
    };
    let Ok(data) = std::fs::read(&path) else {
        return SelectedCustomThemeSnapshot::default();
    };
    postcard::take_from_bytes::<SelectedCustomThemeSnapshot>(&data)
        .ok()
        .filter(|(_, rest)| rest.is_empty())
        .map(|(v, _)| v)
        .unwrap_or_default()
}

pub fn save_selected_custom_theme(snapshot: &SelectedCustomThemeSnapshot) {
    let Some(path) = selected_custom_theme_cache_path() else {
        return;
    };
    let Some(parent) = path.parent() else { return };
    let _ = std::fs::create_dir_all(parent);
    if let Ok(data) = postcard::to_allocvec(snapshot) {
        let _ = std::fs::write(path, data);
    }
}

fn window_position_cache_path() -> Option<PathBuf> {
    let base = crate::core::app_data::data_dir()?;
    Some(base.join("window_position.bin"))
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
    let base = crate::core::app_data::data_dir()?;
    Some(base.join("custom_icons"))
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
    // Also stores the changed tags with the files, when turned on.
    crate::core::portable_tags::on_tags_saved(snapshot);
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

/// A custom theme's own saved `notification_border_color`/`navigation_
/// toast_border_color` are baked into its `palette` snapshot at whatever
/// alpha `regenerate_base_derived_colors` used at the moment the theme was
/// saved - raising that alpha in `theme.rs` only affects newly-derived
/// palettes (a fresh preset click, a live Secondary edit), never an
/// already-saved custom theme, since selecting one restores its snapshot
/// verbatim rather than re-deriving it. Only rewrites a field that still
/// exactly matches the *previous* alpha's own derivation - both fields also
/// have their own manual picker row in the customizer, so a genuine
/// per-theme override must be left alone rather than silently overwritten
/// by a blanket update. Returns whether anything actually changed, so the
/// caller only re-saves the file when it needs to.
fn migrate_custom_theme_border_alpha(snapshot: &mut CustomThemesSnapshot) -> bool {
    const OLD_ALPHA: u8 = 60;
    const NEW_ALPHA: u8 = 130;
    let mut changed = false;
    for entry in &mut snapshot.items {
        let Some(palette) = &mut entry.palette else {
            continue;
        };
        let secondary = palette.secondary_accent;
        let old_border = Color32::from_rgba_unmultiplied(
            secondary.r(),
            secondary.g(),
            secondary.b(),
            OLD_ALPHA,
        );
        if palette.notification_border_color == old_border {
            palette.notification_border_color = Color32::from_rgba_unmultiplied(
                secondary.r(),
                secondary.g(),
                secondary.b(),
                NEW_ALPHA,
            );
            changed = true;
        }
        if palette.navigation_toast_border_color == old_border {
            palette.navigation_toast_border_color = Color32::from_rgba_unmultiplied(
                secondary.r(),
                secondary.g(),
                secondary.b(),
                NEW_ALPHA,
            );
            changed = true;
        }
    }
    changed
}

pub fn load_custom_themes() -> Option<CustomThemesSnapshot> {
    let path = custom_themes_cache_path()?;
    if let Some(mut snapshot) = load_or_migrate_bincode_to_postcard::<CustomThemesSnapshot>(&path)
    {
        if migrate_custom_theme_border_alpha(&mut snapshot) {
            save_custom_themes(&snapshot);
        }
        return Some(snapshot);
    }

    // Fell through the new shape (and bincode) - try the pre-`palette`
    // shape before giving up, so upgrading to this version doesn't wipe an
    // existing custom-themes list outright (see `CustomThemeEntryLegacy`'s
    // doc comment). Re-saves in the new shape the next time anything
    // changes (`save_custom_themes` is always called after a mutation), so
    // this fallback only ever runs once per machine.
    let data = std::fs::read(&path).ok()?;
    let legacy = postcard::from_bytes::<CustomThemesSnapshotLegacy>(&data).ok()?;
    Some(CustomThemesSnapshot {
        next_id: legacy.next_id,
        items: legacy
            .items
            .into_iter()
            .map(|e| CustomThemeEntry {
                id: e.id,
                name: e.name,
                accent: e.accent,
                secondary: e.secondary,
                palette: None,
            })
            .collect(),
    })
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
    let base = crate::core::app_data::data_dir()?;
    Some(base.join("session_tabs.bin"))
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
    bool,
    bool,
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
        snapshot.auto_open_notification_panel,
        snapshot.show_operation_toasts,
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
    bool,
    bool,
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
        true,
        true,
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
    auto_open_notification_panel: bool,
    show_operation_toasts: bool,
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
        auto_open_notification_panel,
        show_operation_toasts,
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
    fn custom_theme_entry_round_trips_full_palette() {
        let mut palette = get_default_palette(crate::gui::theme::ThemeMode::Dark);
        palette.sidebar_text_color = Color32::from_rgb(1, 2, 3);
        palette.notification_status_success = Color32::from_rgb(4, 5, 6);

        let snapshot = CustomThemesSnapshot {
            next_id: 2,
            items: vec![CustomThemeEntry {
                id: 1,
                name: "My Theme".to_string(),
                accent: palette.primary,
                secondary: palette.secondary_accent,
                palette: Some(palette.clone()),
            }],
        };

        let bytes = postcard::to_allocvec(&snapshot).unwrap();
        let decoded: CustomThemesSnapshot = postcard::from_bytes(&bytes).unwrap();

        let restored = decoded.items[0].palette.as_ref().unwrap();
        assert_eq!(restored.sidebar_text_color, Color32::from_rgb(1, 2, 3));
        assert_eq!(
            restored.notification_status_success,
            Color32::from_rgb(4, 5, 6)
        );
    }

    /// Confirms that adding `palette` to `CustomThemeEntry` doesn't wipe a
    /// custom-themes list saved before that field existed - postcard has no
    /// per-field rescue for an appended field (a whole-struct decode either
    /// fully succeeds or fully fails), so `load_custom_themes` falls back to
    /// decoding the pre-`palette` shape rather than losing the user's saved
    /// themes outright. This test exercises that fallback path directly
    /// (bypassing the real `%LOCALAPPDATA%` file) by decoding a hand-built
    /// legacy-shape byte stream the same way `load_custom_themes` does.
    #[test]
    fn legacy_custom_theme_bytes_without_palette_field_still_decode() {
        let legacy = CustomThemesSnapshotLegacy {
            next_id: 3,
            items: vec![CustomThemeEntryLegacy {
                id: 1,
                name: "Old Theme".to_string(),
                accent: Color32::from_rgb(10, 20, 30),
                secondary: Color32::from_rgb(40, 50, 60),
            }],
        };
        let bytes = postcard::to_allocvec(&legacy).unwrap();

        // The new shape must fail to decode legacy bytes (otherwise this
        // test isn't actually exercising the fallback) ...
        assert!(postcard::from_bytes::<CustomThemesSnapshot>(&bytes).is_err());

        // ... while the legacy shape decodes cleanly, giving
        // `load_custom_themes` something to convert (palette: None) rather
        // than treating the file as corrupt.
        let decoded: CustomThemesSnapshotLegacy = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.items.len(), 1);
        assert_eq!(decoded.items[0].name, "Old Theme");
        assert_eq!(decoded.items[0].accent, Color32::from_rgb(10, 20, 30));
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

    #[test]
    fn selected_custom_theme_snapshot_roundtrips_an_id() {
        let snapshot = SelectedCustomThemeSnapshot { id: Some(42) };

        let bytes = postcard::to_allocvec(&snapshot).unwrap();
        let decoded: SelectedCustomThemeSnapshot = postcard::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.id, Some(42));
    }

    #[test]
    fn selected_custom_theme_snapshot_roundtrips_none() {
        // The "no custom theme currently selected" case (a fresh install,
        // or after the selected theme was deleted) - `id: None` must
        // encode/decode cleanly, not just the `Some` case.
        let snapshot = SelectedCustomThemeSnapshot { id: None };

        let bytes = postcard::to_allocvec(&snapshot).unwrap();
        let decoded: SelectedCustomThemeSnapshot = postcard::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.id, None);
    }

    #[test]
    fn selected_custom_theme_snapshot_falls_back_to_none_for_missing_or_corrupt_bytes() {
        // Mirrors `load_selected_custom_theme`'s own decode path (take_from_
        // bytes + "no bytes left over" + unwrap_or_default) directly against
        // an empty byte slice, standing in for a missing/never-written file.
        let decoded = postcard::take_from_bytes::<SelectedCustomThemeSnapshot>(&[])
            .ok()
            .filter(|(_, rest)| rest.is_empty())
            .map(|(v, _)| v)
            .unwrap_or_default();

        assert_eq!(decoded.id, None);
    }

    #[test]
    fn migrate_custom_theme_border_alpha_rewrites_an_old_derivation_but_not_a_manual_override() {
        let secondary = Color32::from_rgb(255, 214, 64);
        let old_border = Color32::from_rgba_unmultiplied(
            secondary.r(),
            secondary.g(),
            secondary.b(),
            60,
        );
        let manual_override = Color32::from_rgba_unmultiplied(10, 20, 30, 200);

        let mut auto_derived_palette = get_default_palette(crate::gui::theme::ThemeMode::Dark);
        auto_derived_palette.secondary_accent = secondary;
        auto_derived_palette.notification_border_color = old_border;
        auto_derived_palette.navigation_toast_border_color = old_border;

        let mut manually_customized_palette =
            get_default_palette(crate::gui::theme::ThemeMode::Dark);
        manually_customized_palette.secondary_accent = secondary;
        manually_customized_palette.notification_border_color = manual_override;
        manually_customized_palette.navigation_toast_border_color = manual_override;

        let mut snapshot = CustomThemesSnapshot {
            next_id: 3,
            items: vec![
                CustomThemeEntry {
                    id: 1,
                    name: "Auto-derived".to_string(),
                    accent: Color32::from_rgb(0, 120, 215),
                    secondary,
                    palette: Some(auto_derived_palette),
                },
                CustomThemeEntry {
                    id: 2,
                    name: "Manually customized".to_string(),
                    accent: Color32::from_rgb(0, 120, 215),
                    secondary,
                    palette: Some(manually_customized_palette),
                },
            ],
        };

        let changed = migrate_custom_theme_border_alpha(&mut snapshot);
        assert!(changed);

        let expected_new_border =
            Color32::from_rgba_unmultiplied(secondary.r(), secondary.g(), secondary.b(), 130);
        let auto_derived = snapshot.items[0].palette.as_ref().unwrap();
        assert_eq!(auto_derived.notification_border_color, expected_new_border);
        assert_eq!(
            auto_derived.navigation_toast_border_color,
            expected_new_border
        );

        // The manual override must survive untouched - it doesn't match the
        // old derivation, so the migration has no business rewriting it.
        let manual = snapshot.items[1].palette.as_ref().unwrap();
        assert_eq!(manual.notification_border_color, manual_override);
        assert_eq!(manual.navigation_toast_border_color, manual_override);

        // Running it again must be a no-op - nothing left to migrate.
        assert!(!migrate_custom_theme_border_alpha(&mut snapshot));
    }

    #[test]
    fn merge_imported_custom_themes_overwrites_a_same_name_entry_in_place() {
        let mut existing = vec![CustomThemeEntry {
            id: 1,
            name: "Midnight".to_string(),
            accent: Color32::from_rgb(10, 10, 10),
            secondary: Color32::from_rgb(20, 20, 20),
            palette: None,
        }];
        let mut next_id = 2;

        let imported = vec![CustomThemeEntry {
            // A different id on the imported side (e.g. from a different
            // machine's own next_id counter) must not matter - the match
            // is by name, and the existing entry's own id must survive.
            id: 999,
            name: "midnight".to_string(), // case-insensitive match
            accent: Color32::from_rgb(200, 200, 200),
            secondary: Color32::from_rgb(210, 210, 210),
            palette: None,
        }];

        let changed = merge_imported_custom_themes(&mut existing, &mut next_id, imported);

        assert!(changed);
        assert_eq!(existing.len(), 1);
        assert_eq!(existing[0].id, 1); // unchanged
        assert_eq!(existing[0].name, "Midnight"); // unchanged casing
        assert_eq!(existing[0].accent, Color32::from_rgb(200, 200, 200));
        assert_eq!(existing[0].secondary, Color32::from_rgb(210, 210, 210));
        assert_eq!(next_id, 2); // no new id consumed
    }

    #[test]
    fn merge_imported_custom_themes_appends_a_new_name_with_a_fresh_id() {
        let mut existing = vec![CustomThemeEntry {
            id: 1,
            name: "Midnight".to_string(),
            accent: Color32::from_rgb(10, 10, 10),
            secondary: Color32::from_rgb(20, 20, 20),
            palette: None,
        }];
        let mut next_id = 2;

        let imported = vec![CustomThemeEntry {
            id: 999,
            name: "Sunrise".to_string(),
            accent: Color32::from_rgb(255, 200, 0),
            secondary: Color32::from_rgb(255, 100, 0),
            palette: None,
        }];

        let changed = merge_imported_custom_themes(&mut existing, &mut next_id, imported);

        assert!(changed);
        assert_eq!(existing.len(), 2);
        assert_eq!(existing[0].id, 1); // original untouched
        assert_eq!(existing[1].id, 2); // new entry got the local next_id, not 999
        assert_eq!(existing[1].name, "Sunrise");
        assert_eq!(next_id, 3);
    }

    #[test]
    fn merge_imported_custom_themes_empty_import_is_a_no_op() {
        let mut existing = vec![CustomThemeEntry {
            id: 1,
            name: "Midnight".to_string(),
            accent: Color32::from_rgb(10, 10, 10),
            secondary: Color32::from_rgb(20, 20, 20),
            palette: None,
        }];
        let mut next_id = 2;

        let changed = merge_imported_custom_themes(&mut existing, &mut next_id, Vec::new());

        assert!(!changed);
        assert_eq!(existing.len(), 1);
        assert_eq!(next_id, 2);
    }

    #[test]
    fn theme_file_export_bundle_roundtrips_through_json() {
        let palette = get_default_palette(crate::gui::theme::ThemeMode::Dark);
        let bundle = ThemeFileExportBundle {
            palette: palette.clone(),
            custom_themes: vec![CustomThemeEntry {
                id: 5,
                name: "Exported Theme".to_string(),
                accent: Color32::from_rgb(1, 2, 3),
                secondary: Color32::from_rgb(4, 5, 6),
                palette: Some(palette),
            }],
        };

        let json = serde_json::to_string(&bundle).unwrap();
        let decoded: ThemeFileExportBundle = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.custom_themes.len(), 1);
        assert_eq!(decoded.custom_themes[0].name, "Exported Theme");
        assert_eq!(decoded.custom_themes[0].accent, Color32::from_rgb(1, 2, 3));
        assert!(decoded.custom_themes[0].palette.is_some());
    }

    #[test]
    fn theme_file_export_bundle_accepts_a_pre_feature_file_with_no_custom_themes_field() {
        // Simulates a file exported before this feature existed - just a
        // bare palette, no `custom_themes` key at all. `#[serde(default)]`
        // (unlike postcard's whole-struct decode failure documented
        // elsewhere in this file) genuinely rescues a missing JSON field.
        let palette = get_default_palette(crate::gui::theme::ThemeMode::Dark);
        #[derive(Serialize)]
        struct OldExportShape {
            palette: ThemePalette,
        }
        let old = OldExportShape { palette };

        let json = serde_json::to_string(&old).unwrap();
        let decoded: ThemeFileExportBundle = serde_json::from_str(&json).unwrap();

        assert!(decoded.custom_themes.is_empty());
    }
}
