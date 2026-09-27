//! The fast whole-drive scan behind "Analyze Disk Usage…": reads an NTFS
//! volume's Master File Table straight off the disk (the way WizTree does)
//! instead of listing every folder one by one. Every file and folder on the
//! volume has a fixed-size record in the MFT holding its name, its parent
//! folder, and its size, so one sequential read of the MFT is enough to
//! rebuild the whole tree - typically a few seconds for a full drive.
//!
//! Opening a raw volume needs administrator rights; `core::disk_usage`
//! only calls in here when the process is elevated and the drive is NTFS,
//! and falls back to its folder-by-folder scan if anything here fails.
//!
//! The parsing (`MftTable`, `apply_fixup`, `decode_data_runs`, ...) is
//! plain byte handling with no Windows calls, so it's unit tested with
//! hand-built records; only `scan_volume` touches the disk.

use crate::core::disk_usage::{DirNode, FileEntry};
use std::sync::atomic::{AtomicBool, Ordering};

/// MFT record number of the volume's root folder.
const ROOT_RECORD: u64 = 5;
const REF_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

const ATTR_STANDARD_INFORMATION: u32 = 0x10;
const ATTR_FILE_NAME: u32 = 0x30;
const ATTR_DATA: u32 = 0x80;
const ATTR_END: u32 = 0xFFFF_FFFF;

const RECORD_IN_USE: u16 = 0x0001;
const RECORD_IS_DIRECTORY: u16 = 0x0002;

const ATTR_FLAG_COMPRESSED: u16 = 0x0001;
const ATTR_FLAG_SPARSE: u16 = 0x8000;

const FLAG_IN_USE: u8 = 1;
const FLAG_DIR: u8 = 2;
const FLAG_HAS_NAME: u8 = 4;

/// How much of the MFT is read per call (a multiple of every record and
/// cluster size NTFS uses).
const READ_CHUNK: usize = 4 * 1024 * 1024;

fn u16_at(buf: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(buf.get(offset..offset + 2)?.try_into().ok()?))
}

fn u32_at(buf: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(buf.get(offset..offset + 4)?.try_into().ok()?))
}

fn u64_at(buf: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(buf.get(offset..offset + 8)?.try_into().ok()?))
}

/// Undoes NTFS's "update sequence" protection: the last two bytes of every
/// sector in a record are replaced on disk by a check value, with the real
/// bytes saved in the record's update sequence array. Returns `false` (a
/// torn or corrupt record) if a sector's check value doesn't match.
pub(crate) fn apply_fixup(record: &mut [u8], sector_size: usize) -> bool {
    let (Some(usa_offset), Some(usa_count)) = (u16_at(record, 4), u16_at(record, 6)) else {
        return false;
    };
    let (usa_offset, usa_count) = (usa_offset as usize, usa_count as usize);
    if usa_count == 0 || sector_size < 2 || usa_offset + usa_count * 2 > record.len() {
        return false;
    }
    let check = [record[usa_offset], record[usa_offset + 1]];
    for i in 1..usa_count {
        let end = i * sector_size;
        if end > record.len() {
            break;
        }
        if record[end - 2..end] != check {
            return false;
        }
        let saved = usa_offset + i * 2;
        record[end - 2] = record[saved];
        record[end - 1] = record[saved + 1];
    }
    true
}

/// One run of a non-resident attribute: `clusters` clusters starting at
/// logical cluster `lcn`, or a sparse hole (`lcn: None`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Extent {
    pub lcn: Option<u64>,
    pub clusters: u64,
}

