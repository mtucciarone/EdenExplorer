use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use parking_lot::RwLock;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use windows::core::{Error, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, GENERIC_READ};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::{
    FSCTL_ENUM_USN_DATA, FSCTL_GET_NTFS_FILE_RECORD, FSCTL_QUERY_USN_JOURNAL,
    FSCTL_READ_USN_JOURNAL, MFT_ENUM_DATA_V0, NTFS_FILE_RECORD_INPUT_BUFFER,
    NTFS_FILE_RECORD_OUTPUT_BUFFER, READ_USN_JOURNAL_DATA_V0,
    USN_JOURNAL_DATA_V0,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileRecord {
    pub file_ref: u64,
    pub parent_ref: u64,
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
}

#[derive(Serialize, Deserialize)]
struct CacheSnapshot {
    journal_id: u64,
    last_usn: i64,
    records: Vec<FileRecord>,
    folder_sizes: Vec<(u64, u64)>,
}

#[derive(Serialize, Deserialize)]
struct FavoritesSnapshot {
    favorites: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum IndexStatus {
    Building,
    Ready,
    Error,
}

pub struct Indexer {
    drive: char,
    handle: HANDLE,
    records: DashMap<u64, FileRecord>,
    children: DashMap<u64, Vec<u64>>,
    folder_sizes: DashMap<u64, u64>,
    path_cache: DashMap<u64, PathBuf>,
    all_ids: RwLock<Vec<u64>>,
    status: RwLock<IndexStatus>,
    last_usn: AtomicI64,
    dirty: AtomicBool,
}

impl Indexer {
    pub fn start(drive: char) -> Arc<Self> {
        let handle = open_volume(drive).unwrap_or_else(|_| HANDLE::default());
        let indexer = Arc::new(Self {
            drive,
            handle,
            records: DashMap::new(),
            children: DashMap::new(),
            folder_sizes: DashMap::new(),
            path_cache: DashMap::new(),
            all_ids: RwLock::new(Vec::new()),
            status: RwLock::new(IndexStatus::Building),
            last_usn: AtomicI64::new(0),
            dirty: AtomicBool::new(false),
        });

        let indexer_clone = indexer.clone();
        std::thread::spawn(move || {
            if let Err(_) = indexer_clone.build_or_load_index() {
                *indexer_clone.status.write() = IndexStatus::Error;
                return;
            }

            *indexer_clone.status.write() = IndexStatus::Ready;
            indexer_clone.start_usn_listener();
        });

        indexer
    }

    pub fn status(&self) -> IndexStatus {
        *self.status.read()
    }

    pub fn search(&self, query: &str) -> Vec<FileRecord> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }

        let ids = self.all_ids.read().clone();
        ids.par_iter()
            .filter_map(|id| self.records.get(id).map(|r| r.clone()))
            .filter(|rec| rec.name.to_lowercase().contains(&q))
            .take_any(2000)
            .collect()
    }

    #[allow(dead_code)]
    pub fn get_children(&self, path: &Path) -> Vec<FileRecord> {
        let ref_id = match self.lookup_file_ref(path) {
            Some(id) => id,
            None => return Vec::new(),
        };

        let ids = match self.children.get(&ref_id) {
            Some(ids) => ids.clone(),
            None => return Vec::new(),
        };

        ids.into_iter()
            .filter_map(|id| self.records.get(&id).map(|r| r.clone()))
            .collect()
    }

    pub fn get_folder_size(&self, path: &Path) -> Option<u64> {
        let ref_id = self.lookup_file_ref(path)?;
        self.folder_sizes.get(&ref_id).map(|v| *v)
    }

    pub fn get_path(&self, file_ref: u64) -> Option<PathBuf> {
        if let Some(path) = self.path_cache.get(&file_ref) {
            return Some(path.clone());
        }

        let mut segments = Vec::new();
        let mut current = file_ref;
        let mut safety = 0;
        while safety < 1024 {
            safety += 1;
            let record = self.records.get(&current)?;
            segments.push(record.name.clone());
            let parent = record.parent_ref;
            if parent == current {
                break;
            }
            current = parent;
        }

        if segments.is_empty() {
            return None;
        }

        segments.reverse();
        let mut path = PathBuf::new();
        path.push(format!("{}:\\", self.drive));
        for seg in segments {
            path.push(seg);
        }

        self.path_cache.insert(file_ref, path.clone());
        Some(path)
    }

    fn build_or_load_index(&self) -> Result<(), Error> {
        if self.handle == HANDLE::default() {
            return Err(Error::from_win32());
        }

        let journal = query_usn_journal(self.handle)?;
        if let Some(snapshot) = load_cache(self.drive) {
            if snapshot.journal_id == journal.UsnJournalID {
                let last_usn = snapshot.last_usn;
                self.apply_snapshot(snapshot);
                self.last_usn.store(last_usn, Ordering::Relaxed);
                return Ok(());
            }
        }

        self.build_index(journal.NextUsn)?;
        save_cache(self.drive, self.snapshot(journal.UsnJournalID));
        Ok(())
    }

    fn apply_snapshot(&self, snapshot: CacheSnapshot) {
        self.records.clear();
        self.children.clear();
        self.folder_sizes.clear();
        self.path_cache.clear();

        let mut ids = Vec::with_capacity(snapshot.records.len());
        for record in snapshot.records {
            ids.push(record.file_ref);
            self.records.insert(record.file_ref, record);
        }
        *self.all_ids.write() = ids;

        for (dir, size) in snapshot.folder_sizes {
            self.folder_sizes.insert(dir, size);
        }

        self.rebuild_children();
    }

    fn snapshot(&self, journal_id: u64) -> CacheSnapshot {
        let records: Vec<FileRecord> = self
            .records
            .iter()
            .map(|r| r.value().clone())
            .collect();
        let folder_sizes: Vec<(u64, u64)> = self
            .folder_sizes
            .iter()
            .map(|r| (*r.key(), *r.value()))
            .collect();

        CacheSnapshot {
            journal_id,
            last_usn: self.last_usn.load(Ordering::Relaxed),
            records,
            folder_sizes,
        }
    }

    fn build_index(&self, high_usn: i64) -> Result<(), Error> {
        self.records.clear();
        self.children.clear();
        self.folder_sizes.clear();
        self.path_cache.clear();

        let mut all_ids = Vec::new();
        enum_mft(self.handle, high_usn, |rec| {
            let file_ref = rec.file_ref;
            let is_dir = rec.is_dir;
            let size = if is_dir {
                0
            } else {
                get_file_size_by_frn(self.handle, file_ref).unwrap_or(0)
            };

            let record = FileRecord {
                file_ref,
                parent_ref: rec.parent_ref,
                name: rec.name,
                size,
                is_dir,
            };

            all_ids.push(file_ref);
            self.records.insert(file_ref, record);
        })?;

        *self.all_ids.write() = all_ids;
        self.rebuild_children();
        self.rebuild_folder_sizes();
        self.last_usn.store(high_usn, Ordering::Relaxed);
        Ok(())
    }

    fn rebuild_children(&self) {
        self.children.clear();
        for entry in self.records.iter() {
            let parent = entry.value().parent_ref;
            self.children
                .entry(parent)
                .or_insert_with(Vec::new)
                .push(entry.value().file_ref);
        }
    }

    fn rebuild_folder_sizes(&self) {
        self.folder_sizes.clear();
        let records: Vec<FileRecord> = self
            .records
            .iter()
            .map(|r| r.value().clone())
            .collect();

        for record in records {
            if record.is_dir || record.size == 0 {
                continue;
            }

            let mut current = record.parent_ref;
            let mut safety = 0;
            while safety < 1024 {
                safety += 1;
                let mut size = self.folder_sizes.entry(current).or_insert(0);
                *size = size.saturating_add(record.size);
                let next = match self.records.get(&current) {
                    Some(parent) => parent.parent_ref,
                    None => break,
                };
                if next == current {
                    break;
                }
                current = next;
            }
        }
    }

    fn lookup_file_ref(&self, path: &Path) -> Option<u64> {
        let mut current_ref = 5u64; // Root directory FRN
        let mut iter = path.iter().peekable();
        let drive = iter.next()?.to_string_lossy().to_string();
        if !drive.to_ascii_lowercase().starts_with(&format!("{}:", self.drive)) {
            return None;
        }

        for part in iter {
            let name = part.to_string_lossy().to_string();
            let children = self.children.get(&current_ref)?;
            let mut found = None;
            for child_ref in children.iter() {
                if let Some(rec) = self.records.get(child_ref) {
                    if rec.name.eq_ignore_ascii_case(&name) {
                        found = Some(*child_ref);
                        break;
                    }
                }
            }
            current_ref = found?;
        }

        Some(current_ref)
    }

    fn start_usn_listener(self: &Arc<Self>) {
        let indexer = self.clone();
        std::thread::spawn(move || {
            let journal = match query_usn_journal(indexer.handle) {
                Ok(j) => j,
                Err(_) => return,
            };

            let mut read_data = READ_USN_JOURNAL_DATA_V0 {
                StartUsn: indexer.last_usn.load(Ordering::Relaxed),
                ReasonMask: u32::MAX,
                ReturnOnlyOnClose: 0,
                Timeout: 0,
                BytesToWaitFor: 0,
                UsnJournalID: journal.UsnJournalID,
            };

            let mut last_save = Instant::now();

            loop {
                let mut buffer = vec![0u8; 1024 * 1024];
                let mut bytes = 0u32;
                let ok = unsafe {
                    DeviceIoControl(
                        indexer.handle,
                        FSCTL_READ_USN_JOURNAL,
                        Some(&mut read_data as *mut _ as _),
                        std::mem::size_of::<READ_USN_JOURNAL_DATA_V0>() as u32,
                        Some(buffer.as_mut_ptr() as _),
                        buffer.len() as u32,
                        Some(&mut bytes as *mut _),
                        None,
                    )
                };

                if ok.is_err() || bytes <= 8 {
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                }

                let next_usn = u64::from_le_bytes(buffer[0..8].try_into().unwrap());
                let mut offset = 8usize;
                while offset < bytes as usize {
                    if let Some(rec) = parse_usn_record(&buffer, offset) {
                        indexer.apply_usn_record(&rec);
                        offset += rec.record_length as usize;
                    } else {
                        break;
                    }
                }

                indexer.last_usn.store(next_usn as i64, Ordering::Relaxed);
                read_data.StartUsn = next_usn as i64;

                if indexer.dirty.swap(false, Ordering::Relaxed)
                    && last_save.elapsed() > Duration::from_secs(5)
                {
                    if let Ok(journal) = query_usn_journal(indexer.handle) {
                        save_cache(indexer.drive, indexer.snapshot(journal.UsnJournalID));
                        last_save = Instant::now();
                    }
                }
            }
        });
    }

    fn apply_usn_record(&self, rec: &ParsedUsnRecord) {
        let file_ref = rec.file_ref;
        let parent_ref = rec.parent_ref;
        let is_dir = rec.is_dir;

        if rec.reason & USN_REASON_FILE_DELETE != 0 {
            if let Some((_, old)) = self.records.remove(&file_ref) {
                self.remove_child(old.parent_ref, file_ref);
                if !old.is_dir {
                    self.apply_size_delta(old.parent_ref, -(old.size as i64));
                }
            }
            self.dirty.store(true, Ordering::Relaxed);
            return;
        }

        let mut size = 0;
        if !is_dir {
            size = get_file_size_by_frn(self.handle, file_ref).unwrap_or(0);
        }

        let prev = self.records.get(&file_ref).map(|r| r.clone());
        let record = FileRecord {
            file_ref,
            parent_ref,
            name: rec.name.clone(),
            size,
            is_dir,
        };

        self.records.insert(file_ref, record.clone());
        self.ensure_child(parent_ref, file_ref);

        if let Some(prev) = prev {
            if prev.parent_ref != parent_ref {
                self.remove_child(prev.parent_ref, file_ref);
                self.ensure_child(parent_ref, file_ref);
            }
            if !is_dir {
                let delta = size as i64 - prev.size as i64;
                if delta != 0 {
                    self.apply_size_delta(parent_ref, delta);
                }
            }
        } else if !is_dir {
            self.apply_size_delta(parent_ref, size as i64);
        }

        self.dirty.store(true, Ordering::Relaxed);
    }

    fn ensure_child(&self, parent: u64, child: u64) {
        let mut entry = self.children.entry(parent).or_insert_with(Vec::new);
        if !entry.contains(&child) {
            entry.push(child);
        }
    }

    fn remove_child(&self, parent: u64, child: u64) {
        if let Some(mut entry) = self.children.get_mut(&parent) {
            entry.retain(|id| *id != child);
        }
    }

    fn apply_size_delta(&self, mut parent: u64, delta: i64) {
        let mut safety = 0;
        while safety < 1024 {
            safety += 1;
            let mut size_entry = self.folder_sizes.entry(parent).or_insert(0);
            if delta >= 0 {
                *size_entry = size_entry.saturating_add(delta as u64);
            } else {
                *size_entry = size_entry.saturating_sub((-delta) as u64);
            }

            let next = match self.records.get(&parent) {
                Some(rec) => rec.parent_ref,
                None => break,
            };
            if next == parent {
                break;
            }
            parent = next;
        }
    }
}

