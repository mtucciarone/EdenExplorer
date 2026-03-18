pub mod explorer;
pub mod features;
pub mod formatting;
pub mod icons;
pub mod itemviewer;
pub mod sidebar;
pub mod sorting;
pub mod tabs;
pub mod topbar;
pub mod utils;

// Re-export main app so main.rs can use it cleanly
pub use explorer::ExplorerApp;
