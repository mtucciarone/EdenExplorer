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
    sender: Sender<IconRequest>,
    size: u32,
}

impl IconCache {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = unbounded::<IconRequest>();

        let textures = Arc::new(Mutex::new(HashMap::new()));
        let textures_bg = textures.clone();
        let ctx_bg = ctx.clone();

        let size = 48;

        thread::spawn(move || {
            let image_list: IImageList =
                unsafe { SHGetImageList(SHIL_EXTRALARGE as i32).unwrap() };

            while let Ok(req) = rx.recv() {
                if let Some(icon_index) = get_icon_index(&req.path, req.is_dir) {
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
            sender: tx,
            size,
        }
    }

    pub fn get(
        &self,
        path: &Path,
        is_dir: bool,
    ) -> Option<egui::TextureHandle> {
        if let Some(icon_index) = get_icon_index(path, is_dir) {
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

fn get_icon_index(path: &Path, is_dir: bool) -> Option<i32> {
    let wide: Vec<u16>;

    if is_dir {
        if path.exists() {
            wide = path
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect();
        } else {
            wide = "folder".encode_utf16().chain(Some(0)).collect();
        }
    } else if path.exists() {
        wide = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
    } else {
        let ext = path.extension()?.to_str()?;
        let fake = format!("file.{}", ext);
        wide = fake.encode_utf16().chain(Some(0)).collect();
    }

    unsafe {
        let mut info = std::mem::zeroed::<SHFILEINFOW>();

        let flags = if is_dir && path.exists() {
            SHGFI_SYSICONINDEX
        } else {
            SHGFI_SYSICONINDEX | SHGFI_USEFILEATTRIBUTES
        };

        let file_attrs = if is_dir {
            FILE_ATTRIBUTE_DIRECTORY
        } else {
            FILE_ATTRIBUTE_NORMAL
        };

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
