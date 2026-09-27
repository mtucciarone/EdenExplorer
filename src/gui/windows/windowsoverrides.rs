use crate::core::drives::mark_drive_cache_dirty;
use crate::core::indexer::{WindowSizeMode, load_app_settings, save_app_settings};
use crate::core::launch::receive_copydata;
use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use crate::gui::utils::clickable_windows_icon;
use eframe::egui;
use egui::Context;
use egui_phosphor::regular;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
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
/// Width, in logical (96-DPI) pixels, of the client-area region reserved for
/// native window resizing. Scaled up per-monitor DPI at hit-test time so the
/// grab area stays a consistent, comfortably-sized target regardless of
/// display scaling - matching the ~8px border Windows uses for its own
/// resizable windows instead of an easy-to-miss sliver.
const RESIZE_BORDER: i32 = 8;

static ORIGINAL_WNDPROC: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Stashed once the main window's real HWND is known (mirrors the
/// `EGUI_CTX`/`set_egui_ctx` pattern just below) so any call site that needs
/// to parent a native dialog to the main window - most of which live in
/// `*_ui.rs` files that only ever see an `&mut egui::Ui`, never the HWND
/// itself - can reach it without threading a new parameter through every
/// settings page. See `dialog_owner`'s own doc comment for why this matters.
static MAIN_HWND: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

pub fn set_main_hwnd(hwnd: HWND) {
    MAIN_HWND.store(hwnd.0, Ordering::SeqCst);
}

fn main_hwnd() -> Option<HWND> {
    let ptr = MAIN_HWND.load(Ordering::SeqCst);
    if ptr.is_null() {
        None
    } else {
        Some(HWND(ptr))
    }
}

/// A minimal `HasWindowHandle` wrapper around a raw HWND, so it can be passed
/// to `rfd::FileDialog::set_parent`.
struct Win32DialogOwner(HWND);

impl HasWindowHandle for Win32DialogOwner {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let value = std::num::NonZeroIsize::new(self.0.0 as isize)
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = RawWindowHandle::Win32(raw_window_handle::Win32WindowHandle::new(value));
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(handle) })
    }
}

impl raw_window_handle::HasDisplayHandle for Win32DialogOwner {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        let handle =
            raw_window_handle::RawDisplayHandle::Windows(raw_window_handle::WindowsDisplayHandle::new());
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(handle) })
    }
}

