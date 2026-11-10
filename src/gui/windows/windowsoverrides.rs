use crate::core::drives::mark_drive_cache_dirty;
use crate::core::i18n::I18n;
use crate::core::indexer::{
    WindowSizeMode, load_app_settings, save_app_settings, save_window_position,
    save_window_was_maximized,
};
use crate::core::launch::receive_copydata;
use crate::gui::icon_system::AppIcon;
use crate::gui::theme::ThemePalette;
use crate::gui::utils::clickable_windows_icon;
use eframe::egui;
use egui::Context;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Mutex, RwLock};
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, RemoveClipboardFormatListener,
};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::*;

const MIN_WIDTH: i32 = 600;
const MIN_HEIGHT: i32 = 400;
const REAPPLY_DPI_MESSAGE: u32 = WM_APP + 1;
const RESIZE_BORDER_FALLBACK: i32 = 8;

/// Title-bar caption button metrics in logical pixels (100% scale). Shared by
/// the drawing code, the layouts that reserve room for the buttons, and the
/// `WM_NCHITTEST` handling so they can never drift apart.
pub const CAPTION_BUTTON_WIDTH: f32 = 45.0;
pub const CAPTION_BUTTON_HEIGHT: f32 = 26.0;
pub const CAPTION_BUTTON_COUNT: f32 = 3.0;
pub const CAPTION_CONTROLS_WIDTH: f32 = CAPTION_BUTTON_WIDTH * CAPTION_BUTTON_COUNT;
pub const TITLE_BAR_HEIGHT: f32 = 32.0;

/// Smallest gap kept between a dialog and the edges of the window.
const DIALOG_WINDOW_MARGIN: f32 = 40.0;

/// Clamp a dialog's fixed size so it still fits inside a small window.
pub fn clamp_dialog_size(size: [f32; 2], available: egui::Vec2) -> [f32; 2] {
    [
        size[0].min((available.x - DIALOG_WINDOW_MARGIN).max(0.0)),
        size[1].min((available.y - DIALOG_WINDOW_MARGIN).max(0.0)),
    ]
}

/// DPI scale factor (1.0 at 96 DPI).
fn dpi_scale(dpi: u32) -> f32 {
    if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 }
}

/// Screen-space rectangle covered by the caption buttons, given the client
/// rectangle in screen coordinates.
fn caption_buttons_rect(client: RECT, scale: f32) -> RECT {
    RECT {
        left: client.right - (CAPTION_CONTROLS_WIDTH * scale).round() as i32,
        top: client.top,
        right: client.right,
        bottom: client.top + (CAPTION_BUTTON_HEIGHT * scale).round() as i32,
    }
}

/// Pure `WM_NCHITTEST` decision. `client` and `caption` are in screen
/// coordinates, `resize` is the resize band thickness in pixels.
fn hit_test(x: i32, y: i32, client: RECT, resize: i32, maximized: bool, caption: RECT) -> u32 {
    // The caption buttons win over the inward top resize band so hovering or
    // clicking their upper edge does not start a resize.
    if x >= caption.left && x < caption.right && y >= caption.top && y < caption.bottom {
        return HTCLIENT;
    }

    let (left, top, right, bottom) = (client.left, client.top, client.right, client.bottom);
    let left_resize = x >= left - resize && x < left;
    let right_resize = x >= right && x < right + resize;
    // There is no outward top frame in the normal window state, so retain
    // the small inward top resize target there.
    let top_resize = if maximized {
        y >= top - resize && y < top
    } else {
        y >= top && y < top + resize
    };
    let bottom_resize = y >= bottom && y < bottom + resize;

    if left_resize && top_resize {
        HTTOPLEFT
    } else if right_resize && top_resize {
        HTTOPRIGHT
    } else if left_resize && bottom_resize {
        HTBOTTOMLEFT
    } else if right_resize && bottom_resize {
        HTBOTTOMRIGHT
    } else if left_resize {
        HTLEFT
    } else if right_resize {
        HTRIGHT
    } else if top_resize {
        HTTOP
    } else if bottom_resize {
        HTBOTTOM
    } else {
        HTCLIENT
    }
}

