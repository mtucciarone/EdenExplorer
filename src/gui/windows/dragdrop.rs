use crate::gui::dragdrop::{DragDropBackend, DropTargets, NativeDropCommand};
use crate::gui::windows::mainwindow_imp::create_data_object;
use crate::gui::windows::windowsoverrides::request_repaint;
use eframe::egui;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::{Arc, Mutex, RwLock};
use windows::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, HWND, POINT,
};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::{
    DVASPECT_CONTENT, FORMATETC, IDataObject, STGMEDIUM, TYMED_HGLOBAL,
};
use windows::Win32::System::Ole::{
    CF_HDROP, DoDragDrop, IDropSource, IDropSource_Impl, IDropTarget, IDropTarget_Impl,
    ReleaseStgMedium,
};
use windows::Win32::System::Ole::{DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_MOVE};
use windows::Win32::System::Ole::{RegisterDragDrop, RevokeDragDrop};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::DragQueryFileW;
use windows::Win32::UI::Shell::HDROP;
use windows::core::{BOOL, Error, HRESULT, Result, implement};

lazy_static::lazy_static! {
    static ref ACTIVE_SHARED: RwLock<Option<Arc<DropShared>>> = RwLock::new(None);
    static ref ACTIVE_HWND: RwLock<Option<isize>> = RwLock::new(None);
}

struct DropShared {
    targets: RwLock<DropTargets>,
    commands: Mutex<VecDeque<NativeDropCommand>>,
    drag_active: RwLock<bool>,
    inbound_drag_active: RwLock<bool>,
    hovered_target: RwLock<Option<PathBuf>>,
    scale_factor: RwLock<f32>,
}

impl DropShared {
    fn new() -> Self {
        Self {
            targets: RwLock::new(DropTargets::default()),
            commands: Mutex::new(VecDeque::new()),
            drag_active: RwLock::new(false),
            inbound_drag_active: RwLock::new(false),
            hovered_target: RwLock::new(None),
            scale_factor: RwLock::new(1.0),
        }
    }

    fn push_command(&self, command: NativeDropCommand) {
        if let Ok(mut queue) = self.commands.lock() {
            queue.push_back(command);
        }
    }

    fn pop_command(&self) -> Option<NativeDropCommand> {
        self.commands.lock().ok()?.pop_front()
    }

    fn targets(&self) -> DropTargets {
        self.targets.read().map(|t| t.clone()).unwrap_or_default()
    }

    fn set_targets(&self, targets: DropTargets) {
        if let Ok(mut current) = self.targets.write() {
            *current = targets;
        }
    }

    fn set_drag_active(&self, active: bool) {
        if let Ok(mut current) = self.drag_active.write() {
            *current = active;
        }
    }

    fn is_drag_active(&self) -> bool {
        self.drag_active.read().map(|v| *v).unwrap_or(false)
    }

    fn set_inbound_drag_active(&self, active: bool) {
        if let Ok(mut current) = self.inbound_drag_active.write() {
            *current = active;
        }
    }

    fn is_inbound_drag_active(&self) -> bool {
        self.inbound_drag_active.read().map(|v| *v).unwrap_or(false)
    }

    fn set_hovered_target(&self, target: Option<PathBuf>) {
        if let Ok(mut current) = self.hovered_target.write() {
            *current = target;
        }
    }

    fn hovered_target(&self) -> Option<PathBuf> {
        self.hovered_target.read().ok().and_then(|v| v.clone())
    }

    fn set_scale_factor(&self, scale_factor: f32) {
        if let Ok(mut current) = self.scale_factor.write() {
            *current = scale_factor.max(0.1);
        }
    }

    fn scale_factor(&self) -> f32 {
        self.scale_factor.read().map(|v| *v).unwrap_or(1.0).max(0.1)
    }
}

#[implement(IDropSource)]
struct FileDropSource;

