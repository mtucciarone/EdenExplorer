use std::fs::read_dir;
use std::mem::{ManuallyDrop, size_of};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicPtr, Ordering};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    CMF_NORMAL, CMINVOKECOMMANDINFO, CMINVOKECOMMANDINFOEX, DEFCONTEXTMENU, IContextMenu,
    IContextMenu2, IContextMenu3, ILCreateFromPathW, ILFindLastID, IShellFolder, IShellItem,
    IShellLinkW, SEE_MASK_INVOKEIDLIST, SHBindToParent, SHCreateDefaultContextMenu,
    SHCreateItemFromParsingName, SHELLEXECUTEINFOW, SIGDN_NORMALDISPLAY, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, GetMenuItemCount, GetMenuItemInfoW, HMENU, MENUITEMINFOW,
    MFS_DISABLED, MFS_GRAYED, MFT_SEPARATOR, MIIM_FTYPE, MIIM_ID, MIIM_STATE, MIIM_STRING,
    MIIM_SUBMENU, SW_SHOWNORMAL, SendMessageW, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx,
};
use windows::core::{Error, HRESULT, Interface, PCSTR, PCWSTR, PWSTR};

const CONTEXT_MENU_ID_BASE: u32 = 1;
const CONTEXT_MENU_ID_MAX: u32 = 0x7FFF;

static ACTIVE_SHELL_CONTEXT_MENU: AtomicPtr<ShellContextMenu> =
    AtomicPtr::new(std::ptr::null_mut());

#[derive(Clone, Debug)]
pub struct ShellContextMenuItem {
    pub id: u32,
    pub label: String,
    pub disabled: bool,
    pub children: Vec<ShellContextMenuItem>,
    pub send_to_path: Option<PathBuf>,
}

pub struct ShellContextMenu {
    context_menu: IContextMenu,
    menu: HMENU,
    items: Vec<ShellContextMenuItem>,
    selected_paths: Vec<PathBuf>,
    id_base: u32,
    full_pidls: Vec<*mut ITEMIDLIST>,
    parent_pidl: *mut ITEMIDLIST,
}

impl ShellContextMenu {
    pub fn for_paths(paths: &[PathBuf], hwnd: HWND) -> windows::core::Result<Self> {
        let parent =
            shared_parent(paths).ok_or_else(|| Error::from(HRESULT(0x80004005u32 as i32)))?;
        let parent_pidl = pidl_from_path(&parent)?;

        let mut full_pidls = Vec::with_capacity(paths.len());
        for path in paths {
            full_pidls.push(pidl_from_path(path)?);
        }

        let mut child_pidls: Vec<*mut ITEMIDLIST> = full_pidls
            .iter()
            .map(|pidl| unsafe { ILFindLastID(*pidl) })
            .collect();

        let parent_folder: IShellFolder =
            unsafe { SHBindToParent::<IShellFolder>(full_pidls[0], None)? };

        let mut dcm = DEFCONTEXTMENU::default();
        dcm.hwnd = hwnd;
        dcm.pcmcb = ManuallyDrop::new(None);
        dcm.pidlFolder = parent_pidl;
        dcm.psf = ManuallyDrop::new(Some(parent_folder));
        dcm.cidl = child_pidls.len() as u32;
        dcm.apidl = child_pidls.as_mut_ptr();
        dcm.punkAssociationInfo = ManuallyDrop::new(None);
        dcm.cKeys = 0;
        dcm.aKeys = std::ptr::null();

        let context_menu: IContextMenu = unsafe { SHCreateDefaultContextMenu(&dcm)? };

        let menu = unsafe { CreatePopupMenu()? };

        unsafe {
            let _ = context_menu.QueryContextMenu(
                menu,
                0,
                CONTEXT_MENU_ID_BASE,
                CONTEXT_MENU_ID_MAX,
                CMF_NORMAL,
            );
        }

        let mut shell_context_menu = Self {
            context_menu,
            menu,
            items: Vec::new(),
            selected_paths: paths.to_vec(),
            id_base: CONTEXT_MENU_ID_BASE,
            full_pidls,
            parent_pidl,
        };

        shell_context_menu.with_active_message_handler(|| {
            initialize_shell_submenus(hwnd, menu);
        });
        debug_dump_native_menu(menu, "after initialization");
        let mut items = collect_menu_items(menu);
        replace_send_to_items(&mut items);
        shell_context_menu.items = items;
        debug_dump_shell_items(&shell_context_menu.items, "converted items");

        Ok(shell_context_menu)
    }

