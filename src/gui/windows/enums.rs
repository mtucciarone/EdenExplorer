#[derive(Clone, Debug)]
pub enum ThemeCustomizerAction {
    ThemeUpdated(crate::gui::theme::ThemeMode),
    ResetToDefaults(crate::gui::theme::ThemeMode),
    ExportTheme(crate::gui::theme::ThemeMode),
    ImportTheme(crate::gui::theme::ThemeMode),
}

#[derive(Clone, Debug)]
pub enum SettingsAction {
    ResetToDefaults,
    ResetFavourites,
    ApplySettings,
}
