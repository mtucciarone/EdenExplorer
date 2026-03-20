pub mod explorer;
pub mod explorer_imp;
pub mod features;
pub mod formatting;
pub mod icons;
pub mod itemviewer;
pub mod sidebar;
pub mod sorting;
pub mod tabs;
pub mod topbar;
pub mod utils;
pub mod customizetheme;
pub mod settings;

// Re-export main app so main.rs can use it cleanly
pub use explorer::ExplorerApp;
