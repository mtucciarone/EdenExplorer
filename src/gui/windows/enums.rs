#[derive(Clone, Debug)]
pub enum ThemeCustomizerAction {
    ThemeUpdated(crate::gui::theme::ThemeMode),
    ResetToDefaults(crate::gui::theme::ThemeMode),
    ExportTheme(crate::gui::theme::ThemeMode),
    ImportTheme(crate::gui::theme::ThemeMode),
    /// The Layout section's sidebar-width control changed - unlike every
    /// other action here, this isn't a per-mode palette edit (the sidebar
    /// has one width regardless of dark/light mode), so it carries the new
    /// width directly instead of a `ThemeMode`.
    SidebarWidthChanged(f32),
    /// The Layout section's tab-gap control changed - like
    /// `SidebarWidthChanged`, not a per-mode palette edit (persisted in its
    /// own small file via `core::indexer`, not `ThemePalette` - appending a
    /// field to `ThemePalette` resets every existing user's saved colors on
    /// next load, see `CLAUDE.md`'s documented finding on this).
    TabGapChanged(f32),
    /// The Layout section's minimum-tab-width control changed - same
    /// reasoning/persistence as `TabGapChanged` (own small file, not
    /// `ThemePalette`).
    MinTabWidthChanged(f32),
    /// The customizer's own Dark/Light toggle was clicked - switches which
    /// palette is being *edited*, but previously never touched the live
    /// app theme (`MainWindow::theme`), so editing the mode that wasn't
    /// currently rendered silently had no visible effect. This makes the
    /// editor's mode toggle also become the live theme, so what's shown in
    /// the editor is always what's on screen.
    SetLiveMode(crate::gui::theme::ThemeMode),
    /// The user-created custom-themes list changed (a theme was saved or
    /// deleted) - persists `ThemeCustomizer::custom_themes`/
    /// `custom_themes_next_id` as-is. Applying a saved custom theme reuses
    /// the ordinary `ThemeUpdated` path instead (it's just a `primary`/
    /// `secondary_accent` edit like any other), so this variant only ever
    /// fires for the list itself changing.
    CustomThemesChanged,
}

/// A kind of user-created data that Settings > Advanced > Reset Data can
/// clear on its own, without touching anything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetTarget {
    Favorites,
    CustomContextMenu,
    CustomThemes,
    SendTo,
    TabGroups,
    Tags,
    FolderViews,
    FolderSizeCache,
}

impl ResetTarget {
    pub const ALL: [ResetTarget; 8] = [
        ResetTarget::Favorites,
        ResetTarget::CustomContextMenu,
        ResetTarget::CustomThemes,
        ResetTarget::SendTo,
        ResetTarget::TabGroups,
        ResetTarget::Tags,
        ResetTarget::FolderViews,
        ResetTarget::FolderSizeCache,
    ];

    /// The i18n key prefix for this target's strings:
    /// `<prefix>` (row label), `<prefix>_confirm` (dialog message).
    pub fn i18n_key(self) -> &'static str {
        match self {
            ResetTarget::Favorites => "reset_data_favorites",
            ResetTarget::CustomContextMenu => "reset_data_custom_context_menu",
            ResetTarget::CustomThemes => "reset_data_custom_themes",
            ResetTarget::SendTo => "reset_data_send_to",
            ResetTarget::TabGroups => "reset_data_tab_groups",
            ResetTarget::Tags => "reset_data_tags",
            ResetTarget::FolderViews => "reset_data_folder_views",
            ResetTarget::FolderSizeCache => "reset_data_folder_size_cache",
        }
    }
}

#[derive(Clone, Debug)]
pub enum SettingsAction {
    ResetToDefaults,
    /// Clears one kind of user-created data (Settings > Advanced > Reset
    /// Data), after the user confirmed it.
    ResetData(ResetTarget),
    ApplySettings,
    /// Open the New File templates folder in a new tab.
    OpenTemplatesFolder,
    ExportSettings,
    ImportSettings,
    /// Export/import *just* the custom context menu list (see
    /// `core::context_menu_settings::ContextMenuExportBundle`) - a full
    /// settings export/import already carries this list as part of
    /// `AppSettings`, this is for sharing/backing up only the custom
    /// commands. Import replaces the current list wholesale, matching how a
    /// full settings import already replaces `AppSettings` wholesale.
    ExportContextMenu,
    ImportContextMenu,
    /// Export/import *just* the Tab Groups list (see
    /// `core::tab_groups::TabGroupsExportBundle`) - same rationale as
    /// `ExportContextMenu`/`ImportContextMenu` above.
    ExportTabGroups,
    ImportTabGroups,
    /// Export/import *just* the Send To groups (see
    /// `core::send_to::SendToExportBundle`) - same rationale as
    /// `ExportContextMenu`/`ImportContextMenu` above.
    ExportSendTo,
    ImportSendTo,
    /// Produced by the Settings page's Appearance category (which embeds the
    /// same theme editor that used to be its own floating window) - handled
    /// identically to how that window's actions always were.
    ThemeCustomizer(ThemeCustomizerAction),
}
