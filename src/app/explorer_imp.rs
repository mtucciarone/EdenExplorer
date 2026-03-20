use super::*;
use crossbeam_channel::Receiver;
use crossbeam_channel::{unbounded, Sender};
use eframe::egui;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{SW_SHOW, SW_SHOWNORMAL};
use super::itemviewer::{ItemViewerAction, ItemViewerContextAction, RenameState};
use super::sorting::{sort_files, SortColumn};
use crate::app::customizetheme::{draw_theme_customizer, ThemeCustomizer, ThemeCustomizerAction};
use crate::app::features::ThemePalette;
use crate::app::settings::{draw_settings_window, SettingsAction};
use crate::app::sidebar::FavoriteItem;
use crate::app::tabs::TabState;
use crate::app::utils::{
    clear_clipboard_files, copy_dir_recursive, get_clipboard_files, set_clipboard_files,
    shell_delete_to_recycle_bin,
};
use crate::drives::{get_drives, parse_drive_display};
use crate::fs::{get_drive_space, parallel_directory_scan, scan_dir_async};
use crate::indexer::{save_app_settings, save_favorites};
use crate::state::{execute_op, FileItem, FileOp, Navigation};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_INVOKEIDLIST, SHELLEXECUTEINFOW};

impl ExplorerApp {
    pub fn current_nav(&self) -> &Navigation {
        &self.tabs[self.active_tab].nav
    }

    pub fn current_nav_mut(&mut self) -> &mut Navigation {
        &mut self.tabs[self.active_tab].nav
    }

    pub fn open_new_tab(&mut self, path: PathBuf) {
        let mut nav = Navigation::new();
        nav.go_to(path);
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(TabState {
            id,
            nav,
            is_editing_path: false,
            path_buffer: String::new(),
        });
        self.active_tab = self.tabs.len() - 1;
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
        self.search_active = false;
        self.search_results.clear();
        self.selected_path = None;
        self.pending_size_queue.clear();
        self.pending_size_set.clear();

        if self.current_nav().is_root() {
            for d in get_drives() {
                let (label, path) = parse_drive_display(&d);

                if let Some((total, free)) = get_drive_space(&path) {
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

        // Setup folder size calculation channels
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
            self.selected_path = Some(path_for_selection.clone());
            self.selected_paths.clear();
            self.selected_paths.insert(path_for_selection);
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
            self.selected_path = Some(path_for_selection.clone());
            self.selected_paths.clear();
            self.selected_paths.insert(path_for_selection);
        }
    }

    pub fn add_favorite(&mut self) {
        if self.current_nav().is_root() {
            return;
        }

        let path = self.current_nav().current.clone();
        if self.favorites.iter().any(|fav| fav.path == path) {
            return;
        }

        let label = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());

        self.favorites.push(FavoriteItem { path, label });
        self.persist_favorites();
    }

    pub fn persist_favorites(&self) {
        let items: Vec<String> = self
            .favorites
            .iter()
            .map(|fav| fav.path.display().to_string())
            .collect();
        save_favorites('C', &items);
    }

    pub fn handle_context_action(&mut self, action: ItemViewerContextAction) {
        println!("Handling context action: {:?}", action);
        match action {
            ItemViewerContextAction::Cut(path) => {
                let _ = set_clipboard_files(&[path.clone()], true);
                self.selected_path = Some(path);
            }
            ItemViewerContextAction::Copy(path) => {
                println!("Copy action received for: {:?}", path);
                let _ = set_clipboard_files(&[path.clone()], false);
                self.selected_path = Some(path);
            }
            ItemViewerContextAction::Paste => {
                println!("Paste action received");
                self.paste_clipboard();
            }
            ItemViewerContextAction::Rename(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string());
                self.rename_state = Some(RenameState {
                    path,
                    new_name: name,
                    should_focus: true,
                });
            }
            ItemViewerContextAction::Delete(path) => {
                self.delete_path(&path);
                self.load_path();
            }
            ItemViewerContextAction::Properties(path) => {
                self.open_properties(&path);
            }
            ItemViewerContextAction::Undo => {
                self.undo();
            }
            ItemViewerContextAction::Redo => {
                self.redo();
            }
        }
    }

