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
            set_egui_ctx(&cc.egui_ctx);

            Ok(Box::new(gui::MainWindow::new_with_paths(
                launch_paths.clone(),
            )))
        }),
    )
}

/// Keep the hover highlight on the cursor. With Vulkan, or with vsync and two
/// queued frames (egui's defaults), what's on screen is several frames behind
/// the mouse, which is easy to see when moving over the item list on a 60 Hz
/// display. Prefer a DX12 adapter, present without vsync, and queue one frame.
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

/// Real hardware first (a discrete GPU, then integrated), then DX12 among
/// adapters of the same kind. The software renderer (WARP, "Microsoft Basic
/// Render Driver") is also a DX12 adapter, so it must never win on backend alone.
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
mod tests {
    use super::adapter_rank;
    use eframe::wgpu::{Backend, DeviceType};

    fn pick(adapters: &[(DeviceType, Backend)]) -> (DeviceType, Backend) {
        *adapters
            .iter()
            .max_by_key(|(device_type, backend)| adapter_rank(*device_type, *backend))
            .unwrap()
    }

    #[test]
    fn integrated_gpu_beats_the_software_renderer() {
        // DXGI lists WARP last, and max_by_key keeps the last of equal keys.
        let adapters = [
            (DeviceType::IntegratedGpu, Backend::Dx12),
            (DeviceType::IntegratedGpu, Backend::Vulkan),
            (DeviceType::Cpu, Backend::Dx12),
        ];
        assert_eq!(pick(&adapters), (DeviceType::IntegratedGpu, Backend::Dx12));
    }

    #[test]
    fn integrated_gpu_without_dx12_still_beats_the_software_renderer() {
        // wgpu skips some older iGPUs on DX12, leaving only GL for the real GPU.
        let adapters = [
            (DeviceType::Cpu, Backend::Dx12),
            (DeviceType::IntegratedGpu, Backend::Gl),
        ];
        assert_eq!(pick(&adapters), (DeviceType::IntegratedGpu, Backend::Gl));
    }

    #[test]
    fn discrete_gpu_on_dx12_is_preferred() {
        let adapters = [
            (DeviceType::DiscreteGpu, Backend::Dx12),
            (DeviceType::DiscreteGpu, Backend::Vulkan),
            (DeviceType::IntegratedGpu, Backend::Dx12),
            (DeviceType::Cpu, Backend::Dx12),
        ];
        assert_eq!(pick(&adapters), (DeviceType::DiscreteGpu, Backend::Dx12));
    }
}
