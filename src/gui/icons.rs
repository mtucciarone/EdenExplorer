use crate::core::portable;
use crossbeam_channel::{Sender, unbounded};
use eframe::egui;
use std::os::windows::ffi::OsStrExt;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::UI::WindowsAndMessaging::HICON;
use windows::{
    Win32::{
        Graphics::Gdi::*,
        Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL},
        UI::{
            Controls::IImageList,
            Shell::{
                SHFILEINFOW, SHGFI_SYSICONINDEX, SHGFI_USEFILEATTRIBUTES, SHGSI_ICON,
                SHGSI_LARGEICON, SHGetFileInfoW, SHGetImageList, SHGetStockIconInfo,
                SHIL_EXTRALARGE, SHSTOCKICONINFO, SIID_DRIVEUNKNOWN,
            },
            WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO},
        },
    },
    core::PCWSTR,
};

struct IconRequest {
    path: PathBuf,
    is_dir: bool,
}

type IconKey = String;

pub struct IconCache {
    textures: Arc<Mutex<HashMap<IconKey, egui::TextureHandle>>>,
    #[allow(dead_code)]
    icon_indices: Arc<Mutex<HashMap<IconKey, i32>>>,
    sender: Sender<IconRequest>,
}

impl IconCache {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = unbounded::<IconRequest>();
        let textures = Arc::new(Mutex::new(HashMap::new()));
        let icon_indices = Arc::new(Mutex::new(HashMap::new()));

        let textures_bg = textures.clone();
        let icon_indices_bg = icon_indices.clone();
        let ctx_bg = ctx.clone();

        thread::spawn(move || {
            let image_list: IImageList = match unsafe { SHGetImageList(SHIL_EXTRALARGE as i32) } {
                Ok(list) => list,
                Err(_) => return,
            };

            while let Ok(req) = rx.recv() {
                let key = icon_key(&req.path, req.is_dir);

                if key.starts_with("portable_device") {
                    if textures_bg.lock().unwrap().contains_key(&key) {
                        continue;
                    }
                    if let Some((pixels, w, h)) = get_portable_device_icon_rgba() {
                        let image = egui::ColorImage::from_rgba_unmultiplied(
                            [w as usize, h as usize],
                            &pixels,
                        );
                        let texture =
                            ctx_bg.load_texture(format!("icon_{}", key), image, Default::default());
                        textures_bg.lock().unwrap().insert(key.clone(), texture);
                        ctx_bg.request_repaint();
                        continue;
                    }
                }

                // 1️⃣ Get icon index (cached)
                let icon_index = {
                    let mut idx_cache = icon_indices_bg.lock().unwrap();
                    if let Some(&idx) = idx_cache.get(&key) {
                        idx
                    } else {
                        let idx = get_icon_index_for_key(&key, &req.path, req.is_dir).unwrap_or(0);
                        idx_cache.insert(key.clone(), idx);
                        idx
                    }
                };

                // 2️⃣ Skip if texture already exists
                if textures_bg.lock().unwrap().contains_key(&key) {
                    continue;
                }

                // 3️⃣ Fetch icon from system image list
                if let Some(icon) = get_icon_from_list(&image_list, icon_index) {
                    if let Some((pixels, w, h)) = icon_to_rgba(icon) {
                        let _ = unsafe { DestroyIcon(icon) };

                        let image = egui::ColorImage::from_rgba_unmultiplied(
                            [w as usize, h as usize],
                            &pixels,
                        );
                        let texture =
                            ctx_bg.load_texture(format!("icon_{}", key), image, Default::default());

                        textures_bg.lock().unwrap().insert(key.clone(), texture);
                        ctx_bg.request_repaint();
                    }
                }
            }
        });