    pub fn paste_clipboard(&mut self) {
        let dest_dir = if self.current_nav().is_root() {
            return;
        } else {
            self.current_nav().current.clone()
        };

        let (paths, cut) = match get_clipboard_files() {
            Some(val) => val,
            None => return,
        };

        for path in paths {
            let name = match path.file_name() {
                Some(name) => name.to_string_lossy().to_string(),
                None => continue,
            };

            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| name.clone());

            let ext = path.extension().map(|e| e.to_string_lossy().to_string());

            let mut dest = dest_dir.join(&name);
            let mut counter = 1;

            while dest.exists() {
                counter += 1;

                let new_name = match &ext {
                    Some(ext) => format!("{} ({}).{}", stem, counter, ext),
                    None => format!("{} ({})", stem, counter),
                };

                dest = dest_dir.join(new_name);
            }

            let res = if cut {
                std::fs::rename(&path, &dest)
            } else if path.is_dir() {
                copy_dir_recursive(&path, &dest)
            } else {
                std::fs::copy(&path, &dest).map(|_| ())
            };

            if res.is_err() {
                continue;
            }
        }

        // ✅ Only clear clipboard if it was a CUT (move)
        if cut {
            clear_clipboard_files();
        }

        self.load_path();
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

    pub fn open_properties(&self, path: &PathBuf) {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let verb: Vec<u16> = OsStr::new("properties")
            .encode_wide()
            .chain(Some(0))
            .collect();

        unsafe {
            let mut info = SHELLEXECUTEINFOW::default();
            info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
            info.fMask = SEE_MASK_INVOKEIDLIST;
            info.lpVerb = PCWSTR(verb.as_ptr());
            info.lpFile = PCWSTR(wide.as_ptr());
            info.nShow = SW_SHOW.0 as i32;
            let _ = ShellExecuteExW(&mut info);
        }
    }

    pub fn undo(&mut self) {
        crate::state::undo(&mut self.action_history);
        self.load_path(); // refresh UI after filesystem change
    }

    pub fn redo(&mut self) {
        crate::state::redo(&mut self.action_history);
        self.load_path(); // refresh UI
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
        self.search_results.clear();
        self.selected_paths.clear();
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
                    );
                    self.settings_window.has_unsaved_changes = false;
                }
                SettingsAction::ResetToDefaults => {
                    self.settings_window.current_settings = Default::default();
                    self.settings_window.has_unsaved_changes = true;
                }
                SettingsAction::ResetFavourites => {
                    self.favorites = self.default_favorites();
                    self.persist_favorites();
                }
            }
        }
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

