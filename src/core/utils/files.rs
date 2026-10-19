use crossbeam_channel::Sender;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs::{copy, create_dir_all, read_dir};
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread;
use windows::Win32::Foundation::HWND;
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    BHID_SFUIObject, CMINVOKECOMMANDINFO, CMINVOKECOMMANDINFOEX, IContextMenu,
    IFileOperationProgressSink, IFileOperationProgressSink_Impl, IShellItem,
    SHCreateItemFromIDList, SHFILEINFOW, SHGFI_TYPENAME, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW,
    SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{Error, HRESULT, PCSTR, PCWSTR, PWSTR, Ref, implement};

pub fn get_file_type_name<'a>(ext: &str, cache: &'a mut HashMap<String, String>) -> &'a str {
    use std::collections::hash_map::Entry;

    match cache.entry(ext.to_string()) {
        Entry::Occupied(entry) => entry.into_mut().as_str(),
        Entry::Vacant(entry) => {
            // Ensure extension starts with "."
            let ext_formatted = if ext.starts_with('.') {
                ext.to_string()
            } else {
                format!(".{}", ext)
            };

            let wide: Vec<u16> = OsStr::new(&ext_formatted)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();

            let mut info = SHFILEINFOW::default();

            let _result = unsafe {
                SHGetFileInfoW(
                    PCWSTR(wide.as_ptr()),
                    FILE_ATTRIBUTE_NORMAL,
                    Some(&mut info),
                    size_of::<SHFILEINFOW>() as u32,
                    SHGFI_TYPENAME | SHGFI_USEFILEATTRIBUTES,
                )
            };

            let len = info.szTypeName.iter().position(|&c| c == 0).unwrap_or(0);
            let type_name = String::from_utf16_lossy(&info.szTypeName[..len]);

            entry.insert(type_name).as_str()
        }
    }
}

pub fn format_size(size: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size_f = size as f64;
    let mut unit_index = 0;

    while size_f >= 1024.0 && unit_index < UNITS.len() - 1 {
        size_f /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", size, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size_f, UNITS[unit_index])
    }
}

#[allow(dead_code)]
pub fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    create_dir_all(dest)?;

    for entry in read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let new_path = dest.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &new_path)?;
        } else {
            copy(entry.path(), new_path)?;
        }
    }

    Ok(())
}

/// Checks if a filename contains valid characters for real-time validation
/// Used during typing to immediately filter invalid characters
pub fn filename_has_valid_characters_realtime(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }

    // Check maximum length
    if name.len() > 255 {
        return false;
    }

    // Windows reserved characters that cannot be used in filenames
    let invalid_chars = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

    // Check for invalid characters
    for ch in name.chars() {
        if invalid_chars.contains(&ch) {
            return false;
        }
    }

    // Windows reserved names (case-insensitive)
    let reserved_names = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];

    let name_upper = name.to_uppercase();
    for reserved in &reserved_names {
        if name_upper == *reserved {
            return false;
        }
    }

    true
}

/// Progress messages streamed from a background shell file-operation worker to
/// the UI thread.
#[derive(Clone, Debug)]
pub enum FileOpProgress {
    /// The shell operation began (fires after any confirmation prompt is accepted).
    Started,
    /// Overall work counters reported by the shell.
    Progress { total: u32, done: u32 },
    /// Absolute path of the item currently being processed.
    Item { name: String },
    /// The operation finished. `cancelled` reflects a user-initiated abort.
    Finished {
        cancelled: bool,
        error: Option<String>,
    },
}

/// Which shell operation `file_op_async` should perform on the given sources.
pub enum FileOpKind {
    /// Delete to the recycle bin (`allow_undo`) or permanently.
    Delete { allow_undo: bool },
    /// Move every source into `dest`.
    Move { dest: PathBuf },
    /// Copy every source into `dest`.
    Copy { dest: PathBuf },
}