    pub fn items(&self) -> &[ShellContextMenuItem] {
        &self.items
    }

    pub fn invoke(&self, hwnd: HWND, id: u32) -> windows::core::Result<()> {
        let verb_offset = id.saturating_sub(self.id_base) as usize;
        let info = CMINVOKECOMMANDINFOEX {
            cbSize: size_of::<CMINVOKECOMMANDINFOEX>() as u32,
            fMask: 0,
            hwnd,
            lpVerb: PCSTR(verb_offset as *const u8),
            lpVerbW: PCWSTR::null(),
            nShow: 1,
            ..Default::default()
        };

        unsafe {
            self.context_menu
                .InvokeCommand(&info as *const _ as *const CMINVOKECOMMANDINFO)
        }
    }

    pub fn invoke_item(
        &self,
        hwnd: HWND,
        item: &ShellContextMenuItem,
    ) -> windows::core::Result<()> {
        if let Some(destination) = &item.send_to_path {
            return self.invoke_send_to(hwnd, destination);
        }
        self.invoke(hwnd, item.id)
    }

    fn invoke_send_to(&self, hwnd: HWND, destination: &Path) -> windows::core::Result<()> {
        if destination
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("DeskLink"))
        {
            return create_desktop_shortcuts(&self.selected_paths);
        }

        if destination
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("ZFSendToTarget"))
        {
            return create_compressed_archive(&self.selected_paths);
        }

        let mut parameters = String::new();
        for path in &self.selected_paths {
            if !parameters.is_empty() {
                parameters.push(' ');
            }
            parameters.push('"');
            parameters.push_str(&path.to_string_lossy().replace('"', "\\\""));
            parameters.push('"');
        }
        let file: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let parameters: Vec<u16> = parameters.encode_utf16().chain(Some(0)).collect();

        let mut info = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_INVOKEIDLIST,
            hwnd,
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(parameters.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        unsafe { ShellExecuteExW(&mut info) }
    }

    /// Displays the complete shell-provided menu at the current pointer
    /// position and invokes the command selected by the user.
    pub fn show_native(&self, hwnd: HWND) -> windows::core::Result<()> {
        let mut point = windows::Win32::Foundation::POINT::default();
        let result = self.with_active_message_handler(|| unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point)?;
            let command = TrackPopupMenuEx(
                self.menu,
                (TPM_RETURNCMD | TPM_RIGHTBUTTON).0,
                point.x,
                point.y,
                hwnd,
                None,
            );
            let command = command.0 as u32;
            if command != 0 {
                self.invoke(hwnd, command)?;
            }
            Ok(())
        });
        result
    }

    fn with_active_message_handler<R>(&self, callback: impl FnOnce() -> R) -> R {
        ACTIVE_SHELL_CONTEXT_MENU.store(self as *const _ as *mut _, Ordering::SeqCst);
        let result = callback();
        ACTIVE_SHELL_CONTEXT_MENU.store(std::ptr::null_mut(), Ordering::SeqCst);
        result
    }

    fn handle_menu_message(&self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        if let Ok(context_menu) = self.context_menu.cast::<IContextMenu3>() {
            let mut result = LRESULT(0);
            if unsafe { context_menu.HandleMenuMsg2(msg, wparam, lparam, Some(&mut result)) }
                .is_ok()
            {
                return Some(result);
            }
        }

        if let Ok(context_menu) = self.context_menu.cast::<IContextMenu2>() {
            if unsafe { context_menu.HandleMenuMsg(msg, wparam, lparam) }.is_ok() {
                return Some(LRESULT(0));
            }
        }

        None
    }
}