impl Drop for Indexer {
    fn drop(&mut self) {
        if self.handle != HANDLE::default() {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

#[derive(Clone)]
struct ParsedUsnRecord {
    record_length: u32,
    file_ref: u64,
    parent_ref: u64,
    name: String,
    is_dir: bool,
    reason: u32,
}

const USN_REASON_FILE_DELETE: u32 = 0x00000200;

fn parse_usn_record(buffer: &[u8], offset: usize) -> Option<ParsedUsnRecord> {
    if offset + 60 > buffer.len() {
        return None;
    }

    let record_length = u32::from_le_bytes(buffer[offset..offset + 4].try_into().ok()?);
    if record_length == 0 || offset + record_length as usize > buffer.len() {
        return None;
    }

    let file_ref = u64::from_le_bytes(buffer[offset + 8..offset + 16].try_into().ok()?);
    let parent_ref = u64::from_le_bytes(buffer[offset + 16..offset + 24].try_into().ok()?);
    let reason = u32::from_le_bytes(buffer[offset + 40..offset + 44].try_into().ok()?);
    let file_attrs = u32::from_le_bytes(buffer[offset + 52..offset + 56].try_into().ok()?);
    let name_len = u16::from_le_bytes(buffer[offset + 56..offset + 58].try_into().ok()?);
    let name_off = u16::from_le_bytes(buffer[offset + 58..offset + 60].try_into().ok()?);

    let name_start = offset + name_off as usize;
    let name_end = name_start + name_len as usize;
    if name_end > buffer.len() {
        return None;
    }

    let name_u16: Vec<u16> = buffer[name_start..name_end]
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    let name = String::from_utf16_lossy(&name_u16);

    Some(ParsedUsnRecord {
        record_length,
        file_ref,
        parent_ref,
        name,
        is_dir: (file_attrs & FILE_ATTRIBUTE_DIRECTORY.0) != 0,
        reason,
    })
}

struct EnumRecord {
    file_ref: u64,
    parent_ref: u64,
    name: String,
    is_dir: bool,
}

fn enum_mft(
    handle: HANDLE,
    high_usn: i64,
    mut on_record: impl FnMut(EnumRecord),
) -> Result<(), Error> {
    let mut data = MFT_ENUM_DATA_V0 {
        StartFileReferenceNumber: 0,
        LowUsn: 0,
        HighUsn: high_usn,
    };

    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let mut bytes = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                handle,
                FSCTL_ENUM_USN_DATA,
                Some(&mut data as *mut _ as _),
                std::mem::size_of::<MFT_ENUM_DATA_V0>() as u32,
                Some(buffer.as_mut_ptr() as _),
                buffer.len() as u32,
                Some(&mut bytes as *mut _),
                None,
            )
        };

        if ok.is_err() || bytes <= 8 {
            break;
        }

        let next_start = u64::from_le_bytes(buffer[0..8].try_into().unwrap());
        let mut offset = 8usize;
        while offset < bytes as usize {
            if let Some(rec) = parse_usn_record(&buffer, offset) {
                on_record(EnumRecord {
                    file_ref: rec.file_ref,
                    parent_ref: rec.parent_ref,
                    name: rec.name,
                    is_dir: rec.is_dir,
                });
                offset += rec.record_length as usize;
            } else {
                break;
            }
        }

        data.StartFileReferenceNumber = next_start;
    }

