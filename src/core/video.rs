//! Video playback for the preview pane, via the Windows Media Foundation
//! "Media Engine" API (`IMFMediaEngine`) - the same decoding stack Windows
//! Media Player/the Photos app use. This is deliberately different from the
//! rest of `core/preview.rs`'s previews: those are all "decode once on a
//! background thread, hand back an immutable payload" (an image, some text).
//! Video needs a genuinely *live*, continuously-updated resource instead - a
//! COM object that's kept alive for as long as the file is being previewed,
//! polled once per UI frame for whether a new decoded frame is ready.
//!
//! Frames are transferred into a plain system-memory buffer (not a Direct3D
//! surface - egui/wgpu owns the GPU device here, not Media Foundation), then
//! uploaded to an egui texture, the same way animated GIF playback works.

use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGR, IWICBitmap, IWICImagingFactory,
    WICBitmapCacheOnDemand, WICBitmapLockRead, WICRect,
};
use windows::Win32::Media::MediaFoundation::{
    CLSID_MFMediaEngineClassFactory, IMFAttributes, IMFMediaEngine, IMFMediaEngineClassFactory,
    IMFMediaEngineNotify, IMFMediaEngineNotify_Impl, MF_MEDIA_ENGINE_CALLBACK,
    MF_MEDIA_ENGINE_EVENT_CANPLAY, MF_MEDIA_ENGINE_EVENT_ENDED, MF_MEDIA_ENGINE_EVENT_ERROR,
    MF_MEDIA_ENGINE_EVENT_LOADEDMETADATA, MF_VERSION, MFCreateAttributes, MFSTARTUP_FULL,
    MFStartup,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::core::{BSTR, Result};

const AUDIO_ONLY_MESSAGE: &str =
    "This file has no video track (audio-only) - nothing to show here, but it's still playing.";

fn ensure_media_foundation_started() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| unsafe {
        let _ = MFStartup(MF_VERSION, MFSTARTUP_FULL);
    });
}

/// Receives `IMFMediaEngine` lifecycle events (fired from a Media Foundation
/// work-queue thread, not ours) and forwards the raw event code through a
/// channel so `VideoPlayer::pump` can react to it on the UI thread.
#[windows_core::implement(IMFMediaEngineNotify)]
struct EngineNotify {
    tx: Sender<i32>,
}

impl IMFMediaEngineNotify_Impl for EngineNotify_Impl {
    fn EventNotify(&self, event: u32, _param1: usize, _param2: u32) -> Result<()> {
        let _ = self.tx.send(event as i32);
        Ok(())
    }
}

fn path_to_file_url(path: &Path) -> String {
    let mut url = String::from("file:///");
    for ch in path.to_string_lossy().replace('\\', "/").chars() {
        match ch {
            ' ' => url.push_str("%20"),
            '#' => url.push_str("%23"),
            '?' => url.push_str("%3F"),
            other => url.push(other),
        }
    }
    url
}

/// One playing (or loading) video, backed by a live `IMFMediaEngine`.
pub struct VideoPlayer {
    engine: IMFMediaEngine,
    events: Receiver<i32>,
    /// Set once the engine has fired `LOADEDMETADATA`/`CANPLAY` - gates
    /// `update_frame` (no point pulling frames before there's anything
    /// decoded yet). Playback itself is never started automatically; the
    /// user has to press play (see `toggle_play_pause`) - previewing a file
    /// shouldn't start making noise/motion on its own.
    metadata_ready: bool,
    pub error: Option<String>,
    /// `TransferVideoFrame` only accepts a Direct3D surface or a WIC bitmap
    /// as its destination - a plain `IMFMediaBuffer` (e.g. from
    /// `MFCreate2DMediaBuffer`) isn't recognized and fails with
    /// E_NOINTERFACE. Since egui/wgpu owns the GPU device here (no D3D
    /// device manager is configured), a WIC bitmap is the only option.
    wic_factory: IWICImagingFactory,
    /// Cached so a fresh bitmap isn't allocated every single frame; only
    /// recreated when the video's native size changes (i.e. essentially
    /// never, after the first frame).
    wic_bitmap: Option<(IWICBitmap, u32, u32)>,
}

impl VideoPlayer {
    pub fn open(path: &Path) -> Result<Self> {
        ensure_media_foundation_started();

        let (tx, rx) = unbounded();
        let notify_impl: EngineNotify = EngineNotify { tx };
        let notify: IMFMediaEngineNotify = notify_impl.into();

        unsafe {
            let mut attributes: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attributes, 1)?;
            let attributes = attributes.ok_or_else(|| windows_core::Error::empty())?;
            attributes.SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &notify)?;

