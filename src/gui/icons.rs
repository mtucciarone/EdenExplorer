use crate::core::fs::MY_PC_PATH;
use crate::core::network;
use crate::core::portable;
use crossbeam_channel::{Sender, unbounded};
use eframe::egui;
use egui_phosphor::regular;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::os::windows::ffi::OsStrExt;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    thread,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
    GetObjectW, HGDIOBJ, ReleaseDC,
};
use windows::Win32::UI::WindowsAndMessaging::HICON;
use windows::{
    Win32::{
        Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL},
        UI::{
            Controls::IImageList,
            Shell::{
                SHFILEINFOW, SHGFI_SYSICONINDEX, SHGFI_USEFILEATTRIBUTES, SHGSI_ICON,
                SHGSI_LARGEICON, SHGetFileInfoW, SHGetImageList, SHGetStockIconInfo,
                SHIL_EXTRALARGE, SHSTOCKICONINFO, SIID_DESKTOPPC, SIID_DRIVEUNKNOWN,
            },
            WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO},
        },
    },
    core::PCWSTR,
};

struct IconRequest {
    path: PathBuf,
    is_dir: bool,
    /// When set, `path` names an arbitrary image file (.ico, .png, .jpg, ...)
    /// whose own pixel content should be loaded and used directly, rather
    /// than asking the shell for an icon representing that path.
    custom_file: bool,
}

type IconKey = String;

/// Per-file icons (`uniqueicon:` keys - one texture per .exe/.dll/.lnk/...)
/// are dropped once the cache holds more than this many textures, so
/// browsing a folder like System32 can't grow GPU memory without bound.
/// Shared icons (`ext:`, `folder`, drives) are always kept; dropped per-file
/// icons are simply loaded again if they scroll back into view.
const MAX_ICON_TEXTURES: usize = 4096;

pub struct IconCache {
    textures: Arc<Mutex<HashMap<IconKey, egui::TextureHandle>>>,
    /// Keys already sent to the loader thread. `get` is called every frame for
    /// every visible row, so without this each still-loading icon was queued
    /// again every frame (thousands of duplicate requests per second), and an
    /// icon that failed to load was retried forever.
    requested: Arc<Mutex<HashSet<IconKey>>>,
    #[allow(dead_code)]
    icon_indices: Arc<Mutex<HashMap<IconKey, i32>>>,
    sender: Sender<IconRequest>,
}

impl IconCache {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = unbounded::<IconRequest>();
        let textures: Arc<Mutex<HashMap<IconKey, egui::TextureHandle>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let icon_indices = Arc::new(Mutex::new(HashMap::new()));
        let requested: Arc<Mutex<HashSet<IconKey>>> = Arc::new(Mutex::new(HashSet::new()));

        let textures_bg = textures.clone();
        let requested_bg = requested.clone();
        let icon_indices_bg = icon_indices.clone();
        let ctx_bg = ctx.clone();