        Self {
            textures,
            icon_indices,
            sender: tx,
        }
    }

    pub fn get(&self, path: &Path, is_dir: bool) -> Option<egui::TextureHandle> {
        let key = icon_key(path, is_dir);

        // Return cached if ready
        if let Some(tex) = self.textures.lock().unwrap().get(&key) {
            return Some(tex.clone());
        }

        // Send request for background thread
        let _ = self.sender.send(IconRequest {
            path: path.to_path_buf(),
            is_dir,
        });

        None
    }
}

// ---------------- helpers ----------------

fn icon_key(path: &Path, is_dir: bool) -> String {
    if portable::is_portable_device_path(&path.to_path_buf()) {
        if let Some((device_id, _)) = portable::parse_portable_path(&path.to_path_buf()) {
            return format!("portable_device:stock:{}", device_id);
        }
        return "portable_device".to_string();
    }
    if is_dir {
        if is_drive_root(path) {
            format!("drive:{}", path.to_string_lossy().to_lowercase())
        } else {
            "folder".to_string()
        }
    } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        format!("ext:{}", ext.to_lowercase())
    } else {
        "file".to_string()
    }
}

fn is_drive_root(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.len() >= 3 && s.ends_with(":\\") && path.parent().is_none()
}

fn get_icon_index_for_key(key: &str, path: &Path, is_dir: bool) -> Option<i32> {
    let wide: Vec<u16>;
    let mut flags = SHGFI_SYSICONINDEX;
    let mut file_attrs = if is_dir {
        FILE_ATTRIBUTE_DIRECTORY
    } else {
        FILE_ATTRIBUTE_NORMAL
    };

    if key.starts_with("drive:") {
        wide = path.as_os_str().encode_wide().chain(Some(0)).collect();
    } else if key.starts_with("portable_device") {
        let fake = PathBuf::from("C:\\");
        wide = fake.as_os_str().encode_wide().chain(Some(0)).collect();
    } else if is_dir {
        wide = "folder".encode_utf16().chain(Some(0)).collect();
        flags |= SHGFI_USEFILEATTRIBUTES;
    } else {
        let ext = key.strip_prefix("ext:").unwrap_or("");
        let fake = if ext.is_empty() {
            "file".to_string()
        } else {
            format!("file.{}", ext)
        };
        wide = fake.encode_utf16().chain(Some(0)).collect();
        flags |= SHGFI_USEFILEATTRIBUTES;
    }

    unsafe {
        let mut info = std::mem::zeroed::<SHFILEINFOW>();
        let res = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            file_attrs,
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        );
        if res == 0 { None } else { Some(info.iIcon) }
    }
}

fn get_icon_from_list(list: &IImageList, index: i32) -> Option<HICON> {
    unsafe { list.GetIcon(index, 0).ok() }
}

fn get_portable_device_icon_rgba() -> Option<(Vec<u8>, u32, u32)> {
    unsafe {
        let mut info = SHSTOCKICONINFO::default();
        info.cbSize = std::mem::size_of::<SHSTOCKICONINFO>() as u32;
        SHGetStockIconInfo(SIID_DRIVEUNKNOWN, SHGSI_ICON | SHGSI_LARGEICON, &mut info).ok()?;

        if info.hIcon.0.is_null() {
            return None;
        }

        let rgba = icon_to_rgba(info.hIcon);
        let _ = DestroyIcon(info.hIcon);
        rgba
    }
}

fn icon_to_rgba(icon: HICON) -> Option<(Vec<u8>, u32, u32)> {
    unsafe {
        let mut icon_info = ICONINFO::default();
        GetIconInfo(icon, &mut icon_info).ok()?;

        let mut bmp = BITMAP::default();
        if GetObjectW(
            icon_info.hbmColor.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bmp as *mut _ as _),
        ) == 0
        {
            return None;
        }

        let width = bmp.bmWidth as u32;
        let height = bmp.bmHeight as u32;

        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let hdc = GetDC(None);

        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = width as i32;
        bmi.bmiHeader.biHeight = -(height as i32);
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let res = GetDIBits(
            hdc,
            icon_info.hbmColor,
            0,
            height,
            Some(pixels.as_mut_ptr() as _),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, hdc);

        let _ = DeleteObject(HGDIOBJ(icon_info.hbmColor.0));
        let _ = DeleteObject(HGDIOBJ(icon_info.hbmMask.0));

        if res == 0 {
            return None;
        }

        // Convert BGRA -> RGBA
        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2);
        }

        Some((pixels, width, height))
    }
}

