use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, GetDC, GetDIBits, GetObjectW,
    HBITMAP, ReleaseDC,
};
use windows::Win32::System::Com::{CoTaskMemFree, IBindCtx};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    BHID_SFUIObject, CMF_EXTENDEDVERBS, CMF_NORMAL, CMINVOKECOMMANDINFO, CMINVOKECOMMANDINFOEX,
    IContextMenu, IContextMenu3, ILCreateFromPathW, ILFindLastID, IShellFolder, IShellItemArray,
    SHBindToObject, SHBindToParent, SHCreateShellItemArray,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, GetMenuItemCount, GetMenuItemInfoW, HMENU, MENUITEMINFOW,
    MFS_DISABLED, MFS_GRAYED, MFT_SEPARATOR, MIIM_BITMAP, MIIM_FTYPE, MIIM_ID, MIIM_STATE,
    MIIM_STRING, MIIM_SUBMENU, WM_INITMENUPOPUP,
};
use windows::core::{Error, HRESULT, Interface, PCSTR, PCWSTR, PWSTR};

const CONTEXT_MENU_ID_BASE: u32 = 1;
const CONTEXT_MENU_ID_MAX: u32 = 0x7FFF;

#[derive(Clone, Debug)]
pub struct ShellContextMenuItem {
    pub id: u32,
    pub label: String,
    pub disabled: bool,
    /// RGBA pixels + (width, height), when the shell extension provided a real bitmap
    /// rather than an owner-drawn (`HBMMENU_CALLBACK`) icon.
    pub icon_rgba: Option<(Vec<u8>, u32, u32)>,
    /// `Some` for a group like "7-Zip" or "Send to" - Explorer nests these as
    /// an actual submenu you hover into, rather than flattening their entries
    /// into the parent list. `id` is meaningless for these (they're never
    /// invoked directly, only their children are).
    pub submenu: Option<Vec<ShellContextMenuItem>>,
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
        if shared_parent(paths).is_none() {
            return Err(Error::from(HRESULT(0x80004005u32 as i32)));
        }

        let mut full_pidls = Vec::with_capacity(paths.len());
        for path in paths {
            full_pidls.push(pidl_from_path(path)?);
        }

        let child_pidls: Vec<*mut ITEMIDLIST> = full_pidls
            .iter()
            .map(|pidl| unsafe { ILFindLastID(*pidl) })
            .collect();

        let parent_folder: IShellFolder =
            unsafe { SHBindToParent::<IShellFolder>(full_pidls[0], None)? };

        // `IShellItemArray::BindToHandler(BHID_SFUIObject)` is the modern,
        // recommended way to get "the same context menu Explorer would show"
        // for a set of items. It goes through the item's full association
        // chain (ProgID, then `SystemFileAssociations\<ext>`, then
        // `SystemFileAssociations\<PerceivedType>`) when merging static
        // verbs, which is how e.g. a `SystemFileAssociations\image\shell\...`
        // verb (a "Convert to PNG/JPG/WEBP" tool registered against the
        // *image* perceived type, not a specific extension) gets included.
        // The plainer `IShellFolder::GetUIObjectOf`/`SHCreateDefaultContextMenu`
        // paths don't reliably resolve that PerceivedType-level association.
        let apidl: Vec<*const ITEMIDLIST> = child_pidls.iter().map(|p| *p as *const _).collect();
        let item_array: IShellItemArray =
            unsafe { SHCreateShellItemArray(None, &parent_folder, Some(&apidl))? };
        let context_menu: IContextMenu =
            unsafe { item_array.BindToHandler(None::<&IBindCtx>, &BHID_SFUIObject)? };
        let _ = hwnd; // no longer needed to build the menu; still used at invoke() time

        let menu = unsafe { CreatePopupMenu()? };

        unsafe {
            // `CMF_EXTENDEDVERBS` matters here: some shell extensions (image
            // conversion tools among them) only register their items when
            // this flag is present - Explorer itself passes it whenever
            // Shift is held during right-click, which is easy to not
            // realize is why an item is "missing" otherwise.
            if let Err(err) = context_menu
                .QueryContextMenu(
                    menu,
                    0,
                    CONTEXT_MENU_ID_BASE,
                    CONTEXT_MENU_ID_MAX,
                    CMF_NORMAL | CMF_EXTENDEDVERBS,
                )
                .ok()
            {
                eprintln!("QueryContextMenu failed: {err}");
            }
        }

        // Some verbs (this app's own motivating case: a "SubCommands"-based
        // submenu like "Convert Image" pointing at CommandStore entries) are
        // populated lazily rather than eagerly during `QueryContextMenu` -
        // real Explorer triggers that population by forwarding
        // `WM_INITMENUPOPUP` to the extension (via `IContextMenu3::
        // HandleMenuMsg2`) right before actually displaying each popup.
        // Since this app draws its own UI instead of showing the native
        // `HMENU`, nothing would ever send that message, so do it ourselves
        // for every level as we walk the tree.
        let ctx_menu3 = context_menu.cast::<IContextMenu3>().ok();

        let mut items = Vec::new();
        collect_menu_items(menu, ctx_menu3.as_ref(), &mut items);

