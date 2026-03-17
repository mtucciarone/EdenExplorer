pub mod explorer;
pub mod table;
pub mod sorting;
pub mod formatting;
pub mod icons;

// Re-export main app so main.rs can use it cleanly
pub use explorer::ExplorerApp;