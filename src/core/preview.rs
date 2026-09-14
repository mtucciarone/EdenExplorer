use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui;
use lru::LruCache;
use std::collections::HashSet;
use std::io::Read;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::thread;

const MAX_TEXT_PREVIEW_BYTES: usize = 512 * 1024;
const MAX_PREVIEW_IMAGE_DIM: u32 = 1600;
const PREVIEW_CACHE_CAPACITY: usize = 12;
const MAX_ARCHIVE_ENTRIES: usize = 5000;
const MAX_GIF_PREVIEW_BYTES: usize = 200 * 1024 * 1024;
const MIN_GIF_FRAME_DELAY_MS: u64 = 20;

/// One entry (file or folder) listed inside a previewed archive.
#[derive(Clone)]
pub struct ArchiveEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub depth: usize,
}

/// One decoded frame of an animated image, with how long it should be shown for.
#[derive(Clone)]
pub struct AnimatedFrame {
    pub rgba: Vec<u8>,
    pub delay: std::time::Duration,
}

/// The extracted/decoded content of a previewed file, ready to render.
#[derive(Clone)]
pub enum PreviewPayload {
    /// Plain text - used for text files, config files, and the text extracted
    /// from PDF and Word documents.
    Text(String),
    /// Raw Markdown source, rendered (headings, bold/italic, lists, code
    /// blocks, etc.) instead of shown as plain text.
    Markdown(String),
    Image {
        size: [usize; 2],
        rgba: Vec<u8>,
    },
    /// An animated GIF - every decoded frame, all the same `size`, played back
    /// in the preview pane instead of showing just the first frame.
    Animated {
        size: [usize; 2],
        frames: Vec<AnimatedFrame>,
    },
    /// The file/folder listing of a zip-format archive.
    Archive {
        entries: Vec<ArchiveEntry>,
        truncated: bool,
    },
    /// A file type we don't know how to preview.
    Unsupported(String),
    Error(String),
}

/// Per-preview animation playback position, tracked so `PreviewService` can
/// tell which frame of an `Animated` payload should be on screen right now.
struct AnimationPlayback {
    frame_index: usize,
    next_frame_at: std::time::Instant,
}

/// Background loader for the preview pane: extracting text from a PDF/Word
/// document or decoding a full-resolution image can take a while, so previews are
/// produced on a worker thread and picked up via `pump`.
pub struct PreviewService {
    cache: LruCache<PathBuf, PreviewPayload>,
    pending: HashSet<PathBuf>,
    tx: Sender<(PathBuf, PreviewPayload)>,
    rx: Receiver<(PathBuf, PreviewPayload)>,
    textures: std::collections::HashMap<PathBuf, egui::TextureHandle>,
    animations: std::collections::HashMap<PathBuf, AnimationPlayback>,
}

impl Default for PreviewService {
    fn default() -> Self {
        let (tx, rx) = unbounded();
        Self {
            cache: LruCache::new(NonZeroUsize::new(PREVIEW_CACHE_CAPACITY).unwrap()),
            pending: HashSet::new(),
            tx,
            rx,
            textures: std::collections::HashMap::new(),
            animations: std::collections::HashMap::new(),
        }
    }
}

impl PreviewService {
    /// Drains completed background loads into the cache. Call once per frame.
    pub fn pump(&mut self, ctx: &egui::Context) {
        while let Ok((path, payload)) = self.rx.try_recv() {
            self.pending.remove(&path);
            if self.cache.pop(&path).is_some() {
                self.textures.remove(&path);
                self.animations.remove(&path);
            }
            self.cache.put(path, payload);
            ctx.request_repaint();
        }
    }

    /// Kicks off loading a preview for `path` if it isn't already cached or in
    /// flight. Safe to call every frame the file is selected.
    pub fn request(&mut self, path: &Path) {
        if self.cache.contains(path) || self.pending.contains(path) {
            return;
        }

        self.pending.insert(path.to_path_buf());
        let tx = self.tx.clone();
        let owned = path.to_path_buf();

        thread::spawn(move || {
            let payload = load_preview_payload(&owned);
            let _ = tx.send((owned, payload));
        });
    }

    pub fn get(&mut self, path: &Path) -> Option<&PreviewPayload> {
        self.cache.get(path)
    }

    /// Lazily uploads an already-loaded image payload to a GPU texture.
    pub fn texture_for(&mut self, ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
        if let Some(texture) = self.textures.get(path) {
            return Some(texture.clone());
        }

        let PreviewPayload::Image { size, rgba } = self.cache.get(path)? else {
            return None;
        };

        let color_image = egui::ColorImage::from_rgba_unmultiplied(*size, rgba.as_slice());
        let texture = ctx.load_texture(
            format!("preview:{}", path.display()),
            color_image,
            egui::TextureOptions::LINEAR,
        );
        self.textures.insert(path.to_path_buf(), texture.clone());
        Some(texture)
    }

    /// Like `texture_for`, but for an `Animated` payload: advances playback
    /// based on wall-clock time, updates the (single, reused) texture's pixel
    /// data in place when the current frame changes, and schedules a repaint
    /// for whenever the next frame is due so the animation keeps playing.
    pub fn animated_texture_for(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
    ) -> Option<egui::TextureHandle> {
        let PreviewPayload::Animated { size, frames } = self.cache.get(path)? else {
            return None;
        };
        if frames.is_empty() {
            return None;
        }

        let now = std::time::Instant::now();
        let playback = self
            .animations
            .entry(path.to_path_buf())
            .or_insert_with(|| AnimationPlayback {
                frame_index: 0,
                next_frame_at: now + frames[0].delay,
            });

        // Advance however many frames have elapsed (bounded by frame count so
        // a huge gap - e.g. the app was minimized - doesn't spin forever).
        let mut advanced = false;
        for _ in 0..frames.len() {
            if now < playback.next_frame_at {
                break;
            }
            playback.frame_index = (playback.frame_index + 1) % frames.len();
            playback.next_frame_at += frames[playback.frame_index].delay;
            advanced = true;
        }
        if playback.next_frame_at <= now {
            // Still behind (e.g. after a long pause) - resync to now + this frame's delay.
            playback.next_frame_at = now + frames[playback.frame_index].delay;
        }

        let frame_index = playback.frame_index;
        let next_frame_at = playback.next_frame_at;
        ctx.request_repaint_after(next_frame_at.saturating_duration_since(now));

        let color_image =
            egui::ColorImage::from_rgba_unmultiplied(*size, frames[frame_index].rgba.as_slice());

        if let Some(texture) = self.textures.get_mut(path) {
            if advanced || texture.size() != *size {
                texture.set(color_image, egui::TextureOptions::LINEAR);
            }
            return Some(texture.clone());
        }

        let texture = ctx.load_texture(
            format!("preview-anim:{}", path.display()),
            color_image,
            egui::TextureOptions::LINEAR,
        );
        self.textures.insert(path.to_path_buf(), texture.clone());
        Some(texture)
    }
}

