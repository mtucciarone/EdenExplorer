#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod core;
mod gui;

use crate::core::indexer::{WindowSizeMode, load_windows_size_mode_on_start};
use crate::core::utils::fonts::apply_custom_font_definitions;
use crate::gui::windows::windowsoverrides::set_egui_ctx;
use eframe::{NativeOptions, egui};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

fn main() -> eframe::Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let icon = load_icon();
    let window_size_mode = load_windows_size_mode_on_start();
    let window_size = match window_size_mode {
        WindowSizeMode::FullScreen => egui::Vec2::new(1920.0, 1080.0),
        WindowSizeMode::Custom { width, height } => egui::Vec2::new(width, height),
    };

    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) } as f32;
    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) } as f32;
    let pos_x = ((screen_w - window_size.x) * 0.5).max(0.0);
    let pos_y = ((screen_h - window_size.y) * 0.5).max(0.0);

    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(window_size)
            .with_position(egui::pos2(pos_x, pos_y))
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

            apply_custom_font_definitions(&mut fonts);

            cc.egui_ctx.set_fonts(fonts);
            set_egui_ctx(&cc.egui_ctx);

            Ok(Box::new(gui::MainWindow::new()))
        }),
    )
}

static ICON_BYTES: &[u8] = include_bytes!("assets/icon.ico");

fn load_icon() -> egui::IconData {
    match image::load_from_memory(ICON_BYTES) {
        Ok(img) => {
            let rgba = img.into_rgba8();
            let (width, height) = rgba.dimensions();
            egui::IconData {
                rgba: rgba.into_raw(),
                width,
                height,
            }
        }
        Err(_) => egui::IconData {
            rgba: vec![0u8; 64 * 64 * 4],
            width: 64,
            height: 64,
        },
    }
}
