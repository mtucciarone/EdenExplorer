use crate::core::state::FileItem;
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::path::PathBuf;
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Shell::{
    FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName,
};
use windows::Win32::UI::Shell::{
    SHFILEINFOW, SHGFI_TYPENAME, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW,
};
use windows::core::PCWSTR;

/// Creates a clickable icon with hover color effect
pub fn clickable_icon(ui: &mut egui::Ui, icon: &str, hover_color: egui::Color32) -> egui::Response {
    let resp = ui.add(egui::Label::new(icon).sense(egui::Sense::click()));

    if resp.hovered() {
        ui.painter().text(
            resp.rect.center(),
            egui::Align2::CENTER_CENTER,
            icon,
            egui::FontId::default(),
            hover_color,
        );
    }

    resp
}

pub fn get_cut_paths() -> HashSet<PathBuf> {
    if let Some((paths, cut)) = get_clipboard_files() {
        if cut {
            return paths.into_iter().collect();
        }
    }
    HashSet::new()
}

pub fn clear_clipboard_files() {
    use windows::Win32::System::DataExchange::{CloseClipboard, OpenClipboard, SetClipboardData};
    use windows::Win32::System::Ole::CF_HDROP;

    unsafe {
        if OpenClipboard(None).is_ok() {
            let _ = SetClipboardData(CF_HDROP.0 as u32, None);
            let _ = CloseClipboard();
        }
    }
}

pub fn drive_usage_color(ratio: f32, palette: &crate::gui::theme::ThemePalette) -> egui::Color32 {
    let base = if ratio > 0.95 {
        palette.drive_usage_critical
    } else if ratio >= 0.85 {
        palette.drive_usage_warning
    } else {
        palette.drive_usage_normal
    };

    base.gamma_multiply(0.6)
}

pub fn drive_usage_gradient(
    ratio: f32,
    palette: &crate::gui::theme::ThemePalette,
) -> (egui::Color32, egui::Color32) {
    let left = drive_usage_color(ratio, palette);
    let right = left.gamma_multiply(0.8);
    (left, right)
}

pub fn drive_usage_bar(
    ui: &mut egui::Ui,
    total: u64,
    free: u64,
    height: f32,
    palette: &crate::gui::theme::ThemePalette,
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
        1.5, // animation speed (lower = faster)
    );

    let max_bar_width = 180.0;
    let bar_width = (ui.available_width() - 8.0).min(max_bar_width);
    let (outer_rect, _) =
        ui.allocate_exact_size(egui::vec2(bar_width, height), egui::Sense::hover());
    let painter = ui.painter();

    let bar_height = outer_rect.height() * 0.65;
    let y_offset = (outer_rect.height() - bar_height) / 2.0;

    let rect = egui::Rect::from_min_size(
        egui::pos2(outer_rect.min.x, outer_rect.min.y + y_offset),
        egui::vec2(outer_rect.width(), bar_height),
    );
    // background track
    painter.rect_filled(
        rect,
        egui::CornerRadius::same(palette.small_radius),
        palette.drive_usage_background,
    );

    // fill width
    let fill_width = rect.width() * animated_ratio;

    if fill_width > 0.0 {
        let fill_rect = egui::Rect::from_min_size(rect.min, egui::vec2(fill_width, rect.height()));
        let (left, _right) = drive_usage_gradient(target_ratio, palette);

        let radius = palette.small_radius;

        // Only round right side if nearly full
        let fill_rounding = if animated_ratio >= 0.999 {
            egui::CornerRadius::same(radius)
        } else {
            egui::CornerRadius {
                nw: radius,
                sw: radius,
                ne: 0,
                se: 0,
            }
        };

        painter.rect_filled(
            fill_rect,
            fill_rounding,
            egui::Color32::from_rgb(left.r(), left.g(), left.b()),
        );

        // 🔥 subtle highlight strip (fake gradient feel)
        let highlight_rect = egui::Rect::from_min_size(
            fill_rect.min,
            egui::vec2(fill_rect.width(), fill_rect.height() * 0.25),
        );

        painter.rect_filled(
            highlight_rect,
            fill_rounding,
            egui::Color32::from_white_alpha(20),
        );
    }

    // percentage text
    let percent = format!("{:.0}%", target_ratio * 100.0);

    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        percent,
        egui::TextStyle::Small.resolve(ui.style()),
        palette.icon_color,
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
    use windows::Win32::UI::Shell::{FO_DELETE, FOF_ALLOWUNDO, SHFILEOPSTRUCTW, SHFileOperationW};
    use windows::core::PCWSTR;

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
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
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
            Some(windows::Win32::Foundation::HANDLE(std::ptr::null_mut())),
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
                    Some(windows::Win32::Foundation::HANDLE(hglobal_effect.0)),
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
        if hglobal.0 != std::ptr::null_mut() {
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

pub fn get_file_type_name(ext: &str, cache: &mut HashMap<String, String>) -> String {
    // Check cache first
    if let Some(cached) = cache.get(ext) {
        return cached.clone();
    }

    // Ensure extension starts with "."
    let ext_formatted = if ext.starts_with('.') {
        ext.to_string()
    } else {
        format!(".{}", ext)
    };

    let wide: Vec<u16> = OsStr::new(&ext_formatted)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut info = SHFILEINFOW::default();

    let _result = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_TYPENAME | SHGFI_USEFILEATTRIBUTES,
        )
    };

    // Convert UTF-16 buffer to Rust String
    let len = info.szTypeName.iter().position(|&c| c == 0).unwrap_or(0);
    let type_name = String::from_utf16_lossy(&info.szTypeName[..len]);

    // Cache the result
    cache.insert(ext.to_string(), type_name.clone());

    type_name
}