fn load_preview_payload(path: &Path) -> PreviewPayload {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "gif" => load_gif_preview(path),
        "png" | "jpg" | "jpeg" | "bmp" | "webp" | "tiff" | "tif" | "ico" => {
            load_image_preview(path)
        }
        "avif" | "avifs" => load_avif_preview(path),
        "pdf" => load_pdf_preview(path),
        "md" | "markdown" => load_markdown_preview(path),
        "docx" | "doc" | "xlsx" | "xls" | "pptx" | "ppt" => load_office_preview(path),
        "zip" | "jar" | "war" | "apk" | "xpi" => load_archive_preview(path),
        "epub" => load_epub_preview(path),
        "7z" => load_7z_preview(path),
        "ttf" | "otf" | "ttc" | "otc" => load_font_preview(path),
        "svg" => load_svg_preview(path, false),
        "svgz" => load_svg_preview(path, true),
        _ if is_known_text_extension(&ext) || sniff_is_text(path) => load_text_preview(path),
        _ => PreviewPayload::Unsupported(
            "No preview available for this file type.\nDouble-click to open it in its default program.".to_string(),
        ),
    }
}

pub(crate) fn is_known_text_extension(ext: &str) -> bool {
    matches!(
        ext,
        "txt"
            | "log"
            | "ini"
            | "cfg"
            | "conf"
            | "config"
            | "toml"
            | "yaml"
            | "yml"
            | "json"
            | "jsonc"
            | "xml"
            | "csv"
            | "tsv"
            | "rs"
            | "py"
            | "js"
            | "mjs"
            | "ts"
            | "tsx"
            | "jsx"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "cc"
            | "java"
            | "go"
            | "rb"
            | "php"
            | "sh"
            | "bash"
            | "zsh"
            | "ps1"
            | "psm1"
            | "bat"
            | "cmd"
            | "sql"
            | "reg"
            | "gitignore"
            | "gitattributes"
            | "editorconfig"
            | "env"
            | "properties"
            | "htaccess"
            | "html"
            | "htm"
            | "css"
            | "scss"
            | "less"
            | "vue"
            | "svelte"
            | "lua"
            | "r"
            | "swift"
            | "kt"
            | "kts"
            | "dart"
            | "gradle"
            | "cmake"
            | "makefile"
            | "dockerfile"
            | "nfo"
            | "diff"
            | "patch"
    )
}

/// Cheap binary sniff for files whose extension we don't recognize: reads a small
/// prefix and treats it as text if it's valid UTF-8/ASCII-ish with no NUL bytes.
pub(crate) fn sniff_is_text(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };

    let mut buf = vec![0u8; 8192];
    let Ok(n) = file.read(&mut buf) else {
        return false;
    };
    let buf = &buf[..n];

    if buf.is_empty() || buf.contains(&0) {
        return false;
    }

    let text = String::from_utf8_lossy(buf);
    let control_chars = text
        .chars()
        .filter(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        .count();

    control_chars * 20 < text.chars().count().max(1)
}

/// Decodes text bytes, honoring a UTF-16 BOM if present - Windows tools
/// (Registry Editor's ".reg" export chief among them) commonly write UTF-16LE
/// with a BOM rather than UTF-8, which `String::from_utf8_lossy` would
/// otherwise turn into a wall of replacement characters/nulls.
fn decode_text_bytes(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        String::from_utf8_lossy(bytes).into_owned()
    }
}

fn load_text_preview(path: &Path) -> PreviewPayload {
    match std::fs::read(path) {
        Ok(bytes) => {
            let truncated = bytes.len() > MAX_TEXT_PREVIEW_BYTES;
            let slice = &bytes[..bytes.len().min(MAX_TEXT_PREVIEW_BYTES)];
            let mut text = decode_text_bytes(slice);
            if truncated {
                text.push_str("\n\n… (preview truncated - file is larger than 512 KB)");
            }
            PreviewPayload::Text(text)
        }
        Err(err) => PreviewPayload::Error(format!("Couldn't read file: {err}")),
    }
}

fn load_markdown_preview(path: &Path) -> PreviewPayload {
    match std::fs::read(path) {
        Ok(bytes) => {
            let truncated = bytes.len() > MAX_TEXT_PREVIEW_BYTES;
            let slice = &bytes[..bytes.len().min(MAX_TEXT_PREVIEW_BYTES)];
            let mut text = decode_text_bytes(slice);
            if truncated {
                text.push_str("\n\n… (preview truncated - file is larger than 512 KB)");
            }
            PreviewPayload::Markdown(text)
        }
        Err(err) => PreviewPayload::Error(format!("Couldn't read file: {err}")),
    }
}

/// Previews an Office document (.docx/.xlsx/.pptx, and their legacy .doc/.xls/
/// .ppt equivalents) by asking Windows for a shell thumbnail rendered at a
/// large size - the same mechanism Explorer itself uses for thumbnails, which
/// for these formats is generated by Office's (or WPS's/LibreOffice's, if
/// that's what's registered instead) own thumbnail handler, so it actually
/// shows the document's real first page/sheet/slide rather than a generic
/// icon. Falls back to extracted plain text for .docx specifically if no
/// thumbnail could be produced (e.g. no Office-compatible app installed).
const OFFICE_PREVIEW_SIZE: u32 = 1024;