fn create_desktop_shortcuts(paths: &[PathBuf]) -> windows::core::Result<()> {
    let desktop = dirs::desktop_dir()
        .or_else(|| dirs::home_dir().map(|path| path.join("Desktop")))
        .ok_or_else(|| Error::from(HRESULT(0x80004005u32 as i32)))?;

    for source in paths {
        let source_name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Shortcut".to_string());
        let shortcut_path = unique_shortcut_path(&desktop, &source_name);
        let source_wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let shortcut_wide: Vec<u16> = shortcut_path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();

        unsafe {
            let shell_link: IShellLinkW = CoCreateInstance(
                &windows::core::GUID::from_u128(0x00021401_0000_0000_c000_000000000046),
                None,
                CLSCTX_INPROC_SERVER,
            )?;
            shell_link.SetPath(PCWSTR(source_wide.as_ptr()))?;
            let persist_file: IPersistFile = shell_link.cast()?;
            persist_file.Save(PCWSTR(shortcut_wide.as_ptr()), true)?;
        }
    }

    Ok(())
}

fn unique_shortcut_path(desktop: &Path, source_name: &str) -> PathBuf {
    let base = format!("{source_name} - Shortcut");
    let first = desktop.join(format!("{base}.lnk"));
    if !first.exists() {
        return first;
    }

    for index in 2..10_000 {
        let candidate = desktop.join(format!("{base} ({index}).lnk"));
        if !candidate.exists() {
            return candidate;
        }
    }

    desktop.join(format!("{base} ({:?}).lnk", std::time::SystemTime::now()))
}

fn create_compressed_archive(paths: &[PathBuf]) -> windows::core::Result<()> {
    let Some(first_path) = paths.first() else {
        return Err(Error::from(HRESULT(0x80004005u32 as i32)));
    };

    let parent = first_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let archive_base = if paths.len() == 1 {
        first_path
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Archive".to_string())
    } else {
        "Archive".to_string()
    };
    let archive_path = unique_archive_path(parent, &archive_base);

    let literal_paths = paths
        .iter()
        .map(|path| powershell_literal(&path.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(", ");
    let destination = powershell_literal(&archive_path.to_string_lossy());
    let script = format!(
        "$ErrorActionPreference = 'Stop'; Compress-Archive -LiteralPath @({literal_paths}) -DestinationPath {destination}"
    );

    // Compress-Archive is intentionally run out of process. Shell compression can take
    // noticeable time for large selections, and Send to should not block Eden's UI thread.
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-WindowStyle",
        "Hidden",
        "-Command",
        &script,
    ]);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| Error::new(HRESULT(0x80004005u32 as i32), error.to_string()))
}

fn unique_archive_path(parent: &Path, archive_base: &str) -> PathBuf {
    let first = parent.join(format!("{archive_base}.zip"));
    if !first.exists() {
        return first;
    }

    for index in 2..10_000 {
        let candidate = parent.join(format!("{archive_base} ({index}).zip"));
        if !candidate.exists() {
            return candidate;
        }
    }

    parent.join(format!(
        "{archive_base} ({:?}).zip",
        std::time::SystemTime::now()
    ))
}

fn powershell_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Forwards native menu messages from Eden's window procedure to the active
/// shell context menu while it is being displayed or initialized.
pub(crate) unsafe fn forward_active_shell_menu_message(
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    let pointer = ACTIVE_SHELL_CONTEXT_MENU.load(Ordering::SeqCst);
    if pointer.is_null() {
        return None;
    }

    let result = unsafe { (&*pointer).handle_menu_message(msg, wparam, lparam) };

    if msg == windows::Win32::UI::WindowsAndMessaging::WM_INITMENUPOPUP
        && std::env::var_os("EDEN_DEBUG_SHELL_MENU").is_some()
    {
        let submenu =
            windows::Win32::UI::WindowsAndMessaging::HMENU(wparam.0 as *mut std::ffi::c_void);
        let count =
            unsafe { windows::Win32::UI::WindowsAndMessaging::GetMenuItemCount(Some(submenu)) };
        eprintln!(
            "[shell-menu] forwarded WM_INITMENUPOPUP menu={:?} index={} result_handled={} count_after={count}",
            submenu.0,
            lparam.0,
            result.is_some(),
        );
    }

    result
}