pub fn show_copy_move_dialog(
    sources: Vec<PathBuf>,
    destination: &PathBuf,
) -> windows::core::Result<()> {
    if sources.is_empty() {
        return Ok(());
    }

    unsafe {
        // ✅ FIX: HRESULT → Result
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;

        let result = (|| {
            let file_op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;

            // Minimal flags (safe default)
            file_op.SetOperationFlags(windows::Win32::UI::Shell::FILEOPERATION_FLAGS(0))?;

            // Destination
            let dest_w: Vec<u16> = destination
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect();

            let dest_item: IShellItem = SHCreateItemFromParsingName(PCWSTR(dest_w.as_ptr()), None)?;

            for src in &sources {
                let src_w: Vec<u16> = src.as_os_str().encode_wide().chain(Some(0)).collect();

                let src_item: IShellItem =
                    SHCreateItemFromParsingName(PCWSTR(src_w.as_ptr()), None)?;

                if same_drive(src, destination) {
                    // ✅ FIX: 4th argument
                    file_op.MoveItem(&src_item, Some(&dest_item), None, None)?;
                } else {
                    file_op.CopyItem(&src_item, Some(&dest_item), None, None)?;
                }
            }

            file_op.PerformOperations()?;

            Ok(())
        })();

        CoUninitialize();

        result
    }
}

fn same_drive(a: &Path, b: &Path) -> bool {
    a.components().next() == b.components().next()
}

pub fn format_size(size: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size_f = size as f64;
    let mut unit_index = 0;

    while size_f >= 1024.0 && unit_index < UNITS.len() - 1 {
        size_f /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", size, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size_f, UNITS[unit_index])
    }
}

#[derive(PartialEq, Clone, Copy)]
pub enum SortColumn {
    Name,
    Size,
    Modified,
    Created,
    Type,
}

pub fn sort_files(files: &mut Vec<FileItem>, column: SortColumn, ascending: bool) {
    files.sort_by(|a, b| {
        if a.is_dir != b.is_dir {
            return if a.is_dir {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }

        let ord = match column {
            SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortColumn::Size => a.file_size.unwrap_or(0).cmp(&b.file_size.unwrap_or(0)),
            SortColumn::Modified => a.modified_time.cmp(&b.modified_time),
            SortColumn::Created => a.created_time.cmp(&b.created_time),
            SortColumn::Type => {
                // Sort by folder/file first, then by file extension
                match (a.is_dir, b.is_dir) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    (true, true) => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                    (false, false) => {
                        // For files, sort by extension
                        let a_ext = a
                            .path
                            .extension()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_lowercase();
                        let b_ext = b
                            .path
                            .extension()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_lowercase();
                        a_ext.cmp(&b_ext)
                    }
                }
            }
        };

        if ascending { ord } else { ord.reverse() }
    });
}