        Ok(Self {
            context_menu,
            menu,
            items,
            id_base: CONTEXT_MENU_ID_BASE,
            full_pidls,
            parent_pidl: std::ptr::null_mut(),
        })
    }

    /// The "background" context menu for a folder itself - what Explorer shows
    /// when you right-click empty space inside a folder (View, Sort by, New,
    /// Paste, Refresh, ...), as opposed to `for_paths`' per-item menu (Cut,
    /// Copy, Rename, Properties, ...). Obtained via `IShellFolder::
    /// CreateViewObject`, which is a different Shell API entirely from the
    /// per-item `IContextMenu` path.
    pub fn for_background(dir: &Path, hwnd: HWND) -> windows::core::Result<Self> {
        let pidl = pidl_from_path(dir)?;

        // `psf = None` tells `SHBindToObject` to treat `pidl` as absolute
        // (desktop-relative) rather than relative to some other folder.
        let folder: IShellFolder = unsafe {
            SHBindToObject(
                None::<&IShellFolder>,
                pidl,
                None::<&windows::Win32::System::Com::IBindCtx>,
            )?
        };

        let context_menu: IContextMenu = unsafe { folder.CreateViewObject(hwnd)? };

        let menu = unsafe { CreatePopupMenu()? };

        unsafe {
            // `CMF_EXTENDEDVERBS` matters here: some shell extensions (image
            // conversion tools among them) only register their items when
            // this flag is present - Explorer itself passes it whenever
            // Shift is held during right-click, which is easy to not
            // realize is why an item is "missing" otherwise.
            if let Err(err) = context_menu
                .QueryContextMenu(
                    menu,
                    0,
                    CONTEXT_MENU_ID_BASE,
                    CONTEXT_MENU_ID_MAX,
                    CMF_NORMAL | CMF_EXTENDEDVERBS,
                )
                .ok()
            {
                eprintln!("QueryContextMenu failed: {err}");
            }
        }

        let ctx_menu3 = context_menu.cast::<IContextMenu3>().ok();

        let mut items = Vec::new();
        collect_menu_items(menu, ctx_menu3.as_ref(), &mut items);

        Ok(Self {
            context_menu,
            menu,
            items,
            id_base: CONTEXT_MENU_ID_BASE,
            full_pidls: vec![pidl],
            parent_pidl: std::ptr::null_mut(),
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
        Err(Error::from(HRESULT(0x80004005u32 as i32)))
    } else {
        Ok(pidl)
    }
}

fn collect_menu_items(
    menu: HMENU,
    ctx_menu3: Option<&IContextMenu3>,
    items: &mut Vec<ShellContextMenuItem>,
) {
    if let Some(cm3) = ctx_menu3 {
        unsafe {
            let _ = cm3.HandleMenuMsg2(WM_INITMENUPOPUP, WPARAM(menu.0 as usize), LPARAM(0), None);
        }
    }

    let count = unsafe { GetMenuItemCount(Some(menu)) };
    if count <= 0 {
        return;
    }

    for index in 0..count {
        let mut buffer = [0u16; 512];
        let mut info = MENUITEMINFOW {
            cbSize: size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_FTYPE | MIIM_ID | MIIM_STATE | MIIM_STRING | MIIM_SUBMENU | MIIM_BITMAP,
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
            if label.is_empty() {
                // An unlabeled submenu (rare) - flatten its children into the
                // parent instead of showing an empty-named group.
                collect_menu_items(info.hSubMenu, ctx_menu3, items);
                continue;
            }

            let mut sub_items = Vec::new();
            collect_menu_items(info.hSubMenu, ctx_menu3, &mut sub_items);
            if sub_items.is_empty() {
                continue;
            }

            let disabled = info.fState.contains(MFS_DISABLED) || info.fState.contains(MFS_GRAYED);
            items.push(ShellContextMenuItem {
                id: 0,
                label,
                disabled,
                icon_rgba: bitmap_to_rgba(info.hbmpItem),
                submenu: Some(sub_items),
            });
            continue;
        }

        if label.is_empty() || info.wID == 0 {
            continue;
        }

        let disabled = info.fState.contains(MFS_DISABLED) || info.fState.contains(MFS_GRAYED);
        let icon_rgba = bitmap_to_rgba(info.hbmpItem);

        items.push(ShellContextMenuItem {
            id: info.wID,
            label,
            disabled,
            icon_rgba,
            submenu: None,
        });
    }
}

/// Reads a menu item's bitmap as straight RGBA pixels, if it's a real bitmap the shell
/// extension set eagerly. Returns `None` for owner-drawn items (`HBMMENU_CALLBACK` and
/// the other `HBMMENU_*` pseudo-handles, which are small sentinel integers rather than
/// real GDI object handles) - those icons are only obtainable by implementing the
/// WM_MEASUREITEM/WM_DRAWITEM owner-draw protocol, which needs a native message loop.
fn bitmap_to_rgba(hbmp: HBITMAP) -> Option<(Vec<u8>, u32, u32)> {
    let raw = hbmp.0 as isize;
    if hbmp.0.is_null() || (-1..=11).contains(&raw) {
        return None;
    }

    unsafe {
        let mut bmp = BITMAP::default();
        if GetObjectW(
            hbmp.into(),
            size_of::<BITMAP>() as i32,
            Some(&mut bmp as *mut _ as _),
        ) == 0
        {
            return None;
        }

        let width = bmp.bmWidth as u32;
        let height = bmp.bmHeight as u32;

        if width == 0 || height == 0 || width > 256 || height > 256 {
            return None;
        }

        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let hdc = GetDC(None);

        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = width as i32;
        bmi.bmiHeader.biHeight = -(height as i32);
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let res = GetDIBits(
            hdc,
            hbmp,
            0,
            height,
            Some(pixels.as_mut_ptr() as _),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, hdc);

        if res == 0 {
            return None;
        }

        // Convert BGRA -> RGBA.
        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2);
        }

        // Many shell bitmaps don't carry a real alpha channel; treat all-zero alpha as opaque.
        if pixels.chunks_exact(4).all(|px| px[3] == 0) {
            for px in pixels.chunks_exact_mut(4) {
                px[3] = 255;
            }
        }

        Some((pixels, width, height))
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