impl Drop for ShellContextMenu {
    fn drop(&mut self) {
        unsafe {
            if !self.menu.is_invalid() {
                let _ = DestroyMenu(self.menu);
            }
            for pidl in self.full_pidls.drain(..) {
                CoTaskMemFree(Some(pidl as _));
            }
            if !self.parent_pidl.is_null() {
                CoTaskMemFree(Some(self.parent_pidl as _));
            }
        }
    }
}

fn shared_parent(paths: &[PathBuf]) -> Option<PathBuf> {
    let parent = paths.first()?.parent()?.to_path_buf();
    if paths.iter().all(|p| p.parent() == Some(parent.as_path())) {
        Some(parent)
    } else {
        None
    }
}

fn pidl_from_path(path: &Path) -> windows::core::Result<*mut ITEMIDLIST> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let pidl = unsafe { ILCreateFromPathW(PCWSTR(wide.as_ptr())) };
    if pidl.is_null() {
        Err(Error::from(HRESULT(0x80004005u32 as i32)))
    } else {
        Ok(pidl)
    }
}

fn collect_menu_items(menu: HMENU) -> Vec<ShellContextMenuItem> {
    let mut items = Vec::new();
    let count = unsafe { GetMenuItemCount(Some(menu)) };
    if count <= 0 {
        return items;
    }

    for index in 0..count {
        let mut buffer = [0u16; 512];
        let mut info = MENUITEMINFOW {
            cbSize: size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_FTYPE | MIIM_ID | MIIM_STATE | MIIM_STRING | MIIM_SUBMENU,
            dwTypeData: PWSTR(buffer.as_mut_ptr()),
            cch: buffer.len() as u32,
            ..Default::default()
        };

        let success = unsafe { GetMenuItemInfoW(menu, index as u32, true, &mut info) };
        if success.is_err() {
            continue;
        }

        if info.fType.contains(MFT_SEPARATOR) {
            continue;
        }

        let raw_label = if info.cch == 0 {
            String::new()
        } else {
            String::from_utf16_lossy(&buffer[..info.cch as usize])
        };
        let label = sanitize_label(&raw_label);

        if !info.hSubMenu.is_invalid() {
            let children = collect_menu_items(info.hSubMenu);
            if !children.is_empty() {
                items.push(ShellContextMenuItem {
                    id: info.wID,
                    label,
                    disabled: info.fState.contains(MFS_DISABLED)
                        || info.fState.contains(MFS_GRAYED),
                    children,
                    send_to_path: None,
                });
            }
            continue;
        }

        if label.is_empty() || info.wID == 0 {
            continue;
        }

        let disabled = info.fState.contains(MFS_DISABLED) || info.fState.contains(MFS_GRAYED);

        items.push(ShellContextMenuItem {
            id: info.wID,
            label,
            disabled,
            children: Vec::new(),
            send_to_path: None,
        });
    }

    items
}

fn replace_send_to_items(items: &mut [ShellContextMenuItem]) {
    for item in items {
        if item.label.eq_ignore_ascii_case("Send to") {
            let send_to_items = enumerate_send_to_items();
            if !send_to_items.is_empty() {
                item.children = send_to_items;
            }
        } else {
            replace_send_to_items(&mut item.children);
        }
    }
}

fn enumerate_send_to_items() -> Vec<ShellContextMenuItem> {
    let Some(send_to_dir) =
        dirs::data_dir().map(|path| path.join("Microsoft").join("Windows").join("SendTo"))
    else {
        return Vec::new();
    };

    let mut items = read_dir(send_to_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if !path.is_file()
                || path
                    .file_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case("desktop.ini"))
            {
                return None;
            }

            let label = shell_display_name(&path).or_else(|| {
                path.file_stem()
                    .or_else(|| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
            })?;

            Some(ShellContextMenuItem {
                id: 0,
                label,
                disabled: false,
                children: Vec::new(),
                send_to_path: Some(path),
            })
        })
        .collect::<Vec<_>>();

    items.sort_by_key(|item| item.label.to_ascii_lowercase());
    items
}

fn shell_display_name(path: &Path) -> Option<String> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let item: IShellItem =
        unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }.ok()?;
    let name = unsafe { item.GetDisplayName(SIGDN_NORMALDISPLAY) }.ok()?;
    let text = unsafe {
        let mut length = 0;
        while *name.0.add(length) != 0 {
            length += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(name.0, length))
    };
    unsafe { CoTaskMemFree(Some(name.0 as _)) };
    (!text.is_empty()).then_some(text)
}