        thread::spawn(move || {
            let image_list: IImageList = match unsafe { SHGetImageList(SHIL_EXTRALARGE as i32) } {
                Ok(list) => list,
                Err(_) => return,
            };

            while let Ok(req) = rx.recv() {
                {
                    let mut textures = textures_bg.lock().unwrap();
                    if textures.len() > MAX_ICON_TEXTURES {
                        textures.retain(|key, _| !key.starts_with("uniqueicon:"));
                        requested_bg
                            .lock()
                            .unwrap()
                            .retain(|key| !key.starts_with("uniqueicon:"));
                    }
                }

                if req.custom_file {
                    let key = custom_file_icon_key(&req.path);
                    if textures_bg.lock().unwrap().contains_key(&key) {
                        continue;
                    }
                    if let Some((pixels, w, h)) = load_image_file_rgba(&req.path) {
                        let image = egui::ColorImage::from_rgba_unmultiplied(
                            [w as usize, h as usize],
                            &pixels,
                        );
                        let texture =
                            ctx_bg.load_texture(format!("icon_{}", key), image, Default::default());
                        textures_bg.lock().unwrap().insert(key, texture);
                        ctx_bg.request_repaint();
                    }
                    continue;
                }

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

                if key == "stock:thispc" {
                    if textures_bg.lock().unwrap().contains_key(&key) {
                        continue;
                    }
                    if let Some((pixels, w, h)) = get_this_pc_icon_rgba() {
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
            requested,
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

        // Send request for background thread (once per key)
        if !self.requested.lock().unwrap().insert(key) {
            return None;
        }
        let _ = self.sender.send(IconRequest {
            path: path.to_path_buf(),
            is_dir,
            custom_file: false,
        });

        None
    }

    /// Like `get`, but `path` is an arbitrary image file (.ico, .png, .jpg,
    /// .bmp, ...) whose own contents should be shown directly - used for a
    /// user-browsed custom icon (a custom context menu command, a favorite),
    /// as opposed to a real filesystem entry's shell icon.
    pub fn get_custom_file_icon(&self, path: &Path) -> Option<egui::TextureHandle> {
        let key = custom_file_icon_key(path);

        if let Some(tex) = self.textures.lock().unwrap().get(&key) {
            return Some(tex.clone());
        }

        if !self.requested.lock().unwrap().insert(key) {
            return None;
        }
        let _ = self.sender.send(IconRequest {
            path: path.to_path_buf(),
            is_dir: false,
            custom_file: true,
        });

        None
    }

    /// Returns a custom Phosphor icon for folders with a recognized name.
    pub fn get_custom_folder_icon(&self, path: &Path, is_dir: bool) -> Option<&'static str> {
        if !is_dir {
            return None;
        }

        if network::unc_host_only(path).is_some() {
            return Some(regular::DESKTOP);
        }

        // A folder customized via desktop.ini should show its real shell icon
        // (queried in `get()`), not one of our generic name-based glyphs.
        if has_custom_folder_icon(path) {
            return None;
        }

        let name = path.file_name()?.to_str()?;

        custom_folder_icon(name)
    }
}

// ---------------- helpers ----------------

fn icon_key(path: &Path, is_dir: bool) -> String {
    if path.to_string_lossy() == MY_PC_PATH {
        return "stock:thispc".to_string();
    }
    if portable::is_portable_device_path(&path.to_path_buf()) {
        if let Some((device_id, _)) = portable::parse_portable_path(&path.to_path_buf()) {
            return format!("portable_device:stock:{}", device_id);
        }
        return "portable_device".to_string();
    }
    if is_dir {
        if is_drive_root(path) {
            format!("drive:{}", path.to_string_lossy().to_lowercase())
        } else if has_custom_folder_icon(path) {
            format!("customfolder:{}", path.to_string_lossy().to_lowercase())
        } else {
            "folder".to_string()
        }
    } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext_lower = ext.to_lowercase();
        if has_per_file_icon(&ext_lower) {
            // These extensions carry their own unique icon per file (an .exe's
            // icon comes from its own embedded PE resources, not a single
            // registry-wide default) - a shared `ext:exe`-style key would cache
            // one file's icon (or a lookup failure) for every other file of the
            // same extension. Keyed by the real path instead, like
            // `drive:`/`customfolder:` already are.
            format!("uniqueicon:{}", path.to_string_lossy().to_lowercase())
        } else {
            format!("ext:{}", ext_lower)
        }
    } else {
        "file".to_string()
    }
}

/// Extensions whose icon is genuinely per-file (baked into that specific
/// file - an .exe/.dll's own PE resources, an .ico's own image data, an
/// .lnk's target/custom-icon setting), as opposed to one shared icon for
/// every file of that type. These need a real per-file shell lookup, not
/// the generic extension-based lookup+cache every other file type uses.
fn has_per_file_icon(ext_lower: &str) -> bool {
    matches!(ext_lower, "exe" | "dll" | "ico" | "lnk" | "scr" | "cpl")
}

fn is_drive_root(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.len() >= 3 && s.ends_with(":\\") && path.parent().is_none()
}

lazy_static::lazy_static! {
    // Checked on the UI thread for every visible folder row each frame, so the
    // filesystem stat() is cached rather than repeated - desktop.ini presence is not
    // expected to change while a folder is being browsed.
    static ref DESKTOP_INI_CACHE: RwLock<LruCache<PathBuf, bool>> =
        RwLock::new(LruCache::new(NonZeroUsize::new(4096).unwrap()));
}

/// Cheap check for whether a folder might carry a custom icon via `desktop.ini`, the
/// same convention Windows Explorer honors. Kept separate from the generic per-folder
/// cache so the common case (no desktop.ini) still shares one cached system icon
/// instead of hitting the shell for every single folder.
fn has_custom_folder_icon(path: &Path) -> bool {
    let key = path.to_path_buf();

    if let Ok(mut cache) = DESKTOP_INI_CACHE.write() {
        if let Some(&cached) = cache.get(&key) {
            return cached;
        }
    }

    let result = path.join("desktop.ini").is_file();

    if let Ok(mut cache) = DESKTOP_INI_CACHE.write() {
        cache.put(key, result);
    }

    result
}

fn get_icon_index_for_key(key: &str, path: &Path, is_dir: bool) -> Option<i32> {
    let wide: Vec<u16>;
    let mut flags = SHGFI_SYSICONINDEX;
    let file_attrs = if is_dir {
        FILE_ATTRIBUTE_DIRECTORY
    } else {
        FILE_ATTRIBUTE_NORMAL
    };

    if key.starts_with("drive:")
        || key.starts_with("customfolder:")
        || key.starts_with("uniqueicon:")
    {
        // Real path lookup (no SHGFI_USEFILEATTRIBUTES) so the shell resolves the
        // folder's actual icon (honoring a desktop.ini customization if present),
        // or - for `uniqueicon:` - the specific file's own embedded icon instead
        // of a generic one shared by every file of that extension.
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

/// The real "This PC" icon (the same one Explorer shows for the Computer
/// node), as opposed to a specific drive's icon.
fn get_this_pc_icon_rgba() -> Option<(Vec<u8>, u32, u32)> {
    unsafe {
        let mut info = SHSTOCKICONINFO::default();
        info.cbSize = std::mem::size_of::<SHSTOCKICONINFO>() as u32;
        SHGetStockIconInfo(SIID_DESKTOPPC, SHGSI_ICON | SHGSI_LARGEICON, &mut info).ok()?;

        if info.hIcon.0.is_null() {
            return None;
        }

        let rgba = icon_to_rgba(info.hIcon);
        let _ = DestroyIcon(info.hIcon);
        rgba
    }
}

/// Loads an .ico file's own icon content (as opposed to a generic shell icon
/// for a file of that type) at a decent preview resolution, using Windows'
/// own icon loader - which tolerates the sort of internal inconsistencies
/// (declared vs. embedded-BMP size mismatches, etc.) that trip up stricter
/// pure-Rust ICO decoders. Returns `None` if Windows itself can't load it.
pub(crate) fn load_ico_file_rgba(path: &Path) -> Option<(Vec<u8>, u32, u32)> {
    use windows::Win32::UI::WindowsAndMessaging::{IMAGE_ICON, LR_LOADFROMFILE, LoadImageW};

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let handle = unsafe {
        LoadImageW(
            None,
            PCWSTR(wide.as_ptr()),
            IMAGE_ICON,
            256,
            256,
            LR_LOADFROMFILE,
        )
    }
    .ok()?;

    let icon = HICON(handle.0);
    let result = icon_to_rgba(icon);
    let _ = unsafe { DestroyIcon(icon) };
    result
}

/// The cache key for a user-browsed custom icon FILE (as opposed to a real
/// filesystem path's shell icon) - namespaced so it can never collide with a
/// real path that happens to share the same text.
fn custom_file_icon_key(path: &Path) -> String {
    format!("customfile:{}", path.to_string_lossy().to_lowercase())
}

/// Loads an arbitrary image file's own pixel content for use as a custom
/// icon (a custom context menu command, a favorite's icon, ...) - unlike
/// `get`/`icon_key`, this never asks the shell "what icon represents this
/// path"; it decodes the file itself. `.ico` goes through Windows' own
/// loader first (handles multi-resolution icons correctly), falling back to
/// the general-purpose decoder - which also handles `.png`, `.jpg`, `.bmp`,
/// and other common formats - for everything else.
pub(crate) fn load_image_file_rgba(path: &Path) -> Option<(Vec<u8>, u32, u32)> {
    let is_ico = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ico"));

    if is_ico {
        if let Some(rgba) = load_ico_file_rgba(path) {
            return Some(rgba);
        }
    }

    let img = image::open(path).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Some((rgba.into_raw(), w, h))
}

pub(crate) fn icon_to_rgba(icon: HICON) -> Option<(Vec<u8>, u32, u32)> {
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

fn custom_folder_icon(name: &str) -> Option<&'static str> {
    if name.eq_ignore_ascii_case("Music") {
        Some(regular::MUSIC_NOTES)
    } else if name.eq_ignore_ascii_case("Pictures") {
        Some(regular::IMAGE)
    } else if name.eq_ignore_ascii_case("Videos") {
        Some(regular::VIDEO_CAMERA)
    } else if name.eq_ignore_ascii_case("Downloads") {
        Some(regular::DOWNLOAD_SIMPLE)
    } else if name.eq_ignore_ascii_case("Documents") {
        Some(regular::FILE_TEXT)
    } else if name.eq_ignore_ascii_case("Network") {
        Some(regular::NETWORK)
    } else if name.eq_ignore_ascii_case("Settings") {
        Some(regular::GEAR)
    } else if name.eq_ignore_ascii_case("Administrative Tools") {
        Some(regular::WRENCH)
    } else {
        None
    }
}
