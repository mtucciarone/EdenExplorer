//! Audio playback for the preview pane, reusing the same Windows Media
//! Foundation "Media Engine" API `video.rs` already uses for video - it plays
//! an audio-only file's sound just fine (confirmed by opening an audio file
//! through the video path before this module existed: Media Foundation
//! silently rendered the audio while `VideoPlayer` just reported "no video
//! track"). `AudioPlayer`/`AudioPreviewService` mirror `VideoPlayer`/
//! `VideoPreviewService`'s shape, minus the WIC/`TransferVideoFrame` frame-
//! pull code, since there's no video track to decode here.
//!
//! The one thing `IMFMediaEngine` doesn't expose is raw decoded samples, so
//! it can't drive a waveform display on its own - that needs a separate,
//! one-shot decode via `IMFSourceReader` (Media Foundation's lower-level,
//! pull-based reader), run once per file on a background thread to build a
//! downsampled min/max peak buffer, cached alongside the live player.

use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use windows::Win32::Media::MediaFoundation::{
    CLSID_MFMediaEngineClassFactory, IMFAttributes, IMFMediaEngine, IMFMediaEngineClassFactory,
    IMFMediaEngineNotify, IMFMediaEngineNotify_Impl, MF_MEDIA_ENGINE_CALLBACK,
    MF_MEDIA_ENGINE_EVENT_ENDED, MF_MEDIA_ENGINE_EVENT_ERROR, MF_MT_AUDIO_BITS_PER_SAMPLE,
    MF_MT_AUDIO_NUM_CHANNELS, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
    MF_SOURCE_READER_FIRST_AUDIO_STREAM, MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION,
    MFAudioFormat_PCM, MFCreateAttributes, MFCreateMediaType, MFCreateSourceReaderFromURL,
    MFMediaType_Audio, MFSTARTUP_FULL, MFStartup,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::core::{BSTR, Result};

fn ensure_media_foundation_started() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| unsafe {
        let _ = MFStartup(MF_VERSION, MFSTARTUP_FULL);
    });
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

/// Receives `IMFMediaEngine` lifecycle events, same shape as `video.rs`'s
/// `EngineNotify` - kept as a separate type rather than shared since the two
/// players don't share any other state either (see `CLAUDE.md`'s "similar
/// call sites" convention).
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

/// One playing (or loading) audio file, backed by a live `IMFMediaEngine`.
pub struct AudioPlayer {
    engine: IMFMediaEngine,
    events: Receiver<i32>,
    pub error: Option<String>,
}

impl AudioPlayer {
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
            // file must not start making noise on its own. `Loop` defaults
            // to `FALSE` already, but set it explicitly too so this doesn't
            // depend on that default staying the same.
            engine.SetAutoPlay(false)?;
            engine.SetLoop(false)?;

            let url = BSTR::from(path_to_file_url(path));
            engine.SetSource(&url)?;

            Ok(Self {
                engine,
                events: rx,
                error: None,
            })
        }
    }

    /// Drains lifecycle events. Playback is never started automatically -
    /// the user has to press play - so this only needs to watch for errors
    /// and stop (rather than loop) once playback reaches the end. Call once
    /// per UI frame.
    pub fn pump_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            if event == MF_MEDIA_ENGINE_EVENT_ERROR.0 {
                self.error = Some("Media Foundation couldn't decode this audio file (missing codec, unsupported container, or corrupt file).".to_string());
            } else if event == MF_MEDIA_ENGINE_EVENT_ENDED.0 {
                unsafe {
                    let _ = self.engine.Pause();
                }
            }
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

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.engine.Shutdown();
        }
    }
}

/// A downsampled min/max peak buffer for drawing a waveform, one `(min, max)`
/// pair per horizontal pixel-ish bucket, each normalized to `-1.0..=1.0`.
#[derive(Clone, Default)]
pub struct Waveform {
    pub peaks: Vec<(f32, f32)>,
    /// Set when decoding stopped early (`MAX_WAVEFORM_SAMPLES` reached)
    /// rather than reaching the real end of the file - the waveform still
    /// covers the whole *decoded* prefix proportionally, it just isn't the
    /// whole file for a very long recording.
    pub truncated: bool,
}

const WAVEFORM_BUCKETS: usize = 400;
/// Caps decoding at ~7.5 minutes of mono 44.1kHz audio worth of samples, so a
/// very long recording can't make the one-shot background decode run for an
/// unbounded amount of time or memory - matches the truncation convention
/// `preview.rs` already uses for text/archive previews.
const MAX_WAVEFORM_SAMPLES: usize = 20_000_000;