    Ok(())
}

fn query_usn_journal(handle: HANDLE) -> Result<USN_JOURNAL_DATA_V0, Error> {
    let mut data = USN_JOURNAL_DATA_V0::default();
    let mut bytes = 0u32;
    let ok = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_QUERY_USN_JOURNAL,
            None,
            0,
            Some(&mut data as *mut _ as _),
            std::mem::size_of::<USN_JOURNAL_DATA_V0>() as u32,
            Some(&mut bytes as *mut _),
            None,
        )
    };

    if ok.is_ok() {
        Ok(data)
    } else {
        Err(Error::from_win32())
    }
}

fn open_volume(drive: char) -> Result<HANDLE, Error> {
    let path = format!("\\\\.\\{}:", drive);
    let wide: Vec<u16> = OsString::from(path).encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }?;
    Ok(handle)
}

#[repr(C)]
struct FileRecordHeader {
    signature: u32,
    _fixup_offset: u16,
    _fixup_count: u16,
    _lsn: u64,
    _sequence: u16,
    _hard_links: u16,
    first_attribute_offset: u16,
    _flags: u16,
    bytes_in_use: u32,
    _bytes_allocated: u32,
    _base_file_record: u64,
    _next_attr_id: u16,
    _align: u16,
    _mft_record_number: u32,
}

