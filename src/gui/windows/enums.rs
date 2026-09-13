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

#[derive(Clone, Debug)]
pub enum SettingsAction {
    ResetToDefaults,
    ResetFavourites,
    ApplySettings,
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
    /// Produced by the Settings page's Appearance category (which embeds the
    /// same theme editor that used to be its own floating window) - handled
    /// identically to how that window's actions always were.
    ThemeCustomizer(ThemeCustomizerAction),
}
