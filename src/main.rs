#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod core;
mod gui;

use crate::core::indexer::{WindowSizeMode, load_windows_size_mode_on_start};
use crate::core::launch::{LaunchError, acquire_or_forward, existing_directories, parse_args};
use crate::core::utils::fonts::apply_custom_font_definitions;
use crate::gui::windows::windowsoverrides::set_egui_ctx;
use eframe::{NativeOptions, egui};
use std::os::windows::ffi::OsStrExt;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, MB_ICONERROR, MB_OK, MessageBoxW, SM_CXSCREEN, SM_CYSCREEN,
};
use windows::core::PCWSTR;

fn main() -> eframe::Result<()> {
    let launch_options = match parse_args(std::env::args()) {
        Ok(options) => options,
        Err(error) => {
            show_launch_error(&error);
            return Ok(());
        }
    };
    let launch_paths = match existing_directories(&launch_options) {
        Ok(paths) => paths,
        Err(error) => {
            show_launch_error(&error);
            return Ok(());
        }
    };

    let _instance_guard = if launch_options.new_window {
        None
    } else {
        match acquire_or_forward(&launch_paths) {
            Ok(Some(guard)) => Some(guard),
            Ok(None) => return Ok(()),
            Err(error) => {
                show_launch_error(&error);
                return Ok(());
            }
        }
    };

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let icon = load_icon();
    let window_size_mode = load_windows_size_mode_on_start();

    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) } as f32;
    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) } as f32;

    let window_size = match window_size_mode {
        WindowSizeMode::FullScreen => egui::Vec2::new(screen_w, screen_h),
        WindowSizeMode::Custom { width, height } => {
            egui::Vec2::new(width.min(screen_w), height.min(screen_h))
        }
    };

    let pos_x = ((screen_w - window_size.x) * 0.5).max(0.0);
    let pos_y = ((screen_h - window_size.y) * 0.5).max(0.0);

    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(window_size)
            .with_position(egui::pos2(pos_x, pos_y))
            .with_icon(icon)
            .with_title_shown(false)
            .with_decorations(false)
            .with_clamp_size_to_monitor_size(true),
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

            Ok(Box::new(gui::MainWindow::new_with_paths(
                launch_paths.clone(),
            )))
        }),
    )
}

fn show_launch_error(error: &LaunchError) {
    let message = error.message();

    #[cfg(windows)]
    {
        let text: Vec<u16> = std::ffi::OsStr::new(&message)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let title: Vec<u16> = std::ffi::OsStr::new("EdenExplorer")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        unsafe {
            let _ = MessageBoxW(
                None,
                PCWSTR(text.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
    }

    #[cfg(not(windows))]
    eprintln!("{message}");
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
