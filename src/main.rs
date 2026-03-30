mod core;
mod gui;

use crate::core::indexer::{WindowSizeMode, load_app_settings};
use crate::gui::windows::windowsoverrides::{get_hwnd_from_cc, set_egui_ctx};
use eframe::{NativeOptions, egui};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

fn main() -> eframe::Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).unwrap();
    }
    let icon = load_icon().expect("Failed to load icon");
    let (_folder_scanning_enabled, window_size_mode, _start_path) = load_app_settings();
    let window_size = match window_size_mode {
        WindowSizeMode::FullScreen => egui::Vec2::new(1920.0, 1080.0),
        WindowSizeMode::HalfScreen => egui::Vec2::new(960.0, 540.0),
        WindowSizeMode::Custom { width, height } => egui::Vec2::new(width, height),
    };

    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(window_size)
            .with_icon(icon)
            .with_title_shown(false)
            .with_decorations(false),
        ..Default::default()
    };

    eframe::run_native(
        "EdenExplorer",
        options,
        Box::new(|cc| {
            let mut fonts = egui::FontDefinitions::default();
            egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
            // Register fill variant under a separate family for selective use (e.g., filled star).
            fonts.font_data.insert(
                "phosphor_fill".to_owned(),
                egui_phosphor::Variant::Fill.font_data().into(),
            );
            fonts.families.insert(
                egui::FontFamily::Name("phosphor_fill".into()),
                vec!["phosphor_fill".to_owned()],
            );
            fonts.font_data.insert(
                "japanese_font".to_owned(),
                egui::FontData::from_static(include_bytes!("assets/NotoSansJP-Regular.ttf")).into(),
            );
            for family in &mut fonts.families.values_mut() {
                family.insert(0, "japanese_font".to_owned());
            }

            cc.egui_ctx.set_fonts(fonts);

            let hwnd = get_hwnd_from_cc(cc);
            set_egui_ctx(&cc.egui_ctx);
            Ok(Box::new(gui::MainWindow::new(hwnd)))
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