/// Thickness of the invisible drag-to-resize border. Matches the native
/// window frame width (SM_CXFRAME + SM_CXPADDEDBORDER) so the grab area
/// feels identical to a normal decorated window. The old hard-coded 2px
/// zone was practically impossible to hit with the mouse.
fn resize_border_thickness() -> i32 {
    unsafe {
        let frame = GetSystemMetrics(SM_CXFRAME);
        let padded = GetSystemMetrics(SM_CXPADDEDBORDER);
        let thickness = frame + padded;
        if thickness > 0 {
            thickness
        } else {
            RESIZE_BORDER_FALLBACK
        }
    }
}

static ORIGINAL_WNDPROC: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
static IN_SIZE_MOVE: AtomicBool = AtomicBool::new(false);

lazy_static::lazy_static! {
    static ref PENDING_DPI: Mutex<Option<WPARAM>> = Mutex::new(None);
}

fn get_original_wndproc() -> Option<WNDPROC> {
    let ptr = ORIGINAL_WNDPROC.load(Ordering::SeqCst);
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut std::ffi::c_void, WNDPROC>(ptr) })
    }
}

fn set_original_wndproc(proc: WNDPROC) {
    let ptr = match proc {
        Some(f) => f as *mut std::ffi::c_void,
        None => std::ptr::null_mut(),
    };
    ORIGINAL_WNDPROC.store(ptr, Ordering::SeqCst);
}

lazy_static::lazy_static! {
    static ref EGUI_CTX: RwLock<Option<Context>> = RwLock::new(None);
}

static CLIPBOARD_DIRTY: AtomicBool = AtomicBool::new(true);

pub fn set_egui_ctx(ctx: &Context) {
    if let Ok(mut guard) = EGUI_CTX.write() {
        *guard = Some(ctx.clone());
    }
}

pub fn request_repaint() {
    if let Ok(guard) = EGUI_CTX.read() {
        if let Some(ctx) = guard.as_ref() {
            ctx.request_repaint();
        }
    }
}

/// For workers that report progress many times a second: wait for the next
/// animation frame instead of repainting on every report.
pub fn request_animation_frame() {
    if let Some(ctx) = EGUI_CTX.read().ok().and_then(|guard| guard.clone()) {
        crate::core::utils::widgets::request_animation_frame(&ctx);
    }
}

pub fn consume_clipboard_dirty() -> bool {
    CLIPBOARD_DIRTY.swap(false, Ordering::AcqRel)
}

pub fn mark_clipboard_dirty() {
    CLIPBOARD_DIRTY.store(true, Ordering::Release);
}