#[implement(IFileOperationProgressSink)]
struct FileOpProgressSink {
    tx: Sender<FileOpProgress>,
    cancel: Arc<AtomicBool>,
    notify: Arc<dyn Fn() + Send + Sync>,
    /// Number of top-level items we asked the shell to process.
    base_total: u32,
    /// Items announced by a `Pre*Item` callback. This can grow for recursive
    /// operations where the shell discovers children after the operation starts.
    pre_count: AtomicU32,
    /// Items confirmed done via a `Post*Item` callback — the reliable per-item signal.
    post_count: AtomicU32,
}

impl FileOpProgressSink {
    fn emit(&self, msg: FileOpProgress) {
        let _ = self.tx.send(msg);
        (self.notify)();
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Report item counts from callbacks rather than the shell's
    /// `UpdateProgress` figures. Those figures represent shell work units and
    /// can make the UI reach X/X while callbacks are still arriving.
    fn emit_progress(&self) {
        let total = self.base_total.max(self.pre_count.load(Ordering::Relaxed));
        let done = self.post_count.load(Ordering::Relaxed).min(total);
        self.emit(FileOpProgress::Progress { total, done });
    }

    /// Shared `Pre*Item` body: abort if cancelled, else report the current item.
    fn report_item(&self, psiitem: Ref<'_, IShellItem>) -> windows::core::Result<()> {
        if self.cancelled() {
            return cancel_abort();
        }
        self.pre_count.fetch_add(1, Ordering::Relaxed);
        if let Some(name) = shell_item_path(psiitem) {
            self.emit(FileOpProgress::Item { name });
        }
        Ok(())
    }

    /// Shared `Post*Item` body: abort if cancelled, else advance the counter.
    fn report_post(&self) -> windows::core::Result<()> {
        if self.cancelled() {
            return cancel_abort();
        }
        self.post_count.fetch_add(1, Ordering::Relaxed);
        self.emit_progress();
        Ok(())
    }
}

/// A failed HRESULT from a progress-sink callback aborts the whole shell operation,
/// which is how the Cancel button stops an in-flight delete.
fn cancel_abort() -> windows::core::Result<()> {
    // HRESULT_FROM_WIN32(ERROR_CANCELLED)
    Err(Error::from(HRESULT(0x8007_04C7u32 as i32)))
}

fn shell_item_path(psiitem: Ref<'_, IShellItem>) -> Option<String> {
    let item = psiitem.ok().ok()?;
    let pwstr = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }.ok()?;
    let text = unsafe { pwstr_to_string(pwstr) };
    unsafe { CoTaskMemFree(Some(pwstr.0 as _)) };
    if text.is_empty() { None } else { Some(text) }
}

unsafe fn pwstr_to_string(pw: PWSTR) -> String {
    if pw.0.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    let mut ptr = pw.0;
    unsafe {
        while !ptr.is_null() && *ptr != 0 {
            len += 1;
            ptr = ptr.add(1);
        }
    }
    let slice = unsafe { std::slice::from_raw_parts(pw.0, len) };
    String::from_utf16_lossy(slice)
}

impl IFileOperationProgressSink_Impl for FileOpProgressSink_Impl {
    fn StartOperations(&self) -> windows::core::Result<()> {
        self.emit(FileOpProgress::Started);
        // Seed the total so the bar shows "0 / N" immediately instead of a spinner.
        self.emit_progress();
        Ok(())
    }

    fn FinishOperations(&self, _hrresult: HRESULT) -> windows::core::Result<()> {
        Ok(())
    }