fn debug_dump_native_menu(menu: HMENU, stage: &str) {
    if std::env::var_os("EDEN_DEBUG_SHELL_MENU").is_none() {
        return;
    }

    eprintln!("[shell-menu] {stage}: menu={:?}", menu.0);
    debug_dump_native_menu_level(menu, 0);
}

fn debug_dump_native_menu_level(menu: HMENU, depth: usize) {
    let count = unsafe { GetMenuItemCount(Some(menu)) };
    eprintln!(
        "[shell-menu] {}menu={:?} count={count}",
        "  ".repeat(depth),
        menu.0
    );

    if count <= 0 {
        return;
    }

    for index in 0..count {
        let mut buffer = [0u16; 512];
        let mut info = MENUITEMINFOW {
            cbSize: size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_FTYPE | MIIM_ID | MIIM_STATE | MIIM_STRING | MIIM_SUBMENU,
            dwTypeData: PWSTR(buffer.as_mut_ptr()),
            cch: buffer.len() as u32,
            ..Default::default()
        };

        if unsafe { GetMenuItemInfoW(menu, index as u32, true, &mut info) }.is_err() {
            eprintln!(
                "[shell-menu] {}[{index}] <GetMenuItemInfoW failed>",
                "  ".repeat(depth + 1)
            );
            continue;
        }

        let label = if info.cch == 0 {
            String::new()
        } else {
            String::from_utf16_lossy(&buffer[..info.cch as usize])
        };
        eprintln!(
            "[shell-menu] {}[{index}] label={label:?} id={} submenu={:?} type={} state={}",
            "  ".repeat(depth + 1),
            info.wID,
            info.hSubMenu.0,
            info.fType.0,
            info.fState.0,
        );

        if !info.hSubMenu.is_invalid() {
            debug_dump_native_menu_level(info.hSubMenu, depth + 1);
        }
    }
}

fn debug_dump_shell_items(items: &[ShellContextMenuItem], stage: &str) {
    if std::env::var_os("EDEN_DEBUG_SHELL_MENU").is_none() {
        return;
    }

    eprintln!("[shell-menu] {stage}:");
    debug_dump_shell_items_level(items, 0);
}

fn debug_dump_shell_items_level(items: &[ShellContextMenuItem], depth: usize) {
    for item in items {
        eprintln!(
            "[shell-menu] {}label={:?} id={} disabled={} children={}",
            "  ".repeat(depth),
            item.label,
            item.id,
            item.disabled,
            item.children.len(),
        );
        debug_dump_shell_items_level(&item.children, depth + 1);
    }
}

/// Some shell extensions add submenu entries lazily in response to
/// WM_INITMENUPOPUP. Explorer sends that notification before displaying a
/// submenu; do the same before converting the native menu into Eden items.
fn initialize_shell_submenus(hwnd: HWND, menu: HMENU) {
    // The shell can defer constructing a submenu until its containing popup
    // is initialized. Initialize the current popup before inspecting its
    // children, including the root popup created by Eden.
    send_shell_menu_message(hwnd, menu, 0);

    let count = unsafe { GetMenuItemCount(Some(menu)) };
    if count <= 0 {
        return;
    }

    for index in 0..count {
        let mut info = MENUITEMINFOW {
            cbSize: size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_SUBMENU,
            ..Default::default()
        };

        if unsafe { GetMenuItemInfoW(menu, index as u32, true, &mut info) }.is_err()
            || info.hSubMenu.is_invalid()
        {
            continue;
        }

        send_shell_menu_message(hwnd, info.hSubMenu, index);
        initialize_shell_submenus(hwnd, info.hSubMenu);
    }
}

fn send_shell_menu_message(hwnd: HWND, submenu: HMENU, index: i32) {
    let lparam = LPARAM(index as isize);
    unsafe {
        let _ = SendMessageW(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::WM_INITMENUPOPUP,
            Some(WPARAM(submenu.0 as usize)),
            Some(lparam),
        );
    }
}

fn sanitize_label(raw: &str) -> String {
    let no_amp = raw.replace('&', "");
    no_amp
        .split('\t')
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}