fn save_manual_window_size(hwnd: HWND) {
    unsafe {
        let is_maximized = IsZoomed(hwnd).as_bool();
        save_window_was_maximized(is_maximized);

        let mut rect = RECT::default();
        if GetClientRect(hwnd, &mut rect).is_err() {
            return;
        }

        let mut window_rect = RECT::default();
        if !is_maximized && GetWindowRect(hwnd, &mut window_rect).is_ok() {
            save_window_position(window_rect.left, window_rect.top);
        }

        let width = (rect.right - rect.left) as f32;
        let height = (rect.bottom - rect.top) as f32;

        if width <= 0.0 || height <= 0.0 {
            return;
        }

        let (
            folder_scanning_enabled,
            show_hidden_files_folders,
            show_item_viewer_icons,
            windows_context_menu_enabled,
            extra_actions_bar,
            window_size_mode,
            start_path,
            saved_theme,
            pinned_tabs,
            remember_open_tabs,
            unremembered_tabs,
            remembered_active_tab,
            time_format_24h,
            sort_column,
            sort_ascending,
            _language,
            date_style,
            item_viewer_file_column_order,
            item_viewer_drive_column_order,
            recycle_bin_column_order,
            item_viewer_file_column_sizes,
            item_viewer_drive_column_sizes,
            recycle_bin_column_sizes,
            directory_settings,
        ) = load_app_settings();
        // Keep the configured mode while maximized so the last normal size is
        // not replaced by the maximized client dimensions.
        let window_size_mode = if is_maximized {
            window_size_mode
        } else {
            WindowSizeMode::Custom { width, height }
        };

        save_app_settings(
            folder_scanning_enabled,
            show_hidden_files_folders,
            show_item_viewer_icons,
            windows_context_menu_enabled,
            extra_actions_bar,
            &window_size_mode,
            &Some(start_path),
            saved_theme.as_deref(),
            &pinned_tabs,
            remember_open_tabs,
            &unremembered_tabs,
            remembered_active_tab.as_ref(),
            time_format_24h,
            sort_column,
            sort_ascending,
            &_language,
            date_style,
            &item_viewer_file_column_order,
            &item_viewer_drive_column_order,
            &recycle_bin_column_order,
            &item_viewer_file_column_sizes,
            &item_viewer_drive_column_sizes,
            &recycle_bin_column_sizes,
            &directory_settings,
        );
    }
}

fn color32_to_dwm(color: egui::Color32) -> u32 {
    let r = color.r() as u32;
    let g = color.g() as u32;
    let b = color.b() as u32;
    (b << 16) | (g << 8) | r
}

fn apply_window_colors(hwnd: HWND, palette: &ThemePalette) {
    unsafe {
        let border_color = color32_to_dwm(palette.borders_default);
        let caption_color = color32_to_dwm(palette.application_bg_color);
        let text_color = color32_to_dwm(palette.application_bg_color);

        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(34),
            &border_color as *const _ as _,
            std::mem::size_of::<u32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(35),
            &caption_color as *const _ as _,
            std::mem::size_of::<u32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(36),
            &text_color as *const _ as _,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

/// Update DWM colors without changing the window frame or triggering a new
/// non-client-area calculation.
pub fn apply_window_theme(hwnd: HWND, palette: &ThemePalette) {
    apply_window_colors(hwnd, palette);
}

pub fn apply_window_override(hwnd: HWND, palette: &ThemePalette) {
    unsafe {
        let style = GetWindowLongW(hwnd, GWL_STYLE);

        let new_style = (style & !(WS_CAPTION.0 as i32))
            | (WS_THICKFRAME.0 as i32)
            | (WS_MINIMIZEBOX.0 as i32)
            | (WS_MAXIMIZEBOX.0 as i32)
            | (WS_SYSMENU.0 as i32);

        let _ = SetWindowLongW(hwnd, GWL_STYLE, new_style);

        let policy = DWMNCRENDERINGPOLICY(2);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(2),
            &policy as *const _ as _,
            std::mem::size_of::<DWMNCRENDERINGPOLICY>() as u32,
        );

        const DWMWA_WINDOW_CORNER_PREFERENCE: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(33);
        let preference: u32 = 2;

        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as _,
            std::mem::size_of::<u32>() as u32,
        );

        let margins = MARGINS {
            cxLeftWidth: 0,
            cxRightWidth: 0,
            cyTopHeight: 0,
            cyBottomHeight: 0,
        };

        apply_window_colors(hwnd, palette);

        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
        );
    }
}

pub unsafe fn install_wndproc(hwnd: HWND) -> Result<(), String> {
    unsafe {
        let result = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, custom_wndproc as *const () as isize);

        if result == 0 {
            let err = std::io::Error::last_os_error();
            return Err(format!("SetWindowLongPtrW failed: {}", err));
        }

        set_original_wndproc(std::mem::transmute::<isize, WNDPROC>(result));

        if AddClipboardFormatListener(hwnd).is_err() {
            return Err("AddClipboardFormatListener failed".to_string());
        }

        mark_clipboard_dirty();
        Ok(())
    }
}

