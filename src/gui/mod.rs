// gui/mod.rs
pub mod dragdrop;
pub mod i18n;
pub mod icons;
pub mod theme;
pub mod utils;
pub mod windows; // for your windows folder

// optionally re-export MainWindow
pub use windows::mainwindow::MainWindow;
