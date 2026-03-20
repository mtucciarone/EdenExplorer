mod app;
mod drives;
mod fs;
mod indexer;
mod state;

use eframe::{NativeOptions, egui};
use indexer::{load_app_settings, WindowSizeMode};

fn main() -> eframe::Result<()> {
    let icon = load_icon().expect("Failed to load icon");
    
    // Load saved window settings
    let (_folder_scanning_enabled, window_size_mode) = load_app_settings();
    
    // Determine window size based on saved settings
    let window_size = match window_size_mode {
        WindowSizeMode::FullScreen => {
            // For fullscreen, use a large size that will be maximized
            egui::Vec2::new(1920.0, 1080.0)
        }
        WindowSizeMode::HalfScreen => {
            // Use half of typical screen dimensions
            egui::Vec2::new(960.0, 540.0)
        }
        WindowSizeMode::Custom { width, height } => {
            egui::Vec2::new(width, height)
        }
    };

    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(window_size)
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "EdenExplorer",
        options,
        Box::new(|cc| {
            let mut fonts = egui::FontDefinitions::default();

            // Add Phosphor icons
            egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

            // Add Japanese/Unicode font support
            // Try to use a system font that supports Japanese characters
            fonts.font_data.insert(
                "japanese_font".to_owned(),
                egui::FontData::from_static(include_bytes!("assets/NotoSansJP-Regular.ttf")).into(),
            );

            // Assign the Japanese font to all font families for better Unicode support
            for family in &mut fonts.families.values_mut() {
                family.insert(0, "japanese_font".to_owned());
            }

            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(app::ExplorerApp::default()))
        }),
    )
}

static ICON_BYTES: &[u8] = include_bytes!("assets/icon.ico");

fn load_icon() -> Option<egui::IconData> {
    let image = image::load_from_memory(ICON_BYTES).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}
