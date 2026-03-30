use crate::gui::theme::ThemePalette;
use crate::gui::utils::clickable_icon;
use eframe::egui;
use egui::Context;
use egui_phosphor::regular;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::Foundation::{HWND};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, ScreenToClient,
};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, RemoveClipboardFormatListener,
};

static mut ORIGINAL_WNDPROC: Option<WNDPROC> = None;
const MIN_WIDTH: i32 = 600;
const MIN_HEIGHT: i32 = 400;
const RESIZE_BORDER: i32 = 6;
const DRAG_HEIGHT: i32 = 36;

pub fn get_hwnd_from_cc(cc: &eframe::CreationContext<'_>) -> Option<HWND> {
    let handle = cc.window_handle().ok()?;
    let raw = handle.as_raw();

    match raw {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut std::ffi::c_void)),
        _ => None,
    }
}

lazy_static::lazy_static! {
    static ref EGUI_CTX: RwLock<Option<Context>> = RwLock::new(None);
}

static CLIPBOARD_DIRTY: AtomicBool = AtomicBool::new(true);

pub fn set_egui_ctx(ctx: &Context) {
    *EGUI_CTX.write().unwrap() = Some(ctx.clone());
}

pub fn consume_clipboard_dirty() -> bool {
    CLIPBOARD_DIRTY.swap(false, Ordering::AcqRel)
}

pub fn mark_clipboard_dirty() {
    CLIPBOARD_DIRTY.store(true, Ordering::Release);
}

fn color32_to_dwm(color: egui::Color32) -> u32 {
    let r = color.r() as u32;
    let g = color.g() as u32;
    let b = color.b() as u32;

    // Windows expects 0x00BBGGRR
    (b << 16) | (g << 8) | r
}

pub fn apply_window_override(hwnd: HWND, palette: &ThemePalette) {
    unsafe {
        // --- 1. Keep minimal frame ---
        let style = GetWindowLongW(hwnd, GWL_STYLE);

        let new_style = (style & !(WS_CAPTION.0 as i32)) // remove title bar
            | (WS_THICKFRAME.0 as i32)
            | (WS_MINIMIZEBOX.0 as i32)
            | (WS_MAXIMIZEBOX.0 as i32)
            | (WS_SYSMENU.0 as i32);

        let _ = SetWindowLongW(hwnd, GWL_STYLE, new_style);

        // --- 2. Disable DWM non-client rendering ---
        let policy = DWMNCRENDERINGPOLICY(2); // 👈 DWMNCRP_DISABLED

        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(2), // 👈 DWMWA_NCRENDERING_POLICY
            &policy as *const _ as _,
            std::mem::size_of::<DWMNCRENDERINGPOLICY>() as u32,
        );

        const DWMWA_WINDOW_CORNER_PREFERENCE: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(33);
        let preference: u32 = 2; // DWMWCP_ROUND

        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as _,
            std::mem::size_of::<u32>() as u32,
        );

        // --- 3. Remove frame insets (fix top gap) ---
        let margins = MARGINS {
            cxLeftWidth: 0,
            cxRightWidth: 0,
            cyTopHeight: 0,
            cyBottomHeight: 0,
        };

        let border_color = color32_to_dwm(palette.application_bg_color);
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

        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

        // --- 4. Apply changes ---
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

pub unsafe fn install_wndproc(hwnd: HWND) {
    unsafe {
        ORIGINAL_WNDPROC = Some(std::mem::transmute::<isize, WNDPROC>(SetWindowLongPtrW(
            hwnd,
            GWLP_WNDPROC,
            custom_wndproc as *const () as isize,
        )));
        let _ = AddClipboardFormatListener(hwnd);
        mark_clipboard_dirty();
    }
}