fn load_office_preview(path: &Path) -> PreviewPayload {
    match crate::core::utils::thumbnails::extract_thumbnail_at_size(path, OFFICE_PREVIEW_SIZE) {
        Ok(thumb) => PreviewPayload::Image {
            size: thumb.size,
            rgba: thumb.rgba,
        },
        Err(err) => {
            let is_docx = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("docx"));

            if is_docx {
                load_docx_text_preview(path)
            } else {
                PreviewPayload::Unsupported(format!(
                    "No preview available for this file.\nShell thumbnail extraction failed: {err}\nDouble-click to open it in its default program."
                ))
            }
        }
    }
}

/// Decodes every frame of an animated GIF (falling back to a plain static
/// `Image` payload for single-frame GIFs), so the preview pane can actually
/// play the animation instead of showing only its first frame.
fn load_gif_preview(path: &Path) -> PreviewPayload {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(err) => return PreviewPayload::Error(format!("Couldn't open file: {err}")),
    };

    let decoder = match image::codecs::gif::GifDecoder::new(std::io::BufReader::new(file)) {
        Ok(d) => d,
        Err(err) => return PreviewPayload::Error(format!("Couldn't decode GIF: {err}")),
    };

    let mut frames = Vec::new();
    let mut size: Option<[usize; 2]> = None;
    let mut total_bytes = 0usize;

    for frame_result in image::AnimationDecoder::into_frames(decoder) {
        let frame = match frame_result {
            Ok(f) => f,
            Err(err) => return PreviewPayload::Error(format!("Couldn't decode GIF frame: {err}")),
        };

        let (numer, denom) = frame.delay().numer_denom_ms();
        let delay_ms =
            if denom == 0 { 100 } else { numer / denom }.max(MIN_GIF_FRAME_DELAY_MS as u32);

        let buffer = frame.into_buffer();
        let (width, height) = buffer.dimensions();
        size.get_or_insert([width as usize, height as usize]);

        let rgba = buffer.into_raw();
        total_bytes += rgba.len();

        frames.push(AnimatedFrame {
            rgba,
            delay: std::time::Duration::from_millis(delay_ms as u64),
        });

        // Safety valve against pathologically large/long animations blowing
        // up memory - just stops collecting further frames, so playback
        // loops through whatever was decoded instead of the full sequence.
        if total_bytes >= MAX_GIF_PREVIEW_BYTES {
            break;
        }
    }

    let Some(size) = size else {
        return PreviewPayload::Unsupported("This GIF has no frames.".to_string());
    };

    if frames.len() <= 1 {
        return PreviewPayload::Image {
            size,
            rgba: frames
                .into_iter()
                .next()
                .map(|f| f.rgba)
                .unwrap_or_default(),
        };
    }

    PreviewPayload::Animated { size, frames }
}

fn load_image_preview(path: &Path) -> PreviewPayload {
    let is_ico = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ico"));

    if is_ico {
        return load_ico_preview(path);
    }

    match image::open(path) {
        Ok(image) => image_to_payload(image),
        Err(err) => PreviewPayload::Error(format!("Couldn't decode image: {err}")),
    }
}

fn image_to_payload(image: image::DynamicImage) -> PreviewPayload {
    let image = if image.width() > MAX_PREVIEW_IMAGE_DIM || image.height() > MAX_PREVIEW_IMAGE_DIM {
        image.resize(
            MAX_PREVIEW_IMAGE_DIM,
            MAX_PREVIEW_IMAGE_DIM,
            image::imageops::FilterType::Triangle,
        )
    } else {
        image
    };
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    PreviewPayload::Image {
        size: [width as usize, height as usize],
        rgba: rgba.into_raw(),
    }
}

/// Decodes an .ico file for preview. The `image` crate's own ICO decoder
/// cross-checks each ICONDIRENTRY's declared size against its embedded BMP
/// header and rejects the *whole file* if even one entry disagrees - a quirk
/// real-world icons (pulled from executables, oddly-authored icons, etc.)
/// trip constantly even though Windows itself renders them fine. So for .ico
/// specifically:
///  1. Ask Windows to load and rasterize the icon itself (`LoadImageW`) -
///     this is what Explorer effectively does, and it's tolerant of the
///     exact kind of malformed entries that make the `image` crate bail.
///  2. If that fails, fall back to manually pulling the largest PNG-encoded
///     frame straight out of the file (common for modern large icons).
///  3. Last resort: the `image` crate's own decoder.
fn load_ico_preview(path: &Path) -> PreviewPayload {
    if let Some((rgba, width, height)) = crate::gui::icons::load_ico_file_rgba(path) {
        return PreviewPayload::Image {
            size: [width as usize, height as usize],
            rgba,
        };
    }

    if let Some(image) = load_largest_ico_png_frame(path) {
        return image_to_payload(image);
    }

    match image::open(path) {
        Ok(image) => image_to_payload(image),
        Err(err) => PreviewPayload::Error(format!("Couldn't decode image: {err}")),
    }
}

/// Manually walks an .ico file's directory entries (ICONDIR + ICONDIRENTRY,
/// per the format's spec) and decodes the largest PNG-compressed frame found,
/// without going through the `image` crate's stricter whole-file ICO decoder.
/// Returns `None` if the file isn't a valid ICO or has no PNG-encoded frame
/// (i.e. it's an older icon using only legacy BMP-encoded frames).
fn load_largest_ico_png_frame(path: &Path) -> Option<image::DynamicImage> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 6 {
        return None;
    }

    let reserved = u16::from_le_bytes([bytes[0], bytes[1]]);
    let image_type = u16::from_le_bytes([bytes[2], bytes[3]]);
    let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;

    // ICONDIR: reserved must be 0, type must be 1 (icon, as opposed to 2 = cursor).
    if reserved != 0 || image_type != 1 {
        return None;
    }

    const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    let mut best: Option<(u32, &[u8])> = None;

    for i in 0..count {
        let entry_offset = 6 + i * 16;
        if bytes.len() < entry_offset + 16 {
            break;
        }
        let entry = &bytes[entry_offset..entry_offset + 16];

        let size_in_bytes = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;
        let data_offset = u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]) as usize;

        let Some(data_end) = data_offset.checked_add(size_in_bytes) else {
            continue;
        };
        if data_end > bytes.len() || size_in_bytes < PNG_MAGIC.len() {
            continue;
        }

        let data = &bytes[data_offset..data_end];
        if data[..PNG_MAGIC.len()] != PNG_MAGIC {
            continue;
        }

        if best
            .map(|(best_size, _)| size_in_bytes as u32 > best_size)
            .unwrap_or(true)
        {
            best = Some((size_in_bytes as u32, data));
        }
    }

    let (_, data) = best?;
    image::load_from_memory_with_format(data, image::ImageFormat::Png).ok()
}

