#[derive(Clone, Debug)]
pub enum ThemeCustomizerAction {
    //ApplyTheme,
    SaveTheme,
    LoadTheme,
    ResetToDefaults,
    ExportTheme,
    ImportTheme,
}

#[derive(Clone, Debug)]
pub enum SettingsAction {
    ResetToDefaults,
    ResetFavourites,
    ApplySettings,
}