            let factory: IMFMediaEngineClassFactory =
                CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_INPROC_SERVER)?;
            let engine = factory.CreateInstance(0, &attributes)?;
            // `IMFMediaEngine` defaults `AutoPlay` to `TRUE` at the COM
            // level - it starts playing on its own once the source is ready
            // even though nothing here ever calls `Play()`, so previewing a
            // file must not start motion/audio on its own. `Loop` defaults
            // to `FALSE` already, but set it explicitly too so this doesn't
            // depend on that default staying the same.
            engine.SetAutoPlay(false)?;
            engine.SetLoop(false)?;

            let url = BSTR::from(path_to_file_url(path));
            engine.SetSource(&url)?;

            let wic_factory: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;

            Ok(Self {
                engine,
                events: rx,
                metadata_ready: false,
                error: None,
                wic_factory,
                wic_bitmap: None,
            })
        }
    }

    /// Drains lifecycle events. Call once per UI frame.
    fn pump_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            if event == MF_MEDIA_ENGINE_EVENT_ERROR.0 {
                self.error = Some("Media Foundation couldn't decode this video (missing codec, unsupported container, or corrupt file).".to_string());
            } else if event == MF_MEDIA_ENGINE_EVENT_LOADEDMETADATA.0
                || event == MF_MEDIA_ENGINE_EVENT_CANPLAY.0
            {
                self.metadata_ready = true;
            } else if event == MF_MEDIA_ENGINE_EVENT_ENDED.0 {
                // Stop at the end rather than looping - pausing here (instead
                // of leaving whatever state `Play()` left it in) makes sure
                // the transport row's play/pause icon correctly flips back
                // to "play" once playback actually reaches the end.
                unsafe {
                    let _ = self.engine.Pause();
                }
            }
        }
    }

    /// Call once per UI frame. Returns a freshly decoded RGBA frame (and its
    /// size) if a new one is ready since the last call.
    pub fn update_frame(&mut self) -> Option<(Vec<u8>, [usize; 2])> {
        self.pump_events();

        if self.error.is_some() || !self.metadata_ready {
            return None;
        }

        unsafe {
            if !self.engine.HasVideo().as_bool() {
                self.error = Some(AUDIO_ONLY_MESSAGE.to_string());
                return None;
            }

            // S_OK means a new frame is ready; S_FALSE ("no new frame yet")
            // is also `Ok` here since HRESULT::ok() only rejects negative
            // (failure) codes, so this alone doesn't tell us much - real
            // failures below are what actually matter.
            if self.engine.OnVideoStreamTick().is_err() {
                return None;
            }

            let mut width = 0u32;
            let mut height = 0u32;
            if let Err(err) = self
                .engine
                .GetNativeVideoSize(Some(&mut width), Some(&mut height))
            {
                self.error = Some(format!("Couldn't get video size: {err}"));
                return None;
            }
            if width == 0 || height == 0 {
                return None;
            }

            let bitmap = match &self.wic_bitmap {
                Some((bitmap, w, h)) if *w == width && *h == height => bitmap.clone(),
                _ => {
                    let bitmap = match self.wic_factory.CreateBitmap(
                        width,
                        height,
                        &GUID_WICPixelFormat32bppBGR,
                        WICBitmapCacheOnDemand,
                    ) {
                        Ok(b) => b,
                        Err(err) => {
                            self.error =
                                Some(format!("Couldn't allocate video frame buffer: {err}"));
                            return None;
                        }
                    };
                    self.wic_bitmap = Some((bitmap.clone(), width, height));
                    bitmap
                }
            };

            let rect = RECT {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            };
            if let Err(err) = self.engine.TransferVideoFrame(&bitmap, None, &rect, None) {
                self.error = Some(format!("Couldn't decode video frame: {err}"));
                return None;
            }

            let wic_rect = WICRect {
                X: 0,
                Y: 0,
                Width: width as i32,
                Height: height as i32,
            };
            let lock = match bitmap.Lock(&wic_rect, WICBitmapLockRead.0 as u32) {
                Ok(l) => l,
                Err(err) => {
                    self.error = Some(format!("Couldn't lock video frame buffer: {err}"));
                    return None;
                }
            };
            let stride = match lock.GetStride() {
                Ok(s) => s as usize,
                Err(err) => {
                    self.error = Some(format!("Couldn't read video frame stride: {err}"));
                    return None;
                }
            };
            let mut data_len: u32 = 0;
            let mut data_ptr: *mut u8 = std::ptr::null_mut();
            if let Err(err) = lock.GetDataPointer(&mut data_len, &mut data_ptr) {
                self.error = Some(format!("Couldn't read video frame data: {err}"));
                return None;
            }

            let mut rgba = vec![0u8; (width * height * 4) as usize];
            let row_bytes = width as usize * 4;
            for y in 0..height as usize {
                let src_row = std::slice::from_raw_parts(data_ptr.add(y * stride), row_bytes);
                let dst_row = &mut rgba[y * row_bytes..(y + 1) * row_bytes];
                for x in 0..width as usize {
                    // Source is BGRX (GUID_WICPixelFormat32bppBGR, the pad
                    // byte unused); egui wants RGBA.
                    dst_row[x * 4] = src_row[x * 4 + 2];
                    dst_row[x * 4 + 1] = src_row[x * 4 + 1];
                    dst_row[x * 4 + 2] = src_row[x * 4];
                    dst_row[x * 4 + 3] = 255;
                }
            }

            drop(lock);

            Some((rgba, [width as usize, height as usize]))
        }
    }

    pub fn toggle_play_pause(&mut self) {
        unsafe {
            if self.engine.IsPaused().as_bool() {
                let _ = self.engine.Play();
            } else {
                let _ = self.engine.Pause();
            }
        }
    }

    pub fn is_paused(&self) -> bool {
        unsafe { self.engine.IsPaused().as_bool() }
    }

    /// Current playback position and total duration, in seconds. `None` if
    /// the duration isn't known yet (still loading) or is infinite (a live
    /// stream).
    pub fn progress(&self) -> Option<(f64, f64)> {
        unsafe {
            let duration = self.engine.GetDuration();
            if !duration.is_finite() || duration <= 0.0 {
                return None;
            }
            Some((self.engine.GetCurrentTime(), duration))
        }
    }

    pub fn seek(&mut self, seconds: f64) {
        unsafe {
            let _ = self.engine.SetCurrentTime(seconds.max(0.0));
        }
    }
}