/// Renders the PDF's first page as an actual image (via the Windows.Data.Pdf
/// API - the same rendering engine the built-in PDF reader uses), so the
/// preview pane shows what the page actually looks like instead of just its
/// extracted text. Falls back to text extraction only if page rendering
/// isn't possible (e.g. a password-protected file, or the API rejecting it
/// for some other reason).
fn load_pdf_preview(path: &Path) -> PreviewPayload {
    match render_pdf_first_page(path) {
        Ok(image) => image_to_payload(image),
        Err(_) => load_pdf_text_preview(path),
    }
}

fn load_pdf_text_preview(path: &Path) -> PreviewPayload {
    match pdf_extract::extract_text(path) {
        Ok(text) if !text.trim().is_empty() => {
            let mut text = text;
            if text.len() > MAX_TEXT_PREVIEW_BYTES {
                text.truncate(MAX_TEXT_PREVIEW_BYTES);
                text.push_str("\n\n… (preview truncated)");
            }
            PreviewPayload::Text(text)
        }
        Ok(_) => PreviewPayload::Unsupported(
            "This PDF has no extractable text (it may be scanned or image-only).".to_string(),
        ),
        Err(err) => PreviewPayload::Error(format!("Couldn't read PDF: {err}")),
    }
}

/// RAII guard that initializes COM/WinRT on the current thread (required to
/// call any WinRT API) and uninitializes it on drop. Shared by every WinRT-
/// based preview loader below - each runs on its own fresh worker thread with
/// no COM state of its own.
struct ComGuard;
impl ComGuard {
    fn init() -> windows_core::Result<Self> {
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
        Ok(Self)
    }
}
impl Drop for ComGuard {
    fn drop(&mut self) {
        use windows::Win32::System::Com::CoUninitialize;
        unsafe { CoUninitialize() };
    }
}

