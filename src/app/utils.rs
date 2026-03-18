use eframe::egui;

pub fn drive_usage_color(ratio: f32) -> egui::Color32 {
    let base = if ratio > 0.95 {
        egui::Color32::from_rgb(200, 72, 72)
    } else if ratio >= 0.85 {
        egui::Color32::from_rgb(214, 170, 76)
    } else {
        egui::Color32::from_rgb(88, 170, 120)
    };

    base.gamma_multiply(0.6) // 👈 dull it down
}

pub fn drive_usage_gradient(ratio: f32) -> (egui::Color32, egui::Color32) {
    let left = drive_usage_color(ratio);
    let right = left.gamma_multiply(0.8); // slightly darker for gradient
    (left, right)
}

pub fn drive_usage_bar(
    ui: &mut egui::Ui,
    total: u64,
    free: u64,
    height: f32,
    palette: &crate::app::features::ThemePalette,
) {
    let used = total.saturating_sub(free);

    let target_ratio = if total == 0 {
        0.0
    } else {
        used as f32 / total as f32
    };

    // 🔥 smooth animation
    let id = ui.id().with("drive_usage_anim");
    let animated_ratio = ui.ctx().animate_value_with_time(
        id,
        target_ratio,
        0.4, // animation speed (lower = faster)
    );

    let width = ui.available_width();

    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());

    let painter = ui.painter();

    // background track
    painter.rect_filled(rect, 2.0, palette.sidebar_hover.gamma_multiply(0.5));

    // fill width
    let fill_width = rect.width() * animated_ratio;

    if fill_width > 0.0 {
        let fill_rect = egui::Rect::from_min_size(rect.min, egui::vec2(fill_width, rect.height()));

        let (left, right) = drive_usage_gradient(target_ratio);

        painter.add(egui::epaint::RectShape::filled(
            fill_rect,
            2.0,
            egui::Color32::from_rgb(left.r(), left.g(), left.b()),
        ));

        // 🔥 subtle highlight strip (fake gradient feel)
        let highlight_rect = egui::Rect::from_min_size(
            fill_rect.min,
            egui::vec2(fill_rect.width(), fill_rect.height() * 0.5),
        );

        painter.rect_filled(highlight_rect, 2.0, egui::Color32::from_white_alpha(20));
    }

    // percentage text
    let percent = format!("{:.0}%", target_ratio * 100.0);

    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        percent,
        egui::TextStyle::Small.resolve(ui.style()),
        egui::Color32::WHITE,
    );
}

pub fn copy_dir_recursive(src: &std::path::Path, dest: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let new_path = dest.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &new_path)?;
        } else {
            std::fs::copy(entry.path(), new_path)?;
        }
    }
    Ok(())
}

pub fn shell_delete_to_recycle_bin(path: &std::path::PathBuf) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{SHFileOperationW, FOF_ALLOWUNDO, FO_DELETE, SHFILEOPSTRUCTW};

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    wide.push(0);

    let mut op = SHFILEOPSTRUCTW::default();
    op.wFunc = FO_DELETE;
    op.pFrom = PCWSTR(wide.as_ptr());
    op.fFlags = FOF_ALLOWUNDO.0 as u16;

    unsafe { SHFileOperationW(&mut op) == 0 }
}

pub fn set_clipboard_files(paths: &[std::path::PathBuf], cut: bool) -> bool {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::CF_HDROP;

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
        pt: windows::Win32::Foundation::POINT,
        f_nc: i32,
        f_wide: i32,
    }

    let size = std::mem::size_of::<DropFiles>() + wide.len() * std::mem::size_of::<u16>();

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
            p_files: std::mem::size_of::<DropFiles>() as u32,
            pt: windows::Win32::Foundation::POINT { x: 0, y: 0 },
            f_nc: 0,
            f_wide: 1,
        };

        std::ptr::copy_nonoverlapping(
            &drop_files as *const _ as *const u8,
            ptr,
            std::mem::size_of::<DropFiles>(),
        );

        std::ptr::copy_nonoverlapping(
            wide.as_ptr() as *const u8,
            ptr.add(std::mem::size_of::<DropFiles>()),
            wide.len() * std::mem::size_of::<u16>(),
        );

        let _ = GlobalUnlock(hglobal);
    }

    let format_name: Vec<u16> = OsStr::new("Preferred DropEffect")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let format = unsafe { RegisterClipboardFormatW(windows::core::PCWSTR(format_name.as_ptr())) };

    if unsafe { OpenClipboard(None).is_err() } {
        return false;
    }
    let _ = unsafe { EmptyClipboard() };
    let _ = unsafe {
        SetClipboardData(
            CF_HDROP.0 as u32,
            windows::Win32::Foundation::HANDLE(hglobal.0 as isize),
        )
    };

    let effect: u32 = if cut { 2 } else { 5 };
    let hglobal_effect = match unsafe { GlobalAlloc(GMEM_MOVEABLE, std::mem::size_of::<u32>()) } {
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
                let _ = SetClipboardData(
                    format,
                    windows::Win32::Foundation::HANDLE(hglobal_effect.0 as isize),
                );
            }
        }
    }

    let _ = unsafe { CloseClipboard() };
    true
}

pub fn get_clipboard_files() -> Option<(Vec<std::path::PathBuf>, bool)> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW,
    };
    use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::CF_HDROP;
    use windows::Win32::UI::Shell::DragQueryFileW;

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
            paths.push(std::path::PathBuf::from(path));
        }
    }

    let format_name: Vec<u16> = OsStr::new("Preferred DropEffect")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let format = unsafe { RegisterClipboardFormatW(windows::core::PCWSTR(format_name.as_ptr())) };

    let mut cut = false;
    if let Ok(hglobal) = unsafe { GetClipboardData(format) } {
        if hglobal.0 != 0 {
            let ptr =
                unsafe { GlobalLock(windows::Win32::Foundation::HGLOBAL(hglobal.0 as *mut _)) }
                    as *const u32;
            if !ptr.is_null() {
                let val = unsafe { *ptr };
                cut = val == 2;
                let _ = unsafe {
                    GlobalUnlock(windows::Win32::Foundation::HGLOBAL(hglobal.0 as *mut _))
                };
            }
        }
    }

    let _ = unsafe { CloseClipboard() };
    Some((paths, cut))
}

pub fn clipboard_has_files() -> bool {
    use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
    use windows::Win32::System::Ole::CF_HDROP;

    if unsafe { OpenClipboard(None).is_err() } {
        return false;
    }
    let has = unsafe { GetClipboardData(CF_HDROP.0 as u32) }.is_ok();
    let _ = unsafe { CloseClipboard() };
    has
}