    fn PreRenameItem(
        &self,
        _dwflags: u32,
        _psiitem: Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PostRenameItem(
        &self,
        _dwflags: u32,
        _psiitem: Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
        _hrrename: HRESULT,
        _psinewlycreated: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PreMoveItem(
        &self,
        _dwflags: u32,
        psiitem: Ref<'_, IShellItem>,
        _psidestinationfolder: Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
    ) -> windows::core::Result<()> {
        self.report_item(psiitem)
    }

    fn PostMoveItem(
        &self,
        _dwflags: u32,
        _psiitem: Ref<'_, IShellItem>,
        _psidestinationfolder: Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
        _hrmove: HRESULT,
        _psinewlycreated: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        self.report_post()
    }

    fn PreCopyItem(
        &self,
        _dwflags: u32,
        psiitem: Ref<'_, IShellItem>,
        _psidestinationfolder: Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
    ) -> windows::core::Result<()> {
        self.report_item(psiitem)
    }

    fn PostCopyItem(
        &self,
        _dwflags: u32,
        _psiitem: Ref<'_, IShellItem>,
        _psidestinationfolder: Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
        _hrcopy: HRESULT,
        _psinewlycreated: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        self.report_post()
    }

    fn PreDeleteItem(
        &self,
        _dwflags: u32,
        psiitem: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        self.report_item(psiitem)
    }

    fn PostDeleteItem(
        &self,
        _dwflags: u32,
        _psiitem: Ref<'_, IShellItem>,
        _hrdelete: HRESULT,
        _psinewlycreated: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        self.report_post()
    }

    fn PreNewItem(
        &self,
        _dwflags: u32,
        _psidestinationfolder: Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PostNewItem(
        &self,
        _dwflags: u32,
        _psidestinationfolder: Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
        _psztemplatename: &PCWSTR,
        _dwfileattributes: u32,
        _hrnew: HRESULT,
        _psinewitem: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn UpdateProgress(&self, _iworktotal: u32, _iworksofar: u32) -> windows::core::Result<()> {
        if self.cancelled() {
            return cancel_abort();
        }
        self.emit_progress();
        Ok(())
    }

    fn ResetTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn PauseTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn ResumeTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Runs a shell file operation (delete/move/copy) on a background STA thread so
/// the UI thread never blocks (blocking it previously made Windows flag
/// EdenExplorer as "Not Responding" on large operations). Progress is streamed to
/// `tx`; `notify` requests a UI repaint from the worker so updates appear without
/// busy-polling.
///
/// Windows' own progress dialog is suppressed (`FOF_SILENT`) in favor of the
/// caller's UI, while confirmation prompts (recycle-bin / merge collisions) are
/// preserved (we do not set `FOF_NOCONFIRMATION`). `owner_hwnd` is the raw pointer
/// value of the window that should own any shell dialog (0 for none). Setting
/// `cancel` aborts the operation at the next item boundary.
pub fn file_op_async(
    kind: FileOpKind,
    sources: Vec<PathBuf>,
    owner_hwnd: isize,
    tx: Sender<FileOpProgress>,
    cancel: Arc<AtomicBool>,
    notify: Arc<dyn Fn() + Send + Sync>,
) {
    if sources.is_empty() {
        let _ = tx.send(FileOpProgress::Finished {
            cancelled: false,
            error: None,
        });
        notify();
        return;
    }

    thread::spawn(move || {
        use windows::Win32::System::Com::{
            CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
        };
        use windows::Win32::UI::Shell::{
            FILEOPERATION_FLAGS, FOF_ALLOWUNDO, FOF_SILENT, FileOperation, IFileOperation,
            SHCreateItemFromParsingName,
        };
        use windows::core::HSTRING;

        unsafe {
            // Shell operations and their dialogs need a single-threaded apartment
            // with a message pump, which PerformOperations provides on this thread.
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            let result = (|| -> windows::core::Result<()> {
                let file_op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;

                // FOF_SILENT hides Windows' progress dialog (we render our own);
                // omitting FOF_NOCONFIRMATION keeps confirmation prompts.
                let allow_undo = match &kind {
                    FileOpKind::Delete { allow_undo } => *allow_undo,
                    FileOpKind::Move { .. } | FileOpKind::Copy { .. } => true,
                };
                let undo = if allow_undo { FOF_ALLOWUNDO.0 } else { 0 };
                // Keep collision confirmation enabled. In particular, do not
                // set FOF_RENAMEONCOLLISION: that flag silently chooses a new
                // name, while omitting it lets IFileOperation show the Shell's
                // conflict dialog with the user's available choices.
                file_op.SetOperationFlags(FILEOPERATION_FLAGS(FOF_SILENT.0 | undo))?;

                if owner_hwnd != 0 {
                    file_op.SetOwnerWindow(HWND(owner_hwnd as *mut core::ffi::c_void))?;
                }

                let base_total = sources.len() as u32;
                let sink: IFileOperationProgressSink = FileOpProgressSink {
                    tx: tx.clone(),
                    cancel: cancel.clone(),
                    notify: notify.clone(),
                    base_total,
                    pre_count: AtomicU32::new(0),
                    post_count: AtomicU32::new(0),
                }
                .into();
                let cookie = file_op.Advise(&sink)?;

                // Build the destination item once for move/copy operations.
                let dest_item: Option<IShellItem> = match &kind {
                    FileOpKind::Delete { .. } => None,
                    FileOpKind::Move { dest } | FileOpKind::Copy { dest } => {
                        let hdest = HSTRING::from(dest.to_string_lossy().as_ref());
                        Some(SHCreateItemFromParsingName(&hdest, None)?)
                    }
                };

                for path in &sources {
                    let hpath = HSTRING::from(path.to_string_lossy().as_ref());
                    let item: IShellItem = SHCreateItemFromParsingName(&hpath, None)?;
                    match &kind {
                        FileOpKind::Delete { .. } => {
                            file_op.DeleteItem(&item, None)?;
                        }
                        FileOpKind::Move { .. } => {
                            file_op.MoveItem(&item, dest_item.as_ref().unwrap(), None, None)?;
                        }
                        FileOpKind::Copy { .. } => {
                            file_op.CopyItem(&item, dest_item.as_ref().unwrap(), None, None)?;
                        }
                    }
                }

                file_op.PerformOperations()?;
                let _ = file_op.Unadvise(cookie);
                Ok(())
            })();

            let _ = tx.send(FileOpProgress::Finished {
                cancelled: cancel.load(Ordering::Relaxed),
                error: result.err().map(|e| e.to_string()),
            });
            notify();

            CoUninitialize();
        }
    });
}

pub fn restore_paths_native(pidls: Vec<Vec<u8>>) -> windows::core::Result<()> {
    use windows::Win32::System::Com::{CoTaskMemAlloc, CoTaskMemFree};

    unsafe {
        for pidl_bytes in pidls {
            let pidl = CoTaskMemAlloc(pidl_bytes.len()) as *mut u8;
            if pidl.is_null() {
                return Err(Error::from(HRESULT(0x80004005u32 as i32)));
            }

            let result = (|| -> windows::core::Result<()> {
                std::ptr::copy_nonoverlapping(pidl_bytes.as_ptr(), pidl, pidl_bytes.len());

                let shell_item: IShellItem = SHCreateItemFromIDList(pidl as *const ITEMIDLIST)?;
                let context_menu: IContextMenu =
                    shell_item.BindToHandler(None, &BHID_SFUIObject)?;

                let restore_verb = PCSTR(b"undelete\0".as_ptr());
                let info = CMINVOKECOMMANDINFOEX {
                    cbSize: std::mem::size_of::<CMINVOKECOMMANDINFOEX>() as u32,
                    fMask: 0,
                    hwnd: HWND(std::ptr::null_mut()),
                    lpVerb: restore_verb,
                    lpVerbW: PCWSTR::null(),
                    nShow: SW_SHOWNORMAL.0,
                    ..Default::default()
                };

                context_menu.InvokeCommand(&info as *const _ as *const CMINVOKECOMMANDINFO)
            })();

            CoTaskMemFree(Some(pidl as _));
            result?;
        }
    }

    Ok(())
}

pub fn directory_child_paths(dir: &Path) -> HashSet<PathBuf> {
    read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .collect()
        })
        .unwrap_or_default()
}

pub fn selection_paths_after_paste(
    target_dir: &Path,
    before_entries: &HashSet<PathBuf>,
    sources: &[PathBuf],
) -> Vec<PathBuf> {
    let mut pasted_paths: Vec<PathBuf> = directory_child_paths(target_dir)
        .difference(before_entries)
        .cloned()
        .collect();

    if pasted_paths.is_empty() {
        for source in sources {
            if let Some(name) = source.file_name() {
                let candidate = target_dir.join(name);
                if candidate.exists() {
                    pasted_paths.push(candidate);
                }
            }
        }
    }

    pasted_paths
}
