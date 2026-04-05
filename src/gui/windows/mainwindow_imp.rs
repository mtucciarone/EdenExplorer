use crate::core::drives::{get_drive_infos, is_raw_physical_drive_path};
use crate::core::fs::FileItem;
use crate::core::fs::{parallel_directory_scan, scan_dir_async};
use crate::core::indexer::{
    load_app_settings, save_app_settings, save_favorites, save_theme_settings,
};
use crate::gui::MainWindow;
use crate::gui::theme::{ThemeMode, ThemePalette, get_default_palette, set_palette};
use crate::gui::utils::{
    SortColumn, get_clipboard_files, is_clipboard_cut, set_clipboard_files,
    shell_delete_to_recycle_bin, show_copy_move_dialog, sort_files,
};
use crate::gui::windows::about::draw_about_window;
use crate::gui::windows::containers::enums::{
    ItemViewerAction, ItemViewerContextAction, TabbarNavAction,
};
use crate::gui::windows::containers::structs::{
    FavoriteItem, ItemViewerFolderSizeState, RenameState, SidebarAction, TabState, TabbarAction,
    TabsAction, TopbarAction,
};
use crate::gui::windows::customizetheme::draw_theme_customizer;
use crate::gui::windows::enums::{SettingsAction, ThemeCustomizerAction};
use crate::gui::windows::settings::draw_settings_window;
use crate::gui::windows::structs::{Navigation, ThemeCustomizer};
use crate::gui::windows::windowsoverrides::mark_clipboard_dirty;
use crossbeam_channel::Receiver;
use crossbeam_channel::{Sender, unbounded};
use eframe::egui;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::Shell::{SEE_MASK_INVOKEIDLIST, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::{Win32::System::Com::*, Win32::UI::Shell::*, core::*};

impl MainWindow {
    pub fn current_nav(&self) -> &Navigation {
        &self.tabs[self.active_tab].nav
    }

    pub fn current_nav_mut(&mut self) -> &mut Navigation {
        &mut self.tabs[self.active_tab].nav
    }

    pub fn open_new_tab(&mut self, path: PathBuf) {
        let nav = Navigation::new(path);
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(TabState {
            id,
            nav,
            breadcrumb_path_editing: false,
            breadcrumb_path_buffer: String::new(),
            breadcrumb_just_started_editing: false,
            breadcrumb_path_error: false,
            breadcrumb_path_error_animation_time: 0.0,
        });
        self.active_tab = self.tabs.len() - 1;
        self.mark_tab_infos_dirty();
    }

    pub fn default_favorites(&self) -> Vec<FavoriteItem> {
        let mut favorites = Vec::new();
        if let Some(home) = dirs::home_dir() {
            let desktop = home.join("Desktop");
            favorites.push(FavoriteItem {
                path: desktop,
                label: "Desktop".to_string(),
            });
            let documents = home.join("Documents");
            favorites.push(FavoriteItem {
                path: documents,
                label: "Documents".to_string(),
            });
            let downloads = home.join("Downloads");
            favorites.push(FavoriteItem {
                path: downloads,
                label: "Downloads".to_string(),
            });
            let pictures = home.join("Pictures");
            favorites.push(FavoriteItem {
                path: pictures,
                label: "Pictures".to_string(),
            });
        }
        favorites
    }

    pub fn toggle_sort(&mut self, col: SortColumn) {
        if self.sort_column == col {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort_column = col;
            self.sort_ascending = true;
        }

        sort_files(&mut self.files, self.sort_column, self.sort_ascending);
    }

    pub fn load_path(&mut self) {
        self.files.clear();
        self.rx = None;
        self.size_req_tx = None;
        self.size_rx = None;
        self.folder_sizes.clear();
        self.file_size_text_cache.clear();
        self.folder_size_text_cache.clear();
        self.drive_size_text_cache.clear();
        self.pending_size_queue.clear();
        self.pending_size_set.clear();
        self.explorer_state.selected_paths.clear();
        self.explorer_state.selection_anchor = None;
        self.explorer_state.selection_focus = None;
        self.item_viewer_filter_state.dirty = true;
        self.item_viewer_filter_state.cached_indices.clear();
        self.is_loading = false;

        if is_raw_physical_drive_path(&self.current_nav().current) {
            self.explorer_state.non_ntfs_popup_path = Some(self.current_nav().current.clone());
            return;
        }

        if self.current_nav().is_root() {
            for d in get_drive_infos() {
                let label = d.display;
                let path = d.path;
                if let (Some(total), Some(free)) = (d.total_space, d.free_space) {
                    self.files.push(FileItem::with_drive_info(
                        label, path, true, None, None, None, total, free,
                    ));
                } else {
                    self.files
                        .push(FileItem::new(label, path, true, None, None, None));
                }
            }

            sort_files(&mut self.files, self.sort_column, self.sort_ascending);
            return;
        }

        // Async directory listing
        let (tx, rx) = unbounded();
        scan_dir_async(self.current_nav().current.clone(), tx);
        self.rx = Some(rx);
        self.is_loading = true;

        // Setup folder size calculation channels only if folder scanning is enabled
        if self
            .settings_window
            .current_settings
            .folder_scanning_enabled
        {
            let (size_req_tx, size_req_rx) = unbounded::<PathBuf>();
            let (size_done_tx, size_done_rx) = unbounded::<(PathBuf, u64, bool)>();
            self.size_req_tx = Some(size_req_tx);
            self.size_rx = Some(size_done_rx);

            // Spawn a thread pool to handle folder size requests in parallel
            let num_threads = num_cpus::get().max(2); // use all available cores
            self.size_threads = calculate_folder_sizes_parallel(
                size_req_rx,
                size_done_tx,
                Arc::clone(&self.shutdown),
                num_threads,
            );
        }
    }

    pub fn create_new_folder(&mut self) {
        if self.current_nav().is_root() {
            return;
        }

        let base = self.current_nav().current.clone();
        let mut name = "New Folder".to_string();
        let mut counter = 1;
        let mut path = base.join(&name);
        while path.exists() {
            counter += 1;
            name = format!("New Folder ({})", counter);
            path = base.join(&name);
        }

        if std::fs::create_dir(&path).is_ok() {
            let path_for_selection = path.clone();
            self.load_path();
            // Immediately start renaming the new folder
            self.rename_state = Some(RenameState {
                path: path.clone(),
                new_name: name,
                should_focus: true,
            });
            // Select the new folder
            self.explorer_state.selected_paths.clear();
            self.explorer_state
                .selected_paths
                .insert(path_for_selection.clone());
            self.explorer_state.newly_created_path = Some(path_for_selection.clone());
        }
    }

    pub fn create_new_file(&mut self) {
        if self.current_nav().is_root() {
            return;
        }

        let base = self.current_nav().current.clone();
        let mut name = "New File.txt".to_string();
        let mut counter = 1;
        let mut path = base.join(&name);
        while path.exists() {
            counter += 1;
            name = format!("New File ({}).txt", counter);
            path = base.join(&name);
        }

        // Create an empty file
        if std::fs::write(&path, "").is_ok() {
            let path_for_selection = path.clone();
            self.load_path();
            // Immediately start renaming the new file
            self.rename_state = Some(RenameState {
                path: path.clone(),
                new_name: name,
                should_focus: true,
            });
            // Select the new file
            self.explorer_state.selected_paths.clear();
            self.explorer_state
                .selected_paths
                .insert(path_for_selection.clone());
            self.explorer_state.newly_created_path = Some(path_for_selection.clone());
        }
    }

    pub fn add_favorite(&mut self) {
        if self.current_nav().is_root() {
            return;
        }

        let path = self.current_nav().current.clone();
        if self
            .sidebar_state
            .favorites
            .iter()
            .any(|fav| fav.path == path)
        {
            return;
        }

        let label = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());

        self.sidebar_state
            .favorites
            .push(FavoriteItem { path, label });
        self.persist_favorites();
    }

    pub fn remove_favorite(&mut self, path: &PathBuf) {
        self.sidebar_state.favorites.retain(|fav| &fav.path != path);
        self.persist_favorites();
        if self
            .sidebar_state
            .item_clicked
            .as_ref()
            .map(|p| p == path)
            .unwrap_or(false)
        {
            self.sidebar_state.item_clicked = None;
        }
    }

    pub fn persist_favorites(&self) {
        let items: Vec<String> = self
            .sidebar_state
            .favorites
            .iter()
            .map(|fav| fav.path.display().to_string())
            .collect();
        save_favorites('C', &items);
    }

    pub fn handle_context_action(&mut self, action: ItemViewerContextAction) {
        match action {
            ItemViewerContextAction::Cut(paths) => {
                let _ = set_clipboard_files(&paths, true);
                mark_clipboard_dirty();
                if let Some(first) = paths.first() {
                    self.explorer_state.selected_paths.clear();
                    self.explorer_state.selected_paths.insert(first.clone());
                }
            }
            ItemViewerContextAction::Copy(paths) => {
                let _ = set_clipboard_files(&paths, false);
                mark_clipboard_dirty();
            }
            ItemViewerContextAction::Paste => {
                if let Err(e) = self.paste_clipboard_native() {
                    eprintln!("Paste failed: {}", e);
                }
            }
            ItemViewerContextAction::RenameRequest(path, new_name) => {
                let trimmed = new_name.trim();

                if trimmed.is_empty() {
                    self.rename_state = None;
                    return;
                }

                if let Some(parent) = path.parent() {
                    let target = parent.join(trimmed);

                    // Avoid no-op rename
                    if path != target {
                        unsafe {
                            use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
                            use windows::Win32::UI::Shell::{
                                FOF_ALLOWUNDO, FileOperation, IFileOperation, IShellItem,
                                SHCreateItemFromParsingName,
                            };
                            use windows::core::HSTRING;

                            let file_op: IFileOperation =
                                CoCreateInstance(&FileOperation, None, CLSCTX_ALL).unwrap();

                            file_op.SetOperationFlags(FOF_ALLOWUNDO).ok();

                            let source_item: IShellItem = SHCreateItemFromParsingName(
                                &HSTRING::from(path.to_string_lossy().to_string()),
                                None,
                            )
                            .unwrap();

                            // Rename keeps same parent, so only pass new name
                            file_op
                                .RenameItem(&source_item, &HSTRING::from(trimmed), None)
                                .ok();

                            file_op.PerformOperations().ok();
                        }

                        // Set the renamed file path for auto-selection and scrolling
                        self.explorer_state.newly_created_path = Some(target);
                    }
                }

                self.rename_state = None;
                self.load_path();
            }

            ItemViewerContextAction::RenameCancel => {
                self.rename_state = None;
            }
            ItemViewerContextAction::Delete(paths) => {
                if let Err(e) = self.delete_paths_native(paths.clone()) {
                    eprintln!("Native delete failed: {:?}", e);

                    // fallback (rare, but safe)
                    for path in paths {
                        self.delete_path(&path);
                    }
                }

                self.load_path();
            }
            ItemViewerContextAction::Properties(paths) => {
                self.open_properties_multi(&paths);
            }
        }
    }

    pub fn paste_clipboard_native(&mut self) -> windows::core::Result<()> {
        use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
        use windows::Win32::UI::Shell::{
            FOF_ALLOWUNDO, FOF_NOCONFIRMMKDIR, FOF_RENAMEONCOLLISION, FileOperation,
            IFileOperation, IShellItem, SHCreateItemFromParsingName,
        };
        use windows::core::HSTRING;

        let paths = match get_clipboard_files() {
            Some(p) if !p.is_empty() => p,
            _ => return Ok(()),
        };

        let is_cut = is_clipboard_cut();

        unsafe {
            let file_op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;

            file_op
                .SetOperationFlags(FOF_ALLOWUNDO | FOF_RENAMEONCOLLISION | FOF_NOCONFIRMMKDIR)?;

            let target_item: IShellItem = SHCreateItemFromParsingName(
                &HSTRING::from(self.current_nav().current.to_string_lossy().to_string()),
                None,
            )?;

            for path in paths {
                let source_item: IShellItem = SHCreateItemFromParsingName(
                    &HSTRING::from(path.to_string_lossy().to_string()),
                    None,
                )?;

                if is_cut {
                    file_op.MoveItem(&source_item, &target_item, None, None)?;
                } else {
                    file_op.CopyItem(&source_item, &target_item, None, None)?;
                }
            }

            file_op.PerformOperations()?;
        }

        self.load_path();
        Ok(())
    }

    pub fn delete_path(&self, path: &PathBuf) {
        if !shell_delete_to_recycle_bin(path) {
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(path);
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    pub fn delete_paths_native(&self, paths: Vec<PathBuf>) -> windows::core::Result<()> {
        use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
        use windows::Win32::UI::Shell::{
            FOF_ALLOWUNDO, FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName,
        };
        use windows::core::HSTRING;

        unsafe {
            let file_op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;

            // ✅ This enables recycle bin + undo
            file_op.SetOperationFlags(FOF_ALLOWUNDO | FOF_WANTNUKEWARNING)?;

            for path in paths {
                let item: IShellItem = SHCreateItemFromParsingName(
                    &HSTRING::from(path.to_string_lossy().to_string()),
                    None,
                )?;

                file_op.DeleteItem(&item, None)?;
            }

            file_op.PerformOperations()?;
        }

        Ok(())
    }

    pub fn open_properties_multi(&self, paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }

        if paths.len() == 1 {
            self.open_properties(&paths[0]);
            return;
        }

        if let Some(data_object) = create_data_object(paths) {
            unsafe {
                let _ = SHMultiFileProperties(&data_object, 0);
            }
        }
    }

    pub fn open_properties(&self, path: &Path) {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let verb: Vec<u16> = OsStr::new("properties")
            .encode_wide()
            .chain(Some(0))
            .collect();

        unsafe {
            let mut info = SHELLEXECUTEINFOW {
                cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
                fMask: SEE_MASK_INVOKEIDLIST,
                lpVerb: PCWSTR(verb.as_ptr()),
                lpFile: PCWSTR(wide.as_ptr()),
                nShow: SW_SHOW.0,
                ..Default::default()
            };
            let _ = ShellExecuteExW(&mut info);
        }
    }

    pub fn cleanup(&mut self) {
        // Signal all background threads to shutdown
        self.shutdown.store(true, Ordering::Relaxed);

        // Close channels to wake up waiting threads
        drop(self.size_req_tx.take());
        drop(self.size_rx.take());
        drop(self.rx.take());

        // Wait for all size calculation threads to finish
        for handle in self.size_threads.drain(..) {
            let _ = handle.join();
        }

        // Clear caches and collections
        self.folder_sizes.clear();
        self.files.clear();
        self.explorer_state.selected_paths.clear();
        self.pending_size_queue.clear();
        self.pending_size_set.clear();
        self.file_type_cache.clear();

        // Drop icon cache
        drop(self.icon_cache.take());
    }

    pub fn handle_draw_settings_window(&mut self, ctx: &egui::Context, palette: &ThemePalette) {
        if let Some(action) = draw_settings_window(ctx, &mut self.settings_window, palette) {
            match action {
                SettingsAction::ApplySettings => {
                    save_app_settings(
                        self.settings_window
                            .current_settings
                            .folder_scanning_enabled,
                        &self.settings_window.current_settings.window_size_mode,
                        &self.settings_window.current_settings.start_path,
                        Some(match self.theme {
                            crate::gui::theme::ThemeMode::Dark => "dark",
                            crate::gui::theme::ThemeMode::Light => "light",
                        }),
                        &self.settings_window.current_settings.pinned_tabs,
                    );
                }
                SettingsAction::ResetToDefaults => {
                    self.settings_window.current_settings = Default::default();
                }
                SettingsAction::ResetFavourites => {
                    self.sidebar_state.favorites = self.default_favorites();
                    self.persist_favorites();
                }
            }
        }
    }

    pub fn handle_draw_about_window(&mut self, ctx: &egui::Context, palette: &ThemePalette) {
        // TODO: Implement about window
        draw_about_window(ctx, &mut self.about_window, palette);
    }

    pub fn handle_tabbar_action(&mut self, tabbar_action: Option<TabbarAction>) {
        if let Some(action) = tabbar_action.as_ref().and_then(|t| t.nav.as_ref()) {
            match action {
                TabbarNavAction::Back => self.current_nav_mut().go_back(),
                TabbarNavAction::Forward => self.current_nav_mut().go_forward(),
                TabbarNavAction::Up => self.current_nav_mut().go_up(),
            }
            self.mark_tab_infos_dirty();
            self.load_path();
        } else {
            if let Some(path) = tabbar_action.as_ref().and_then(|t| t.nav_to.as_ref()) {
                self.current_nav_mut().go_to(path.clone());
                self.mark_tab_infos_dirty();
                self.load_path();
            }
            if tabbar_action
                .as_ref()
                .map(|t| t.refresh_current_directory)
                .unwrap_or(false)
            {
                self.load_path();
            }
            if tabbar_action
                .as_ref()
                .map(|t| t.create_folder)
                .unwrap_or(false)
            {
                self.create_new_folder();
            }
            if tabbar_action
                .as_ref()
                .map(|t| t.create_file)
                .unwrap_or(false)
            {
                self.create_new_file();
            }
            if tabbar_action
                .as_ref()
                .map(|t| t.add_favorite)
                .unwrap_or(false)
            {
                self.add_favorite();
            }
            if tabbar_action
                .as_ref()
                .map(|t| t.remove_favorite)
                .unwrap_or(false)
            {
                let path = self.current_nav().current.clone();
                self.remove_favorite(&path);
            }
        }
    }

    pub fn handle_tabs_action(&mut self, tabs_action: Option<TabsAction>) {
        if let Some(action) = tabs_action {
            if let Some(id) = action.activate {
                self.active_tab = self.tabs.iter().position(|t| t.id == id).unwrap();
                self.pending_tab_scroll_id = Some(id);
                self.load_path();
            }
            if action.open_new {
                let cloned_nav = self.current_nav().clone();
                let id = self.next_tab_id;
                self.next_tab_id += 1;
                self.tabs.push(TabState {
                    id,
                    nav: cloned_nav,
                    breadcrumb_path_editing: false,
                    breadcrumb_path_buffer: String::new(),
                    breadcrumb_just_started_editing: false,
                    breadcrumb_path_error: false,
                    breadcrumb_path_error_animation_time: 0.0,
                });
                self.active_tab = self.tabs.len() - 1;
                self.pending_tab_scroll_id = Some(id);
                self.mark_tab_infos_dirty();
                self.load_path();
            }
            if let Some(id) = action.close {
                if self.tabs.len() > 1 {
                    if let Some(idx) = self.tabs.iter().position(|t| t.id == id) {
                        self.tabs.remove(idx);
                        if self.active_tab >= self.tabs.len() {
                            self.active_tab = self.tabs.len() - 1;
                        }
                        if let Some(active_id) = self.tabs.get(self.active_tab).map(|t| t.id) {
                            self.pending_tab_scroll_id = Some(active_id);
                        }
                        self.mark_tab_infos_dirty();
                        self.load_path();
                    }
                } else {
                    let (
                        _folder_scanning_enabled,
                        _window_size_mode,
                        start_path,
                        _saved_theme,
                        _pinned_tabs,
                    ) = load_app_settings();
                    self.tabs[0].nav = Navigation::new(start_path);
                    self.active_tab = 0;
                    self.mark_tab_infos_dirty();
                    self.load_path();
                }
            }
            if let Some(path) = action.toggle_pin {
                if self
                    .settings_window
                    .current_settings
                    .pinned_tabs
                    .iter()
                    .any(|p| p == &path)
                {
                    self.settings_window
                        .current_settings
                        .pinned_tabs
                        .retain(|p| p != &path);
                } else {
                    self.settings_window.current_settings.pinned_tabs.push(path);
                }

                save_app_settings(
                    self.settings_window
                        .current_settings
                        .folder_scanning_enabled,
                    &self.settings_window.current_settings.window_size_mode,
                    &self.settings_window.current_settings.start_path,
                    Some(match self.theme {
                        ThemeMode::Dark => "dark",
                        ThemeMode::Light => "light",
                    }),
                    &self.settings_window.current_settings.pinned_tabs,
                );

                self.mark_tab_infos_dirty();
            }
        }
    }

    pub fn handle_sidebar_action(&mut self, sidebar_action: Option<SidebarAction>) {
        if let Some(action) = sidebar_action {
            if let Some((from, to)) = action.reorder {
                let len = self.sidebar_state.favorites.len();

                if from < len {
                    let item = self.sidebar_state.favorites.remove(from);

                    // Clamp target index AFTER removal
                    let mut target = to;

                    if to > from {
                        target -= 1;
                    }

                    target = target.min(self.sidebar_state.favorites.len());

                    self.sidebar_state.favorites.insert(target, item);
                }

                self.persist_favorites();
            }
            if let Some(path) = action.nav_to {
                self.current_nav_mut().go_to(path);
                self.mark_tab_infos_dirty();
                self.load_path();
            }
            if let Some(path) = action.open_new_tab {
                self.open_new_tab(path);
                self.load_path();
            }
            if let Some(path) = action.select_favorite {
                self.sidebar_state.item_clicked = Some(path);
            }
            if let Some(path) = action.remove_favorite {
                self.remove_favorite(&path);
            }
        }
    }

    pub fn handle_topbar_action(&mut self, topbar_action: Option<TopbarAction>) {
        if let Some(action) = topbar_action {
            if action.toggle_theme {
                self.theme = match self.theme {
                    ThemeMode::Dark => ThemeMode::Light,
                    ThemeMode::Light => ThemeMode::Dark,
                };
                self.theme_dirty = true;

                // Save the theme setting
                save_app_settings(
                    self.settings_window
                        .current_settings
                        .folder_scanning_enabled,
                    &self.settings_window.current_settings.window_size_mode,
                    &self.settings_window.current_settings.start_path,
                    Some(match self.theme {
                        ThemeMode::Dark => "dark",
                        ThemeMode::Light => "light",
                    }),
                    &self.settings_window.current_settings.pinned_tabs,
                );
            }

            if action.customize_theme {
                self.theme_customizer.open = true;
            }

            if action.open_settings {
                self.settings_window.open = true;
            }

            if action.about {
                self.about_window.open = true;
            }

            if action.exit {
                if let Some(hwnd) = self.hwnd {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                }
            }
        }
    }

    pub fn handle_throttle_size_requests(&mut self, ctx: &egui::Context) {
        // Throttle size requests to keep UI responsive
        if let Some(size_req_tx) = &self.size_req_tx {
            let should_pause =
                ctx.input(|i| i.pointer.any_down() || i.smooth_scroll_delta.y.abs() > 0.0);
            if !should_pause {
                for _ in 0..6 {
                    if let Some(path) = self.pending_size_queue.pop_front() {
                        let _ = size_req_tx.send(path);
                    } else {
                        break;
                    }
                }
            }
        }
    }

    pub fn handle_directory_size_updates(&mut self, ctx: &egui::Context) {
        // Folder size updates
        if let Some(size_rx) = &self.size_rx {
            let mut updated = false;

            for _ in 0..128 {
                match size_rx.try_recv() {
                    Ok((path, size, done)) => {
                        if done {
                            self.pending_size_set.remove(&path);
                        }
                        self.folder_sizes.insert(
                            path.clone(),
                            ItemViewerFolderSizeState { bytes: size, done },
                        );
                        if let Some(item) = self.files.iter_mut().find(|f| f.path == path) {
                            item.file_size = Some(size);
                            updated = true;
                        }
                    }
                    Err(_) => break,
                }
            }

            if updated {
                sort_files(&mut self.files, self.sort_column, self.sort_ascending);
                ctx.request_repaint();
            }
        }
    }

    pub fn handle_directory_batch_recieve(&mut self, ctx: &egui::Context) {
        // Batch receive
        if let Some(rx) = &self.rx {
            let mut batch = Vec::with_capacity(128);
            let mut disconnected = false;

            for _ in 0..128 {
                match rx.try_recv() {
                    Ok(item) => batch.push(item),
                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }

            if !batch.is_empty() {
                for item in batch.iter() {
                    if item.is_dir {
                        // Only set up folder size tracking if scanning is enabled
                        if self
                            .settings_window
                            .current_settings
                            .folder_scanning_enabled
                        {
                            self.folder_sizes.entry(item.path.clone()).or_insert(
                                ItemViewerFolderSizeState {
                                    bytes: 0,
                                    done: false,
                                },
                            );
                            if self.pending_size_set.insert(item.path.clone()) {
                                self.pending_size_queue.push_back(item.path.clone());
                            }
                        }
                    }
                }

                self.files.extend(batch);
                sort_files(&mut self.files, self.sort_column, self.sort_ascending);
                ctx.request_repaint();
            }

            if disconnected {
                self.rx = None;
                self.is_loading = false;
                ctx.request_repaint();
            }
        }
    }
}

fn split_parent(paths: &[PathBuf]) -> Option<(PathBuf, Vec<PathBuf>)> {
    if paths.is_empty() {
        return None;
    }

    let parent = paths[0].parent()?.to_path_buf();

    // Ensure all share same parent (Explorer requirement)
    if !paths.iter().all(|p| p.parent() == Some(parent.as_path())) {
        return None;
    }

    let children: Vec<PathBuf> = paths
        .iter()
        .filter_map(|p| p.file_name().map(PathBuf::from))
        .collect();

    Some((parent, children))
}

pub fn create_data_object(paths: &[PathBuf]) -> Option<IDataObject> {
    let (parent, children) = split_parent(paths)?;

    unsafe {
        // Parent PIDL
        let parent_w: Vec<u16> = parent.as_os_str().encode_wide().chain(Some(0)).collect();
        let parent_pidl = ILCreateFromPathW(PCWSTR(parent_w.as_ptr()));
        if parent_pidl.is_null() {
            return None;
        }

        let mut child_pidls: Vec<*const ITEMIDLIST> = Vec::new();

        for child in children {
            let full = parent.join(child);
            let wide: Vec<u16> = full.as_os_str().encode_wide().chain(Some(0)).collect();

            let full_pidl = ILCreateFromPathW(PCWSTR(wide.as_ptr()));
            if !full_pidl.is_null() {
                // 🔥 Convert to relative PIDL
                let rel = ILFindLastID(full_pidl);
                child_pidls.push(rel);
                CoTaskMemFree(Some(full_pidl as _));
            }
        }

        let result: Result<IDataObject> =
            SHCreateDataObject(Some(parent_pidl), Some(&child_pidls), None);

        CoTaskMemFree(Some(parent_pidl as _));

        result.ok()
    }
}

pub fn calculate_folder_sizes_parallel(
    req_rx: Receiver<PathBuf>,
    done_tx: Sender<(PathBuf, u64, bool)>,
    shutdown: Arc<AtomicBool>,
    num_threads: usize,
) -> Vec<std::thread::JoinHandle<()>> {
    let req_rx = Arc::new(req_rx);
    let mut handles = Vec::with_capacity(num_threads);

    for _ in 0..num_threads {
        let rx = Arc::clone(&req_rx);
        let tx = done_tx.clone();
        let shutdown = Arc::clone(&shutdown);

        let handle = thread::spawn(move || {
            // Loop until channel closes or shutdown is triggered
            while !shutdown.load(Ordering::Relaxed) {
                // Use blocking recv; will wake immediately on a message or channel close
                let path = match rx.recv() {
                    Ok(p) => p,
                    Err(_) => break, // Channel closed
                };

                // Optional: check shutdown inside heavy work
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }

                parallel_directory_scan(path, tx.clone());

                // Optional: if parallel_directory_scan is very CPU heavy,
                // you can add a small yield:
                // thread::yield_now();
            }
        });

        handles.push(handle);
    }

    handles
}

pub fn handle_pending_actions(pending_action: Option<ItemViewerAction>, explorer: &mut MainWindow) {
    if let Some(action) = pending_action {
        match action {
            ItemViewerAction::Sort(col) => explorer.toggle_sort(col),
            ItemViewerAction::Select(path) => {
                explorer.explorer_state.selected_paths.insert(path.clone());

                if let Some(idx) = explorer.files.iter().position(|f| f.path == path) {
                    explorer.explorer_state.selection_anchor = Some(idx);
                    explorer.explorer_state.selection_focus = Some(idx);
                }
            }
            ItemViewerAction::Deselect(path) => {
                explorer.explorer_state.selected_paths.remove(&path);
            }
            ItemViewerAction::SelectAll => {
                explorer.explorer_state.selected_paths.clear();
                for file in &explorer.files {
                    explorer
                        .explorer_state
                        .selected_paths
                        .insert(file.path.clone());
                }
            }
            ItemViewerAction::DeselectAll => {
                explorer.explorer_state.selected_paths.clear();
            }
            ItemViewerAction::RangeSelect(paths) => {
                // Clear current selection and add all range-selected files
                explorer.explorer_state.selected_paths.clear();
                for path in &paths {
                    explorer.explorer_state.selected_paths.insert(path.clone());
                }

                // Set selection_focus to the edge of the range that is farthest from the anchor
                if let Some(anchor_idx) = explorer.explorer_state.selection_anchor {
                    if let (Some(first_path), Some(last_path)) = (paths.first(), paths.last()) {
                        let first_idx = explorer
                            .files
                            .iter()
                            .position(|f| &f.path == first_path)
                            .unwrap_or(anchor_idx);
                        let last_idx = explorer
                            .files
                            .iter()
                            .position(|f| &f.path == last_path)
                            .unwrap_or(anchor_idx);

                        // If moving down, focus the last item; if moving up, focus the first item
                        explorer.explorer_state.selection_focus =
                            Some(if anchor_idx <= first_idx {
                                last_idx // moved down
                            } else {
                                first_idx // moved up
                            });
                    }
                }
            }
            ItemViewerAction::Open(path) => {
                explorer.explorer_state.selected_paths.clear();
                explorer.explorer_state.selected_paths.insert(path.clone());
                explorer.item_viewer_filter_state.dirty = true;
                explorer.item_viewer_filter_state.cached_indices.clear();
                explorer.current_nav_mut().go_to(path);
                explorer.mark_tab_infos_dirty();
                explorer.load_path();
            }
            ItemViewerAction::OpenWithDefault(paths) => {
                for path in paths {
                    let path_str = path.to_string_lossy().to_string();
                    let wide_path: Vec<u16> = OsStr::new(&path_str)
                        .encode_wide()
                        .chain(std::iter::once(0))
                        .collect();

                    unsafe {
                        let result = ShellExecuteW(
                            None,
                            PCWSTR::null(),
                            PCWSTR(wide_path.as_ptr()),
                            PCWSTR::null(),
                            PCWSTR::null(),
                            SW_SHOWNORMAL,
                        );

                        if result.0 <= std::ptr::null_mut() {
                            eprintln!("Failed to open file: {}", path.display());
                        }
                    }
                }
            }
            ItemViewerAction::OpenInNewTab(path) => {
                explorer.open_new_tab(path);
                explorer.load_path();
            }
            ItemViewerAction::Context(action) => {
                explorer.handle_context_action(action);
            }
            ItemViewerAction::StartEdit(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string());
                explorer.rename_state = Some(RenameState {
                    path,
                    new_name: name,
                    should_focus: true,
                });
            }
            ItemViewerAction::ReplaceSelection(path) => {
                explorer.explorer_state.selected_paths.clear();
                explorer.explorer_state.selected_paths.insert(path.clone());
                if let Some(idx) = explorer.files.iter().position(|f| f.path == path) {
                    explorer.explorer_state.selection_anchor = Some(idx);
                    explorer.explorer_state.selection_focus = Some(idx);
                }
            }
            ItemViewerAction::FilesDropped(dropped_files) => {
                let valid_files: Vec<PathBuf> =
                    dropped_files.into_iter().filter(|p| p.exists()).collect();

                if valid_files.is_empty() {
                    return;
                }

                explorer.dropped_files = valid_files.clone();

                let current_path = explorer.current_nav().current.clone();

                if let Err(e) = show_copy_move_dialog(valid_files, &current_path) {
                    eprintln!("Failed to show copy/move dialog: {}", e);
                }

                // ✅ Defer refresh (important)
                explorer.dropped_files_pending_ui_refresh = true;
            }
            ItemViewerAction::BackNavigation => {
                explorer.current_nav_mut().go_back();
                explorer.mark_tab_infos_dirty();
                explorer.load_path();
                explorer.explorer_state.selection_anchor = None;
                explorer.explorer_state.selected_paths.clear();
                explorer.explorer_state.selection_focus = None;
            }
            ItemViewerAction::MoveItems {
                sources,
                target_dir,
            } => {
                unsafe {
                    let file_op: IFileOperation =
                        CoCreateInstance(&FileOperation, None, CLSCTX_ALL).unwrap();

                    // Optional: show UI + allow TeraCopy hooks
                    file_op
                        .SetOperationFlags(
                            FOF_SIMPLEPROGRESS | FOF_ALLOWUNDO | FOFX_SHOWELEVATIONPROMPT,
                        )
                        .ok();

                    // Convert target dir to IShellItem
                    let target_item: IShellItem = SHCreateItemFromParsingName(
                        &HSTRING::from(target_dir.to_string_lossy().to_string()),
                        None,
                    )
                    .unwrap();

                    for source in sources {
                        let source_item: IShellItem = SHCreateItemFromParsingName(
                            &HSTRING::from(source.to_string_lossy().to_string()),
                            None,
                        )
                        .unwrap();

                        file_op
                            .MoveItem(&source_item, &target_item, None, None)
                            .ok();
                    }

                    file_op.PerformOperations().ok();
                }

                explorer.explorer_state.selected_paths.clear();
                explorer.explorer_state.selection_anchor = None;
                explorer.explorer_state.selection_focus = None;
                explorer.load_path();
            }
            ItemViewerAction::MoveFilesToBreadcrumbDirectory {
                sources,
                target_dir,
            } => {
                unsafe {
                    let file_op: IFileOperation =
                        CoCreateInstance(&FileOperation, None, CLSCTX_ALL).unwrap();

                    // Optional: show UI + allow TeraCopy hooks
                    file_op
                        .SetOperationFlags(
                            FOF_SIMPLEPROGRESS | FOF_ALLOWUNDO | FOFX_SHOWELEVATIONPROMPT,
                        )
                        .ok();

                    // Convert target dir to IShellItem
                    let target_item: IShellItem = SHCreateItemFromParsingName(
                        &HSTRING::from(target_dir.to_string_lossy().to_string()),
                        None,
                    )
                    .unwrap();

                    for source in sources {
                        let source_item: IShellItem = SHCreateItemFromParsingName(
                            &HSTRING::from(source.to_string_lossy().to_string()),
                            None,
                        )
                        .unwrap();

                        file_op
                            .MoveItem(&source_item, &target_item, None, None)
                            .ok();
                    }

                    file_op.PerformOperations().ok();
                }

                explorer.explorer_state.selected_paths.clear();
                explorer.explorer_state.selection_anchor = None;
                explorer.explorer_state.selection_focus = None;
                explorer.load_path();
            }
            ItemViewerAction::MoveFilesToTabDirectory {
                sources,
                target_dir,
            } => {
                unsafe {
                    let file_op: IFileOperation =
                        CoCreateInstance(&FileOperation, None, CLSCTX_ALL).unwrap();

                    // Optional: show UI + allow TeraCopy hooks
                    file_op
                        .SetOperationFlags(
                            FOF_SIMPLEPROGRESS | FOF_ALLOWUNDO | FOFX_SHOWELEVATIONPROMPT,
                        )
                        .ok();

                    // Convert target dir to IShellItem
                    let target_item: IShellItem = SHCreateItemFromParsingName(
                        &HSTRING::from(target_dir.to_string_lossy().to_string()),
                        None,
                    )
                    .unwrap();

                    for source in sources {
                        let source_item: IShellItem = SHCreateItemFromParsingName(
                            &HSTRING::from(source.to_string_lossy().to_string()),
                            None,
                        )
                        .unwrap();

                        file_op
                            .MoveItem(&source_item, &target_item, None, None)
                            .ok();
                    }

                    file_op.PerformOperations().ok();
                }

                explorer.explorer_state.selected_paths.clear();
                explorer.explorer_state.selection_anchor = None;
                explorer.explorer_state.selection_focus = None;
                explorer.load_path();
            }
        }
    }
}

