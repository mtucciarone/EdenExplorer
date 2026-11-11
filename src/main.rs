#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod core;
mod gui;

use crate::core::indexer::{
    WindowSizeMode, load_window_position_on_start, load_window_was_maximized_on_start,
    load_windows_size_mode_on_start,
};
use crate::core::launch::{LaunchError, acquire_or_forward, existing_directories, parse_args};
use crate::core::utils::fonts::apply_custom_font_definitions;
use crate::gui::MainWindow;
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

    let _instance_guard = if launch_options.new_window || launch_paths.is_empty() {
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
    let was_maximized = load_window_was_maximized_on_start();
    let should_maximize = was_maximized || matches!(window_size_mode, WindowSizeMode::FullScreen);

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

    let saved_position = load_window_position_on_start();
    let initial_position = saved_position
        .map(|(x, y)| egui::pos2(x as f32, y as f32))
        .unwrap_or_else(|| egui::pos2(pos_x, pos_y));

    let mut viewport = egui::ViewportBuilder::default()
        .with_maximized(should_maximize)
        .with_icon(icon)
        .with_title_shown(false)
        .with_decorations(false)
        .with_clamp_size_to_monitor_size(true);

    // Supplying normal-window geometry alongside a maximized viewport can
    // make Windows briefly realize the restore rectangle before maximizing.
    if !should_maximize {
        viewport = viewport
            .with_inner_size(window_size)
            .with_position(initial_position);
    }

    // Before the graphics device exists, or the driver won't see it this launch.
    crate::core::nvidia_profile::ensure_gsync_disabled();

    let options = NativeOptions {
        viewport,
        wgpu_options: low_latency_wgpu_options(),
        ..Default::default()
    };

    eframe::run_native(
        "EdenExplorer",
        options,
        Box::new(|cc| {
            let mut fonts = egui::FontDefinitions::default();

            apply_custom_font_definitions(&mut fonts);

            cc.egui_ctx.set_fonts(fonts);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            set_egui_ctx(&cc.egui_ctx);

            Ok(Box::new(MainWindow::new_with_paths(launch_paths.clone())))
        }),
    )
}

/// Real hardware first (a discrete GPU, then integrated), then DX12 among
/// adapters of the same kind. The software renderer must not win on backend alone.
fn adapter_rank(
    device_type: eframe::wgpu::DeviceType,
    backend: eframe::wgpu::Backend,
) -> (u8, bool) {
    use eframe::wgpu::{Backend, DeviceType};

    let hardware = match device_type {
        DeviceType::DiscreteGpu => 3,
        DeviceType::IntegratedGpu => 2,
        DeviceType::VirtualGpu | DeviceType::Other => 1,
        DeviceType::Cpu => 0,
    };
    (hardware, backend == Backend::Dx12)
}

/// Keep the hover highlight on the cursor. With Vulkan, or with vsync (eframe's
/// default), what's on screen trails the mouse, which is easy to see when
/// moving over the item list on a 60 Hz display. Prefer a DX12 adapter and
/// present without vsync. eframe already queues at most one frame
/// (`SurfaceConfig::LOW_LATENCY`); this keeps that.
/// Eden repaints on demand, so this only costs anything while the UI changes;
/// animations ask for their next frame at ANIMATION_FRAME_INTERVAL instead of
/// right away, so they don't run uncapped.
fn low_latency_wgpu_options() -> eframe::egui_wgpu::WgpuConfiguration {
    use eframe::egui_wgpu::{SurfaceConfig, WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
    use eframe::wgpu;

    let mut setup = WgpuSetupCreateNew::without_display_handle();
    setup.native_adapter_selector = Some(std::sync::Arc::new(|adapters, surface| {
        adapters
            .iter()
            .filter(|adapter| surface.is_none_or(|surface| adapter.is_surface_supported(surface)))
            .max_by_key(|adapter| {
                let info = adapter.get_info();
                adapter_rank(info.device_type, info.backend)
            })
            .cloned()
            .ok_or_else(|| "no graphics adapter can present to this window".to_owned())
    }));

    WgpuConfiguration {
        surface: SurfaceConfig {
            present_mode: wgpu::PresentMode::AutoNoVsync,
            desired_maximum_frame_latency: Some(1),
        },
        wgpu_setup: WgpuSetup::CreateNew(setup),
        ..Default::default()
    }
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

#[cfg(test)]
#[path = "../tests/unit/gpu.rs"]
mod tests;