/// Decodes an attribute's run list. Each run starts with a header byte
/// whose low nibble is the size of the length field and high nibble the
/// size of the (signed, relative to the previous run) starting-cluster
/// field; a zero byte ends the list, and a zero-size offset is a sparse run.
pub(crate) fn decode_data_runs(runs: &[u8]) -> Option<Vec<Extent>> {
    let mut extents = Vec::new();
    let mut pos = 0usize;
    let mut lcn: i64 = 0;
    while let Some(&header) = runs.get(pos) {
        if header == 0 {
            return Some(extents);
        }
        let len_size = (header & 0x0F) as usize;
        let offset_size = (header >> 4) as usize;
        pos += 1;
        if len_size == 0 || len_size > 8 || offset_size > 8 {
            return None;
        }
        let len_bytes = runs.get(pos..pos + len_size)?;
        let clusters = len_bytes
            .iter()
            .rev()
            .fold(0u64, |acc, b| (acc << 8) | *b as u64);
        pos += len_size;

        if offset_size == 0 {
            extents.push(Extent { lcn: None, clusters });
            continue;
        }
        let offset_bytes = runs.get(pos..pos + offset_size)?;
        pos += offset_size;
        let mut delta = offset_bytes
            .iter()
            .rev()
            .fold(0i64, |acc, b| (acc << 8) | *b as i64);
        // Sign-extend from the field's own width.
        if offset_bytes.last().is_some_and(|b| b & 0x80 != 0) && offset_size < 8 {
            delta -= 1i64 << (offset_size * 8);
        }
        lcn = lcn.checked_add(delta)?;
        if lcn < 0 {
            return None;
        }
        extents.push(Extent { lcn: Some(lcn as u64), clusters });
    }
    None
}

/// One attribute's header fields that the scan cares about.
struct Attribute<'a> {
    kind: u32,
    non_resident: bool,
    name_len: u8,
    flags: u16,
    /// The whole attribute, header included.
    bytes: &'a [u8],
}

impl Attribute<'_> {
    fn resident_value(&self) -> Option<&[u8]> {
        if self.non_resident {
            return None;
        }
        let len = u32_at(self.bytes, 0x10)? as usize;
        let offset = u16_at(self.bytes, 0x14)? as usize;
        self.bytes.get(offset..offset.checked_add(len)?)
    }
}

/// Walks a (fixed-up) record's attributes.
fn attributes(record: &[u8]) -> impl Iterator<Item = Attribute<'_>> {
    let mut pos = u16_at(record, 0x14).unwrap_or(0) as usize;
    let used = (u32_at(record, 0x18).unwrap_or(0) as usize).min(record.len());
    std::iter::from_fn(move || {
        if pos + 16 > used {
            return None;
        }
        let kind = u32_at(record, pos)?;
        if kind == ATTR_END {
            return None;
        }
        let len = u32_at(record, pos + 4)? as usize;
        if len < 16 || pos + len > used {
            return None;
        }
        let bytes = &record[pos..pos + len];
        pos += len;
        Some(Attribute {
            kind,
            non_resident: bytes[8] != 0,
            name_len: bytes[9],
            flags: u16_at(bytes, 0x0C).unwrap_or(0),
            bytes,
        })
    })
}

/// The unnamed `$DATA` stream's (logical size, allocated size), if this
/// attribute holds them. A non-resident stream split across several
/// records only carries its sizes in the piece that starts at cluster 0.
fn data_sizes(attr: &Attribute) -> Option<(u64, u64)> {
    if attr.kind != ATTR_DATA || attr.name_len != 0 {
        return None;
    }
    if !attr.non_resident {
        // Small files live inside the MFT record itself: no clusters.
        return Some((attr.resident_value()?.len() as u64, 0));
    }
    if u64_at(attr.bytes, 0x10)? != 0 {
        return None;
    }
    let mut allocated = u64_at(attr.bytes, 0x28)?;
    let size = u64_at(attr.bytes, 0x30)?;
    if attr.flags & (ATTR_FLAG_COMPRESSED | ATTR_FLAG_SPARSE) != 0
        && let Some(on_disk) = u64_at(attr.bytes, 0x40)
    {
        allocated = on_disk;
    }
    Some((size, allocated))
}