pub fn handle_pending_actions(
    pending_action: Option<ItemViewerAction>,
    explorer: &mut ExplorerApp,
) {
    if let Some(action) = pending_action {
        match action {
            ItemViewerAction::Sort(col) => explorer.toggle_sort(col),
            ItemViewerAction::Select(path) => {
                explorer.selected_path = Some(path.clone());
                explorer.selected_paths.insert(path);
            }
            ItemViewerAction::Deselect(path) => {
                explorer.selected_paths.remove(&path);
            }
            ItemViewerAction::SelectAll => {
                explorer.selected_paths.clear();
                for file in &explorer.files {
                    explorer.selected_paths.insert(file.path.clone());
                }
            }
            ItemViewerAction::DeselectAll => {
                explorer.selected_paths.clear();
            }
            ItemViewerAction::BoxSelect(paths) => {
                // Clear current selection and add box-selected files
                explorer.selected_paths.clear();
                for path in paths {
                    explorer.selected_paths.insert(path);
                }
            }
            ItemViewerAction::RangeSelect(paths) => {
                // Clear current selection and add range-selected files
                explorer.selected_paths.clear();
                for path in &paths {
                    explorer.selected_paths.insert(path.clone());
                }
                // Set the current position to the target edge of the range
                // The target should be the item that was just moved to
                if let Some(anchor_idx) = explorer.selection_anchor {
                    if let Some(current_selected) = explorer.selected_path.as_ref() {
                        if let Some(current_idx) = explorer
                            .files
                            .iter()
                            .position(|f| &f.path == current_selected)
                        {
                            // Determine which edge was just selected
                            if current_idx > anchor_idx {
                                // Moving down - set current to the bottom of range
                                if let Some(bottom_path) = paths.last() {
                                    explorer.selected_path = Some(bottom_path.clone());
                                }
                            } else if current_idx < anchor_idx {
                                // Moving up - set current to the top of range
                                if let Some(top_path) = paths.first() {
                                    explorer.selected_path = Some(top_path.clone());
                                }
                            }
                            // If current_idx == anchor_idx, no change needed
                        }
                    }
                }
            }
            ItemViewerAction::Open(path) => {
                explorer.selected_path = Some(path.clone());
                explorer.current_nav_mut().go_to(path);
                explorer.load_path();
            }
            ItemViewerAction::OpenWithDefault(path) => {
                // Open file with default Windows application
                let path_str = path.to_string_lossy().to_string();
                let wide_path: Vec<u16> = OsStr::new(&path_str)
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();

                unsafe {
                    let result = ShellExecuteW(
                        HWND::default(),
                        PCWSTR::null(),
                        PCWSTR(wide_path.as_ptr()),
                        PCWSTR::null(),
                        PCWSTR::null(),
                        SW_SHOWNORMAL,
                    );

                    // Check if the operation was successful (result > 32)
                    if result.0 <= 32 {
                        eprintln!("Failed to open file: {}", path.display());
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
            ItemViewerAction::RenameRequest(path, new_name) => {
                if let Some(parent) = path.parent() {
                    let target = parent.join(new_name.trim());

                    if new_name.is_empty() {
                        explorer.rename_state = None;
                        return;
                    }

                    // Avoid no-op rename
                    if path != target {
                        execute_op(
                            &mut explorer.action_history,
                            FileOp::Rename {
                                from: path.clone(),
                                to: target.clone(),
                            },
                        );
                    }

                    explorer.rename_state = None;
                    explorer.load_path();
                }
            }

            ItemViewerAction::RenameCancel => {
                explorer.rename_state = None;
            }
            ItemViewerAction::ReplaceSelection(path) => {
                explorer.selected_paths.clear();
                explorer.selected_paths.insert(path.clone());
                explorer.selected_path = Some(path.clone());
                // Set anchor index for extended selection
                if let Some(anchor_idx) = explorer.files.iter().position(|f| f.path == path) {
                    explorer.selection_anchor = Some(anchor_idx);
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

                if let Err(e) = crate::app::utils::show_copy_move_dialog(valid_files, &current_path)
                {
                    eprintln!("Failed to show copy/move dialog: {}", e);
                }

                // ✅ Defer refresh (important)
                explorer.pending_refresh = true;
            }
            ItemViewerAction::BackNavigation => {
                explorer.current_nav_mut().go_back();
                explorer.load_path();
            }
        }
    }
}

pub fn handle_draw_customizetheme_window(
    ctx: &egui::Context,
    theme_customizer: &mut ThemeCustomizer,
) {
    if let Some(action) = draw_theme_customizer(ctx, theme_customizer) {
        match action {
            ThemeCustomizerAction::ApplyTheme => {
                // Theme will be applied when theme_dirty is set to true
            }
            ThemeCustomizerAction::ResetToDefaults => {
                theme_customizer.current_theme = Default::default();
            }
            ThemeCustomizerAction::SaveTheme => {
                // implement later
            }
            ThemeCustomizerAction::LoadTheme => {
                // implement later
            }
            ThemeCustomizerAction::ExportTheme => {
                // implement later
            }
            ThemeCustomizerAction::ImportTheme => {
                // implement later
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