/// Starts a new native file/folder dialog already parented to the main
/// window, if its HWND is known yet. Use this instead of
/// `rfd::FileDialog::new()` directly everywhere in this app.
///
/// This app strips `WS_CAPTION` and paints its entire window chrome
/// (title bar, min/max/close, border) itself every frame - there's no
/// native non-client area for Windows to fall back on. An *ownerless*
/// native common dialog (`rfd::FileDialog::new()` with no parent set) blocks
/// this window's own message loop while it's open without ever properly
/// marking this window as that dialog's owned/disabled parent - so if the
/// window is redrawn by the OS for any reason while blocked (losing focus,
/// a monitor/DPI change, Windows deciding the frozen message queue needs a
/// "ghost" placeholder), it falls back to a minimal *native* frame in the
/// corner (a bare system Close box, sometimes a context-help "?" glyph)
/// since there's no real caption for Windows to render - reported directly
/// as "weird border and control button" while an Export Settings dialog was
/// open. Explicitly parenting every dialog to the main HWND (via
/// `set_parent`, standard Win32 modal-ownership) fixes this the correct
/// way: Windows properly treats the main window as disabled-but-owned while
/// the dialog is up, instead of guessing.
pub fn dialog() -> rfd::FileDialog {
    let dialog = rfd::FileDialog::new();
    match main_hwnd() {
        Some(hwnd) => dialog.set_parent(&Win32DialogOwner(hwnd)),
        None => dialog,
    }
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

pub fn consume_clipboard_dirty() -> bool {
    CLIPBOARD_DIRTY.swap(false, Ordering::AcqRel)
}

pub fn mark_clipboard_dirty() {
    CLIPBOARD_DIRTY.store(true, Ordering::Release);
}

fn save_manual_window_size(hwnd: HWND) {
    unsafe {
        let mut rect = RECT::default();
        if GetClientRect(hwnd, &mut rect).is_err() {
            return;
        }

        // `GetClientRect` reports physical pixels, but every other reader of
        // `WindowSizeMode::Custom` (the per-frame save in mainwindow.rs, and
        // `.with_inner_size()` at startup in main.rs) works in egui's
        // logical points - the same space `i.viewport().inner_rect` uses.
        // Saving the raw physical value here silently inflated the
        // remembered size by the display's scale factor on any monitor not
        // running at 100% (125%/150% is a common Windows default), which
        // then got clamped to the monitor by `.with_clamp_size_to_monitor_
        // size(true)` on the next launch - looking like the window size
        // (and, since the startup position math uses this same size, the
        // position too) wasn't being remembered at all. Divide by the
        // window's own DPI scale to convert back to logical points before
        // saving, matching every other write to this field.
        let dpi = GetDpiForWindow(hwnd).max(1);
        let scale = dpi as f32 / 96.0;

        let width = (rect.right - rect.left) as f32 / scale;
        let height = (rect.bottom - rect.top) as f32 / scale;

        if width <= 0.0 || height <= 0.0 {
            return;
        }

        let (
            folder_scanning_enabled,
            show_hidden_files_folders,
            show_item_viewer_icons,
            windows_context_menu_enabled,
            _window_size_mode,
            start_path,
            saved_theme,
            pinned_tabs,
            time_format_24h,
            sort_column,
            sort_ascending,
            _language,
            date_style,
            custom_date_format,
            item_viewer_file_column_order,
            item_viewer_drive_column_order,
            recycle_bin_column_order,
            item_viewer_file_column_sizes,
            item_viewer_drive_column_sizes,
            recycle_bin_column_sizes,
            directory_settings,
            double_click_navigates_up,
            show_selection_checkboxes,
            middle_click_opens_new_tab,
            restore_last_session_tabs,
            default_display_mode,
            default_search_scope,
            search_engine,
            auto_open_notification_panel,
            show_operation_toasts,
        ) = load_app_settings();
        let window_size_mode = WindowSizeMode::Custom { width, height };

        save_app_settings(
            folder_scanning_enabled,
            show_hidden_files_folders,
            show_item_viewer_icons,
            windows_context_menu_enabled,
            &window_size_mode,
            &Some(start_path),
            saved_theme.as_deref(),
            &pinned_tabs,
            time_format_24h,
            sort_column,
            sort_ascending,
            &_language,
            date_style,
            &custom_date_format,
            &item_viewer_file_column_order,
            &item_viewer_drive_column_order,
            &recycle_bin_column_order,
            &item_viewer_file_column_sizes,
            &item_viewer_drive_column_sizes,
            &recycle_bin_column_sizes,
            &directory_settings,
            double_click_navigates_up,
            show_selection_checkboxes,
            middle_click_opens_new_tab,
            restore_last_session_tabs,
            default_display_mode,
            default_search_scope,
            search_engine,
            auto_open_notification_panel,
            show_operation_toasts,
        );
    }
}

fn color32_to_dwm(color: egui::Color32) -> u32 {
    let r = color.r() as u32;
    let g = color.g() as u32;
    let b = color.b() as u32;
    (b << 16) | (g << 8) | r
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

        let caption_color = color32_to_dwm(palette.application_bg_color);
        let text_color = color32_to_dwm(palette.application_bg_color);

        // `mainwindow.rs` already paints its own accent-colored 3px border
        // around the whole viewport by hand every frame (the one place this
        // app's border actually comes from) - setting DWMWA_BORDER_COLOR
        // (attribute 34) to that same accent color additionally asked
        // *Windows itself* to draw a second, native ~1px border on top of
        // it. The two together read as a double border specifically along
        // the top edge (where DWM's own border renders as a distinct thin
        // outer line above the app's thicker painted one; left/right/bottom
        // don't show the same visible gap between the two). Using
        // `DWMWA_COLOR_NONE` here tells DWM not to draw a border of its own
        // at all, leaving the app's single hand-painted border as the only
        // one visible on every edge.
        let border_color = DWMWA_COLOR_NONE;
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

        WM_EXITSIZEMOVE => {
            save_manual_window_size(hwnd);

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

        // Keep the custom borderless frame's client area aligned with the
        // window. Without this, Windows 10 may retain a non-client top inset
        // after WS_THICKFRAME is added, offsetting both painting and input.
        WM_NCCALCSIZE if wparam.0 != 0 => LRESULT(0),

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

                let left = top_left.x;
                let top = top_left.y;
                let right = bottom_right.x;
                let bottom = bottom_right.y;

                let dpi = GetDpiForWindow(hwnd).max(1);
                let resize = (RESIZE_BORDER as f32 * dpi as f32 / 96.0).round() as i32;

                // Top-left
                if x >= left && x < left + resize && y >= top && y < top + resize {
                    return LRESULT(HTTOPLEFT as _);
                }

                // Top-right
                if x >= right - resize && x < right && y >= top && y < top + resize {
                    return LRESULT(HTTOPRIGHT as _);
                }

                // Bottom-left
                if x >= left && x < left + resize && y >= bottom - resize && y < bottom {
                    return LRESULT(HTBOTTOMLEFT as _);
                }

                // Bottom-right
                if x >= right - resize && x < right && y >= bottom - resize && y < bottom {
                    return LRESULT(HTBOTTOMRIGHT as _);
                }

                // Left
                if x >= left && x < left + resize {
                    return LRESULT(HTLEFT as _);
                }

                // Right
                if x >= right - resize && x < right {
                    return LRESULT(HTRIGHT as _);
                }

                // Top
                if y >= top && y < top + resize {
                    return LRESULT(HTTOP as _);
                }

                // Bottom
                if y >= bottom - resize && y < bottom {
                    return LRESULT(HTBOTTOM as _);
                }

                LRESULT(HTCLIENT as _)
            }
        }

        WM_KEYDOWN | WM_SYSKEYDOWN => unsafe {
            // Every key press records whether it was Shift+Delete, so a
            // later, unrelated Cut (Ctrl+X) is never mistaken for it.
            use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_DELETE, VK_SHIFT};
            let shift_down = GetKeyState(VK_SHIFT.0 as i32) < 0;
            SHIFT_DELETE_PRESSED.store(
                shift_down && wparam.0 == VK_DELETE.0 as usize,
                Ordering::SeqCst,
            );
            if let Some(orig) = get_original_wndproc() {
                CallWindowProcW(orig, hwnd, msg, wparam, lparam)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        },

        _ => unsafe {
            if let Some(orig) = get_original_wndproc() {
                CallWindowProcW(orig, hwnd, msg, wparam, lparam)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        },
    }
}

