use std::mem::{ManuallyDrop, size_of};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    CMF_NORMAL, CMINVOKECOMMANDINFO, CMINVOKECOMMANDINFOEX, DEFCONTEXTMENU, IContextMenu,
    ILCreateFromPathW, ILFindLastID, IShellFolder, SHBindToParent, SHCreateDefaultContextMenu,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, GetMenuItemCount, GetMenuItemInfoW, HMENU, MENUITEMINFOW,
    MFS_DISABLED, MFS_GRAYED, MFT_SEPARATOR, MIIM_FTYPE, MIIM_ID, MIIM_STATE, MIIM_STRING,
    MIIM_SUBMENU,
};
use windows::core::{PCSTR, PCWSTR, PWSTR};

const CONTEXT_MENU_ID_BASE: u32 = 1;
const CONTEXT_MENU_ID_MAX: u32 = 0x7FFF;

#[derive(Clone, Debug)]
pub struct ShellContextMenuItem {
    pub id: u32,
    pub label: String,
    pub disabled: bool,
}

pub struct ShellContextMenu {
    context_menu: IContextMenu,
    menu: HMENU,
    items: Vec<ShellContextMenuItem>,
    id_base: u32,
    full_pidls: Vec<*mut ITEMIDLIST>,
    parent_pidl: *mut ITEMIDLIST,
}

impl ShellContextMenu {
    pub fn for_paths(paths: &[PathBuf], hwnd: HWND) -> windows::core::Result<Self> {
        let parent = shared_parent(paths).ok_or_else(windows::core::Error::from_win32)?;
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

        let mut items = Vec::new();
        collect_menu_items(menu, "", &mut items);

        Ok(Self {
            context_menu,
            menu,
            items,
            id_base: CONTEXT_MENU_ID_BASE,
            full_pidls,
            parent_pidl,
        })
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
        Err(windows::core::Error::from_win32())
    } else {
        Ok(pidl)
    }
}

fn collect_menu_items(menu: HMENU, prefix: &str, items: &mut Vec<ShellContextMenuItem>) {
    let count = unsafe { GetMenuItemCount(Some(menu)) };
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
            let new_prefix = if label.is_empty() {
                prefix.to_string()
            } else {
                format!("{prefix}{label} > ")
            };
            collect_menu_items(info.hSubMenu, &new_prefix, items);
            continue;
        }

        if label.is_empty() || info.wID == 0 {
            continue;
        }

        let disabled = info.fState.contains(MFS_DISABLED) || info.fState.contains(MFS_GRAYED);

        items.push(ShellContextMenuItem {
            id: info.wID,
            label: format!("{prefix}{label}"),
            disabled,
        });
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
