//! Runs the fast MFT scan (`core::ntfs_mft`) for a user who isn't running
//! EdenExplorer as administrator: the app starts a second copy of itself
//! with `HELPER_ARG` through Windows' "Run as administrator" (one UAC
//! prompt), and that copy reads the drive's Master File Table and sends
//! the result back. EdenExplorer itself stays unelevated.
//!
//! The two talk over a named pipe the app creates (inbound only, one
//! instance, local clients only, and only the helper's own process may
//! connect), so the elevated helper never writes a file anywhere. The
//! helper sends progress messages while it reads, then either the whole
//! tree (postcard-encoded) or an error message. Cancelling closes the
//! pipe, which makes the helper's next write fail and it exits.

use crate::core::disk_usage::DirNode;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};

/// The command-line switch that turns a launch into the helper.
pub const HELPER_ARG: &str = "--disk-usage-mft-helper";
const PIPE_PREFIX: &str = r"\\.\pipe\EdenExplorer-mft-";

const MSG_PROGRESS: u8 = 1;
const MSG_TREE: u8 = 2;
const MSG_ERROR: u8 = 3;

/// Refuse anything bigger than this from the pipe (a real tree for tens
/// of millions of files is still well under it).
const MAX_MESSAGE: u64 = 16 * 1024 * 1024 * 1024;

/// Helper exit codes.
const EXIT_OK: i32 = 0;
const EXIT_CANCELLED: i32 = 1;
const EXIT_BAD_ARGS: i32 = 2;
const EXIT_FAILED: i32 = 3;

#[derive(Debug, PartialEq)]
pub(crate) enum Message {
    Progress { done: u64, total: u64, bytes: u64 },
    Tree(DirNode),
    Error(String),
}

fn write_message(w: &mut impl Write, tag: u8, payload: &[u8]) -> io::Result<()> {
    w.write_all(&[tag])?;
    w.write_all(&(payload.len() as u64).to_le_bytes())?;
    w.write_all(payload)?;
    w.flush()
}

pub(crate) fn write_progress(w: &mut impl Write, done: u64, total: u64, bytes: u64) -> io::Result<()> {
    let mut payload = [0u8; 24];
    payload[0..8].copy_from_slice(&done.to_le_bytes());
    payload[8..16].copy_from_slice(&total.to_le_bytes());
    payload[16..24].copy_from_slice(&bytes.to_le_bytes());
    write_message(w, MSG_PROGRESS, &payload)
}

pub(crate) fn write_tree(w: &mut impl Write, tree: &DirNode) -> io::Result<()> {
    let bytes = postcard::to_allocvec(tree).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write_message(w, MSG_TREE, &bytes)
}

pub(crate) fn write_error(w: &mut impl Write, message: &str) -> io::Result<()> {
    write_message(w, MSG_ERROR, message.as_bytes())
}

pub(crate) fn read_message(r: &mut impl Read) -> io::Result<Message> {
    let invalid = |what: &str| io::Error::new(io::ErrorKind::InvalidData, what.to_string());
    let mut tag = [0u8; 1];
    r.read_exact(&mut tag)?;
    let mut len = [0u8; 8];
    r.read_exact(&mut len)?;
    let len = u64::from_le_bytes(len);
    if len > MAX_MESSAGE {
        return Err(invalid("message too large"));
    }
    let mut payload = vec![0u8; len as usize];
    r.read_exact(&mut payload)?;
    match tag[0] {
        MSG_PROGRESS if payload.len() == 24 => {
            let at = |i: usize| u64::from_le_bytes(payload[i..i + 8].try_into().unwrap());
            Ok(Message::Progress {
                done: at(0),
                total: at(8),
                bytes: at(16),
            })
        }
        MSG_TREE => postcard::from_bytes(&payload)
            .map(Message::Tree)
            .map_err(|_| invalid("unreadable scan result")),
        MSG_ERROR => Ok(Message::Error(String::from_utf8_lossy(&payload).into_owned())),
        _ => Err(invalid("unknown message")),
    }
}

/// If this process was started as the helper, runs it and returns its
/// exit code; otherwise `None` (a normal launch).
pub fn run_helper_from_args() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next()? != HELPER_ARG {
        return None;
    }
    let letter = args
        .next()
        .and_then(|a| a.to_str().and_then(|s| s.chars().next()))
        .filter(char::is_ascii_alphabetic);
    let pipe = args
        .next()
        .and_then(|a| a.into_string().ok())
        .filter(|p| p.starts_with(PIPE_PREFIX) && p.len() < 128);
    let (Some(letter), Some(pipe)) = (letter, pipe) else {
        return Some(EXIT_BAD_ARGS);
    };
    // Building and encoding a deep tree recurses; the main thread's
    // default stack is small.
    let worker = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || helper_main(letter, &pipe));
    Some(
        worker
            .ok()
            .and_then(|w| w.join().ok())
            .unwrap_or(EXIT_FAILED),
    )
}