impl IDropSource_Impl for FileDropSource_Impl {
    fn QueryContinueDrag(&self, fescapepressed: BOOL, grfkeystate: MODIFIERKEYS_FLAGS) -> HRESULT {
        if fescapepressed.as_bool() {
            return DRAGDROP_S_CANCEL;
        }

        if !grfkeystate.contains(MK_LBUTTON) {
            return DRAGDROP_S_DROP;
        }

        HRESULT(0)
    }

    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

#[implement(IDropTarget)]
struct WindowDropTarget;

fn active_shared() -> Option<Arc<DropShared>> {
    ACTIVE_SHARED.read().ok().and_then(|shared| shared.clone())
}

fn active_hwnd() -> Option<HWND> {
    ACTIVE_HWND
        .read()
        .ok()
        .and_then(|hwnd| hwnd.map(|raw| HWND(raw as *mut core::ffi::c_void)))
}

fn hit_test_target_at(pt: windows::Win32::Foundation::POINTL) -> Option<PathBuf> {
    let shared = active_shared()?;
    let hwnd = active_hwnd()?;
    let scale = shared.scale_factor();
    let mut client_pt = POINT { x: pt.x, y: pt.y };
    unsafe {
        if !ScreenToClient(hwnd, &mut client_pt).as_bool() {
            return None;
        }
    }
    let pos = egui::pos2(client_pt.x as f32 / scale, client_pt.y as f32 / scale);
    let targets = shared.targets();

    let ordered = [
        &targets.breadcrumb_target,
        &targets.tab_target,
        &targets.item_target,
    ];

    for region in ordered {
        if let (Some(target), Some(rect)) = (&region.target, region.rect) {
            if rect.contains(pos) {
                return Some(target.clone());
            }
        }
    }

    None
}

impl WindowDropTarget {
    fn current_effect(target: Option<PathBuf>) -> DROPEFFECT {
        if target.is_some() {
            DROPEFFECT_MOVE
        } else {
            DROPEFFECT_COPY
        }
    }
}

impl IDropTarget_Impl for WindowDropTarget_Impl {
    fn DragEnter(
        &self,
        _pdataobj: windows::core::Ref<'_, IDataObject>,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        _pt: &windows::Win32::Foundation::POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> Result<()> {
        let target = hit_test_target_at(*_pt);
        if let Some(shared) = active_shared() {
            shared.set_drag_active(true);
            shared.set_inbound_drag_active(true);
            shared.set_hovered_target(target.clone());
        }
        request_repaint();
        unsafe {
            *pdweffect = WindowDropTarget::current_effect(target);
        }
        Ok(())
    }