unsafe extern "system" fn custom_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COPYDATA => {
            if receive_copydata(lparam) {
                request_repaint();
                LRESULT(1)
            } else {
                LRESULT(0)
            }
        }
        WM_CLIPBOARDUPDATE => {
            mark_clipboard_dirty();
            LRESULT(0)
        }

        WM_DEVICECHANGE => {
            mark_drive_cache_dirty();
            request_repaint();
            LRESULT(0)
        }

        WM_NCDESTROY => unsafe {
            let _ = RemoveClipboardFormatListener(hwnd);

            if let Some(orig) = get_original_wndproc() {
                CallWindowProcW(orig, hwnd, msg, wparam, lparam)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        },

        // Let winit update the scale factor, but avoid its resize while the
        // user is dragging the window across monitors. Replaying the latest
        // DPI value after the drag avoids the boundary oscillation/flicker.
        WM_DPICHANGED if IN_SIZE_MOVE.load(Ordering::Acquire) => {
            if let Ok(mut pending) = PENDING_DPI.lock() {
                *pending = Some(wparam);
            }
            LRESULT(0)
        }

        REAPPLY_DPI_MESSAGE => unsafe {
            let pending_dpi = PENDING_DPI
                .lock()
                .ok()
                .and_then(|mut pending| pending.take());
            if let Some(pending_wparam) = pending_dpi {
                let mut current_rect = RECT::default();
                if GetWindowRect(hwnd, &mut current_rect).is_ok() {
                    let pending_lparam = LPARAM((&current_rect as *const RECT) as isize);
                    if let Some(orig) = get_original_wndproc() {
                        return CallWindowProcW(
                            orig,
                            hwnd,
                            WM_DPICHANGED,
                            pending_wparam,
                            pending_lparam,
                        );
                    }
                }
            }
            LRESULT(0)
        },

        WM_CLOSE => {
            // Persist the final size before closing: this is the single
            // authoritative save, in physical pixels, that the next launch
            // restores via set_window_mode.
            save_manual_window_size(hwnd);

            unsafe {
                if let Some(orig) = get_original_wndproc() {
                    CallWindowProcW(orig, hwnd, msg, wparam, lparam)
                } else {
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
            }
        }

        WM_EXITSIZEMOVE => {
            save_manual_window_size(hwnd);

            unsafe {
                if let Some(orig) = get_original_wndproc() {
                    let result = CallWindowProcW(orig, hwnd, msg, wparam, lparam);
                    IN_SIZE_MOVE.store(false, Ordering::Release);

                    if PENDING_DPI
                        .lock()
                        .map(|pending| pending.is_some())
                        .unwrap_or(false)
                    {
                        let _ = PostMessageW(Some(hwnd), REAPPLY_DPI_MESSAGE, WPARAM(0), LPARAM(0));
                    }

                    result
                } else {
                    IN_SIZE_MOVE.store(false, Ordering::Release);
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
            }
        }

        WM_ENTERSIZEMOVE => {
            IN_SIZE_MOVE.store(true, Ordering::Release);
            unsafe {
                if let Some(orig) = get_original_wndproc() {
                    CallWindowProcW(orig, hwnd, msg, wparam, lparam)
                } else {
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
            }
        }

        WM_GETMINMAXINFO => {
            unsafe {
                let info = &mut *(lparam.0 as *mut MINMAXINFO);

                info.ptMinTrackSize.x = MIN_WIDTH;
                info.ptMinTrackSize.y = MIN_HEIGHT;

                let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                if !monitor.is_invalid() {
                    let mut monitor_info = MONITORINFO::default();
                    monitor_info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;

                    if GetMonitorInfoW(monitor, &mut monitor_info).as_bool() {
                        let work_area = monitor_info.rcWork;
                        let monitor_area = monitor_info.rcMonitor;

                        info.ptMaxPosition.x = work_area.left - monitor_area.left;
                        info.ptMaxPosition.y = work_area.top - monitor_area.top;
                        info.ptMaxSize.x = work_area.right - work_area.left;
                        info.ptMaxSize.y = work_area.bottom - work_area.top;
                    }
                }
            }

            LRESULT(0)
        }

        // Reserve the native resize frame outside the client area. This keeps
        // the resize band available to Windows without taking pixels away
        // from edge controls such as the vertical scrollbar. Maximized
        // windows need the same inset to avoid clipping their outer edge.
        WM_NCCALCSIZE if wparam.0 != 0 => {
            unsafe {
                let params = &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS);
                let border = resize_border_thickness();
                let rect = &mut params.rgrc[0];
                rect.left += border;
                rect.right -= border;
                rect.bottom -= border;
                // Keep the custom title-bar controls flush with the top of
                // the window. A top inset creates a dead strip above them.
                if IsZoomed(hwnd).as_bool() {
                    rect.top += border;
                }
            }
            LRESULT(0)
        }

        WM_NCHITTEST => {
            let x = get_x_lparam(lparam);
            let y = get_y_lparam(lparam);

            unsafe {
                let mut client_rect = RECT::default();

                if GetClientRect(hwnd, &mut client_rect).is_err() {
                    return DefWindowProcW(hwnd, msg, wparam, lparam);
                }

                // Convert client top-left and bottom-right to screen coordinates.
                let mut top_left = POINT {
                    x: client_rect.left,
                    y: client_rect.top,
                };

                let mut bottom_right = POINT {
                    x: client_rect.right,
                    y: client_rect.bottom,
                };

                let _ = ClientToScreen(hwnd, &mut top_left);
                let _ = ClientToScreen(hwnd, &mut bottom_right);

                let client = RECT {
                    left: top_left.x,
                    top: top_left.y,
                    right: bottom_right.x,
                    bottom: bottom_right.y,
                };
                let scale = dpi_scale(GetDpiForWindow(hwnd));
                let caption = caption_buttons_rect(client, scale);

                // The resize band lives outside the client area, like a
                // normal decorated window, so it does not steal the outer
                // pixels of edge controls such as the vertical scrollbar.
                LRESULT(hit_test(
                    x,
                    y,
                    client,
                    resize_border_thickness(),
                    IsZoomed(hwnd).as_bool(),
                    caption,
                ) as _)
            }
        }

        _ => unsafe {
            if let Some(orig) = get_original_wndproc() {
                CallWindowProcW(orig, hwnd, msg, wparam, lparam)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        },
    }
}

fn get_x_lparam(lparam: LPARAM) -> i32 {
    (lparam.0 & 0xFFFF) as i16 as i32
}

fn get_y_lparam(lparam: LPARAM) -> i32 {
    ((lparam.0 >> 16) & 0xFFFF) as i16 as i32
}

pub fn handle_draw_windows_buttons(
    i18n: &I18n,
    ui: &mut egui::Ui,
    hwnd: Option<HWND>,
    palette: &ThemePalette,
) {
    let old_item_spacing_x = ui.spacing().item_spacing.x;
    ui.spacing_mut().item_spacing.x = 0.0;

    if let Some(hwnd) = hwnd {
        let is_maximized = unsafe { IsZoomed(hwnd).as_bool() };

        let maximize_icon = if is_maximized {
            AppIcon::Copy.regular()
        } else {
            AppIcon::Square.regular()
        };

        if clickable_windows_icon(ui, AppIcon::X.regular(), palette.tab_close_hover, palette)
            .on_hover_text(
                egui::RichText::new(i18n.tr("close"))
                    .size(palette.tooltip_text_size)
                    .color(palette.tooltip_text_color),
            )
            .clicked()
        {
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }

        if clickable_windows_icon(ui, maximize_icon, palette.primary, palette)
            .on_hover_text(
                egui::RichText::new(if is_maximized {
                    i18n.tr("restore")
                } else {
                    i18n.tr("maximize")
                })
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
            )
            .clicked()
        {
            toggle_window_fullscreen(hwnd);
        }

        if clickable_windows_icon(ui, AppIcon::Minus.regular(), palette.primary, palette)
            .on_hover_text(
                egui::RichText::new(i18n.tr("minimize"))
                    .size(palette.tooltip_text_size)
                    .color(palette.tooltip_text_color),
            )
            .clicked()
        {
            unsafe {
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            }
        }
    }
    ui.spacing_mut().item_spacing.x = old_item_spacing_x;
}

pub fn toggle_window_fullscreen(hwnd: HWND) {
    unsafe {
        let mut placement = WINDOWPLACEMENT {
            length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };

        if GetWindowPlacement(hwnd, &mut placement).is_ok() {
            if placement.showCmd == SW_SHOWMAXIMIZED.0 as u32 {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            } else {
                let _ = ShowWindow(hwnd, SW_MAXIMIZE);
            }
        }
    }
}

pub fn get_hwnd_from_frame(frame: &eframe::Frame) -> Option<HWND> {
    let handle = frame.window_handle().ok()?;
    let raw = handle.as_raw();

    match raw {
        RawWindowHandle::Win32(h) => {
            let val = h.hwnd.get();
            if val == 0 {
                None
            } else {
                Some(HWND(val as *mut std::ffi::c_void))
            }
        }
        _ => None,
    }
}

fn resize_and_center_window(hwnd: HWND, width: i32, height: i32) {
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);

        if monitor.is_invalid() {
            return;
        }

        let mut monitor_info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };

        if !GetMonitorInfoW(monitor, &mut monitor_info).as_bool() {
            return;
        }

        let work = monitor_info.rcWork;

        let x = work.left + ((work.right - work.left) - width) / 2;
        let y = work.top + ((work.bottom - work.top) - height) / 2;

        let _ = ShowWindow(hwnd, SW_RESTORE);

        let _ = SetWindowPos(
            hwnd,
            None,
            x,
            y,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// Multi-monitor safety net: a size remembered on one monitor can overflow
/// the monitor the window actually opens on (e.g. 3360x2100 remembered,
/// 2560x1600 primary). An oversized borderless window pushes ALL its resize
/// edges off-screen, making it impossible to grab. Clamp once at startup so
/// the window is fully inside its monitor's work area and always resizable.
pub fn clamp_window_to_monitor_work_area(hwnd: HWND) {
    unsafe {
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return;
        }

        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if monitor.is_invalid() {
            return;
        }

        let mut info = MONITORINFO::default();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }

        let work = info.rcWork;
        let max_w = work.right - work.left;
        let max_h = work.bottom - work.top;
        let mut width = rect.right - rect.left;
        let mut height = rect.bottom - rect.top;
        if width <= 0 || height <= 0 || max_w <= 0 || max_h <= 0 {
            return;
        }
        if width > max_w {
            width = max_w;
        }
        if height > max_h {
            height = max_h;
        }

        let mut x = rect.left;
        let mut y = rect.top;
        if x < work.left {
            x = work.left;
        }
        if y < work.top {
            y = work.top;
        }
        if x + width > work.right {
            x = work.right - width;
        }
        if y + height > work.bottom {
            y = work.bottom - height;
        }

        let _ = SetWindowPos(
            hwnd,
            None,
            x,
            y,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

pub fn set_window_mode(hwnd: HWND, mode: &WindowSizeMode) {
    match mode {
        WindowSizeMode::FullScreen => unsafe {
            let _ = ShowWindow(hwnd, SW_MAXIMIZE);
        },

        WindowSizeMode::Custom { width, height } => {
            resize_and_center_window(hwnd, width.round() as i32, height.round() as i32);
        }
    }
}

/// Restore the saved top-left position without changing the current size.
pub fn set_window_position(hwnd: HWND, x: i32, y: i32) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// Resize the window while keeping its current top-left corner. Used for
/// live width/height edits from the settings dialog: re-centering on every
/// DragValue tick made the window visibly jump around the screen.
pub fn set_window_size_keep_origin(hwnd: HWND, width: f32, height: f32) {
    unsafe {
        // Ignore the request while maximized: SetWindowPos with a different
        // size would un-maximize the window.
        if IsZoomed(hwnd).as_bool() {
            return;
        }

        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return;
        }

        let _ = SetWindowPos(
            hwnd,
            None,
            rect.left,
            rect.top,
            width.round() as i32,
            height.round() as i32,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENT: RECT = RECT {
        left: 100,
        top: 100,
        right: 1100,
        bottom: 700,
    };
    const RESIZE: i32 = 8;

    fn caption() -> RECT {
        caption_buttons_rect(CLIENT, 1.0)
    }

    fn hit(x: i32, y: i32, maximized: bool) -> u32 {
        hit_test(x, y, CLIENT, RESIZE, maximized, caption())
    }

    #[test]
    fn caption_rect_matches_drawn_buttons() {
        let r = caption();
        assert_eq!(r.right - r.left, 135);
        assert_eq!(r.bottom - r.top, 26);
        assert_eq!(r.right, CLIENT.right);
        let r2 = caption_buttons_rect(CLIENT, 1.5);
        assert_eq!(r2.right - r2.left, 203);
        assert_eq!(r2.bottom - r2.top, 39);
    }

    #[test]
    fn top_band_over_caption_buttons_is_client() {
        // Top 8 px of Close/Max/Min must not start a resize.
        assert_eq!(hit(1050, 100, false), HTCLIENT);
        assert_eq!(hit(1050, 103, false), HTCLIENT);
        assert_eq!(hit(CLIENT.right - 135, 100, false), HTCLIENT);
    }

    #[test]
    fn top_band_elsewhere_still_resizes() {
        assert_eq!(hit(500, 100, false), HTTOP);
        assert_eq!(hit(500, 107, false), HTTOP);
        assert_eq!(hit(500, 108, false), HTCLIENT);
        assert_eq!(hit(CLIENT.right - 136, 100, false), HTTOP);
    }

    #[test]
    fn corners_and_edges() {
        assert_eq!(hit(95, 100, false), HTTOPLEFT);
        assert_eq!(hit(1105, 100, false), HTTOPRIGHT);
        assert_eq!(hit(95, 705, false), HTBOTTOMLEFT);
        assert_eq!(hit(1105, 705, false), HTBOTTOMRIGHT);
        assert_eq!(hit(95, 400, false), HTLEFT);
        assert_eq!(hit(1105, 400, false), HTRIGHT);
        assert_eq!(hit(500, 705, false), HTBOTTOM);
    }

    #[test]
    fn maximized_uses_outward_top_band() {
        assert_eq!(hit(500, 95, true), HTTOP);
        assert_eq!(hit(500, 101, true), HTCLIENT);
        assert_eq!(hit(1050, 105, true), HTCLIENT);
    }

    #[test]
    fn drag_area_and_content_are_client() {
        assert_eq!(hit(500, 120, false), HTCLIENT);
        assert_eq!(hit(500, 400, false), HTCLIENT);
    }

    #[test]
    fn dpi_scale_defaults_to_one() {
        assert_eq!(dpi_scale(0), 1.0);
        assert_eq!(dpi_scale(96), 1.0);
        assert_eq!(dpi_scale(144), 1.5);
    }

    #[test]
    fn dialog_size_is_clamped_to_window() {
        let small = egui::vec2(600.0, 400.0);
        assert_eq!(clamp_dialog_size([600.0, 550.0], small), [560.0, 360.0]);
        let big = egui::vec2(1920.0, 1080.0);
        assert_eq!(clamp_dialog_size([600.0, 550.0], big), [600.0, 550.0]);
    }
}