fn helper_main(letter: char, pipe: &str) -> i32 {
    // Stay out of the way of whatever the user is doing meanwhile.
    unsafe {
        use windows::Win32::System::Threading::{
            BELOW_NORMAL_PRIORITY_CLASS, GetCurrentProcess, SetPriorityClass,
        };
        let _ = SetPriorityClass(GetCurrentProcess(), BELOW_NORMAL_PRIORITY_CLASS);
    }
    let Ok(out) = std::fs::OpenOptions::new().write(true).open(pipe) else {
        return EXIT_FAILED;
    };
    let out = std::cell::RefCell::new(io::BufWriter::new(out));
    let cancel = AtomicBool::new(false);
    let last_sent = std::cell::Cell::new(std::time::Instant::now());
    let progress = |done: u64, total: u64, bytes: u64| {
        if last_sent.get().elapsed() < std::time::Duration::from_millis(100) && done < total {
            return;
        }
        last_sent.set(std::time::Instant::now());
        // The app went away or cancelled (closed the pipe).
        if write_progress(&mut *out.borrow_mut(), done, total, bytes).is_err() {
            cancel.store(true, Ordering::Relaxed);
        }
    };

    let result = crate::core::ntfs_mft::scan_volume(letter, &cancel, &progress);
    let mut out = out.into_inner();
    match result {
        Ok(Some(tree)) => {
            if write_tree(&mut out, &tree).is_ok() {
                EXIT_OK
            } else {
                EXIT_CANCELLED
            }
        }
        Ok(None) => EXIT_CANCELLED,
        Err(reason) => {
            let _ = write_error(&mut out, &reason);
            EXIT_FAILED
        }
    }
}

/// Why the elevated scan didn't produce a tree.
#[derive(Debug, PartialEq, Eq)]
pub enum ElevatedScanError {
    /// The UAC prompt was answered No (or closed).
    Declined,
    Failed(String),
}