/// A `$FILE_NAME` value: (parent reference, namespace rank, name).
fn file_name(attr: &Attribute) -> Option<(u64, u8, String)> {
    if attr.kind != ATTR_FILE_NAME {
        return None;
    }
    let value = attr.resident_value()?;
    let parent = u64_at(value, 0)?;
    let chars = *value.get(0x40)? as usize;
    let namespace = *value.get(0x41)?;
    let raw = value.get(0x42..0x42 + chars * 2)?;
    let units: Vec<u16> = raw
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    // Prefer the long name; a DOS-only 8.3 alias ranks lowest.
    let rank = match namespace {
        1 | 3 => 3,
        0 => 2,
        _ => 1,
    };
    Some((parent, rank, String::from_utf16_lossy(&units)))
}

/// The `$DATA` run list of record 0 (the MFT's own record), which says
/// where on disk the rest of the MFT lives, plus the MFT's size in bytes.
pub(crate) fn mft_extents(record0: &[u8]) -> Option<(Vec<Extent>, u64)> {
    for attr in attributes(record0) {
        if attr.kind != ATTR_DATA || attr.name_len != 0 || !attr.non_resident {
            continue;
        }
        let size = u64_at(attr.bytes, 0x30)?;
        let runs_offset = u16_at(attr.bytes, 0x20)? as usize;
        let extents = decode_data_runs(attr.bytes.get(runs_offset..)?)?;
        return Some((extents, size));
    }
    None
}

#[derive(Clone, Copy, Default)]
struct Rec {
    parent: u64,
    size: u64,
    allocated: u64,
    /// Last modified (FILETIME), from $STANDARD_INFORMATION.
    modified: i64,
    name_offset: u32,
    name_len: u16,
    parent_seq: u16,
    seq: u16,
    name_rank: u8,
    flags: u8,
}

/// Records collected from the MFT so far, indexed by record number.
pub(crate) struct MftTable {
    recs: Vec<Rec>,
    names: String,
    record_size: usize,
    sector_size: usize,
    next_index: u64,
    pub bytes_seen: u64,
}

impl MftTable {
    pub(crate) fn new(record_count: u64, record_size: usize, sector_size: usize) -> Self {
        Self {
            recs: vec![Rec::default(); record_count as usize],
            names: String::new(),
            record_size,
            sector_size,
            next_index: 0,
            bytes_seen: 0,
        }
    }

    pub(crate) fn records_seen(&self) -> u64 {
        self.next_index
    }

    fn total(&self) -> u64 {
        self.recs.len() as u64
    }

    /// Skips records that aren't on disk (a sparse part of the MFT).
    pub(crate) fn skip_records(&mut self, count: u64) {
        self.next_index = (self.next_index + count).min(self.total());
    }

    /// Feeds whole records, in MFT order. `buf.len()` must be a multiple
    /// of the record size.
    pub(crate) fn feed(&mut self, buf: &mut [u8]) {
        let record_size = self.record_size;
        for record in buf.chunks_exact_mut(record_size) {
            if self.next_index >= self.total() {
                return;
            }
            let index = self.next_index;
            self.next_index += 1;
            self.add_record(index, record);
        }
    }