unsafe extern "system" fn custom_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CLIPBOARDUPDATE => {
            mark_clipboard_dirty();
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let _ = RemoveClipboardFormatListener(hwnd);
            if let Some(orig) = ORIGINAL_WNDPROC {
                CallWindowProcW(orig, hwnd, msg, wparam, lparam)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        WM_GETMINMAXINFO => {
            let info = &mut *(lparam.0 as *mut MINMAXINFO);

            // minimum size
            info.ptMinTrackSize.x = MIN_WIDTH;
            info.ptMinTrackSize.y = MIN_HEIGHT;

            // Get monitor work area
            unsafe {
                let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                if !monitor.is_invalid() {
                    let mut monitor_info = MONITORINFO::default();
                    monitor_info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                    if GetMonitorInfoW(monitor, &mut monitor_info).as_bool() {
                        let work_area = monitor_info.rcWork;
                        let monitor_area = monitor_info.rcMonitor;

                        // max window size = monitor work area (leaves taskbar visible)
                        info.ptMaxPosition.x = work_area.left - monitor_area.left;
                        info.ptMaxPosition.y = work_area.top - monitor_area.top;
                        info.ptMaxSize.x = work_area.right - work_area.left;
                        info.ptMaxSize.y = work_area.bottom - work_area.top;
                    }
                }
            }

            LRESULT(0)
        }
        WM_NCHITTEST => {
            let x = get_x_lparam(lparam);
            let y = get_y_lparam(lparam);

            let mut rect = RECT::default();
            unsafe {
                let _ = GetWindowRect(hwnd, &mut rect);
            }

            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;

            let local_x = x - rect.left;
            let local_y = y - rect.top;

            // 8 resize corners
            if local_x < RESIZE_BORDER && local_y < RESIZE_BORDER {
                return LRESULT(HTTOPLEFT as _);
            }
            if local_x >= width - RESIZE_BORDER && local_y < RESIZE_BORDER {
                return LRESULT(HTTOPRIGHT as _);
            }
            if local_x < RESIZE_BORDER && local_y >= height - RESIZE_BORDER {
                return LRESULT(HTBOTTOMLEFT as _);
            }
            if local_x >= width - RESIZE_BORDER && local_y >= height - RESIZE_BORDER {
                return LRESULT(HTBOTTOMRIGHT as _);
            }

            // edges
            if local_x < RESIZE_BORDER {
                return LRESULT(HTLEFT as _);
            }
            if local_x >= width - RESIZE_BORDER {
                return LRESULT(HTRIGHT as _);
            }
            if local_y < RESIZE_BORDER {
                return LRESULT(HTTOP as _);
            }
            if local_y >= height - RESIZE_BORDER {
                return LRESULT(HTBOTTOM as _);
            }

            // drag zone
            if local_y < DRAG_HEIGHT {
                // convert screen -> client pixels
                let mut point = POINT { x, y };
                unsafe { ScreenToClient(hwnd, &mut point) };

                if let Some(ctx) = EGUI_CTX.read().unwrap().as_ref() {
                    let ppp = ctx.pixels_per_point();
                    let client_pos = egui::pos2(point.x as f32 / ppp, point.y as f32 / ppp);
                    // check if pointer is over any egui widget (includes areas/popup windows)
                    let pointer_over = ctx.is_pointer_over_area();
                    if !pointer_over {
                        return LRESULT(HTCAPTION as _); // allow dragging
                    }
                }

                // over egui -> don't drag
                return LRESULT(HTCLIENT as _);
            }

            // everything else is client
            LRESULT(HTCLIENT as _)
        }
        _ => {
            unsafe {
                // forward everything else to original WNDPROC
                if let Some(orig) = ORIGINAL_WNDPROC {
                    CallWindowProcW(orig, hwnd, msg, wparam, lparam)
                } else {
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
            }
        }
    }
}

// helper
fn get_x_lparam(lparam: LPARAM) -> i32 {
    (lparam.0 & 0xFFFF) as i16 as i32
}
fn get_y_lparam(lparam: LPARAM) -> i32 {
    ((lparam.0 >> 16) & 0xFFFF) as i16 as i32
}

pub fn handle_draw_windows_buttons(ui: &mut egui::Ui, hwnd: Option<HWND>, palette: &ThemePalette) {
    if let Some(hwnd) = hwnd {
        if clickable_icon(ui, regular::X, palette.primary)
            .on_hover_text(
                egui::RichText::new("Close")
                    .size(palette.tooltip_text_size)
                    .color(palette.tooltip_text_color),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::*;
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }

        if clickable_icon(ui, regular::SQUARE, palette.primary)
            .on_hover_text(
                egui::RichText::new("Maximize")
                    .size(palette.tooltip_text_size)
                    .color(palette.tooltip_text_color),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::*;
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

        if clickable_icon(ui, regular::MINUS, palette.primary)
            .on_hover_text(
                egui::RichText::new("Minimize")
                    .size(palette.tooltip_text_size)
                    .color(palette.tooltip_text_color),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::*;
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            }
        }
    }
}
