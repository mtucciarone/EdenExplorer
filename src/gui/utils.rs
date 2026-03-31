use crate::core::fs::FileItem;
use crate::gui::theme::ThemePalette;
use eframe::egui::*;
use egui_phosphor::regular::DOTS_SIX_VERTICAL;
use lru::LruCache;
use std::cmp::Ordering::{Greater, Less};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{copy, create_dir_all, read_dir};
use std::mem::size_of;
use std::num::NonZeroUsize;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
use windows::Win32::System::DataExchange::{
    EmptyClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_HDROP;
use windows::Win32::UI::Shell::DragQueryFileW;
use windows::Win32::UI::Shell::FILEOPERATION_FLAGS;
use windows::Win32::UI::Shell::{FO_DELETE, FOF_ALLOWUNDO, SHFILEOPSTRUCTW, SHFileOperationW};
use windows::Win32::UI::Shell::{
    FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName,
};
use windows::Win32::UI::Shell::{
    SHFILEINFOW, SHGFI_TYPENAME, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW,
};
use windows::core::PCWSTR;
use windows::core::Result;

type TruncKey = (String, u32, u32); // (text, width_bucket, font_size_bucket)

lazy_static::lazy_static! {
    static ref TRUNCATION_CACHE: RwLock<LruCache<TruncKey, String>> =
        RwLock::new(LruCache::new(NonZeroUsize::new(1024).unwrap()));
}

pub fn clickable_icon(ui: &mut Ui, icon: &str, hover_color: Color32) -> Response {
    let font_id = egui::FontId::default();

    // Measure exact text size
    let galley =
        ui.painter()
            .layout_no_wrap(icon.to_string(), font_id.clone(), ui.visuals().text_color());

    let (rect, resp) = ui.allocate_exact_size(galley.size(), egui::Sense::click());

    let color = if resp.hovered() {
        hover_color
    } else {
        ui.visuals().text_color()
    };

    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        font_id,
        color,
    );

    resp
}

pub fn drive_usage_color(ratio: f32, palette: &ThemePalette) -> Color32 {
    let base = if ratio > 0.95 {
        palette.drive_usage_critical
    } else if ratio >= 0.85 {
        palette.drive_usage_warning
    } else {
        palette.drive_usage_normal
    };

    base.gamma_multiply(0.6)
}

pub fn drive_usage_bar(ui: &mut Ui, total: u64, free: u64, height: f32, palette: &ThemePalette) {
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
    let (outer_rect, _) = ui.allocate_exact_size(vec2(bar_width, height), Sense::hover());
    let painter = ui.painter();

    let bar_height = outer_rect.height() * 0.65;
    let y_offset = (outer_rect.height() - bar_height) / 2.0;

    let rect = Rect::from_min_size(
        pos2(outer_rect.min.x, outer_rect.min.y + y_offset),
        vec2(outer_rect.width(), bar_height),
    );
    // background track
    painter.rect_filled(
        rect,
        CornerRadius::same(palette.small_radius),
        palette.drive_usage_background,
    );

    // fill width
    let fill_width = rect.width() * animated_ratio;

    if fill_width > 0.0 {
        let fill_rect = Rect::from_min_size(rect.min, vec2(fill_width, rect.height()));
        let fill_color = drive_usage_color(target_ratio, palette);

        let radius = palette.small_radius;

        // Only round right side if nearly full
        let fill_rounding = if animated_ratio >= 0.999 {
            CornerRadius::same(radius)
        } else {
            CornerRadius {
                nw: radius,
                sw: radius,
                ne: 0,
                se: 0,
            }
        };

        painter.rect_filled(fill_rect, fill_rounding, fill_color);
    }

    // percentage text
    let percent = format!("{:.0}%", target_ratio * 100.0);

    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        percent,
        TextStyle::Small.resolve(ui.style()),
        palette.drive_usage_text,
    );
}

pub fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    create_dir_all(dest)?;
    for entry in read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let new_path = dest.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &new_path)?;
        } else {
            copy(entry.path(), new_path)?;
        }
    }
    Ok(())
}

