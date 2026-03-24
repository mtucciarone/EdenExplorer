mod core;
mod gui;

use crate::core::indexer::{WindowSizeMode, load_app_settings};
use eframe::{NativeOptions, egui};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::HWND;

fn get_hwnd_from_cc(cc: &eframe::CreationContext<'_>) -> Option<HWND> {
    let handle = cc.window_handle().ok()?;
    let raw = handle.as_raw();

    match raw {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut std::ffi::c_void)),
        _ => None,
    }
}

fn main() -> eframe::Result<()> {
    let icon = load_icon().expect("Failed to load icon");
    let (_folder_scanning_enabled, window_size_mode) = load_app_settings();
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
            fonts.font_data.insert(
                "japanese_font".to_owned(),
                egui::FontData::from_static(include_bytes!("assets/NotoSansJP-Regular.ttf")).into(),
            );
            for family in &mut fonts.families.values_mut() {
                family.insert(0, "japanese_font".to_owned());
            }

            cc.egui_ctx.set_fonts(fonts);

            let hwnd = get_hwnd_from_cc(cc);
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