    fn DragOver(
        &self,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        _pt: &windows::Win32::Foundation::POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> Result<()> {
        let target = hit_test_target_at(*_pt);
        if let Some(shared) = active_shared() {
            shared.set_hovered_target(target.clone());
            shared.set_inbound_drag_active(true);
        }
        request_repaint();
        unsafe {
            *pdweffect = WindowDropTarget::current_effect(target);
        }
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        if let Some(shared) = active_shared() {
            shared.set_drag_active(false);
            shared.set_inbound_drag_active(false);
            shared.set_hovered_target(None);
        }
        request_repaint();
        Ok(())
    }

    fn Drop(
        &self,
        pdataobj: windows::core::Ref<'_, IDataObject>,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        _pt: &windows::Win32::Foundation::POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> Result<()> {
        let target = hit_test_target_at(*_pt);
        request_repaint();
        let Some(shared) = active_shared() else {
            return Ok(());
        };
        let sources = read_paths_from_data_object(pdataobj)?;

        let command = if let Some(target_dir) = target.clone() {
            NativeDropCommand::MoveFiles {
                sources,
                target_dir,
            }
        } else {
            NativeDropCommand::ImportFiles(sources)
        };

        shared.push_command(command);
        shared.set_drag_active(false);
        shared.set_inbound_drag_active(false);
        shared.set_hovered_target(None);

        unsafe {
            *pdweffect = WindowDropTarget::current_effect(target);
        }
        Ok(())
    }
}

fn read_paths_from_data_object(
    data_object: windows::core::Ref<'_, IDataObject>,
) -> Result<Vec<PathBuf>> {
    unsafe {
        let data_object = data_object.unwrap();
        let format = FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };

        let mut medium: STGMEDIUM = data_object.GetData(&format)?;
        if medium.tymed != TYMED_HGLOBAL.0 as u32 {
            ReleaseStgMedium(&mut medium);
            return Err(Error::from_win32());
        }

        let hdrop = HDROP(unsafe { medium.u.hGlobal.0 });
        let count = DragQueryFileW(hdrop, 0xFFFFFFFF, None);
        let mut paths = Vec::with_capacity(count as usize);

        for i in 0..count {
            let len = DragQueryFileW(hdrop, i, None);
            if len == 0 {
                continue;
            }

            let mut buf = vec![0u16; (len + 1) as usize];
            let copied = DragQueryFileW(hdrop, i, Some(&mut buf));
            if copied > 0 {
                paths.push(PathBuf::from(String::from_utf16_lossy(
                    &buf[..copied as usize],
                )));
            }
        }

        ReleaseStgMedium(&mut medium);
        Ok(paths)
    }
}

pub struct WindowsDragDropBackend {
    hwnd: Option<HWND>,
    shared: Arc<DropShared>,
    drop_target: Option<IDropTarget>,
}

impl WindowsDragDropBackend {
    pub fn new(hwnd: Option<HWND>) -> Self {
        let shared = Arc::new(DropShared::new());
        if let Ok(mut active) = ACTIVE_SHARED.write() {
            *active = Some(shared.clone());
        }
        if let Ok(mut active) = ACTIVE_HWND.write() {
            *active = hwnd.map(|h| h.0 as isize);
        }

        let drop_target = hwnd.and_then(|hwnd| {
            let target: IDropTarget = WindowDropTarget.into();

            unsafe {
                if RegisterDragDrop(hwnd, &target).is_ok() {
                    Some(target)
                } else {
                    None
                }
            }
        });

        Self {
            hwnd,
            shared,
            drop_target,
        }
    }

    pub fn uninstall(&mut self) {
        if let Some(hwnd) = self.hwnd.take() {
            let _ = unsafe { RevokeDragDrop(hwnd) };
        }
        self.drop_target = None;
        if let Ok(mut active) = ACTIVE_SHARED.write() {
            *active = None;
        }
        if let Ok(mut active) = ACTIVE_HWND.write() {
            *active = None;
        }
    }
}

impl Drop for WindowsDragDropBackend {
    fn drop(&mut self) {
        self.uninstall();
    }
}

impl DragDropBackend for WindowsDragDropBackend {
    fn begin_file_drag(&self, paths: &[PathBuf]) -> bool {
        if paths.is_empty() {
            return false;
        }

        let Some(data_object) = create_data_object(paths) else {
            return false;
        };

        let drop_source: IDropSource = FileDropSource.into();
        let mut effect = DROPEFFECT(0);
        self.shared.set_drag_active(true);
        let result = unsafe {
            DoDragDrop(
                &data_object,
                &drop_source,
                DROPEFFECT_COPY | DROPEFFECT_MOVE,
                &mut effect,
            )
        };
        self.shared.set_drag_active(false);
        self.shared.set_hovered_target(None);

        result.is_ok()
    }

    fn is_drag_active(&self) -> bool {
        self.shared.is_drag_active()
    }

    fn is_inbound_drag_active(&self) -> bool {
        self.shared.is_inbound_drag_active()
    }

    fn hovered_drop_target(&self) -> Option<PathBuf> {
        self.shared.hovered_target()
    }

    fn set_scale_factor(&self, scale_factor: f32) {
        self.shared.set_scale_factor(scale_factor);
    }

    fn update_drop_targets(&self, targets: DropTargets) {
        self.shared.set_targets(targets);
    }

    fn poll_command(&self) -> Option<NativeDropCommand> {
        self.shared.pop_command()
    }
}