use windows::Win32::UI::Shell::SHGFI_FLAGS;
use windows::Win32::{
    Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC,
        GetDIBits, GetObjectW, HGDIOBJ, ReleaseDC,
    },
    UI::Shell::SHIL_JUMBO,
};

/// Fetch the sharpest available HICON for a file or folder path
/// Returns (pixels, width, height) as RGBA
pub fn get_icon_sharpest(path: &std::path::Path, is_dir: bool) -> Option<(Vec<u8>, u32, u32)> {
    unsafe {
        // 1️⃣ Convert path to wide
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let attrs = if is_dir { 0x10 } else { 0x80 }; // FILE_ATTRIBUTE_DIRECTORY/NORMAL
        let mut shfi = SHFILEINFOW::default();

        // 2️⃣ Get system icon index
        let attrs = if is_dir {
            FILE_ATTRIBUTE_DIRECTORY
        } else {
            FILE_ATTRIBUTE_NORMAL
        };
        let res = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(attrs.0),
            Some(&mut shfi as *mut _),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_SYSICONINDEX
                | if !is_dir {
                    SHGFI_USEFILEATTRIBUTES
                } else {
                    SHGFI_FLAGS(0)
                },
        );
        if res == 0 {
            return None;
        }
        let icon_index = shfi.iIcon;

        // 3️⃣ Try SHIL_JUMBO (256x256+), fallback to SHIL_EXTRALARGE
        let mut image_list: Option<IImageList> = None;
        for size in [SHIL_JUMBO, SHIL_EXTRALARGE] {
            if let Ok(list) = SHGetImageList(size as i32) {
                image_list = Some(list);
                break;
            }
        }
        let image_list = image_list?;

        // 4️⃣ Fetch HICON from image list
        let hicon: HICON = image_list.GetIcon(icon_index, 0).ok()?;
        if hicon.0.is_null() {
            return None;
        }

        // 5️⃣ Convert HICON -> RGBA bitmap
        let mut icon_info = ICONINFO::default();
        if GetIconInfo(hicon, &mut icon_info).is_err() {
            let _ = DestroyIcon(hicon).is_ok();
            return None;
        }

        let mut bmp = BITMAP::default();
        if GetObjectW(
            HGDIOBJ(icon_info.hbmColor.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bmp as *mut _ as _),
        ) == 0
        {
            let _ = DestroyIcon(hicon).is_ok();
            return None;
        }

        let width = bmp.bmWidth as u32;
        let height = bmp.bmHeight as u32;
        let mut pixels = vec![0u8; (width * height * 4) as usize];

        let hdc = GetDC(None);
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = width as i32;
        bmi.bmiHeader.biHeight = -(height as i32);
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        if GetDIBits(
            hdc,
            icon_info.hbmColor,
            0,
            height,
            Some(pixels.as_mut_ptr() as _),
            &mut bmi,
            DIB_RGB_COLORS,
        ) == 0
        {
            ReleaseDC(None, hdc);
            let _ = DestroyIcon(hicon).is_ok();
            return None;
        }

        ReleaseDC(None, hdc);

        // 6️⃣ Cleanup
        let _ = DeleteObject(HGDIOBJ(icon_info.hbmColor.0));
        let _ = DeleteObject(HGDIOBJ(icon_info.hbmMask.0));
        let _ = DestroyIcon(hicon).is_ok();

        // 7️⃣ Convert BGRA -> RGBA
        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2);
        }

        Some((pixels, width, height))
    }
}
