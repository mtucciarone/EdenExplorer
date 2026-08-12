use crate::core::fs::FileItem;
use crossbeam_channel::{Receiver, Sender, bounded};
use eframe::egui;
use lru::LruCache;
use rayon::{ThreadPool, ThreadPoolBuilder};
use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, GetDIBits, HBITMAP,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Shell::{
    ISharedBitmap, IShellItem, IThumbnailCache, LocalThumbnailCache, SHCreateItemFromParsingName,
    WTS_CACHEFLAGS, WTS_EXTRACT, WTS_FASTEXTRACT, WTS_FLAGS, WTS_INCACHEONLY, WTS_THUMBNAILID,
};
use windows::core::{Error, HRESULT, HSTRING, Result};

pub const THUMBNAIL_SOURCE_SIZE: u32 = 256;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ThumbnailCacheKey {
    path: PathBuf,
    file_size: Option<u64>,
    modified_time_raw: Option<i64>,
    created_time_raw: Option<i64>,
    is_dir: bool,
}

impl ThumbnailCacheKey {
    pub fn from_file(file: &FileItem) -> Self {
        Self {
            path: file.path.clone(),
            file_size: file.file_size,
            modified_time_raw: file.modified_time_raw,
            created_time_raw: file.created_time_raw,
            is_dir: file.is_dir,
        }
    }
}

#[derive(Clone)]
pub struct ThumbnailImage {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

struct ThumbnailEntry {
    image: ThumbnailImage,
    texture: Option<egui::TextureHandle>,
}

struct ThumbnailResult {
    key: ThumbnailCacheKey,
    image: Option<ThumbnailImage>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailPriority {
    Visible,
    Nearby,
}

pub struct ThumbnailService {
    cache: LruCache<ThumbnailCacheKey, ThumbnailEntry>,
    pending: HashSet<ThumbnailCacheKey>,
    failed: HashSet<ThumbnailCacheKey>,
    tx: Sender<ThumbnailResult>,
    rx: Receiver<ThumbnailResult>,
    pool: Arc<ThreadPool>,
}

impl Default for ThumbnailService {
    fn default() -> Self {
        Self::new(512)
    }
}

impl ThumbnailService {
    pub fn new(capacity: usize) -> Self {
        let (tx, rx) = bounded(256);
        let threads = num_cpus::get().clamp(2, 4);
        let pool = ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|idx| format!("thumbnail-worker-{idx}"))
            .build()
            .unwrap_or_else(|_| ThreadPoolBuilder::new().num_threads(2).build().unwrap());

        Self {
            cache: LruCache::new(NonZeroUsize::new(capacity.max(1)).unwrap()),
            pending: HashSet::new(),
            failed: HashSet::new(),
            tx,
            rx,
            pool: Arc::new(pool),
        }
    }

    pub fn pump_completed(&mut self, ctx: &egui::Context) {
        while let Ok(result) = self.rx.try_recv() {
            self.pending.remove(&result.key);

            if let Some(image) = result.image {
                self.failed.remove(&result.key);
                self.cache.put(
                    result.key,
                    ThumbnailEntry {
                        image,
                        texture: None,
                    },
                );
                ctx.request_repaint();
            } else {
                self.failed.insert(result.key);
            }
        }
    }

    pub fn request(&mut self, file: &FileItem, priority: ThumbnailPriority) {
        let key = ThumbnailCacheKey::from_file(file);

        if self.cache.contains(&key) || self.pending.contains(&key) || self.failed.contains(&key) {
            return;
        }

        self.pending.insert(key.clone());

        let job = ThumbnailJob {
            key,
            path: file.path.clone(),
        };
        let tx = self.tx.clone();

        match priority {
            ThumbnailPriority::Visible => self.pool.spawn_fifo(move || run_thumbnail_job(job, tx)),
            ThumbnailPriority::Nearby => self.pool.spawn(move || run_thumbnail_job(job, tx)),
        }
    }