/// Decodes `path`'s first audio stream to 16-bit PCM via `IMFSourceReader`
/// (a one-shot pull, independent of the live `IMFMediaEngine` used for
/// playback) and reduces it to a fixed-size min/max peak buffer. Returns
/// `None` if the file has no audio stream Media Foundation can read.
fn decode_waveform(path: &Path) -> Option<Waveform> {
    ensure_media_foundation_started();

    unsafe {
        let url = BSTR::from(path_to_file_url(path));
        let reader = MFCreateSourceReaderFromURL(&url, None).ok()?;

        let requested_type = MFCreateMediaType().ok()?;
        requested_type
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
            .ok()?;
        requested_type
            .SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)
            .ok()?;
        requested_type
            .SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)
            .ok()?;
        reader
            .SetCurrentMediaType(
                MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32,
                None,
                &requested_type,
            )
            .ok()?;

        let current_type =
            reader.GetCurrentMediaType(MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32).ok()?;
        let channels = current_type
            .GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS)
            .unwrap_or(1)
            .max(1) as usize;

        // Mono-mixed samples, one `i16` per audio frame (all channels
        // averaged) - kept flat rather than per-channel since the waveform
        // only ever shows one combined trace.
        let mut samples: Vec<i16> = Vec::new();
        let mut truncated = false;

        loop {
            let mut stream_flags: u32 = 0;
            let mut sample = None;
            let read_result = reader.ReadSample(
                MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32,
                0,
                None,
                Some(&mut stream_flags),
                None,
                Some(&mut sample),
            );
            if read_result.is_err() {
                break;
            }
            if stream_flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                break;
            }
            let Some(sample) = sample else {
                continue;
            };

            let Ok(buffer) = sample.ConvertToContiguousBuffer() else {
                continue;
            };
            let mut data_ptr: *mut u8 = std::ptr::null_mut();
            let mut current_len: u32 = 0;
            if buffer
                .Lock(&mut data_ptr, None, Some(&mut current_len))
                .is_err()
            {
                continue;
            }

            let frame_count = (current_len as usize / 2) / channels;
            let raw = std::slice::from_raw_parts(data_ptr as *const i16, frame_count * channels);
            for frame in raw.chunks_exact(channels) {
                let sum: i32 = frame.iter().map(|&s| s as i32).sum();
                samples.push((sum / channels as i32) as i16);
            }

            let _ = buffer.Unlock();

            if samples.len() >= MAX_WAVEFORM_SAMPLES {
                truncated = true;
                break;
            }
        }

        if samples.is_empty() {
            return None;
        }

        let bucket_size = (samples.len() / WAVEFORM_BUCKETS).max(1);
        let mut peaks = Vec::with_capacity(WAVEFORM_BUCKETS);
        for chunk in samples.chunks(bucket_size) {
            let min = chunk.iter().copied().min().unwrap_or(0) as f32 / i16::MAX as f32;
            let max = chunk.iter().copied().max().unwrap_or(0) as f32 / i16::MAX as f32;
            peaks.push((min, max));
        }

        Some(Waveform { peaks, truncated })
    }
}

/// Owns the currently-open audio player (if any) for one preview pane, plus
/// the background-decoded waveform for whichever file is playing.
#[derive(Default)]
pub struct AudioPreviewService {
    current_path: Option<PathBuf>,
    player: Option<AudioPlayer>,
    load_error: Option<String>,
    waveform: Option<Waveform>,
    waveform_rx: Option<Receiver<Option<Waveform>>>,
}

impl AudioPreviewService {
    /// Ensures the player is open for `path` and a waveform decode has been
    /// kicked off, swapping both out if the selection changed. Safe to call
    /// every frame.
    pub fn set_current(&mut self, path: &Path) {
        if self.current_path.as_deref() == Some(path) {
            return;
        }

        self.current_path = Some(path.to_path_buf());
        self.load_error = None;
        self.waveform = None;
        self.player = match AudioPlayer::open(path) {
            Ok(player) => Some(player),
            Err(err) => {
                self.load_error = Some(format!("Couldn't open audio: {err}"));
                None
            }
        };

        let (tx, rx) = unbounded();
        self.waveform_rx = Some(rx);
        let owned = path.to_path_buf();
        std::thread::spawn(move || {
            let waveform = decode_waveform(&owned);
            let _ = tx.send(waveform);
        });
    }

    pub fn error(&self) -> Option<&str> {
        self.player
            .as_ref()
            .and_then(|p| p.error.as_deref())
            .or(self.load_error.as_deref())
    }

    /// Call once per UI frame while an audio file is selected: pumps
    /// playback lifecycle events and picks up a finished waveform decode.
    pub fn tick(&mut self, ctx: &egui::Context) {
        if let Some(player) = self.player.as_mut() {
            player.pump_events();
            ctx.request_repaint();
        }

        if let Some(rx) = &self.waveform_rx
            && let Ok(waveform) = rx.try_recv()
        {
            self.waveform = waveform;
            self.waveform_rx = None;
            ctx.request_repaint();
        }
    }

    pub fn waveform(&self) -> Option<&Waveform> {
        self.waveform.as_ref()
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