/// Set when the most recent key press was Shift+Delete. egui's Windows input
/// layer turns Shift+Delete into a Cut command (the legacy Windows Cut
/// shortcut) and never reports the Delete key itself, so without this the
/// app could only see a Cut - Shift+Delete silently marked the selection for
/// cutting instead of permanently deleting it.
static SHIFT_DELETE_PRESSED: AtomicBool = AtomicBool::new(false);

/// Whether the Cut command currently being handled actually came from
/// Shift+Delete. Consumes the flag, so it's only honored once.
pub fn take_shift_delete() -> bool {
    SHIFT_DELETE_PRESSED.swap(false, Ordering::SeqCst)
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
            regular::COPY
        } else {
            regular::SQUARE
        };

        if clickable_windows_icon(ui, regular::X, palette.tab_close_hover, palette)
            .on_hover_text(
                egui::RichText::new(i18n.tr("close"))
                    .size(palette.tooltip_text_size)
                    .color(palette.tooltip_text_color),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
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
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            toggle_window_fullscreen(hwnd);
        }

        if clickable_windows_icon(ui, regular::MINUS, palette.primary, palette)
            .on_hover_text(
                egui::RichText::new(i18n.tr("minimize"))
                    .size(palette.tooltip_text_size)
                    .color(palette.tooltip_text_color),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
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