    pub fn texture_for(
        &mut self,
        ctx: &egui::Context,
        file: &FileItem,
    ) -> Option<egui::TextureHandle> {
        let key = ThumbnailCacheKey::from_file(file);
        let entry = self.cache.get_mut(&key)?;

        if entry.texture.is_none() {
            let color_image = egui::ColorImage::from_rgba_unmultiplied(
                entry.image.size,
                entry.image.rgba.as_slice(),
            );
            entry.texture = Some(ctx.load_texture(
                format!("thumbnail:{}", file.path.display()),
                color_image,
                egui::TextureOptions::LINEAR,
            ));
        }

        entry.texture.clone()
    }
}

struct ThumbnailJob {
    key: ThumbnailCacheKey,
    path: PathBuf,
}

struct ComGuard;

impl ComGuard {
    fn init() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        }
        Ok(Self)
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

fn run_thumbnail_job(job: ThumbnailJob, tx: Sender<ThumbnailResult>) {
    let image = extract_thumbnail(&job.path).or_else(|_| decode_image_fallback(&job.path));
    let _ = tx.send(ThumbnailResult {
        key: job.key,
        image: image.ok(),
    });
}

fn extract_thumbnail(path: &Path) -> Result<ThumbnailImage> {
    let _com = ComGuard::init()?;
    let shell_item: IShellItem = unsafe {
        SHCreateItemFromParsingName(&HSTRING::from(path.to_string_lossy().as_ref()), None)?
    };
    let cache: IThumbnailCache =
        unsafe { CoCreateInstance(&LocalThumbnailCache, None, CLSCTX_INPROC_SERVER)? };

    for flags in [WTS_INCACHEONLY, WTS_FASTEXTRACT, WTS_EXTRACT] {
        if let Ok(image) = get_thumbnail_with_flags(&cache, &shell_item, flags) {
            return Ok(image);
        }
    }

    Err(thumbnail_error("thumbnail unavailable"))
}

fn get_thumbnail_with_flags(
    cache: &IThumbnailCache,
    shell_item: &IShellItem,
    flags: WTS_FLAGS,
) -> Result<ThumbnailImage> {
    let mut bitmap: Option<ISharedBitmap> = None;
    let mut out_flags = WTS_CACHEFLAGS::default();
    let mut thumbnail_id = WTS_THUMBNAILID::default();

    unsafe {
        cache.GetThumbnail(
            shell_item,
            THUMBNAIL_SOURCE_SIZE,
            flags,
            Some(&mut bitmap),
            Some(&mut out_flags),
            Some(&mut thumbnail_id),
        )?;
    }

    let Some(bitmap) = bitmap else {
        return Err(thumbnail_error("thumbnail bitmap missing"));
    };

    shared_bitmap_to_rgba(&bitmap)
}

fn shared_bitmap_to_rgba(bitmap: &ISharedBitmap) -> Result<ThumbnailImage> {
    let size = unsafe { bitmap.GetSize()? };
    let hbitmap = unsafe { bitmap.GetSharedBitmap()? };
    hbitmap_to_rgba(hbitmap, size)
}

fn hbitmap_to_rgba(hbitmap: HBITMAP, size: SIZE) -> Result<ThumbnailImage> {
    let width = size.cx.max(0) as usize;
    let height = size.cy.max(0) as usize;
    if width == 0 || height == 0 || hbitmap.is_invalid() {
        return Err(thumbnail_error("thumbnail bitmap has invalid dimensions"));
    }

    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.is_invalid() {
        return Err(thumbnail_error("failed to create thumbnail device context"));
    }

    let mut info = BITMAPINFO::default();
    info.bmiHeader.biSize =
        std::mem::size_of::<windows::Win32::Graphics::Gdi::BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = width as i32;
    info.bmiHeader.biHeight = -(height as i32);
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB.0;

    let mut bgra = vec![0u8; width * height * 4];
    let lines = unsafe {
        GetDIBits(
            hdc,
            hbitmap,
            0,
            height as u32,
            Some(bgra.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        )
    };

    unsafe {
        let _ = DeleteDC(hdc);
    }

    if lines == 0 {
        return Err(thumbnail_error("failed to read thumbnail bitmap pixels"));
    }

    let mut rgba = Vec::with_capacity(bgra.len());
    let mut has_alpha = false;
    for px in bgra.chunks_exact(4) {
        has_alpha |= px[3] != 0;
        rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
    }

    if !has_alpha {
        for px in rgba.chunks_exact_mut(4) {
            px[3] = 255;
        }
    }

    Ok(ThumbnailImage {
        size: [width, height],
        rgba,
    })
}

fn decode_image_fallback(path: &Path) -> Result<ThumbnailImage> {
    let image = image::open(path).map_err(|_| thumbnail_error("fallback image decode failed"))?;
    let thumb = image.thumbnail(THUMBNAIL_SOURCE_SIZE, THUMBNAIL_SOURCE_SIZE);
    let rgba = thumb.to_rgba8();
    let (width, height) = rgba.dimensions();

    Ok(ThumbnailImage {
        size: [width as usize, height as usize],
        rgba: rgba.into_raw(),
    })
}

fn thumbnail_error(message: &str) -> Error {
    Error::new(HRESULT(0x80004005u32 as i32), message)
}