#[repr(C)]
struct AttrHeaderCommon {
    type_code: u32,
    length: u32,
    non_resident: u8,
    name_length: u8,
    name_offset: u16,
    flags: u16,
    attr_id: u16,
}

#[repr(C)]
struct AttrHeaderResident {
    common: AttrHeaderCommon,
    value_length: u32,
    value_offset: u16,
    _resident_flags: u8,
    _reserved: u8,
}

#[repr(C)]
struct AttrHeaderNonResident {
    common: AttrHeaderCommon,
    _lowest_vcn: u64,
    _highest_vcn: u64,
    _mapping_pairs_offset: u16,
    _compression_unit: u8,
    _reserved: [u8; 5],
    _allocated_size: u64,
    data_size: u64,
    _initialized_size: u64,
    _compressed_size: u64,
}

fn get_file_size_by_frn(handle: HANDLE, file_ref: u64) -> Option<u64> {
    let mut input = NTFS_FILE_RECORD_INPUT_BUFFER {
        FileReferenceNumber: file_ref as i64,
    };

    let mut out = vec![0u8; 1024 * 4];
    let mut bytes = 0u32;
    let ok = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_GET_NTFS_FILE_RECORD,
            Some(&mut input as *mut _ as _),
            std::mem::size_of::<NTFS_FILE_RECORD_INPUT_BUFFER>() as u32,
            Some(out.as_mut_ptr() as _),
            out.len() as u32,
            Some(&mut bytes as *mut _),
            None,
        )
    };
    if ok.is_err() {
        return None;
    }

    let output = unsafe {
        &*(out.as_ptr() as *const NTFS_FILE_RECORD_OUTPUT_BUFFER)
    };

    let record = unsafe {
        std::slice::from_raw_parts(
            output.FileRecordBuffer.as_ptr(),
            output.FileRecordLength as usize,
        )
    };

    if record.len() < std::mem::size_of::<FileRecordHeader>() {
        return None;
    }

    let header = unsafe {
        &*(record.as_ptr() as *const FileRecordHeader)
    };

    if header.signature != 0x454C4946 {
        return None;
    }

    let mut offset = header.first_attribute_offset as usize;
    while offset + std::mem::size_of::<AttrHeaderCommon>() <= record.len() {
        let common = unsafe {
            &*(record.as_ptr().add(offset) as *const AttrHeaderCommon)
        };

        if common.type_code == 0xFFFFFFFF {
            break;
        }

        if common.length == 0 {
            break;
        }

        if common.type_code == 0x80 {
            if common.non_resident == 0 {
                if offset + std::mem::size_of::<AttrHeaderResident>() > record.len() {
                    return None;
                }
                let resident = unsafe {
                    &*(record.as_ptr().add(offset) as *const AttrHeaderResident)
                };
                return Some(resident.value_length as u64);
            } else {
                if offset + std::mem::size_of::<AttrHeaderNonResident>() > record.len() {
                    return None;
                }
                let non_resident = unsafe {
                    &*(record.as_ptr().add(offset) as *const AttrHeaderNonResident)
                };
                return Some(non_resident.data_size);
            }
        }

        offset = offset.saturating_add(common.length as usize);
    }

    None
}