// `windows-future`'s blocking-wait helper (`Async::join`) isn't public, so
// this app's own worker threads (which have no async runtime) poll the
// operation's status directly instead - simple and fine for something that
// finishes in well under a second.
fn wait_op<T: windows_core::RuntimeType + 'static>(
    op: windows_core::Result<windows_future::IAsyncOperation<T>>,
) -> windows_core::Result<T> {
    let op = op?;
    loop {
        if op.Status()? != windows_future::AsyncStatus::Started {
            return op.GetResults();
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn wait_action(
    action: windows_core::Result<windows_future::IAsyncAction>,
) -> windows_core::Result<()> {
    let action = action?;
    loop {
        if action.Status()? != windows_future::AsyncStatus::Started {
            return action.GetResults();
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Uses the Windows.Data.Pdf WinRT API to load a PDF and rasterize its first
/// page to a PNG in memory, then decodes that PNG via the `image` crate.
/// Requires COM/WinRT to be initialized on the calling thread, which this
/// sets up (and tears down) itself.
fn render_pdf_first_page(path: &Path) -> windows_core::Result<image::DynamicImage> {
    use windows::Data::Pdf::PdfDocument;
    use windows::Storage::StorageFile;
    use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
    use windows_core::HSTRING;

    let _com = ComGuard::init()?;

    let hpath = HSTRING::from(path.to_string_lossy().as_ref());
    let file = wait_op(StorageFile::GetFileFromPathAsync(&hpath))?;
    let document = wait_op(PdfDocument::LoadFromFileAsync(&file))?;

    if document.PageCount()? == 0 {
        return Err(windows_core::Error::from(
            windows::Win32::Foundation::E_FAIL,
        ));
    }

    let page = document.GetPage(0)?;
    let stream = InMemoryRandomAccessStream::new()?;
    wait_action(page.RenderToStreamAsync(&stream))?;
    stream.Seek(0)?;

    let size = stream.Size()? as u32;
    let reader = DataReader::CreateDataReader(&stream)?;
    wait_op(reader.LoadAsync(size))?;

    let mut buffer = vec![0u8; size as usize];
    reader.ReadBytes(&mut buffer)?;

    image::load_from_memory(&buffer)
        .map_err(|_| windows_core::Error::from(windows::Win32::Foundation::E_FAIL))
}

/// Decodes an AVIF (or any other format Windows' own image codecs support)
/// via WIC/Windows.Graphics.Imaging instead of the `image` crate's own AVIF
/// decoder, which needs `dav1d` - a native C library requiring a C toolchain
/// to build. This keeps the app's dependency chain pure-Rust: it re-encodes
/// the decoded bitmap to a PNG in memory via `BitmapEncoder`, then decodes
/// that PNG with `image` like everything else.
fn render_via_windows_imaging(path: &Path) -> windows_core::Result<image::DynamicImage> {
    use windows::Graphics::Imaging::{
        BitmapDecoder, BitmapEncoder, BitmapPixelFormat, SoftwareBitmap,
    };
    use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
    use windows::Storage::{FileAccessMode, StorageFile};
    use windows_core::HSTRING;

    let _com = ComGuard::init()?;

    let hpath = HSTRING::from(path.to_string_lossy().as_ref());
    let file = wait_op(StorageFile::GetFileFromPathAsync(&hpath))?;
    let read_stream = wait_op(file.OpenAsync(FileAccessMode::Read))?;

    let decoder = wait_op(BitmapDecoder::CreateAsync(&read_stream))?;
    let software_bitmap = wait_op(decoder.GetSoftwareBitmapAsync())?;
    let converted = SoftwareBitmap::Convert(&software_bitmap, BitmapPixelFormat::Bgra8)?;

    let out_stream = InMemoryRandomAccessStream::new()?;
    let encoder = wait_op(BitmapEncoder::CreateAsync(
        BitmapEncoder::PngEncoderId()?,
        &out_stream,
    ))?;
    encoder.SetSoftwareBitmap(&converted)?;
    wait_action(encoder.FlushAsync())?;

    out_stream.Seek(0)?;
    let size = out_stream.Size()? as u32;
    let reader = DataReader::CreateDataReader(&out_stream)?;
    wait_op(reader.LoadAsync(size))?;

    let mut buffer = vec![0u8; size as usize];
    reader.ReadBytes(&mut buffer)?;

    image::load_from_memory(&buffer)
        .map_err(|_| windows_core::Error::from(windows::Win32::Foundation::E_FAIL))
}

fn load_avif_preview(path: &Path) -> PreviewPayload {
    match render_via_windows_imaging(path) {
        Ok(image) => image_to_payload(image),
        Err(err) => PreviewPayload::Error(format!(
            "Couldn't decode AVIF image: {err}\n(requires the AV1 image codec, usually preinstalled on Windows 11 or available from the Store as \"AV1 Video Extension\" on Windows 10)"
        )),
    }
}

fn load_docx_text_preview(path: &Path) -> PreviewPayload {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(err) => return PreviewPayload::Error(format!("Couldn't open file: {err}")),
    };

    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(err) => return PreviewPayload::Error(format!("Couldn't read .docx: {err}")),
    };

    let mut xml = String::new();
    match archive.by_name("word/document.xml") {
        Ok(mut entry) => {
            if let Err(err) = entry.read_to_string(&mut xml) {
                return PreviewPayload::Error(format!("Couldn't read document content: {err}"));
            }
        }
        Err(err) => return PreviewPayload::Error(format!("Couldn't find document content: {err}")),
    }

    let mut text = extract_docx_text(&xml);
    if text.trim().is_empty() {
        return PreviewPayload::Unsupported(
            "This document appears to have no text content.".to_string(),
        );
    }
    if text.len() > MAX_TEXT_PREVIEW_BYTES {
        text.truncate(MAX_TEXT_PREVIEW_BYTES);
        text.push_str("\n\n… (preview truncated)");
    }
    PreviewPayload::Text(text)
}

/// Extracts an EPUB's readable text (in reading order) rather than showing
/// its internal zip file tree, which is all the generic archive listing
/// would otherwise offer for what's really a text document. Reuses the same
/// `zip::ZipArchive` + `quick_xml::reader::Reader` combo `load_docx_text_preview`
/// already does, since an EPUB is a zip container too - just walking the
/// standard EPUB structure (a `META-INF/container.xml` pointer to an OPF
/// manifest/spine) instead of Word's fixed `word/document.xml` path.
fn load_epub_preview(path: &Path) -> PreviewPayload {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(err) => return PreviewPayload::Error(format!("Couldn't open file: {err}")),
    };

    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(err) => return PreviewPayload::Error(format!("Couldn't read .epub: {err}")),
    };

    let container_xml = match read_zip_entry_to_string(&mut archive, "META-INF/container.xml") {
        Ok(s) => s,
        Err(err) => return PreviewPayload::Error(format!("Couldn't find EPUB container: {err}")),
    };

    let Some(opf_path) = find_opf_path(&container_xml) else {
        return PreviewPayload::Error("Couldn't find EPUB content manifest".to_string());
    };

    let opf_xml = match read_zip_entry_to_string(&mut archive, &opf_path) {
        Ok(s) => s,
        Err(err) => return PreviewPayload::Error(format!("Couldn't read EPUB manifest: {err}")),
    };

    let opf_dir = Path::new(&opf_path).parent().unwrap_or(Path::new(""));
    let spine_paths = spine_content_paths(&opf_xml, opf_dir);

    if spine_paths.is_empty() {
        return PreviewPayload::Unsupported(
            "This EPUB has no readable content documents.".to_string(),
        );
    }

    let mut text = String::new();
    for content_path in spine_paths {
        let Ok(xhtml) = read_zip_entry_to_string(&mut archive, &content_path) else {
            continue;
        };

        let chapter_text = extract_xhtml_text(&xhtml);
        let chapter_text = chapter_text.trim();
        if !chapter_text.is_empty() {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(chapter_text);
        }

        if text.len() > MAX_TEXT_PREVIEW_BYTES {
            break;
        }
    }

    if text.trim().is_empty() {
        return PreviewPayload::Unsupported(
            "This EPUB appears to have no text content.".to_string(),
        );
    }
    if text.len() > MAX_TEXT_PREVIEW_BYTES {
        text.truncate(MAX_TEXT_PREVIEW_BYTES);
        text.push_str("\n\n… (preview truncated)");
    }
    PreviewPayload::Text(text)
}

fn read_zip_entry_to_string(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Result<String, String> {
    let mut entry = archive.by_name(name).map_err(|err| err.to_string())?;
    let mut s = String::new();
    entry
        .read_to_string(&mut s)
        .map_err(|err| err.to_string())?;
    Ok(s)
}

/// Pulls the OPF manifest's path out of an EPUB's `META-INF/container.xml`,
/// e.g. `<rootfile full-path="OEBPS/content.opf" .../>` -> `"OEBPS/content.opf"`.
fn find_opf_path(container_xml: &str) -> Option<String> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(container_xml);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if e.local_name().as_ref() == b"rootfile" =>
            {
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref() == b"full-path"
                        && let Ok(value) = attr.unescape_value()
                    {
                        return Some(value.to_string());
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }
}

/// Walks an EPUB's OPF manifest (`<item id="..." href="...">`) and spine
/// (`<itemref idref="...">`, the reading order) to produce the ordered list
/// of content-document zip entry paths, resolved relative to the OPF file's
/// own directory (hrefs inside an OPF are always relative to it, not to the
/// zip root).
fn spine_content_paths(opf_xml: &str, opf_dir: &Path) -> Vec<String> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;
    use std::collections::HashMap;

    let mut reader = Reader::from_str(opf_xml);
    let mut buf = Vec::new();
    let mut manifest: HashMap<String, String> = HashMap::new();
    let mut spine_order: Vec<String> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"item" => {
                    let mut id = None;
                    let mut href = None;
                    for attr in e.attributes().flatten() {
                        match attr.key.as_ref() {
                            b"id" => id = attr.unescape_value().ok().map(|v| v.to_string()),
                            b"href" => href = attr.unescape_value().ok().map(|v| v.to_string()),
                            _ => {}
                        }
                    }
                    if let (Some(id), Some(href)) = (id, href) {
                        manifest.insert(id, href);
                    }
                }
                b"itemref" => {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"idref"
                            && let Ok(value) = attr.unescape_value()
                        {
                            spine_order.push(value.to_string());
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    spine_order
        .into_iter()
        .filter_map(|idref| manifest.get(&idref))
        .map(|href| normalize_zip_entry_path(opf_dir.join(href)))
        .collect()
}

/// Joins an OPF-relative href onto the OPF's own directory and renders it as
/// a forward-slash zip entry name - `Path::join` produces backslashes on
/// Windows, which `zip::ZipArchive::by_name` won't match against a real
/// (always forward-slash) zip entry path.
fn normalize_zip_entry_path(path: PathBuf) -> String {
    path.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Extracts visible text from an EPUB content document (XHTML), inserting a
/// newline at block-level boundaries (`</p>`, `<br/>`, headings, list items)
/// so chapters read as actual paragraphs rather than one run-on line -
/// simpler than `extract_docx_text` since XHTML doesn't need WordprocessingML's
/// `w:t`/`w:tab` run-splitting, just plain tag-stripping.
fn extract_xhtml_text(xhtml: &str) -> String {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xhtml);
    reader.config_mut().trim_text(false);

    let mut out = String::new();
    let mut buf = Vec::new();
    let mut skip_depth = 0u32;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"script" | b"style" => skip_depth += 1,
                b"br" => out.push('\n'),
                _ => {}
            },
            Ok(Event::Empty(e)) if e.local_name().as_ref() == b"br" => out.push('\n'),
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"script" | b"style" => skip_depth = skip_depth.saturating_sub(1),
                b"p" | b"div" | b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6" | b"li" | b"tr" => {
                    out.push('\n');
                }
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if skip_depth == 0
                    && let Ok(text) = t.decode()
                {
                    out.push_str(&text);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    out
}

fn load_archive_preview(path: &Path) -> PreviewPayload {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(err) => return PreviewPayload::Error(format!("Couldn't open file: {err}")),
    };

    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(err) => return PreviewPayload::Error(format!("Couldn't read archive: {err}")),
    };

    let mut sort_keys: Vec<(String, ArchiveEntry)> = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index(i) else {
            continue;
        };
        let raw_name = entry.name().replace('\\', "/");
        let trimmed = raw_name.trim_end_matches('/');
        if trimmed.is_empty() {
            continue;
        }
        let is_dir = entry.is_dir() || raw_name.ends_with('/');
        let depth = trimmed.matches('/').count();
        let display_name = trimmed.rsplit('/').next().unwrap_or(trimmed).to_string();

        sort_keys.push((
            trimmed.to_string(),
            ArchiveEntry {
                name: display_name,
                is_dir,
                size: entry.size(),
                depth,
            },
        ));
    }

    if sort_keys.is_empty() {
        return PreviewPayload::Unsupported("This archive appears to be empty.".to_string());
    }

    sort_keys.sort_by(|a, b| a.0.cmp(&b.0));

    let truncated = sort_keys.len() > MAX_ARCHIVE_ENTRIES;
    let entries = sort_keys
        .into_iter()
        .take(MAX_ARCHIVE_ENTRIES)
        .map(|(_, entry)| entry)
        .collect();

    PreviewPayload::Archive { entries, truncated }
}