/// Reads drive `letter`'s MFT in an elevated helper. `owner` is the window
/// the UAC prompt belongs to (0 for none). `waiting(true)` is called while
/// Windows asks for permission. Returns `Ok(None)` if `cancel` was set.
pub fn scan_elevated(
    letter: char,
    owner: isize,
    cancel: &AtomicBool,
    waiting: &dyn Fn(bool),
    progress: &dyn Fn(u64, u64, u64),
) -> Result<Option<DirNode>, ElevatedScanError> {
    use std::os::windows::io::FromRawHandle;
    use windows::Win32::Foundation::{
        CloseHandle, ERROR_CANCELLED, ERROR_PIPE_CONNECTED, HANDLE, HWND, WAIT_OBJECT_0,
    };
    use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_INBOUND};
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE,
        PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessId, INFINITE, WaitForSingleObject,
    };
    use windows::Win32::UI::Shell::{
        SEE_MASK_FLAG_NO_UI, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows::core::{HRESULT, PCWSTR, w};

    let failed = |what: &str, detail: String| ElevatedScanError::Failed(format!("{what}: {detail}"));
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain([0]).collect() };

    // A name nobody can guess ahead of time.
    let nonce = {
        use std::hash::BuildHasher;
        // `RandomState` is seeded from the OS's random number generator.
        std::collections::hash_map::RandomState::new().hash_one(std::time::SystemTime::now())
    };
    let pipe_name = format!("{PIPE_PREFIX}{}-{nonce:016x}", std::process::id());
    let pipe_wide = wide(&pipe_name);
    let pipe = unsafe {
        CreateNamedPipeW(
            PCWSTR(pipe_wide.as_ptr()),
            PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            0,
            1024 * 1024,
            0,
            None,
        )
    };
    if pipe.is_invalid() {
        return Err(failed("couldn't create the pipe", io::Error::last_os_error().to_string()));
    }
    // Closes the pipe on every return path.
    let pipe_file = unsafe { std::fs::File::from_raw_handle(pipe.0 as _) };

    let exe = std::env::current_exe().map_err(|e| failed("couldn't find EdenExplorer.exe", e.to_string()))?;
    let exe_wide: Vec<u16> = {
        use std::os::windows::ffi::OsStrExt;
        exe.as_os_str().encode_wide().chain([0]).collect()
    };
    let params = wide(&format!("{HELPER_ARG} {letter} {pipe_name}"));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI,
        hwnd: HWND(owner as *mut _),
        lpVerb: w!("runas"),
        lpFile: PCWSTR(exe_wide.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    waiting(true);
    let launched = unsafe { ShellExecuteExW(&mut info) };
    waiting(false);
    if let Err(e) = launched {
        return Err(if e.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
            ElevatedScanError::Declined
        } else {
            failed("couldn't start the helper", e.message())
        });
    }
    let process = info.hProcess;
    if process.is_invalid() {
        return Err(ElevatedScanError::Failed("the helper didn't start".into()));
    }
    struct Process(HANDLE);
    impl Drop for Process {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    // SAFETY: a process handle may be used from any thread.
    unsafe impl Send for Process {}
    unsafe impl Sync for Process {}
    // Shared with the thread below, so the handle stays open until both
    // are done with it.
    let process = std::sync::Arc::new(Process(process));
    let helper_pid = unsafe { GetProcessId(process.0) };

    // `ConnectNamedPipe` waits for a client; if the helper dies before
    // connecting, connect to the pipe ourselves so it returns. Not joined:
    // after a cancel the helper can take a moment to notice and exit, and
    // the scan shouldn't wait for it.
    {
        let process = process.clone();
        let pipe_name = pipe_name.clone();
        std::thread::spawn(move || {
            let wait = unsafe { WaitForSingleObject(process.0, INFINITE) };
            if wait == WAIT_OBJECT_0 {
                let _ = std::fs::OpenOptions::new().write(true).open(&pipe_name);
            }
        });
    }

    let connected = match unsafe { ConnectNamedPipe(pipe, None) } {
        Ok(()) => true,
        Err(e) => e.code() == HRESULT::from_win32(ERROR_PIPE_CONNECTED.0),
    };
    let mut client_pid = 0u32;
    let client_ok = connected
        && unsafe { GetNamedPipeClientProcessId(pipe, &mut client_pid) }.is_ok()
        && client_pid == helper_pid;

    let exit_code = || {
        let mut code = 0u32;
        unsafe {
            WaitForSingleObject(process.0, 5000);
            let _ = GetExitCodeProcess(process.0, &mut code);
        }
        code as i32
    };

    let result = if !client_ok {
        Err(ElevatedScanError::Failed(format!(
            "the helper didn't connect (exit code {})",
            exit_code()
        )))
    } else {
        let mut reader = io::BufReader::new(&pipe_file);
        loop {
            if cancel.load(Ordering::Relaxed) {
                break Ok(None);
            }
            match read_message(&mut reader) {
                Ok(Message::Progress { done, total, bytes }) => progress(done, total, bytes),
                Ok(Message::Tree(tree)) => break Ok(Some(tree)),
                Ok(Message::Error(reason)) => break Err(ElevatedScanError::Failed(reason)),
                Err(_) if cancel.load(Ordering::Relaxed) => break Ok(None),
                Err(e) => {
                    break match exit_code() {
                        EXIT_CANCELLED => Ok(None),
                        code => Err(failed("the helper stopped", format!("{e} (exit code {code})"))),
                    };
                }
            }
        }
    };
    // Closing our end makes a still-running helper stop at its next write.
    drop(pipe_file);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::disk_usage::FileEntry;

    #[test]
    fn messages_round_trip() {
        let mut tree = DirNode {
            name: r"C:\".into(),
            files: vec![FileEntry {
                name: "pagefile.sys".into(),
                size: 4096,
                allocated: 4096,
                modified: 1,
            }],
            dirs: vec![DirNode {
                name: "Windows".into(),
                unreadable: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        tree.recompute_totals();

        let mut buf = Vec::new();
        write_progress(&mut buf, 5, 10, 1234).unwrap();
        write_error(&mut buf, "no access").unwrap();
        write_tree(&mut buf, &tree).unwrap();

        let mut r = io::Cursor::new(buf);
        assert_eq!(
            read_message(&mut r).unwrap(),
            Message::Progress {
                done: 5,
                total: 10,
                bytes: 1234
            }
        );
        assert_eq!(read_message(&mut r).unwrap(), Message::Error("no access".into()));
        assert_eq!(read_message(&mut r).unwrap(), Message::Tree(tree));
        assert!(read_message(&mut r).is_err(), "end of stream");
    }

    #[test]
    fn garbage_is_rejected() {
        let mut too_big = vec![MSG_TREE];
        too_big.extend_from_slice(&u64::MAX.to_le_bytes());
        assert!(read_message(&mut io::Cursor::new(too_big)).is_err());

        let mut bad_tree = vec![MSG_TREE];
        bad_tree.extend_from_slice(&3u64.to_le_bytes());
        bad_tree.extend_from_slice(&[0xFF, 0xFF, 0xFF]);
        assert!(read_message(&mut io::Cursor::new(bad_tree)).is_err());

        let mut unknown = vec![9u8];
        unknown.extend_from_slice(&0u64.to_le_bytes());
        assert!(read_message(&mut io::Cursor::new(unknown)).is_err());
    }
}