fn cache_path(drive: char) -> Option<PathBuf> {
    let base = dirs::data_local_dir()?;
    Some(base.join("ExplorerEden").join(format!("index_{}.bin", drive)))
}

fn favorites_cache_path(drive: char) -> Option<PathBuf> {
    let base = dirs::data_local_dir()?;
    Some(base.join("ExplorerEden").join(format!("favorites_{}.bin", drive)))
}

fn load_cache(drive: char) -> Option<CacheSnapshot> {
    let path = cache_path(drive)?;
    let data = std::fs::read(path).ok()?;
    bincode::deserialize::<CacheSnapshot>(&data).ok()
}

fn save_cache(drive: char, snapshot: CacheSnapshot) {
    let path = match cache_path(drive) {
        Some(path) => path,
        None => return,
    };
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    if let Ok(data) = bincode::serialize(&snapshot) {
        let _ = std::fs::write(path, data);
    }
}

pub fn load_favorites(drive: char) -> Vec<String> {
    let path = match favorites_cache_path(drive) {
        Some(path) => path,
        None => return Vec::new(),
    };
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(_) => return Vec::new(),
    };
    match bincode::deserialize::<FavoritesSnapshot>(&data) {
        Ok(snapshot) => snapshot.favorites,
        Err(_) => Vec::new(),
    }
}

pub fn save_favorites(drive: char, favorites: &[String]) {
    let path = match favorites_cache_path(drive) {
        Some(path) => path,
        None => return,
    };
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let snapshot = FavoritesSnapshot {
        favorites: favorites.to_vec(),
    };
    if let Ok(data) = bincode::serialize(&snapshot) {
        let _ = std::fs::write(path, data);
    }
}