    fn add_record(&mut self, index: u64, record: &mut [u8]) {
        if record.get(0..4) != Some(b"FILE".as_slice()) || !apply_fixup(record, self.sector_size) {
            return;
        }
        let flags = u16_at(record, 0x16).unwrap_or(0);
        if flags & RECORD_IN_USE == 0 {
            return;
        }
        let base = u64_at(record, 0x20).unwrap_or(0) & REF_MASK;
        // An extension record holds overflow attributes of its base record.
        let target = if base != 0 { base } else { index };
        if target >= self.total() {
            return;
        }
        if base == 0 {
            let rec = &mut self.recs[index as usize];
            rec.seq = u16_at(record, 0x10).unwrap_or(0);
            rec.flags |= FLAG_IN_USE;
            if flags & RECORD_IS_DIRECTORY != 0 {
                rec.flags |= FLAG_DIR;
            }
        }

        for attr in attributes(record) {
            if attr.kind == ATTR_STANDARD_INFORMATION {
                // Created at 0x00, last modified at 0x08.
                if let Some(modified) = attr.resident_value().and_then(|v| u64_at(v, 0x08)) {
                    self.recs[target as usize].modified = modified as i64;
                }
                continue;
            }
            if let Some((size, allocated)) = data_sizes(&attr) {
                let rec = &mut self.recs[target as usize];
                rec.size = size;
                rec.allocated = allocated;
                self.bytes_seen += size;
            } else if let Some((parent, rank, name)) = file_name(&attr) {
                let current = self.recs[target as usize];
                if current.flags & FLAG_HAS_NAME != 0 && current.name_rank >= rank {
                    continue;
                }
                let (Ok(offset), Ok(len)) =
                    (u32::try_from(self.names.len()), u16::try_from(name.len()))
                else {
                    continue;
                };
                self.names.push_str(&name);
                let rec = &mut self.recs[target as usize];
                rec.parent = parent & REF_MASK;
                rec.parent_seq = (parent >> 48) as u16;
                rec.name_offset = offset;
                rec.name_len = len;
                rec.name_rank = rank;
                rec.flags |= FLAG_HAS_NAME;
            }
        }
    }

    fn name(&self, rec: &Rec) -> &str {
        let start = rec.name_offset as usize;
        self.names.get(start..start + rec.name_len as usize).unwrap_or("")
    }

    /// Builds the folder tree under the root folder, named `root_name`.
    /// Records whose parent is gone (or was reused for something else) are
    /// left out, as Explorer would never show them either.
    pub(crate) fn build_tree(&self, root_name: &str) -> Option<DirNode> {
        let root = *self.recs.get(ROOT_RECORD as usize)?;
        if root.flags & (FLAG_IN_USE | FLAG_DIR) != FLAG_IN_USE | FLAG_DIR {
            return None;
        }
        let n = self.recs.len();
        let parent_of = |i: usize| -> Option<usize> {
            let rec = &self.recs[i];
            if i as u64 == ROOT_RECORD || rec.flags & (FLAG_IN_USE | FLAG_HAS_NAME) != FLAG_IN_USE | FLAG_HAS_NAME {
                return None;
            }
            let parent = rec.parent as usize;
            let p = self.recs.get(parent)?;
            let valid = p.flags & (FLAG_IN_USE | FLAG_DIR) == FLAG_IN_USE | FLAG_DIR
                && (rec.parent_seq == 0 || p.seq == 0 || rec.parent_seq == p.seq)
                && parent != i;
            valid.then_some(parent)
        };

        // Children lists in one flat array (CSR): `start[p]..start[p + 1]`
        // indexes `children` for parent `p`.
        let mut start = vec![0u32; n + 1];
        for i in 0..n {
            if let Some(p) = parent_of(i) {
                start[p + 1] += 1;
            }
        }
        for i in 0..n {
            start[i + 1] += start[i];
        }
        let mut fill = start.clone();
        let mut children = vec![0u32; start[n] as usize];
        for i in 0..n {
            if let Some(p) = parent_of(i) {
                children[fill[p] as usize] = i as u32;
                fill[p] += 1;
            }
        }

        let mut visited = vec![false; n];
        Some(self.build_node(ROOT_RECORD as usize, root_name.into(), &start, &children, &mut visited))
    }

    fn build_node(
        &self,
        index: usize,
        name: Box<str>,
        start: &[u32],
        children: &[u32],
        visited: &mut [bool],
    ) -> DirNode {
        visited[index] = true;
        let mut node = DirNode {
            name,
            ..Default::default()
        };
        for &child in &children[start[index] as usize..start[index + 1] as usize] {
            let child = child as usize;
            if visited[child] {
                continue;
            }
            let rec = &self.recs[child];
            let child_name: Box<str> = self.name(rec).into();
            if rec.flags & FLAG_DIR != 0 {
                node.dirs
                    .push(self.build_node(child, child_name, start, children, visited));
            } else {
                visited[child] = true;
                node.files.push(FileEntry {
                    name: child_name,
                    size: rec.size,
                    allocated: rec.allocated,
                    modified: rec.modified,
                });
            }
        }
        node.recompute_totals();
        node.sort_children();
        node
    }
}

