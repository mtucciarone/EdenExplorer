pub mod explorer;
pub mod itemviewer;
pub mod sorting;
pub mod formatting;
pub mod icons;
pub mod utils;
pub mod sidebar;
pub mod topbar;
pub mod tabs;

// Re-export main app so main.rs can use it cleanly
pub use explorer::ExplorerApp;