pub fn shell_delete_to_recycle_bin(path: &PathBuf) -> bool {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    wide.push(0);

    let mut op = SHFILEOPSTRUCTW::default();
    op.wFunc = FO_DELETE;
    op.pFrom = PCWSTR(wide.as_ptr());
    op.fFlags = FOF_ALLOWUNDO.0 as u16;

    unsafe { SHFileOperationW(&mut op) == 0 }
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
        pt: windows::Win32::Foundation::POINT,
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
            pt: windows::Win32::Foundation::POINT { x: 0, y: 0 },
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
    let _ = unsafe {
        SetClipboardData(CF_HDROP.0 as u32, Some(HANDLE(hglobal.0)));
    };

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

                unsafe {
                    GlobalUnlock(windows::Win32::Foundation::HGLOBAL(hglobal.0 as *mut _));
                }
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

pub fn get_file_type_name<'a>(ext: &str, cache: &'a mut HashMap<String, String>) -> &'a str {
    use std::collections::hash_map::Entry;

    match cache.entry(ext.to_string()) {
        Entry::Occupied(entry) => entry.into_mut().as_str(),
        Entry::Vacant(entry) => {
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
                    size_of::<SHFILEINFOW>() as u32,
                    SHGFI_TYPENAME | SHGFI_USEFILEATTRIBUTES,
                )
            };

            // Convert UTF-16 buffer to Rust String
            let len = info.szTypeName.iter().position(|&c| c == 0).unwrap_or(0);
            let type_name = String::from_utf16_lossy(&info.szTypeName[..len]);

            entry.insert(type_name).as_str()
        }
    }
}

