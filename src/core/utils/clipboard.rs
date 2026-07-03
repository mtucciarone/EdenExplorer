use std::ffi::OsStr;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use windows::Win32::Foundation::{HANDLE, POINT};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{CF_HDROP, CF_UNICODETEXT};
use windows::Win32::UI::Shell::DragQueryFileW;
use windows::core::PCWSTR;

pub fn clear_clipboard_files() {
    unsafe {
        if OpenClipboard(None).is_ok() {
            let _ = SetClipboardData(CF_HDROP.0 as u32, None);
            let _ = CloseClipboard();
        }
    }
}

pub fn set_clipboard_files(paths: &[PathBuf], cut: bool) -> bool {
    if paths.is_empty() {
        return false;
    }

    let mut wide: Vec<u16> = Vec::new();
    for path in paths {
        wide.extend(path.as_os_str().encode_wide());
        wide.push(0);
    }
    wide.push(0);

    #[repr(C)]
    struct DropFiles {
        p_files: u32,
        pt: POINT,
        f_nc: i32,
        f_wide: i32,
    }

    let size = size_of::<DropFiles>() + wide.len() * size_of::<u16>();

    let hglobal = match unsafe { GlobalAlloc(GMEM_MOVEABLE, size) } {
        Ok(h) => h,
        Err(_) => return false,
    };

    unsafe {
        let ptr = GlobalLock(hglobal) as *mut u8;
        if ptr.is_null() {
            let _ = GlobalUnlock(hglobal);
            return false;
        }

        let drop_files = DropFiles {
            p_files: size_of::<DropFiles>() as u32,
            pt: POINT { x: 0, y: 0 },
            f_nc: 0,
            f_wide: 1,
        };

        std::ptr::copy_nonoverlapping(
            &drop_files as *const _ as *const u8,
            ptr,
            size_of::<DropFiles>(),
        );

        std::ptr::copy_nonoverlapping(
            wide.as_ptr() as *const u8,
            ptr.add(size_of::<DropFiles>()),
            wide.len() * size_of::<u16>(),
        );

        let _ = GlobalUnlock(hglobal);
    }

    let format_name: Vec<u16> = OsStr::new("Preferred DropEffect")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let format = unsafe { RegisterClipboardFormatW(PCWSTR(format_name.as_ptr())) };

    if unsafe { OpenClipboard(None).is_err() } {
        return false;
    }
    let _ = unsafe { EmptyClipboard() };
    let _ = unsafe { SetClipboardData(CF_HDROP.0 as u32, Some(HANDLE(hglobal.0))) };

    let effect: u32 = if cut { 2 } else { 5 };
    let hglobal_effect = match unsafe { GlobalAlloc(GMEM_MOVEABLE, size_of::<u32>()) } {
        Ok(h) => h,
        Err(_) => {
            let _ = unsafe { CloseClipboard() };
            return true;
        }
    };
    if !hglobal_effect.0.is_null() {
        unsafe {
            let ptr = GlobalLock(hglobal_effect) as *mut u32;
            if !ptr.is_null() {
                *ptr = effect;
                let _ = GlobalUnlock(hglobal_effect);
                let _ = SetClipboardData(format, Some(HANDLE(hglobal_effect.0)));
            }
        }
    }

    let _ = unsafe { CloseClipboard() };
    true
}

pub fn get_clipboard_files() -> Option<Vec<PathBuf>> {
    if unsafe { OpenClipboard(None).is_err() } {
        return None;
    }

    let hdrop = unsafe { GetClipboardData(CF_HDROP.0 as u32) };
    let hdrop = match hdrop {
        Ok(h) => h,
        Err(_) => {
            let _ = unsafe { CloseClipboard() };
            return None;
        }
    };

    let hdrop = windows::Win32::UI::Shell::HDROP(hdrop.0);
    let count = unsafe { DragQueryFileW(hdrop, 0xFFFFFFFF, None) };

    let mut paths = Vec::new();

    for i in 0..count {
        let len = unsafe { DragQueryFileW(hdrop, i, None) };
        if len == 0 {
            continue;
        }

        let mut buf = vec![0u16; (len + 1) as usize];
        let copied = unsafe { DragQueryFileW(hdrop, i, Some(&mut buf)) };

        if copied > 0 {
            let path = String::from_utf16_lossy(&buf[..copied as usize]);
            paths.push(PathBuf::from(path));
        }
    }

    let _ = unsafe { CloseClipboard() };

    Some(paths)
}

pub fn is_clipboard_cut() -> bool {
    if unsafe { OpenClipboard(None).is_err() } {
        return false;
    }

    let format_name: Vec<u16> = OsStr::new("Preferred DropEffect")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let format = unsafe { RegisterClipboardFormatW(PCWSTR(format_name.as_ptr())) };

    let mut is_cut = false;

    if let Ok(hglobal) = unsafe { GetClipboardData(format) } {
        if !hglobal.0.is_null() {
            let ptr =
                unsafe { GlobalLock(windows::Win32::Foundation::HGLOBAL(hglobal.0 as *mut _)) }
                    as *const u32;

            if !ptr.is_null() {
                let val = unsafe { *ptr };

                // 2 = DROPEFFECT_MOVE (cut)
                // 5 = DROPEFFECT_COPY | DROPEFFECT_LINK (rare combos)
                is_cut = val == 2;

                let _ = unsafe {
                    GlobalUnlock(windows::Win32::Foundation::HGLOBAL(hglobal.0 as *mut _))
                };
            }
        }
    }

    let _ = unsafe { CloseClipboard() };

    is_cut
}

pub fn clipboard_has_files() -> bool {
    if unsafe { OpenClipboard(None).is_err() } {
        return false;
    }
    let has = unsafe { GetClipboardData(CF_HDROP.0 as u32) }.is_ok();
    let _ = unsafe { CloseClipboard() };
    has
}

pub fn copy_text_to_clipboard(text: &str) -> bool {
    if unsafe { OpenClipboard(None).is_err() } {
        return false;
    }

    let _ = unsafe { EmptyClipboard() };

    // Convert string to wide string (UTF-16)
    let wide_text: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let text_size = wide_text.len() * std::mem::size_of::<u16>();

    let hglobal = match unsafe { GlobalAlloc(GMEM_MOVEABLE, text_size) } {
        Ok(h) => h,
        Err(_) => {
            let _ = unsafe { CloseClipboard() };
            return false;
        }
    };

    let ptr = unsafe { GlobalLock(hglobal) };
    if !ptr.is_null() {
        unsafe {
            std::ptr::copy_nonoverlapping(wide_text.as_ptr(), ptr as *mut u16, wide_text.len());
            let _ = GlobalUnlock(hglobal);
            let _ = SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(hglobal.0)));
        }
    } else {
        let _ = unsafe { CloseClipboard() };
        return false;
    }

    let _ = unsafe { CloseClipboard() };
    true
}