pub fn handle_draw_customizetheme_window(
    ctx: &egui::Context,
    theme_customizer: &mut ThemeCustomizer,
    palette: &ThemePalette,
    current_mode: ThemeMode,
    theme_dirty: &mut bool,
) {
    if let Some(action) = draw_theme_customizer(ctx, theme_customizer, palette) {
        match action {
            ThemeCustomizerAction::ThemeUpdated(mode) => {
                let updated = match mode {
                    ThemeMode::Dark => theme_customizer.dark_palette.clone(),
                    ThemeMode::Light => theme_customizer.light_palette.clone(),
                };
                set_palette(mode, updated);
                save_theme_settings(
                    &theme_customizer.light_palette,
                    &theme_customizer.dark_palette,
                );

                if mode == current_mode {
                    *theme_dirty = true;
                }
            }
            ThemeCustomizerAction::ResetToDefaults(mode) => {
                let default = get_default_palette(mode);
                match mode {
                    ThemeMode::Dark => theme_customizer.dark_palette = default.clone(),
                    ThemeMode::Light => theme_customizer.light_palette = default.clone(),
                }
                set_palette(mode, default);
                save_theme_settings(
                    &theme_customizer.light_palette,
                    &theme_customizer.dark_palette,
                );

                if mode == current_mode {
                    *theme_dirty = true;
                }
            }
            ThemeCustomizerAction::ExportTheme(mode) => {
                let palette_to_export = match mode {
                    ThemeMode::Dark => &theme_customizer.dark_palette,
                    ThemeMode::Light => &theme_customizer.light_palette,
                };

                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Theme JSON", &["json"])
                    .set_file_name(match mode {
                        ThemeMode::Dark => "eden_theme_dark.json",
                        ThemeMode::Light => "eden_theme_light.json",
                    })
                    .save_file()
                {
                    if let Ok(json) = serde_json::to_string_pretty(palette_to_export) {
                        let _ = std::fs::write(path, json);
                    }
                }
            }
            ThemeCustomizerAction::ImportTheme(mode) => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Theme JSON", &["json"])
                    .pick_file()
                {
                    if let Ok(json) = std::fs::read_to_string(path) {
                        if let Ok(imported) = serde_json::from_str::<ThemePalette>(&json) {
                            match mode {
                                ThemeMode::Dark => theme_customizer.dark_palette = imported.clone(),
                                ThemeMode::Light => {
                                    theme_customizer.light_palette = imported.clone()
                                }
                            }
                            set_palette(mode, imported);
                            save_theme_settings(
                                &theme_customizer.light_palette,
                                &theme_customizer.dark_palette,
                            );

                            if mode == current_mode {
                                *theme_dirty = true;
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn tab_title_for(nav: &Navigation) -> String {
    if nav.is_root() {
        return "This PC".to_string();
    }

    nav.current
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| nav.current.display().to_string())
}