/// Same idea as `load_archive_preview`, but for `.7z` - a completely
/// different container format from zip, so it needs its own crate
/// (`sevenz-rust2`, a maintained fork of the unmaintained `sevenz-rust`; RAR
/// support was deliberately skipped instead, since the only practical crate
/// for it wraps the non-free-licensed official UnRAR library). Only listing
/// is needed here (no extraction), and `sevenz_rust2::Archive::open` already
/// hands back every entry's name/size/directory-ness directly - no need to
/// open a reader or decompress anything just to show the file tree. Builds
/// the exact same `ArchiveEntry` shape `load_archive_preview` does, so the
/// existing indented-by-depth rendering in `itemviewer_preview.rs` needs no
/// changes at all for this new format.
fn load_7z_preview(path: &Path) -> PreviewPayload {
    let archive = match sevenz_rust2::Archive::open(path) {
        Ok(a) => a,
        Err(err) => return PreviewPayload::Error(format!("Couldn't read .7z archive: {err}")),
    };

    let mut sort_keys: Vec<(String, ArchiveEntry)> = Vec::with_capacity(archive.files.len());
    for entry in &archive.files {
        let raw_name = entry.name.replace('\\', "/");
        let trimmed = raw_name.trim_end_matches('/');
        if trimmed.is_empty() {
            continue;
        }
        let is_dir = entry.is_directory || raw_name.ends_with('/');
        let depth = trimmed.matches('/').count();
        let display_name = trimmed.rsplit('/').next().unwrap_or(trimmed).to_string();

        sort_keys.push((
            trimmed.to_string(),
            ArchiveEntry {
                name: display_name,
                is_dir,
                size: entry.size,
                depth,
            },
        ));
    }

    if sort_keys.is_empty() {
        return PreviewPayload::Unsupported("This archive appears to be empty.".to_string());
    }

    sort_keys.sort_by(|a, b| a.0.cmp(&b.0));

    let truncated = sort_keys.len() > MAX_ARCHIVE_ENTRIES;
    let entries = sort_keys
        .into_iter()
        .take(MAX_ARCHIVE_ENTRIES)
        .map(|(_, entry)| entry)
        .collect();

    PreviewPayload::Archive { entries, truncated }
}

