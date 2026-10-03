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
    /// Draft copy of the tab strip's persisted gap (see
    /// `core::indexer::TabLayoutSnapshot::tab_gap`) - stored in its own
    /// small file rather than `ThemePalette`, since appending a field to
    /// that struct resets every existing user's saved colors on next load
    /// (see `CLAUDE.md`).
    pub tab_gap: f32,
    /// Draft copy of the tab strip's persisted minimum tab width (see
    /// `core::indexer::TabLayoutSnapshot::min_tab_width`) - same file as
    /// `tab_gap` above, for the same reason.
    pub min_tab_width: f32,
    /// Draft copy of the preview frame (border around previews), saved in
    /// `ui_prefs.json` - see `core::ui_prefs::PreviewFrame`.
    pub preview_frame: crate::core::ui_prefs::PreviewFrame,
    /// User-created named themes (name + accent + secondary, like a
    /// built-in `ThemePresetDef`) - persisted in their own file via
    /// `core::indexer::{load_custom_themes, save_custom_themes}`, not part
    /// of either palette above.
    pub custom_themes: Vec<crate::core::indexer::CustomThemeEntry>,
    pub custom_themes_next_id: u64,
    /// Draft text for the "save current colors as a new theme" input -
    /// pre-filled on startup from `selected_custom_theme_id` below (see
    /// `core::indexer::SelectedCustomThemeSnapshot`'s own doc comment).
    pub new_custom_theme_name: String,
    /// Id of whichever custom theme is currently considered "selected"
    /// (last clicked, saved, or updated) - kept in sync with, and persisted
    /// via, `core::indexer::{load_selected_custom_theme, save_selected_
    /// custom_theme}` every time it changes, so `new_custom_theme_name` can
    /// be restored from it on the next launch. `None` means no custom theme
    /// is currently selected (the live colors were reached some other way -
    /// a preset, a manual edit, Reset Theme, or the previously-selected
    /// theme was deleted).
    pub selected_custom_theme_id: Option<u64>,
    /// Id of the custom theme pending a delete confirmation, if any.
    pub custom_theme_delete_confirm: Option<u64>,
}