/// Reads the MFT of drive `letter` and returns its whole folder tree.
/// `progress(records_done, records_total, bytes_seen)` is called after each
/// chunk; returns `Ok(None)` if `cancel` was set part way through.
#[cfg(windows)]
pub fn scan_volume(
    letter: char,
    cancel: &AtomicBool,
    progress: &dyn Fn(u64, u64, u64),
) -> Result<Option<DirNode>, String> {
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_BEGIN, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING, ReadFile, SetFilePointerEx,
    };
    use windows::Win32::System::IO::DeviceIoControl;
    use windows::Win32::System::Ioctl::{FSCTL_GET_NTFS_VOLUME_DATA, NTFS_VOLUME_DATA_BUFFER};
    use windows::core::PCWSTR;

    struct Volume(HANDLE);
    impl Drop for Volume {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    fn read_at(volume: &Volume, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        unsafe {
            SetFilePointerEx(volume.0, offset as i64, None, FILE_BEGIN)
                .map_err(|e| format!("seek failed: {e}"))?;
            let mut read = 0u32;
            ReadFile(volume.0, Some(buf), Some(&mut read), None)
                .map_err(|e| format!("read failed: {e}"))?;
            if (read as usize) < buf.len() {
                return Err("short read from the volume".into());
            }
        }
        Ok(())
    }

    let device: Vec<u16> = format!(r"\\.\{letter}:").encode_utf16().chain([0]).collect();
    let volume = unsafe {
        CreateFileW(
            PCWSTR(device.as_ptr()),
            GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .map(Volume)
    .map_err(|e| format!("couldn't open the volume: {e}"))?;

    let mut data = NTFS_VOLUME_DATA_BUFFER::default();
    let mut returned = 0u32;
    unsafe {
        DeviceIoControl(
            volume.0,
            FSCTL_GET_NTFS_VOLUME_DATA,
            None,
            0,
            Some(&mut data as *mut _ as *mut core::ffi::c_void),
            size_of::<NTFS_VOLUME_DATA_BUFFER>() as u32,
            Some(&mut returned),
            None,
        )
    }
    .map_err(|e| format!("couldn't read the NTFS volume data: {e}"))?;

    let cluster = data.BytesPerCluster as u64;
    let record_size = data.BytesPerFileRecordSegment as usize;
    let sector_size = data.BytesPerSector as usize;
    if cluster == 0 || record_size == 0 || sector_size == 0 || data.MftStartLcn < 0 {
        return Err("unexpected NTFS volume geometry".into());
    }

    // Record 0 describes the MFT itself, including where its pieces are.
    let mut first = vec![0u8; record_size.max(sector_size)];
    read_at(&volume, data.MftStartLcn as u64 * cluster, &mut first)?;
    let record0 = &mut first[..record_size];
    if record0.get(0..4) != Some(b"FILE".as_slice()) || !apply_fixup(record0, sector_size) {
        return Err("the MFT's first record is unreadable".into());
    }
    let (extents, mft_bytes) = mft_extents(record0).ok_or("couldn't locate the MFT's extents")?;
    let valid = (data.MftValidDataLength.max(0) as u64).min(mft_bytes);
    let record_count = valid / record_size as u64;

    let mut table = MftTable::new(record_count, record_size, sector_size);
    let mut buf = vec![0u8; READ_CHUNK];
    let mut carry: Vec<u8> = Vec::with_capacity(READ_CHUNK + record_size);
    let bytes_needed = record_count * record_size as u64;
    let mut bytes_done = 0u64;

    'extents: for extent in extents {
        let extent_bytes = extent.clusters * cluster;
        let Some(lcn) = extent.lcn else {
            carry.clear();
            table.skip_records(extent_bytes / record_size as u64);
            bytes_done += extent_bytes;
            continue;
        };
        let mut offset = 0u64;
        while offset < extent_bytes {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            if bytes_done >= bytes_needed {
                break 'extents;
            }
            let len = (extent_bytes - offset).min(READ_CHUNK as u64) as usize;
            read_at(&volume, lcn * cluster + offset, &mut buf[..len])?;
            offset += len as u64;
            bytes_done += len as u64;

            carry.extend_from_slice(&buf[..len]);
            let whole = carry.len() / record_size * record_size;
            table.feed(&mut carry[..whole]);
            carry.drain(..whole);
            progress(table.records_seen(), record_count, table.bytes_seen);
        }
    }

    let root_name = format!("{letter}:\\");
    table
        .build_tree(&root_name)
        .map(Some)
        .ok_or_else(|| "the root folder's record is missing".into())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const SECTOR: usize = 512;
    pub const RECORD: usize = 1024;

    /// Builds one MFT record with the given attributes and applies the
    /// on-disk update sequence (the inverse of `apply_fixup`).
    pub fn record(seq: u16, flags: u16, base: u64, attrs: &[Vec<u8>]) -> Vec<u8> {
        let mut r = vec![0u8; RECORD];
        r[0..4].copy_from_slice(b"FILE");
        let usa_offset = 0x30u16;
        let usa_count = (RECORD / SECTOR + 1) as u16;
        r[4..6].copy_from_slice(&usa_offset.to_le_bytes());
        r[6..8].copy_from_slice(&usa_count.to_le_bytes());
        r[0x10..0x12].copy_from_slice(&seq.to_le_bytes());
        let first_attr = 0x38u16;
        r[0x14..0x16].copy_from_slice(&first_attr.to_le_bytes());
        r[0x16..0x18].copy_from_slice(&flags.to_le_bytes());
        r[0x20..0x28].copy_from_slice(&base.to_le_bytes());
        let mut pos = first_attr as usize;
        for a in attrs {
            r[pos..pos + a.len()].copy_from_slice(a);
            pos += a.len();
        }
        r[pos..pos + 4].copy_from_slice(&ATTR_END.to_le_bytes());
        pos += 8;
        r[0x18..0x1C].copy_from_slice(&(pos as u32).to_le_bytes());
        r[0x1C..0x20].copy_from_slice(&(RECORD as u32).to_le_bytes());

        let usn = [0x07u8, 0x00];
        let usa = usa_offset as usize;
        r[usa..usa + 2].copy_from_slice(&usn);
        for i in 1..usa_count as usize {
            let end = i * SECTOR;
            let saved = [r[end - 2], r[end - 1]];
            r[usa + i * 2..usa + i * 2 + 2].copy_from_slice(&saved);
            r[end - 2..end].copy_from_slice(&usn);
        }
        r
    }

    fn pad8(mut v: Vec<u8>) -> Vec<u8> {
        while v.len() % 8 != 0 {
            v.push(0);
        }
        v
    }

    pub fn resident(kind: u32, value: &[u8], name_len: u8) -> Vec<u8> {
        let mut a = vec![0u8; 0x18];
        a[0..4].copy_from_slice(&kind.to_le_bytes());
        a[9] = name_len;
        a[0x10..0x14].copy_from_slice(&(value.len() as u32).to_le_bytes());
        a[0x14..0x16].copy_from_slice(&0x18u16.to_le_bytes());
        a.extend_from_slice(value);
        let mut a = pad8(a);
        let len = a.len() as u32;
        a[4..8].copy_from_slice(&len.to_le_bytes());
        a
    }

    pub fn standard_info(modified: i64) -> Vec<u8> {
        let mut v = vec![0u8; 0x30];
        v[0x08..0x10].copy_from_slice(&modified.to_le_bytes());
        v
    }

    pub fn file_name_attr(parent: u64, parent_seq: u16, namespace: u8, name: &str) -> Vec<u8> {
        let units: Vec<u16> = name.encode_utf16().collect();
        let mut v = vec![0u8; 0x42];
        let reference = parent | ((parent_seq as u64) << 48);
        v[0..8].copy_from_slice(&reference.to_le_bytes());
        v[0x40] = units.len() as u8;
        v[0x41] = namespace;
        for u in units {
            v.extend_from_slice(&u.to_le_bytes());
        }
        resident(ATTR_FILE_NAME, &v, 0)
    }

    pub fn non_resident_data(start_vcn: u64, size: u64, allocated: u64, runs: &[u8]) -> Vec<u8> {
        let mut a = vec![0u8; 0x40];
        a[0..4].copy_from_slice(&ATTR_DATA.to_le_bytes());
        a[8] = 1;
        a[0x10..0x18].copy_from_slice(&start_vcn.to_le_bytes());
        a[0x20..0x22].copy_from_slice(&0x40u16.to_le_bytes());
        a[0x28..0x30].copy_from_slice(&allocated.to_le_bytes());
        a[0x30..0x38].copy_from_slice(&size.to_le_bytes());
        a[0x38..0x40].copy_from_slice(&size.to_le_bytes());
        a.extend_from_slice(runs);
        a.push(0);
        let mut a = pad8(a);
        let len = a.len() as u32;
        a[4..8].copy_from_slice(&len.to_le_bytes());
        a
    }

    const DIR: u16 = RECORD_IN_USE | RECORD_IS_DIRECTORY;
    const FILE: u16 = RECORD_IN_USE;

    #[test]
    fn data_runs_decode_relative_offsets_and_sparse_runs() {
        // 24 clusters at 0x5634; 16 clusters 16 before that; a sparse run
        // of 8; 2 clusters at +0x100 from the last real run.
        let runs = [
            0x21, 0x18, 0x34, 0x56, //
            0x11, 0x10, 0xF0, //
            0x01, 0x08, //
            0x21, 0x02, 0x00, 0x01, //
            0x00,
        ];
        assert_eq!(
            decode_data_runs(&runs).unwrap(),
            vec![
                Extent { lcn: Some(0x5634), clusters: 24 },
                Extent { lcn: Some(0x5624), clusters: 16 },
                Extent { lcn: None, clusters: 8 },
                Extent { lcn: Some(0x5724), clusters: 2 },
            ]
        );
        assert_eq!(decode_data_runs(&[0x21, 0x18]), None, "truncated run list");
    }

    #[test]
    fn fixup_restores_sector_tails_and_rejects_torn_records() {
        let original = record(1, FILE, 0, &[file_name_attr(5, 5, 1, "a.txt")]);
        let mut fixed = original.clone();
        assert!(apply_fixup(&mut fixed, SECTOR));
        assert_ne!(fixed[SECTOR - 2..SECTOR], [0x07, 0x00]);

        let mut torn = original;
        torn[SECTOR - 1] ^= 0xFF;
        assert!(!apply_fixup(&mut torn, SECTOR));
    }

    #[test]
    fn mft_extents_are_read_from_record_zero() {
        let runs = [0x21, 0x40, 0x00, 0x10];
        let mut r0 = record(1, FILE, 0, &[
            file_name_attr(5, 5, 3, "$MFT"),
            non_resident_data(0, 0x40 * 4096, 0x40 * 4096, &runs),
        ]);
        assert!(apply_fixup(&mut r0, SECTOR));
        let (extents, size) = mft_extents(&r0).unwrap();
        assert_eq!(extents, vec![Extent { lcn: Some(0x1000), clusters: 0x40 }]);
        assert_eq!(size, 0x40 * 4096);
    }

    /// A tiny volume: root (5) holding `docs` (16, with `a.txt` and a
    /// large `b.bin` whose $DATA is in extension record 20), `c.log`, a
    /// deleted record, an orphan whose parent was reused, and a file with
    /// both a DOS and a long name.
    pub fn sample_mft() -> Vec<u8> {
        let empty = vec![0u8; RECORD];
        let mut records: Vec<Vec<u8>> = (0..24).map(|_| empty.clone()).collect();
        records[5] = record(5, DIR, 0, &[file_name_attr(5, 5, 3, ".")]);
        records[16] = record(2, DIR, 0, &[file_name_attr(5, 5, 1, "docs")]);
        records[17] = record(1, FILE, 0, &[
            resident(ATTR_STANDARD_INFORMATION, &standard_info(133_000_000_000_000_000), 0),
            file_name_attr(16, 2, 1, "a.txt"),
            resident(ATTR_DATA, b"hello", 0),
        ]);
        records[18] = record(1, FILE, 0, &[file_name_attr(16, 2, 1, "b.bin")]);
        records[20] = record(1, FILE, 18, &[non_resident_data(0, 10_000, 12_288, &[0x11, 0x03, 0x20])]);
        records[19] = record(1, FILE, 0, &[
            file_name_attr(5, 5, 2, "CLOG~1.LOG"),
            file_name_attr(5, 5, 1, "c.log"),
            non_resident_data(0, 5000, 8192, &[0x11, 0x02, 0x40]),
        ]);
        // Not in use: must not appear.
        records[21] = record(1, 0, 0, &[file_name_attr(5, 5, 1, "deleted.txt")]);
        // Points at record 16 with an old sequence number: an orphan.
        records[22] = record(1, FILE, 0, &[
            file_name_attr(16, 1, 1, "stale.txt"),
            resident(ATTR_DATA, b"x", 0),
        ]);
        // A named stream doesn't count toward the file's size.
        records[23] = record(1, FILE, 0, &[
            file_name_attr(5, 5, 1, "ads.txt"),
            resident(ATTR_DATA, b"1234", 0),
            resident(ATTR_DATA, &[0u8; 64], 3),
        ]);
        records.concat()
    }

    #[test]
    fn table_builds_the_folder_tree() {
        let mut bytes = sample_mft();
        let mut table = MftTable::new(24, RECORD, SECTOR);
        // Feed in two uneven halves, as `scan_volume` does per chunk.
        let (a, b) = bytes.split_at_mut(RECORD * 7);
        table.feed(a);
        table.feed(b);
        assert_eq!(table.records_seen(), 24);

        let root = table.build_tree(r"C:\").unwrap();
        assert_eq!(&*root.name, r"C:\");
        assert_eq!(root.dirs.len(), 1);
        let docs = &root.dirs[0];
        assert_eq!(&*docs.name, "docs");
        let names: Vec<&str> = docs.files.iter().map(|f| &*f.name).collect();
        assert_eq!(names, vec!["b.bin", "a.txt"], "largest first");
        assert_eq!(docs.files[0].size, 10_000);
        assert_eq!(docs.files[0].allocated, 12_288);
        assert_eq!(docs.size, 10_005);
        assert_eq!(docs.files[1].modified, 133_000_000_000_000_000, "from $STANDARD_INFORMATION");
        assert_eq!(docs.files[0].modified, 0, "no $STANDARD_INFORMATION");

        let root_files: Vec<&str> = root.files.iter().map(|f| &*f.name).collect();
        assert_eq!(root_files, vec!["c.log", "ads.txt"], "long name wins, deleted/orphans skipped");
        assert_eq!(root.size, 10_005 + 5000 + 4);
        assert_eq!(root.file_count, 4);
        assert_eq!(root.dir_count, 1);
    }

    #[test]
    fn sparse_mft_regions_keep_record_numbers_aligned() {
        let bytes = sample_mft();
        let mut table = MftTable::new(24, RECORD, SECTOR);
        let mut head = bytes[..RECORD * 6].to_vec();
        table.feed(&mut head);
        // Records 6-11 (all unused) are a hole; the rest follows.
        table.skip_records(6);
        let mut tail = bytes[RECORD * 12..].to_vec();
        table.feed(&mut tail);
        let root = table.build_tree(r"C:\").unwrap();
        assert_eq!(root.dirs.len(), 1);
        assert_eq!(root.dirs[0].file_count, 2);
    }
}