impl Drop for VideoPlayer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.engine.Shutdown();
        }
    }
}

/// Owns the currently-open video player (if any) for one preview pane, and
/// the GPU texture its decoded frames are uploaded into.
#[derive(Default)]
pub struct VideoPreviewService {
    current_path: Option<PathBuf>,
    player: Option<VideoPlayer>,
    texture: Option<egui::TextureHandle>,
    load_error: Option<String>,
}

impl VideoPreviewService {
    /// Ensures the player is open for `path`, swapping it out if the
    /// selection changed. Safe to call every frame.
    pub fn set_current(&mut self, path: &Path) {
        if self.current_path.as_deref() == Some(path) {
            return;
        }

        self.current_path = Some(path.to_path_buf());
        self.texture = None;
        self.load_error = None;
        self.player = match VideoPlayer::open(path) {
            Ok(player) => Some(player),
            Err(err) => {
                self.load_error = Some(format!("Couldn't open video: {err}"));
                None
            }
        };
    }

    pub fn error(&self) -> Option<&str> {
        self.player
            .as_ref()
            .and_then(|p| p.error.as_deref())
            .or(self.load_error.as_deref())
    }

    /// Call once per UI frame while a video is selected: advances playback
    /// and uploads any newly decoded frame, returning the texture to draw.
    pub fn texture_for(&mut self, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        let player = self.player.as_mut()?;

        if let Some((rgba, size)) = player.update_frame() {
            let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
            match self.texture.as_mut() {
                Some(texture) => texture.set(color_image, egui::TextureOptions::LINEAR),
                None => {
                    self.texture = Some(ctx.load_texture(
                        "video-preview",
                        color_image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
            ctx.request_repaint();
        } else {
            // Still waiting on the next frame - keep the UI ticking so
            // playback doesn't stall waiting for unrelated input.
            ctx.request_repaint();
        }

        self.texture.clone()
    }

    pub fn toggle_play_pause(&mut self) {
        if let Some(player) = self.player.as_mut() {
            player.toggle_play_pause();
        }
    }

    pub fn is_paused(&self) -> bool {
        self.player.as_ref().is_none_or(|p| p.is_paused())
    }

    /// Current playback position and total duration, in seconds - `None` if
    /// not yet known or there's no active player.
    pub fn progress(&self) -> Option<(f64, f64)> {
        self.player.as_ref().and_then(|p| p.progress())
    }

    pub fn seek(&mut self, seconds: f64) {
        if let Some(player) = self.player.as_mut() {
            player.seek(seconds);
        }
    }
}