pub fn show_copy_move_dialog(sources: Vec<PathBuf>, destination: &PathBuf) -> Result<()> {
    if sources.is_empty() {
        return Ok(());
    }

    unsafe {
        let result = (|| {
            let file_op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;

            // Minimal flags (safe default)
            file_op.SetOperationFlags(FILEOPERATION_FLAGS(0))?;

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
            return if a.is_dir { Less } else { Greater };
        }

        let ord = match column {
            SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortColumn::Size => a.file_size.unwrap_or(0).cmp(&b.file_size.unwrap_or(0)),
            SortColumn::Modified => a.modified_time.cmp(&b.modified_time),
            SortColumn::Created => a.created_time.cmp(&b.created_time),
            SortColumn::Type => {
                // Sort by folder/file first, then by file extension
                match (a.is_dir, b.is_dir) {
                    (true, false) => Less,
                    (false, true) => Greater,
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

pub fn draw_object_drag_ghost(
    ui: &Ui,
    palette: &ThemePalette,
    label: &str,
    show_reordering_handle: bool,
) {
    if let Some(pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
        let painter = ui
            .ctx()
            .layer_painter(LayerId::new(Order::Foreground, Id::new("drag_ghost")));

        // Get Ui's width in screen coordinates
        let ui_rect = ui.min_rect();
        let ghost_width = ui_rect.width(); // full width of the UI block

        // --- Background ---
        let ghost_rect = Rect::from_center_size(pos, vec2(ghost_width, 18.0));

        painter.rect_filled(
            ghost_rect,
            CornerRadius::same(palette.medium_radius),
            palette.primary_hover,
        );

        // --- Text ---
        let font_id = FontId::new(palette.text_size, FontFamily::Proportional);

        painter.text(
            pos2(ghost_rect.left() + 8.0, ghost_rect.center().y),
            Align2::LEFT_CENTER,
            label,
            font_id,
            palette.icon_color.gamma_multiply(0.7),
        );

        ui.ctx().set_cursor_icon(CursorIcon::Grab);

        if show_reordering_handle {
            // --- Handle on the right ---
            let handle_width = 12.0;

            let handle_rect = Rect::from_min_size(
                pos2(ghost_rect.right() - handle_width - 4.0, ghost_rect.top()),
                vec2(handle_width, ghost_rect.height()),
            );

            painter.text(
                handle_rect.center(),
                Align2::CENTER_CENTER,
                DOTS_SIX_VERTICAL,
                FontId::new(14.0, FontFamily::Proportional),
                palette.icon_color,
            );
        }
    }
}

pub fn styled_button(ui: &mut Ui, label: impl Into<String>, palette: &ThemePalette) -> Response {
    let label = label.into();
    let font_id = FontId::new(palette.text_size, FontFamily::Proportional);

    // Calculate button size
    let desired_height = ui.spacing().interact_size.y;
    let desired_width = ui.available_width(); // full width
    let size = vec2(desired_width, desired_height);

    // Allocate space and get response
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    // Draw the background and stroke
    ui.painter().rect(
        rect,
        CornerRadius::same(palette.medium_radius),
        palette.button_background,
        Stroke::new(1.0, palette.tab_border_default),
        StrokeKind::Inside,
    );

    ui.centered_and_justified(|ui| {
        let text_label = Label::new(
            RichText::new(label)
                .color(palette.button_stroke)
                .font(font_id),
        );
        ui.add(text_label);
    });

    // // Draw the text, centered
    // ui.painter().text(
    //     rect.center(),
    //     Align2::CENTER_CENTER,
    //     label,
    //     font_id,
    //     palette.button_stroke,
    // );

    response
}

pub fn fuzzy_match(name: &str, query: &str) -> bool {
    let mut query_chars = query.chars().map(|c| c.to_ascii_lowercase());
    let mut current = query_chars.next();

    for c in name.chars().map(|c| c.to_ascii_lowercase()) {
        if let Some(q) = current {
            if c == q {
                current = query_chars.next();
            }
        } else {
            return true;
        }
    }

    current.is_none()
}

fn width_bucket(width: f32) -> u32 {
    (width / 8.0).round() as u32
}

/// The fast binary search truncation algorithm.
fn truncate_text_binary_search(
    ui: &mut egui::Ui,
    text: &str,
    max_width: f32,
    font_id: &egui::FontId,
    color: egui::Color32,
) -> (String, bool) {
    ui.fonts_mut(|f| {
        let full = f.layout_no_wrap(text.to_owned(), font_id.clone(), color);

        if full.size().x <= max_width {
            return (text.to_string(), false);
        }

        let ellipsis = "...";
        let ellipsis_width = f
            .layout_no_wrap(ellipsis.to_string(), font_id.clone(), color)
            .size()
            .x;
        let target_width = max_width - ellipsis_width;

        let chars: Vec<char> = text.chars().collect();
        let mut low = 0;
        let mut high = chars.len();
        let mut buffer = String::with_capacity(text.len());

        while low < high {
            let mid = (low + high) / 2;

            buffer.clear();
            for ch in &chars[..mid] {
                buffer.push(*ch);
            }

            let width = f
                .layout_no_wrap(buffer.clone(), font_id.clone(), color)
                .size()
                .x;

            if width <= target_width {
                low = mid + 1;
            } else {
                high = mid;
            }
        }

        let final_len = low.saturating_sub(1);
        buffer.clear();
        for ch in &chars[..final_len] {
            buffer.push(*ch);
        }
        buffer.push_str(ellipsis);

        (buffer, true)
    })
}

/// Truncates text to fit within `max_width`, caching results for performance.
pub fn truncate_item_text(
    ui: &mut egui::Ui,
    text: &str,
    max_width: f32,
    font_id: &egui::FontId,
    color: egui::Color32,
) -> (String, bool) {
    let width_bucket = width_bucket(max_width);
    let font_bucket = font_id.size.round() as u32;
    let key = (text.to_string(), width_bucket, font_bucket);

    // Try cache first
    if let Ok(mut cache) = TRUNCATION_CACHE.write() {
        if let Some(cached) = cache.get(&key) {
            return (cached.clone(), cached.ends_with("..."));
        }

        let (result, truncated) = truncate_text_binary_search(ui, text, max_width, font_id, color);

        cache.put(key, result.clone());

        return (result, truncated);
    }

    // fallback if lock poisoned
    truncate_text_binary_search(ui, text, max_width, font_id, color)
}
