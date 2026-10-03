//! Archive extraction: Extract Here / Extract To Folder, and listing an
//! archive's contents so it can be browsed like a folder.
//!
//! Built in: zip, 7z, tar, and tar or single files compressed with gzip,
//! bzip2 or xz (`.tar.gz`/`.tgz`, `.tar.bz2`/`.tbz2`, `.tar.xz`/`.txz`,
//! `.gz`, `.bz2`, `.xz`). Anything else 7-Zip reads (RAR, ISO, CAB, ...) is
//! handed to 7-Zip's `7z.exe` when it's installed.
//!
//! Extraction never overwrites: extracting to a folder makes a new one
//! (numbered if the name is taken), and Extract Here numbers any top-level
//! item whose name is already used. Entry names that would escape the
//! destination (`..`, absolute paths, drive letters) are refused, and a
//! cancelled or failed extraction removes what it created.

use crossbeam_channel::{Receiver, Sender};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

const COPY_BUFFER: usize = 256 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    SevenZip,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    Gz,
    Bz2,
    Xz,
    /// Only 7-Zip reads it (RAR, ISO, CAB, ...).
    External,
}

impl ArchiveKind {
    /// Suffixes, longest first so `.tar.gz` wins over `.gz`.
    const SUFFIXES: [(&'static str, ArchiveKind); 16] = [
        (".tar.gz", ArchiveKind::TarGz),
        (".tar.bz2", ArchiveKind::TarBz2),
        (".tar.xz", ArchiveKind::TarXz),
        (".tgz", ArchiveKind::TarGz),
        (".tbz2", ArchiveKind::TarBz2),
        (".tbz", ArchiveKind::TarBz2),
        (".txz", ArchiveKind::TarXz),
        (".zip", ArchiveKind::Zip),
        (".7z", ArchiveKind::SevenZip),
        (".tar", ArchiveKind::Tar),
        (".gz", ArchiveKind::Gz),
        (".bz2", ArchiveKind::Bz2),
        (".xz", ArchiveKind::Xz),
        (".rar", ArchiveKind::External),
        (".iso", ArchiveKind::External),
        (".cab", ArchiveKind::External),
    ];

    /// The archive type a file name says it is, and the name without that
    /// suffix (the folder name Extract To uses).
    pub fn of(name: &str) -> Option<(ArchiveKind, &str)> {
        let lower = name.to_lowercase();
        for (suffix, kind) in Self::SUFFIXES {
            if lower.len() > suffix.len() && lower.ends_with(suffix) {
                return Some((kind, &name[..name.len() - suffix.len()]));
            }
        }
        None
    }

    /// Holds several entries (a single `.gz` holds one file).
    pub fn is_multi(self) -> bool {
        !matches!(self, ArchiveKind::Gz | ArchiveKind::Bz2 | ArchiveKind::Xz)
    }
}

/// 7-Zip's command-line tool, if installed (looked up once).
pub fn seven_zip_exe() -> Option<PathBuf> {
    static FOUND: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    FOUND.get_or_init(find_seven_zip).clone()
}

fn find_seven_zip() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(dir) = std::env::var_os(var) {
            candidates.push(PathBuf::from(dir).join("7-Zip").join("7z.exe"));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Whether `path` can be extracted here (by name; the content is checked
/// when extracting).
pub fn can_extract(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { return false };
    match ArchiveKind::of(name) {
        Some((ArchiveKind::External, _)) => seven_zip_exe().is_some(),
        Some(_) => true,
        None => false,
    }
}

/// `path` if nothing is there, else `path (2)`, `path (3)`, ... (the
/// number goes before the extension of a file).
pub fn unique_path(path: &Path) -> PathBuf {
    if std::fs::symlink_metadata(path).is_err() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or(Path::new(""));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let is_dir = path.is_dir();
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if !is_dir && dot > 0 => (&name[..dot], &name[dot..]),
        _ => (name.as_str(), ""),
    };
    (2..)
        .map(|n| parent.join(format!("{stem} ({n}){ext}")))
        .find(|p| std::fs::symlink_metadata(p).is_err())
        .unwrap_or_else(|| path.to_path_buf())
}

/// Characters Windows doesn't allow in names become `_`.
fn clean_component(part: &str) -> String {
    let cleaned: String = part
        .chars()
        .map(|c| if matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control() { '_' } else { c })
        .collect();
    cleaned.trim_end_matches([' ', '.']).to_string()
}

/// An entry name's parts, or `None` if it would leave the destination (an
/// absolute path, a drive, or `..`).
pub fn safe_parts(name: &str) -> Option<Vec<String>> {
    let name = name.replace('\\', "/");
    if name.starts_with('/') || name.as_bytes().get(1) == Some(&b':') {
        return None;
    }
    let mut parts = Vec::new();
    for component in Path::new(&name).components() {
        match component {
            Component::Normal(part) => {
                let part = clean_component(&part.to_string_lossy());
                if !part.is_empty() {
                    parts.push(part);
                }
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!parts.is_empty()).then_some(parts)
}

#[derive(Clone, Debug, Default)]
pub struct ExtractProgress {
    pub done: u64,
    pub total: u64,
    pub files: u64,
    pub current: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExtractSummary {
    /// The folder the results are in (the new folder for Extract To).
    pub folder: PathBuf,
    /// What was created at the top level.
    pub created: Vec<PathBuf>,
    pub files: u64,
    pub bytes: u64,
}

pub enum ExtractEvent {
    Progress(ExtractProgress),
    /// `Err("")` = cancelled.
    Finished(Result<ExtractSummary, String>),
}

pub struct ExtractHandle {
    pub rx: Receiver<ExtractEvent>,
    cancel: Arc<AtomicBool>,
}

impl ExtractHandle {
    /// The flag `cancel` sets (for a Cancel button elsewhere).
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// Into the archive's folder, numbering top-level names already used.
    Here,
    /// Into a new folder named after the archive, next to it.
    OwnFolder,
    /// Into a new folder named after the archive, inside this folder.
    FolderIn(PathBuf),
    /// Straight into this folder (numbering names already used).
    Into(PathBuf),
}

const CANCELLED: &str = "";

/// Writes entries under `base`, keeping track of what it created.
struct Sink<'a> {
    base: PathBuf,
    /// Top-level names as they came, and the (possibly numbered) names used.
    tops: HashMap<String, String>,
    created: Vec<PathBuf>,
    /// Only entries under one of these (`a/b` style) are extracted, with
    /// that prefix's parent removed - for extracting a selection while
    /// browsing an archive.
    only: Option<Vec<Vec<String>>>,
    progress: ExtractProgress,
    counted: Option<&'a AtomicU64>,
    last: Instant,
    tx: &'a Sender<ExtractEvent>,
    cancel: &'a AtomicBool,
    buffer: Vec<u8>,
}

impl Sink<'_> {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn report(&mut self, force: bool) {
        if force || self.last.elapsed() >= PROGRESS_INTERVAL {
            self.last = Instant::now();
            if let Some(counted) = self.counted {
                self.progress.done = counted.load(Ordering::Relaxed);
            }
            let _ = self.tx.send(ExtractEvent::Progress(self.progress.clone()));
            // Keep the notification's progress moving while the app is idle.
            crate::gui::windows::windowsoverrides::request_repaint();
        }
    }

    /// Where `parts` goes, or `None` when it's filtered out.
    fn destination(&mut self, parts: Vec<String>) -> io::Result<Option<PathBuf>> {
        let parts = match &self.only {
            None => parts,
            Some(prefixes) => {
                let Some(prefix) = prefixes.iter().find(|p| parts.starts_with(p)) else {
                    return Ok(None);
                };
                parts[prefix.len() - 1..].to_vec()
            }
        };
        let top = parts[0].clone();
        let used = match self.tops.get(&top) {
            Some(used) => used.clone(),
            None => {
                let unique = unique_path(&self.base.join(&top));
                let used = unique.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(top.clone());
                self.created.push(self.base.join(&used));
                self.tops.insert(top, used.clone());
                used
            }
        };
        let mut path = self.base.join(used);
        for part in &parts[1..] {
            path.push(part);
        }
        Ok(Some(path))
    }

    fn dir(&mut self, name: &str) -> io::Result<()> {
        let Some(parts) = safe_parts(name) else { return Ok(()) };
        if let Some(path) = self.destination(parts)? {
            std::fs::create_dir_all(path)?;
        }
        Ok(())
    }

    /// Writes one file. `sized` = the reader's bytes count toward progress
    /// (for formats whose progress is the uncompressed total).
    fn file(&mut self, name: &str, reader: &mut dyn Read, modified: Option<SystemTime>, sized: bool) -> io::Result<()> {
        if self.cancelled() {
            return Err(io::Error::other(CANCELLED));
        }
        let Some(parts) = safe_parts(name) else {
            // Refused (it would land outside the destination): skip its data.
            io::copy(reader, &mut io::sink())?;
            return Ok(());
        };
        let Some(path) = self.destination(parts)? else {
            io::copy(reader, &mut io::sink())?;
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        self.progress.current = name.rsplit('/').next().unwrap_or(name).to_string();
        let mut out = io::BufWriter::with_capacity(COPY_BUFFER, File::create(&path)?);
        loop {
            let read = reader.read(&mut self.buffer)?;
            if read == 0 {
                break;
            }
            out.write_all(&self.buffer[..read])?;
            if sized {
                self.progress.done += read as u64;
            }
            if self.cancelled() {
                return Err(io::Error::other(CANCELLED));
            }
            self.report(false);
        }
        let file = out.into_inner().map_err(|e| e.into_error())?;
        if let Some(time) = modified {
            let _ = file.set_modified(time);
        }
        self.progress.files += 1;
        self.report(false);
        Ok(())
    }

    /// Removes everything this extraction created (after a cancel or error).
    fn undo(&self) {
        for path in &self.created {
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(path);
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// Counts bytes read from the archive file, for progress on formats whose
/// uncompressed size isn't known up front.
struct Counting<R> {
    inner: R,
    count: Arc<AtomicU64>,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.count.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

/// A zip entry's time (stored as local time) as a `SystemTime`. Looking up
/// the local time zone costs ~0.25 ms on Windows, which added up to seconds
/// for a big archive, so the offset is looked up once per hour of the date
/// (entries cluster, and that still follows daylight-saving changes).
fn zip_time(time: zip::DateTime) -> Option<SystemTime> {
    use chrono::{Offset, TimeZone};
    thread_local! {
        static OFFSETS: std::cell::RefCell<HashMap<(u16, u8, u8, u8), Option<chrono::FixedOffset>>> =
            std::cell::RefCell::new(HashMap::new());
    }
    let date = chrono::NaiveDate::from_ymd_opt(time.year() as i32, time.month() as u32, time.day() as u32)?;
    let naive = date.and_hms_opt(time.hour() as u32, time.minute() as u32, time.second() as u32)?;
    let key = (time.year(), time.month(), time.day(), time.hour());
    let offset = OFFSETS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() > 4096 {
            cache.clear();
        }
        *cache.entry(key).or_insert_with(|| {
            chrono::Local.offset_from_local_datetime(&naive).single().map(|o| o.fix())
        })
    })?;
    let utc = naive - chrono::Duration::seconds(offset.local_minus_utc() as i64);
    Some(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(utc, chrono::Utc).into())
}

fn extract_zip(archive: &Path, sink: &mut Sink) -> io::Result<()> {
    let mut zip = zip::ZipArchive::new(BufReader::new(File::open(archive)?)).map_err(io::Error::other)?;
    sink.progress.total = (0..zip.len()).filter_map(|i| zip.by_index_raw(i).ok().map(|e| e.size())).sum();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(io::Error::other)?;
        let name = entry.name().to_string();
        if entry.is_dir() {
            sink.dir(&name)?;
        } else {
            let modified = entry.last_modified().and_then(zip_time);
            sink.file(&name, &mut entry, modified, true)?;
        }
    }
    Ok(())
}

fn extract_7z(archive: &Path, sink: &mut Sink) -> io::Result<()> {
    let mut reader = sevenz_rust2::ArchiveReader::open(archive, sevenz_rust2::Password::empty()).map_err(io::Error::other)?;
    sink.progress.total = reader.archive().files.iter().map(|f| f.size).sum();
    let mut failure: Option<io::Error> = None;
    let result = reader.for_each_entries(|entry, data| {
        let outcome = if entry.is_directory {
            sink.dir(&entry.name)
        } else {
            let modified = entry.has_last_modified_date.then(|| SystemTime::from(entry.last_modified_date));
            sink.file(&entry.name, data, modified, true)
        };
        match outcome {
            Ok(()) => Ok(true),
            Err(e) => {
                failure = Some(e);
                Ok(false)
            }
        }
    });
    if let Some(e) = failure {
        return Err(e);
    }
    result.map_err(io::Error::other)
}

/// A tar stream: reads 512-byte headers (ustar, GNU long names, pax paths)
/// and hands regular files and folders to the sink. Links are skipped.
fn extract_tar(mut input: impl Read, sink: &mut Sink) -> io::Result<()> {
    let mut header = [0u8; 512];
    let mut long_name: Option<String> = None;
    loop {
        if !read_block(&mut input, &mut header)? {
            return Ok(());
        }
        if header.iter().all(|&b| b == 0) {
            return Ok(());
        }
        let field = |range: std::ops::Range<usize>| {
            let raw = &header[range];
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            String::from_utf8_lossy(&raw[..end]).into_owned()
        };
        let size = tar_number(&header[124..136]);
        let mtime = tar_number(&header[136..148]);
        let kind = header[156];
        let mut name = field(0..100);
        if &header[257..262] == b"ustar" {
            let prefix = field(345..500);
            if !prefix.is_empty() {
                name = format!("{prefix}/{name}");
            }
        }
        if let Some(long) = long_name.take() {
            name = long;
        }
        let padded = size.div_ceil(512) * 512;
        match kind {
            b'L' | b'x' => {
                // GNU long name, or a pax header with a `path=` record.
                let mut data = vec![0u8; padded as usize];
                input.read_exact(&mut data)?;
                data.truncate(size as usize);
                long_name = if kind == b'L' {
                    Some(String::from_utf8_lossy(&data).trim_end_matches('\0').to_string())
                } else {
                    pax_path(&data)
                };
            }
            b'5' => {
                sink.dir(&name)?;
                skip(&mut input, padded)?;
            }
            b'0' | 0 | b'7' => {
                let modified = (mtime > 0).then(|| SystemTime::UNIX_EPOCH + Duration::from_secs(mtime));
                let mut limited = (&mut input).take(size);
                sink.file(&name, &mut limited, modified, false)?;
                io::copy(&mut limited, &mut io::sink())?;
                skip(&mut input, padded - size)?;
            }
            _ => skip(&mut input, padded)?,
        }
    }
}

/// Reads a whole block, or returns false at a clean end of the stream.
fn read_block(input: &mut impl Read, block: &mut [u8; 512]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < block.len() {
        let n = input.read(&mut block[filled..])?;
        if n == 0 {
            return if filled == 0 { Ok(false) } else { Err(io::ErrorKind::UnexpectedEof.into()) };
        }
        filled += n;
    }
    Ok(true)
}

fn skip(input: &mut impl Read, bytes: u64) -> io::Result<()> {
    io::copy(&mut input.take(bytes), &mut io::sink())?;
    Ok(())
}

/// A tar number: octal text, or big-endian binary when the top bit is set.
fn tar_number(field: &[u8]) -> u64 {
    if field.first().is_some_and(|&b| b & 0x80 != 0) {
        return field[1..].iter().fold(0u64, |n, &b| (n << 8) | b as u64);
    }
    let text: String = field.iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
    u64::from_str_radix(text.trim(), 8).unwrap_or(0)
}

fn pax_path(data: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(data);
    text.lines().find_map(|line| {
        let record = line.split_once(' ')?.1;
        record.strip_prefix("path=").map(str::to_string)
    })
}

fn open_decompressed(path: &Path, kind: ArchiveKind, count: &Arc<AtomicU64>) -> io::Result<Box<dyn Read>> {
    let file = Counting { inner: BufReader::with_capacity(COPY_BUFFER, File::open(path)?), count: count.clone() };
    Ok(match kind {
        ArchiveKind::Tar => Box::new(file),
        ArchiveKind::TarGz | ArchiveKind::Gz => Box::new(flate2::read::MultiGzDecoder::new(file)),
        ArchiveKind::TarBz2 | ArchiveKind::Bz2 => Box::new(bzip2::read::MultiBzDecoder::new(file)),
        ArchiveKind::TarXz | ArchiveKind::Xz => Box::new(lzma_rust2::XzReader::new(file, true)),
        _ => return Err(io::Error::other("not a stream format")),
    })
}

/// Runs 7-Zip into a hidden temporary folder inside `base`, then moves the
/// results up (numbering names already used).
fn extract_external(archive: &Path, sink: &mut Sink) -> io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let exe = seven_zip_exe().ok_or_else(|| io::Error::other("7-Zip isn't installed"))?;
    let temp = unique_path(&sink.base.join(".eden-extracting"));
    std::fs::create_dir_all(&temp)?;
    let result = (|| {
        let mut child = std::process::Command::new(exe)
            .arg("x")
            .arg("-y")
            .arg("-bsp1")
            .arg("-bso0")
            .arg(format!("-o{}", temp.display()))
            .arg(archive)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()?;
        // Progress lines look like " 42% 12 - name".
        let mut stdout = child.stdout.take().ok_or_else(|| io::Error::other("no output"))?;
        sink.progress.total = 100;
        let mut buf = [0u8; 4096];
        loop {
            if sink.cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::other(CANCELLED));
            }
            let n = stdout.read(&mut buf)?;
            if n == 0 {
                break;
            }
            let text = String::from_utf8_lossy(&buf[..n]);
            if let Some(pct) = text.rsplit('%').nth(1).and_then(|s| s.split_whitespace().last()).and_then(|s| s.parse::<u64>().ok()) {
                sink.progress.done = pct.min(100);
                sink.report(false);
            }
        }
        let status = child.wait()?;
        if !status.success() {
            let mut err = String::new();
            if let Some(mut stderr) = child.stderr.take() {
                let _ = stderr.read_to_string(&mut err);
            }
            return Err(io::Error::other(if err.trim().is_empty() { format!("7-Zip failed ({status})") } else { err.trim().to_string() }));
        }
        for entry in std::fs::read_dir(&temp)?.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let target = unique_path(&sink.base.join(&name));
            std::fs::rename(entry.path(), &target)?;
            sink.created.push(target);
            sink.progress.files += 1;
        }
        sink.progress.done = 100;
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&temp);
    result
}

/// Where Extract To puts the results: a new folder named after the archive.
pub fn folder_for(archive: &Path, parent: &Path) -> PathBuf {
    let name = archive.file_name().and_then(|n| n.to_str()).unwrap_or("Extracted");
    let stem = ArchiveKind::of(name).map(|(_, stem)| stem).unwrap_or(name);
    unique_path(&parent.join(clean_component(stem)))
}

/// Extracts `archive` (all of it, or only the entries under `only`) and
/// returns what it created. Removes everything it made if it fails or is
/// cancelled (error text "").
pub fn extract(
    archive: &Path,
    target: &Target,
    only: Option<Vec<Vec<String>>>,
    tx: &Sender<ExtractEvent>,
    cancel: &AtomicBool,
) -> Result<ExtractSummary, String> {
    let name = archive.file_name().and_then(|n| n.to_str()).ok_or("no file name")?;
    let (kind, stem) = ArchiveKind::of(name).ok_or("not a known archive type")?;
    let parent = archive.parent().unwrap_or(Path::new(".")).to_path_buf();
    let (base, own_folder) = match target {
        Target::Here => (parent, None),
        Target::Into(dir) => (dir.clone(), None),
        Target::OwnFolder => {
            let folder = folder_for(archive, &parent);
            (folder.clone(), Some(folder))
        }
        Target::FolderIn(dir) => {
            let folder = folder_for(archive, dir);
            (folder.clone(), Some(folder))
        }
    };
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let count = Arc::new(AtomicU64::new(0));
    let stream_format = matches!(
        kind,
        ArchiveKind::Tar | ArchiveKind::TarGz | ArchiveKind::TarBz2 | ArchiveKind::TarXz | ArchiveKind::Gz | ArchiveKind::Bz2 | ArchiveKind::Xz
    );
    let mut sink = Sink {
        base: base.clone(),
        tops: HashMap::new(),
        created: Vec::new(),
        only,
        progress: ExtractProgress::default(),
        counted: stream_format.then_some(count.as_ref()),
        last: Instant::now(),
        tx,
        cancel,
        buffer: vec![0u8; COPY_BUFFER],
    };
    if stream_format {
        sink.progress.total = std::fs::metadata(archive).map(|m| m.len()).unwrap_or(0);
    }
    sink.report(true);

    let result = match kind {
        ArchiveKind::Zip => extract_zip(archive, &mut sink),
        ArchiveKind::SevenZip => extract_7z(archive, &mut sink),
        ArchiveKind::External => extract_external(archive, &mut sink),
        ArchiveKind::Tar | ArchiveKind::TarGz | ArchiveKind::TarBz2 | ArchiveKind::TarXz => {
            open_decompressed(archive, kind, &count).and_then(|reader| extract_tar(reader, &mut sink))
        }
        ArchiveKind::Gz | ArchiveKind::Bz2 | ArchiveKind::Xz => open_decompressed(archive, kind, &count).and_then(|mut reader| {
            let modified = std::fs::metadata(archive).and_then(|m| m.modified()).ok();
            sink.file(stem, &mut reader, modified, false)
        }),
    };
    match result {
        Ok(()) => {
            sink.progress.done = sink.progress.total;
            sink.report(true);
            let created = match own_folder {
                Some(folder) => vec![folder],
                None => sink.created.clone(),
            };
            Ok(ExtractSummary { folder: base, created, files: sink.progress.files, bytes: sink.progress.done })
        }
        Err(e) => {
            sink.undo();
            if let Some(folder) = own_folder {
                let _ = std::fs::remove_dir_all(folder);
            }
            let text = e.to_string();
            Err(if text == CANCELLED || cancel.load(Ordering::Relaxed) { String::new() } else { text })
        }
    }
}

pub fn start_extract(archive: PathBuf, target: Target, only: Option<Vec<Vec<String>>>) -> ExtractHandle {
    let (tx, rx) = crossbeam_channel::unbounded();
    let cancel = Arc::new(AtomicBool::new(false));
    let thread_cancel = cancel.clone();
    std::thread::spawn(move || {
        let result = extract(&archive, &target, only, &tx, &thread_cancel);
        let _ = tx.send(ExtractEvent::Finished(result));
        crate::gui::windows::windowsoverrides::request_repaint();
    });
    ExtractHandle { rx, cancel }
}

/// One entry of an archive's listing (for browsing it like a folder).
#[derive(Clone, Debug, PartialEq)]
pub struct ArchiveItem {
    /// `folder/sub/name`, cleaned (see `safe_parts`).
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// Every entry in `archive`, with the folders implied by the file paths
/// added (many archives don't store folder entries). Stream formats (tar,
/// .gz) are read through once.
pub fn list(archive: &Path) -> Result<Vec<ArchiveItem>, String> {
    let name = archive.file_name().and_then(|n| n.to_str()).ok_or("no file name")?;
    let (kind, stem) = ArchiveKind::of(name).ok_or("not a known archive type")?;
    list_as(archive, kind, stem)
}

/// `list` for an archive whose type is already known (a `.jar` or `.apk`
/// is a zip); `stem` names the file inside a single `.gz`/`.bz2`/`.xz`.
pub fn list_as(archive: &Path, kind: ArchiveKind, stem: &str) -> Result<Vec<ArchiveItem>, String> {
    let mut items: Vec<ArchiveItem> = Vec::new();
    let mut push = |raw: &str, is_dir: bool, size: u64, modified: Option<SystemTime>| {
        if let Some(parts) = safe_parts(raw) {
            items.push(ArchiveItem { path: parts.join("/"), is_dir, size, modified });
        }
    };
    match kind {
        ArchiveKind::Zip => {
            let mut zip = zip::ZipArchive::new(BufReader::new(File::open(archive).map_err(|e| e.to_string())?)).map_err(|e| e.to_string())?;
            for i in 0..zip.len() {
                let Ok(entry) = zip.by_index_raw(i) else { continue };
                push(entry.name(), entry.is_dir(), entry.size(), entry.last_modified().and_then(zip_time));
            }
        }
        ArchiveKind::SevenZip => {
            let parsed = sevenz_rust2::Archive::open(archive).map_err(|e| e.to_string())?;
            for f in &parsed.files {
                push(&f.name, f.is_directory, f.size, f.has_last_modified_date.then(|| SystemTime::from(f.last_modified_date)));
            }
        }
        ArchiveKind::Tar | ArchiveKind::TarGz | ArchiveKind::TarBz2 | ArchiveKind::TarXz => {
            let count = Arc::new(AtomicU64::new(0));
            let mut input = open_decompressed(archive, kind, &count).map_err(|e| e.to_string())?;
            list_tar(&mut input, &mut push).map_err(|e| e.to_string())?;
        }
        ArchiveKind::Gz | ArchiveKind::Bz2 | ArchiveKind::Xz => {
            let modified = std::fs::metadata(archive).and_then(|m| m.modified()).ok();
            push(stem, false, 0, modified);
        }
        ArchiveKind::External => {
            for (path, is_dir, size) in list_external(archive)? {
                push(&path, is_dir, size, None);
            }
        }
    }
    // Add missing parent folders, drop duplicates.
    let mut seen: std::collections::HashSet<String> = items.iter().map(|i| i.path.clone()).collect();
    let mut folders = Vec::new();
    for item in &items {
        let mut path = item.path.as_str();
        while let Some((parent, _)) = path.rsplit_once('/') {
            if seen.insert(parent.to_string()) {
                folders.push(ArchiveItem { path: parent.to_string(), is_dir: true, size: 0, modified: None });
            }
            path = parent;
        }
    }
    items.extend(folders);
    items.sort_by(|a, b| a.path.cmp(&b.path));
    items.dedup_by(|a, b| a.path == b.path);
    Ok(items)
}

/// Lists an archive with 7-Zip (`7z l -slt`): path, folder or not, size.
fn list_external(archive: &Path) -> Result<Vec<(String, bool, u64)>, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let exe = seven_zip_exe().ok_or("7-Zip isn't installed")?;
    let output = std::process::Command::new(exe)
        .args(["l", "-slt", "-sccUTF-8", "--"])
        .arg(archive)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(parse_7z_listing(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_7z_listing(text: &str) -> Vec<(String, bool, u64)> {
    // The entries follow a "----------" line, one "Key = value" block each.
    let Some((_, body)) = text.split_once("\n----------") else { return Vec::new() };
    let mut out = Vec::new();
    let mut current: Option<(String, bool, u64)> = None;
    for line in body.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(path) = line.strip_prefix("Path = ") {
            out.extend(current.take());
            current = Some((path.replace('\\', "/"), false, 0));
        } else if let Some(entry) = current.as_mut() {
            if let Some(size) = line.strip_prefix("Size = ") {
                entry.2 = size.trim().parse().unwrap_or(0);
            } else if line == "Folder = +" {
                entry.1 = true;
            } else if let Some(attributes) = line.strip_prefix("Attributes = ") {
                entry.1 |= attributes.starts_with('D');
            }
        }
    }
    out.extend(current);
    out
}

fn list_tar(input: &mut dyn Read, push: &mut dyn FnMut(&str, bool, u64, Option<SystemTime>)) -> io::Result<()> {
    let mut header = [0u8; 512];
    let mut long_name: Option<String> = None;
    let mut reader = input;
    loop {
        if !read_block(&mut reader, &mut header)? || header.iter().all(|&b| b == 0) {
            return Ok(());
        }
        let field = |range: std::ops::Range<usize>| {
            let raw = &header[range];
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            String::from_utf8_lossy(&raw[..end]).into_owned()
        };
        let size = tar_number(&header[124..136]);
        let mtime = tar_number(&header[136..148]);
        let kind = header[156];
        let mut name = field(0..100);
        if &header[257..262] == b"ustar" {
            let prefix = field(345..500);
            if !prefix.is_empty() {
                name = format!("{prefix}/{name}");
            }
        }
        if let Some(long) = long_name.take() {
            name = long;
        }
        let padded = size.div_ceil(512) * 512;
        match kind {
            b'L' | b'x' => {
                let mut data = vec![0u8; padded as usize];
                reader.read_exact(&mut data)?;
                data.truncate(size as usize);
                long_name = if kind == b'L' {
                    Some(String::from_utf8_lossy(&data).trim_end_matches('\0').to_string())
                } else {
                    pax_path(&data)
                };
            }
            b'5' => {
                push(&name, true, 0, None);
                skip(&mut reader, padded)?;
            }
            b'0' | 0 | b'7' => {
                push(&name, false, size, (mtime > 0).then(|| SystemTime::UNIX_EPOCH + Duration::from_secs(mtime)));
                skip(&mut reader, padded)?;
            }
            _ => skip(&mut reader, padded)?,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn reads_7z_listings() {
        let text = "7-Zip 24.08\r\n\r\n--\r\nPath = C:\\x.rar\r\nType = Rar\r\n\r\n----------\r\nPath = docs\\a.txt\r\nFolder = -\r\nSize = 12\r\n\r\nPath = docs\r\nFolder = +\r\nSize = 0\r\n\r\nPath = b\r\nSize = 3\r\nAttributes = D....\r\n";
        assert_eq!(
            parse_7z_listing(text),
            vec![("docs/a.txt".to_string(), false, 12), ("docs".to_string(), true, 0), ("b".to_string(), true, 3)]
        );
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eden-extract-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn run(archive: &Path, target: Target, only: Option<Vec<Vec<String>>>) -> Result<ExtractSummary, String> {
        let (tx, _rx) = crossbeam_channel::unbounded();
        extract(archive, &target, only, &tx, &AtomicBool::new(false))
    }

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, data) in entries {
            if name.ends_with('/') {
                zip.add_directory(*name, zip::write::SimpleFileOptions::default()).unwrap();
            } else {
                zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
                zip.write_all(data).unwrap();
            }
        }
        zip.finish().unwrap();
    }

    /// A minimal ustar archive.
    pub(crate) fn make_tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, data) in entries {
            let mut header = [0u8; 512];
            let is_dir = name.ends_with('/');
            header[..name.len()].copy_from_slice(name.as_bytes());
            header[100..107].copy_from_slice(b"0000644");
            header[124..135].copy_from_slice(format!("{:011o}", data.len()).as_bytes());
            header[136..147].copy_from_slice(b"14000000000");
            header[156] = if is_dir { b'5' } else { b'0' };
            header[257..263].copy_from_slice(b"ustar\0");
            header[148..156].copy_from_slice(b"        ");
            let sum: u32 = header.iter().map(|&b| b as u32).sum();
            header[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
            out.extend_from_slice(&header);
            out.extend_from_slice(data);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        out.extend_from_slice(&[0u8; 1024]);
        out
    }

    #[test]
    fn names_and_kinds() {
        assert_eq!(ArchiveKind::of("Backup.TAR.GZ"), Some((ArchiveKind::TarGz, "Backup")));
        assert_eq!(ArchiveKind::of("a.tgz"), Some((ArchiveKind::TarGz, "a")));
        assert_eq!(ArchiveKind::of("notes.txt.gz"), Some((ArchiveKind::Gz, "notes.txt")));
        assert_eq!(ArchiveKind::of("x.zip"), Some((ArchiveKind::Zip, "x")));
        assert_eq!(ArchiveKind::of("movie.rar").map(|k| k.0), Some(ArchiveKind::External));
        assert_eq!(ArchiveKind::of("readme.md"), None);
        assert_eq!(ArchiveKind::of(".zip"), None);
    }

    #[test]
    fn zip_times_match_a_direct_local_time_conversion() {
        use chrono::TimeZone;
        for (y, mo, d, h, mi) in [(2024, 6, 1, 12, 30), (2024, 1, 15, 3, 0), (2019, 10, 27, 23, 59)] {
            let zt = zip::DateTime::from_date_and_time(y, mo, d, h, mi, 0).unwrap();
            let naive = chrono::NaiveDate::from_ymd_opt(y as i32, mo as u32, d as u32)
                .unwrap()
                .and_hms_opt(h as u32, mi as u32, 0)
                .unwrap();
            let expected: SystemTime = chrono::Local.from_local_datetime(&naive).single().unwrap().into();
            assert_eq!(zip_time(zt), Some(expected));
            assert_eq!(zip_time(zt), Some(expected), "cached");
        }
    }

    #[test]
    fn unsafe_names_are_refused() {
        assert_eq!(safe_parts("a/b/c.txt"), Some(vec!["a".into(), "b".into(), "c.txt".into()]));
        assert_eq!(safe_parts(r"a\b"), Some(vec!["a".into(), "b".into()]));
        assert_eq!(safe_parts("./a"), Some(vec!["a".into()]));
        assert_eq!(safe_parts("../evil"), None);
        assert_eq!(safe_parts("a/../../evil"), None);
        assert_eq!(safe_parts("/etc/passwd"), None);
        assert_eq!(safe_parts("C:/Windows/x"), None);
        assert_eq!(safe_parts("what?.txt"), Some(vec!["what_.txt".into()]));
    }

    #[test]
    fn zip_to_own_folder_and_here_never_overwrite() {
        let dir = scratch("zip");
        let archive = dir.join("Photos.zip");
        make_zip(&archive, &[("Trip/", b""), ("Trip/a.jpg", b"AAAA"), ("readme.txt", b"hi"), ("../evil.txt", b"x")]);

        let summary = run(&archive, Target::OwnFolder, None).unwrap();
        assert_eq!(summary.folder, dir.join("Photos"));
        assert_eq!(std::fs::read(dir.join("Photos/Trip/a.jpg")).unwrap(), b"AAAA");
        assert_eq!(summary.files, 2);
        assert!(!dir.join("evil.txt").exists());

        // Again: a second, numbered folder.
        let again = run(&archive, Target::OwnFolder, None).unwrap();
        assert_eq!(again.folder, dir.join("Photos (2)"));

        // Here, twice: the second time the names are numbered.
        run(&archive, Target::Here, None).unwrap();
        let second = run(&archive, Target::Here, None).unwrap();
        assert!(dir.join("readme.txt").is_file());
        assert!(dir.join("readme (2).txt").is_file());
        assert!(dir.join("Trip (2)/a.jpg").is_file());
        assert_eq!(second.created.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_selected_entries() {
        let dir = scratch("only");
        let archive = dir.join("a.zip");
        make_zip(&archive, &[("top/sub/x.txt", b"x"), ("top/sub/y.txt", b"y"), ("top/other.txt", b"o")]);
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        run(&archive, Target::Into(out.clone()), Some(vec![vec!["top".into(), "sub".into()]])).unwrap();
        assert!(out.join("sub/x.txt").is_file());
        assert!(out.join("sub/y.txt").is_file());
        assert!(!out.join("other.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tar_gz_bz2_xz_and_single_files() {
        let dir = scratch("tar");
        let tar = make_tar(&[("proj/", b""), ("proj/src/main.rs", b"fn main() {}"), ("proj/README", b"read me")]);
        std::fs::write(dir.join("p.tar"), &tar).unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar).unwrap();
        std::fs::write(dir.join("p2.tar.gz"), gz.finish().unwrap()).unwrap();
        let mut bz = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
        bz.write_all(&tar).unwrap();
        std::fs::write(dir.join("p3.tbz2"), bz.finish().unwrap()).unwrap();
        let mut xz = lzma_rust2::XzWriter::new(Vec::new(), lzma_rust2::XzOptions::default()).unwrap();
        xz.write_all(&tar).unwrap();
        std::fs::write(dir.join("p4.tar.xz"), xz.finish().unwrap()).unwrap();

        for (name, folder) in [("p.tar", "p"), ("p2.tar.gz", "p2"), ("p3.tbz2", "p3"), ("p4.tar.xz", "p4")] {
            let summary = run(&dir.join(name), Target::OwnFolder, None).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(summary.folder, dir.join(folder));
            assert_eq!(std::fs::read(dir.join(folder).join("proj/src/main.rs")).unwrap(), b"fn main() {}", "{name}");
            let listed = list(&dir.join(name)).unwrap();
            let paths: Vec<&str> = listed.iter().map(|i| i.path.as_str()).collect();
            assert_eq!(paths, vec!["proj", "proj/README", "proj/src", "proj/src/main.rs"], "{name}");
        }

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(b"plain text").unwrap();
        std::fs::write(dir.join("notes.txt.gz"), gz.finish().unwrap()).unwrap();
        run(&dir.join("notes.txt.gz"), Target::Here, None).unwrap();
        assert_eq!(std::fs::read(dir.join("notes.txt")).unwrap(), b"plain text");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn seven_zip_round_trip() {
        let dir = scratch("7z");
        let archive = dir.join("docs.7z");
        {
            let mut writer = sevenz_rust2::ArchiveWriter::create(&archive).unwrap();
            writer
                .push_archive_entry(sevenz_rust2::ArchiveEntry::new_file("folder/a.txt"), Some(&b"seven"[..]))
                .unwrap();
            writer.finish().unwrap();
        }
        run(&archive, Target::OwnFolder, None).unwrap();
        assert_eq!(std::fs::read(dir.join("docs/folder/a.txt")).unwrap(), b"seven");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancelling_removes_what_was_made() {
        let dir = scratch("cancel");
        let archive = dir.join("big.zip");
        make_zip(&archive, &[("one.bin", &[1u8; 1000][..]), ("two.bin", &[2u8; 1000][..])]);
        let (tx, _rx) = crossbeam_channel::unbounded();
        let cancel = AtomicBool::new(true);
        assert_eq!(extract(&archive, &Target::OwnFolder, None, &tx, &cancel), Err(String::new()));
        assert!(!dir.join("big").exists());
        assert!(archive.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_archives_fail_cleanly() {
        let dir = scratch("corrupt");
        std::fs::write(dir.join("bad.zip"), b"not a zip").unwrap();
        assert!(run(&dir.join("bad.zip"), Target::OwnFolder, None).is_err());
        assert!(!dir.join("bad").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