impl Default for ThemeCustomizer {
    fn default() -> Self {
        let custom_themes_snapshot = crate::core::indexer::load_custom_themes();
        let dark_palette = crate::gui::theme::get_palette(ThemeMode::Dark);
        let light_palette = crate::gui::theme::get_palette(ThemeMode::Light);
        let custom_themes: Vec<crate::core::indexer::CustomThemeEntry> = custom_themes_snapshot
            .as_ref()
            .map(|s| s.items.clone())
            .unwrap_or_default();
        let selected_custom_theme_id = crate::core::indexer::load_selected_custom_theme().id;
        // Only pre-fill the name field if the persisted id still resolves
        // to a real entry - the theme it pointed at may have been deleted
        // (or the file predates this feature and holds `None`), in which
        // case this falls back to the existing empty-field behavior.
        let new_custom_theme_name = selected_custom_theme_id
            .and_then(|id| custom_themes.iter().find(|e| e.id == id))
            .map(|e| e.name.clone())
            .unwrap_or_default();
        Self {
            selected_mode: ThemeMode::Dark,
            light_palette,
            dark_palette,
            sidebar_width: crate::core::indexer::load_sidebar_sections().sidebar_width,
            tab_gap: crate::core::indexer::load_tab_layout().tab_gap,
            min_tab_width: crate::core::indexer::load_tab_layout().min_tab_width,
            preview_frame: crate::core::ui_prefs::load_ui_prefs().preview_frame,
            custom_themes_next_id: custom_themes_snapshot.map(|s| s.next_id).unwrap_or(1),
            custom_themes,
            new_custom_theme_name,
            selected_custom_theme_id,
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
    /// Whether starting a file operation automatically opens the
    /// notification bell's dropdown panel. The bell icon itself always
    /// shows; this only controls the auto-open behavior.
    #[serde(default = "default_true")]
    pub auto_open_notification_panel: bool,
    /// Whether starting/finishing a file operation shows a transient toast
    /// popup.
    #[serde(default = "default_true")]
    pub show_operation_toasts: bool,
    /// Loaded/saved separately from the rest of these fields (including its
    /// own `custom_context_menu_enabled` toggle) - see
    /// `core::context_menu_settings`.
    #[serde(default)]
    pub custom_context_menu: Vec<crate::core::context_menu_settings::CustomContextMenuEntry>,
    #[serde(default)]
    pub custom_context_menu_enabled: bool,
    /// Loaded/saved separately from the rest of these fields - see
    /// `core::tab_groups`.
    #[serde(default)]
    pub tab_groups: Vec<crate::core::tab_groups::TabGroup>,
    /// Loaded/saved separately from the rest of these fields (including its
    /// own `send_to_context_menu_enabled` toggle) - see `core::send_to`.
    #[serde(default)]
    pub send_to: Vec<crate::core::send_to::SendToGroup>,
    #[serde(default)]
    pub send_to_context_menu_enabled: bool,
    /// Loaded/saved separately from the rest of these fields - see
    /// `core::indexer::{load_tag_icon_style, save_tag_icon_style}`.
    #[serde(default)]
    pub tag_icon_style: crate::core::indexer::TagIconStyle,
    /// Loaded/saved separately from the rest of these fields - see
    /// `core::context_menu_order`.
    #[serde(default = "crate::core::context_menu_order::default_order")]
    pub context_menu_order: Vec<crate::core::context_menu_order::ContextMenuSection>,
    /// Loaded/saved separately from the rest of these fields - see
    /// `core::indexer::{load_sidebar_visibility, save_sidebar_visibility}`.
    #[serde(default)]
    pub sidebar_visibility: crate::core::indexer::SidebarSectionVisibility,
    /// Loaded/saved separately from the rest of these fields - see
    /// `core::perf::{load_performance_panel_visible, save_performance_panel_visible}`.
    #[serde(default)]
    pub show_performance_panel: bool,
    /// Loaded/saved separately (as JSON) - see `core::ui_prefs`.
    #[serde(default)]
    pub ui_prefs: crate::core::ui_prefs::UiPrefs,
    /// Bumped whenever a folder's remembered view changes, so the main
    /// window can save it shortly after (see
    /// `MainWindow::save_folder_views_if_due`). Never saved itself.
    #[serde(skip)]
    pub folder_views_revision: u64,
}

fn default_true() -> bool {
    true
}

#[derive(Default)]
pub struct SettingsWindow {
    pub current_settings: AppSettings,
    /// Which Reset Data row is waiting for its confirmation dialog, if any.
    pub pending_reset_confirmation: Option<crate::gui::windows::enums::ResetTarget>,
    /// An action produced while drawing the Settings tab's content, picked up and
    /// handled once per frame after all tabs have been drawn.
    pub pending_action: Option<crate::gui::windows::enums::SettingsAction>,
    /// Which category the Settings page's sidebar currently has selected.
    pub selected_category: crate::gui::windows::settings::SettingsCategory,
    /// Shared search text for the icon picker used by Custom Context Menu
    /// and Favorites settings (only one picker is open at a time in
    /// practice, so a single field is enough).
    pub icon_picker_search: String,
    /// Which entry is selected in each master-detail settings page's left
    /// column (Favorites/Custom Context Menu/Tab Groups/Tags) - by a stable
    /// id rather than a positional index, so reordering or deleting an
    /// unrelated entry doesn't silently select the wrong one. Purely
    /// transient UI state, never persisted.
    pub selected_favorite_index: Option<usize>,
    pub selected_context_menu_id: Option<u64>,
    pub selected_tab_group_id: Option<u64>,
    pub selected_tag_group_id: Option<u64>,
    pub selected_send_to_id: Option<u64>,
    /// Result of the last Portable Mode switch (Settings > Advanced):
    /// `(succeeded, message)`, shown under the checkbox.
    pub portable_mode_status: Option<(bool, String)>,
    /// Settings > Shortcuts: the action waiting for a new key combination,
    /// the last message shown there, and the frame the page was last drawn
    /// (to cancel recording when the user leaves the page).
    pub recording_shortcut: Option<crate::core::keymap::ShortcutAction>,
    pub shortcut_message: Option<(bool, String)>,
    pub shortcuts_page_last_pass: u64,
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
    /// The Folders section's tree (expanded folders, loaded subfolders).
    pub folder_tree: crate::gui::windows::containers::folder_tree::FolderTreeState,
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
            folder_tree: Default::default(),
        }
    }
}
