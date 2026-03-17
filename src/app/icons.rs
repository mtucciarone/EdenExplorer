use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
};

use std::os::windows::ffi::OsStrExt;

use crossbeam_channel::{unbounded, Sender};
use eframe::egui;
use windows::{
    core::PCWSTR,
    Win32::{
        Graphics::Gdi::*,
        Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL},
        UI::{
            Shell::{
                SHGetFileInfoW, SHGetImageList, SHFILEINFOW,
                SHGFI_SYSICONINDEX, SHGFI_USEFILEATTRIBUTES,
                SHIL_EXTRALARGE,
            },
            WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO},
        },
    },
};
use windows::Win32::UI::Controls::IImageList;

struct IconRequest {
    path: PathBuf,
    is_dir: bool,
}

type IconKey = (i32, u32);

pub struct IconCache {
    textures: Arc<Mutex<HashMap<IconKey, egui::TextureHandle>>>,
    icon_indices: Arc<Mutex<HashMap<String, i32>>>,
    sender: Sender<IconRequest>,
    size: u32,
}

impl IconCache {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = unbounded::<IconRequest>();

        let textures = Arc::new(Mutex::new(HashMap::new()));
        let icon_indices = Arc::new(Mutex::new(HashMap::new()));
        let textures_bg = textures.clone();
        let icon_indices_bg = icon_indices.clone();
        let ctx_bg = ctx.clone();

        let size = 48;

        thread::spawn(move || {
            let image_list: IImageList = match unsafe { SHGetImageList(SHIL_EXTRALARGE as i32) } {
                Ok(list) => list,
                Err(_) => return,
            };

            while let Ok(req) = rx.recv() {
                if let Some(icon_index) =
                    get_icon_index_cached(&icon_indices_bg, &req.path, req.is_dir)
                {
                    let key = (icon_index, size);

                    if textures_bg.lock().unwrap().contains_key(&key) {
                        continue;
                    }

                    if let Some(icon) = get_icon_from_list(&image_list, icon_index) {
                        if let Some((pixels, w, h)) = icon_to_rgba(icon) {
                            unsafe { let _ = DestroyIcon(icon); };

                            let image = egui::ColorImage::from_rgba_unmultiplied(
                                [w as usize, h as usize],
                                &pixels,
                            );

                            let texture = ctx_bg.load_texture(
                                format!("icon_{}_{}", icon_index, size),
                                image,
                                Default::default(),
                            );

                            textures_bg.lock().unwrap().insert(key, texture);
                            ctx_bg.request_repaint();
                        }
                    }
                }
            }
        });

        Self {
            textures,
            icon_indices,
            sender: tx,
            size,
        }
    }

    pub fn get(
        &self,
        path: &Path,
        is_dir: bool,
    ) -> Option<egui::TextureHandle> {
        if let Some(icon_index) =
            get_icon_index_cached(&self.icon_indices, path, is_dir)
        {
            let key = (icon_index, self.size);

            if let Some(tex) = self.textures.lock().unwrap().get(&key) {
                return Some(tex.clone());
            }

            let _ = self.sender.send(IconRequest {
                path: path.to_path_buf(),
                is_dir,
            });
        }

        None
    }
}

// ---------------- helpers ----------------

fn get_icon_index_cached(
    cache: &Arc<Mutex<HashMap<String, i32>>>,
    path: &Path,
    is_dir: bool,
) -> Option<i32> {
    let key = icon_key(path, is_dir);

    if let Some(idx) = cache.lock().unwrap().get(&key) {
        return Some(*idx);
    }

    let idx = get_icon_index_for_key(&key, path, is_dir)?;
    cache.lock().unwrap().insert(key, idx);
    Some(idx)
}

fn icon_key(path: &Path, is_dir: bool) -> String {
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

fn get_icon_index_for_key(
    key: &str,
    path: &Path,
    is_dir: bool,
) -> Option<i32> {
    let wide: Vec<u16>;
    let mut flags = SHGFI_SYSICONINDEX;
    let file_attrs = if is_dir {
        FILE_ATTRIBUTE_DIRECTORY
    } else {
        FILE_ATTRIBUTE_NORMAL
    };

    if key.starts_with("drive:") {
        wide = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
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

        if res == 0 {
            None
        } else {
            Some(info.iIcon)
        }
    }
}

fn get_icon_from_list(list: &IImageList, index: i32) -> Option<HICON> {
    unsafe {
        list.GetIcon(index, 0).ok()
    }
}

fn icon_to_rgba(icon: HICON) -> Option<(Vec<u8>, u32, u32)> {
    unsafe {
        let mut icon_info = ICONINFO::default();

        GetIconInfo(icon, &mut icon_info).ok()?; // 🔥 FIX

        let mut bmp = BITMAP::default();
        if GetObjectW(
            icon_info.hbmColor,
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bmp as *mut _ as _),
        ) == 0 {
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

        let _ = DeleteObject(icon_info.hbmColor);
        let _ = DeleteObject(icon_info.hbmMask);

        if res == 0 {
            return None;
        }

        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2);
        }

        Some((pixels, width, height))
    }
}