const SVG_PREVIEW_TARGET_LONG_EDGE: f32 = 1024.0;

/// Rasterizes an SVG (or gzip-compressed .svgz) file to an RGBA bitmap via
/// `resvg`, so it previews as an actual rendered image instead of raw XML
/// markup. The output canvas is scaled so its longer edge is a fixed size
/// regardless of the SVG's own declared dimensions, since those are often
/// either tiny (icons) or arbitrary/relative - vector content scales
/// cleanly either way.
fn load_svg_preview(path: &Path, gzip_compressed: bool) -> PreviewPayload {
    let raw = match std::fs::read(path) {
        Ok(d) => d,
        Err(err) => return PreviewPayload::Error(format!("Couldn't read file: {err}")),
    };

    let data = if gzip_compressed {
        use std::io::Read;
        let mut decompressed = Vec::new();
        if let Err(err) = flate2::read::GzDecoder::new(raw.as_slice()).read_to_end(&mut decompressed)
        {
            return PreviewPayload::Error(format!("Couldn't decompress .svgz: {err}"));
        }
        decompressed
    } else {
        raw
    };

    let opt = resvg::usvg::Options {
        resources_dir: path.parent().map(|p| p.to_path_buf()),
        fontdb: svg_fontdb(),
        ..Default::default()
    };

    let tree = match resvg::usvg::Tree::from_data(&data, &opt) {
        Ok(t) => t,
        Err(err) => return PreviewPayload::Error(format!("Couldn't parse SVG: {err}")),
    };

    let native_size = tree.size();
    let native_w = native_size.width().max(1.0);
    let native_h = native_size.height().max(1.0);
    let scale = SVG_PREVIEW_TARGET_LONG_EDGE / native_w.max(native_h);
    let target_w = (native_w * scale).round().max(1.0) as u32;
    let target_h = (native_h * scale).round().max(1.0) as u32;

    let Some(mut pixmap) = resvg::tiny_skia::Pixmap::new(target_w, target_h) else {
        return PreviewPayload::Error("Couldn't allocate image buffer for SVG".to_string());
    };

    let transform = resvg::tiny_skia::Transform::from_scale(
        target_w as f32 / native_w,
        target_h as f32 / native_h,
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // tiny_skia stores premultiplied-alpha RGBA; egui's ColorImage expects
    // straight (unmultiplied) alpha.
    let mut rgba = Vec::with_capacity(pixmap.pixels().len() * 4);
    for pixel in pixmap.pixels() {
        let c = pixel.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }

    PreviewPayload::Image {
        size: [target_w as usize, target_h as usize],
        rgba,
    }
}

/// A process-wide cache of the system font database used to render text
/// inside SVGs - scanning installed fonts is comparatively slow (on the
/// order of hundreds of milliseconds), so it's built once and reused.
fn svg_fontdb() -> std::sync::Arc<fontdb::Database> {
    static FONTDB: std::sync::OnceLock<std::sync::Arc<fontdb::Database>> =
        std::sync::OnceLock::new();
    FONTDB
        .get_or_init(|| {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            std::sync::Arc::new(db)
        })
        .clone()
}

const FONT_PREVIEW_WIDTH: usize = 760;
const FONT_PREVIEW_MARGIN: f32 = 24.0;
const FONT_PANGRAM: &str = "The quick brown fox jumps over the lazy dog";
const FONT_SAMPLE_SIZES: [f32; 4] = [42.0, 30.0, 22.0, 16.0];

/// Renders a font file's family name plus a handful of sample lines (a
/// pangram at a few sizes, alphabet, and digits) into a plain RGBA bitmap,
/// reusing the existing `Image` preview payload/rendering path.
fn load_font_preview(path: &Path) -> PreviewPayload {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(err) => return PreviewPayload::Error(format!("Couldn't read font file: {err}")),
    };

    let font = match ab_glyph::FontArc::try_from_vec(bytes.clone()) {
        Ok(f) => f,
        Err(err) => return PreviewPayload::Error(format!("Couldn't parse font: {err}")),
    };

    let family_name = ttf_parser::Face::parse(&bytes, 0)
        .ok()
        .and_then(|face| {
            face.names().into_iter().find_map(|n| {
                if n.name_id == ttf_parser::name_id::FULL_NAME
                    || n.name_id == ttf_parser::name_id::FAMILY
                {
                    n.to_string()
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });

    let canvas = render_font_sample(&font, &family_name);
    PreviewPayload::Image {
        size: [canvas.width, canvas.height],
        rgba: canvas.pixels,
    }
}

struct FontSampleCanvas {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

impl FontSampleCanvas {
    fn new(width: usize, height: usize) -> Self {
        // Opaque white background.
        let mut pixels = vec![255u8; width * height * 4];
        for chunk in pixels.chunks_exact_mut(4) {
            chunk[3] = 255;
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    fn blend_pixel(&mut self, x: i32, y: i32, coverage: f32, color: [u8; 3]) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let coverage = coverage.clamp(0.0, 1.0);
        let idx = (y as usize * self.width + x as usize) * 4;
        for c in 0..3 {
            let bg = self.pixels[idx + c] as f32;
            let fg = color[c] as f32;
            self.pixels[idx + c] = (bg * (1.0 - coverage) + fg * coverage).round() as u8;
        }
    }
}

/// Draws one line of `text` at `scale_px`, with its baseline at `(start_x, baseline_y)`;
/// returns the x position right after the last glyph drawn.
fn draw_font_sample_line(
    canvas: &mut FontSampleCanvas,
    font: &ab_glyph::FontArc,
    text: &str,
    scale_px: f32,
    start_x: f32,
    baseline_y: f32,
    color: [u8; 3],
) -> f32 {
    use ab_glyph::{Font, ScaleFont};

    let scaled = font.as_scaled(scale_px);
    let mut cursor_x = start_x;
    let mut last_glyph_id: Option<ab_glyph::GlyphId> = None;

    for ch in text.chars() {
        let glyph_id = scaled.glyph_id(ch);

        if let Some(last) = last_glyph_id {
            cursor_x += scaled.kern(last, glyph_id);
        }

        let glyph =
            glyph_id.with_scale_and_position(scale_px, ab_glyph::point(cursor_x, baseline_y));

        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, coverage| {
                canvas.blend_pixel(
                    bounds.min.x as i32 + gx as i32,
                    bounds.min.y as i32 + gy as i32,
                    coverage,
                    color,
                );
            });
        }

        cursor_x += scaled.h_advance(glyph_id);
        last_glyph_id = Some(glyph_id);
    }

    cursor_x
}

fn render_font_sample(font: &ab_glyph::FontArc, family_name: &str) -> FontSampleCanvas {
    let title_size = 26.0;
    let line_gap = 14.0;

    let mut height = FONT_PREVIEW_MARGIN + title_size + line_gap;
    for size in FONT_SAMPLE_SIZES {
        height += size + line_gap;
    }
    height += FONT_PREVIEW_MARGIN;

    let mut canvas = FontSampleCanvas::new(FONT_PREVIEW_WIDTH, height as usize);

    let text_color = [40u8, 40u8, 40u8];
    let mut baseline_y = FONT_PREVIEW_MARGIN + title_size * 0.8;

    draw_font_sample_line(
        &mut canvas,
        font,
        family_name,
        title_size,
        FONT_PREVIEW_MARGIN,
        baseline_y,
        [20, 20, 20],
    );
    baseline_y += line_gap;

    for size in FONT_SAMPLE_SIZES {
        baseline_y += size * 0.8;
        draw_font_sample_line(
            &mut canvas,
            font,
            FONT_PANGRAM,
            size,
            FONT_PREVIEW_MARGIN,
            baseline_y,
            text_color,
        );
        baseline_y += size * 0.2 + line_gap;
    }

    canvas
}

/// Walks a WordprocessingML `document.xml` and pulls out the plain text runs,
/// turning paragraph/line breaks into newlines and tabs into `\t`.
fn extract_docx_text(xml: &str) -> String {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut out = String::new();
    let mut buf = Vec::new();
    let mut in_text_run = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"t" => in_text_run = true,
                b"tab" => out.push('\t'),
                b"br" | b"cr" => out.push('\n'),
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"tab" => out.push('\t'),
                b"br" | b"cr" => out.push('\n'),
                _ => {}
            },
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"t" => in_text_run = false,
                b"p" => out.push('\n'),
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if in_text_run {
                    if let Ok(text) = t.decode() {
                        out.push_str(&text);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    out
}

#[cfg(test)]
mod epub_tests {
    use super::*;

    #[test]
    fn finds_opf_path_in_container_xml() {
        let xml = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;
        assert_eq!(find_opf_path(xml), Some("OEBPS/content.opf".to_string()));
    }

    #[test]
    fn resolves_spine_order_from_manifest_and_spine() {
        let opf = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf">
  <manifest>
    <item id="ch2" href="chapter2.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
    <item id="css" href="style.css" media-type="text/css"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
    <itemref idref="ch2"/>
  </spine>
</package>"#;
        let paths = spine_content_paths(opf, Path::new("OEBPS"));
        assert_eq!(
            paths,
            vec![
                "OEBPS/chapter1.xhtml".to_string(),
                "OEBPS/chapter2.xhtml".to_string(),
            ]
        );
    }

    #[test]
    fn extracts_paragraphs_with_newlines_between_them() {
        let xhtml = r#"<html><body>
<h1>Chapter One</h1>
<p>First paragraph.</p>
<p>Second paragraph.</p>
</body></html>"#;
        let text = extract_xhtml_text(xhtml);
        assert!(text.contains("Chapter One"));
        assert!(text.contains("First paragraph."));
        assert!(text.contains("Second paragraph."));
        // Each block ends with its own newline, so the paragraphs aren't
        // run together into one line.
        assert!(text.contains("Chapter One\n"));
        assert!(text.contains("First paragraph.\n"));
    }

    #[test]
    fn extraction_skips_script_and_style_content() {
        let xhtml = r#"<html><body>
<style>body { color: red; }</style>
<script>alert('hi');</script>
<p>Visible text.</p>
</body></html>"#;
        let text = extract_xhtml_text(xhtml);
        assert!(text.contains("Visible text."));
        assert!(!text.contains("color: red"));
        assert!(!text.contains("alert"));
    }
}

#[cfg(test)]
mod sevenz_tests {
    use super::*;

    /// Builds a small real `.7z` archive (one top-level file, one nested
    /// file inside a directory) via `sevenz_rust2`'s own writer, then
    /// verifies `load_7z_preview` lists it the same way `load_archive_preview`
    /// already lists a zip: sorted, with depth reflecting nesting.
    #[test]
    fn lists_entries_from_a_real_7z_archive() {
        let dir = std::env::temp_dir().join(format!(
            "eden_explorer_sevenz_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let archive_path = dir.join("test.7z");

        {
            let mut writer = sevenz_rust2::ArchiveWriter::create(&archive_path).unwrap();
            writer
                .push_archive_entry(
                    sevenz_rust2::ArchiveEntry {
                        name: "root.txt".to_string(),
                        ..Default::default()
                    },
                    Some(std::io::Cursor::new(b"hello".to_vec())),
                )
                .unwrap();
            writer
                .push_archive_entry(
                    sevenz_rust2::ArchiveEntry {
                        name: "subdir/nested.txt".to_string(),
                        ..Default::default()
                    },
                    Some(std::io::Cursor::new(b"world".to_vec())),
                )
                .unwrap();
            writer.finish().unwrap();
        }

        let payload = load_7z_preview(&archive_path);
        let _ = std::fs::remove_dir_all(&dir);

        let PreviewPayload::Archive { entries, truncated } = payload else {
            panic!("expected an Archive payload");
        };
        assert!(!truncated);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "root.txt");
        assert_eq!(entries[0].depth, 0);
        assert_eq!(entries[1].name, "nested.txt");
        assert_eq!(entries[1].depth, 1);
    }
}
