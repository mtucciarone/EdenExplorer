use eframe::egui;
use std::collections::HashMap;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel::{unbounded, Receiver, Sender};

use crate::app::icons::IconCache; // 🔥 FIXED PATH
use crate::app::sidebar::{draw_sidebar, FavoriteItem, SidebarAction, SidebarPalette};
use crate::app::tabs::{draw_tabbar, draw_tabs, TabInfo, TabbarNavAction};
use crate::app::topbar::draw_topbar;
use crate::app::utils::{
    clipboard_has_files, copy_dir_recursive, get_clipboard_files, set_clipboard_files,
    shell_delete_to_recycle_bin,
};
use crate::drives::{get_drive_infos, get_drives, parse_drive_display};
use crate::fs::{calculate_folder_size_fast_progress, get_drive_space, scan_dir_async};
use crate::indexer::{load_favorites, save_favorites, IndexStatus, Indexer};
use crate::state::{FileItem, Navigation};

use super::features::{apply_theme, palette, ThemeMode};
use super::itemviewer::{
    draw_item_viewer, ItemViewerAction, ItemViewerContextAction, ItemViewerFolderSizeState,
    RenameState,
};
use super::sorting::{sort_files, SortColumn};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows::core::PCWSTR;
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_INVOKEIDLIST, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOW;

struct TabState {
    id: u64,
    nav: Navigation,
}

pub struct ExplorerApp {
    tabs: Vec<TabState>,
    active_tab: usize,
    next_tab_id: u64,
    files: Vec<FileItem>,
    rx: Option<Receiver<FileItem>>,
    size_req_tx: Option<Sender<PathBuf>>,
    size_rx: Option<Receiver<(PathBuf, u64, bool)>>,
    folder_sizes: HashMap<PathBuf, ItemViewerFolderSizeState>,
    indexer: Arc<Indexer>,
    search_query: String,
    search_results: Vec<FileItem>,
    search_active: bool,
    favorites: Vec<FavoriteItem>,
    dragging_favorite: Option<usize>,
    sidebar_selected: Option<PathBuf>,
    selected_path: Option<PathBuf>,
    rename_state: Option<RenameState>,
    pending_size_queue: VecDeque<PathBuf>,
    pending_size_set: HashSet<PathBuf>,
    theme: ThemeMode,
    theme_dirty: bool,
    sort_column: SortColumn,
    sort_ascending: bool,
    icon_cache: Option<IconCache>, // 🔥 FIX: lazy init
    sidebar_width: f32,
    file_type_cache: HashMap<String, String>,
}

impl Default for ExplorerApp {
    fn default() -> Self {
        let mut app = Self {
            tabs: vec![TabState {
                id: 1,
                nav: Navigation::new(),
            }],
            active_tab: 0,
            next_tab_id: 2,
            files: vec![],
            rx: None,
            size_req_tx: None,
            size_rx: None,
            folder_sizes: HashMap::new(),
            indexer: Indexer::start('C'),
            search_query: String::new(),
            search_results: Vec::new(),
            search_active: false,
            favorites: Vec::new(),
            dragging_favorite: None,
            sidebar_selected: None,
            selected_path: None,
            rename_state: None,
            pending_size_queue: VecDeque::new(),
            pending_size_set: HashSet::new(),
            theme: ThemeMode::Dark,
            theme_dirty: true,
            sort_column: SortColumn::Name,
            sort_ascending: true,
            icon_cache: None, // 🔥 FIX
            sidebar_width: 200.0,
            file_type_cache: HashMap::new(),
        };

        let stored = load_favorites('C');
        if stored.is_empty() {
            app.favorites = app.default_favorites();
            app.persist_favorites();
        } else {
            app.favorites = stored
                .into_iter()
                .map(|path| {
                    let path = PathBuf::from(path);
                    let label = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string());
                    FavoriteItem { path, label }
                })
                .collect();
        }
        app.load_path();
        app
    }
}

impl ExplorerApp {
    fn current_nav(&self) -> &Navigation {
        &self.tabs[self.active_tab].nav
    }

    fn current_nav_mut(&mut self) -> &mut Navigation {
        &mut self.tabs[self.active_tab].nav
    }

    fn open_new_tab(&mut self, path: PathBuf) {
        let mut nav = Navigation::new();
        nav.go_to(path);
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(TabState { id, nav });
        self.active_tab = self.tabs.len() - 1;
    }

    fn default_favorites(&self) -> Vec<FavoriteItem> {
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

    fn toggle_sort(&mut self, col: SortColumn) {
        if self.sort_column == col {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort_column = col;
            self.sort_ascending = true;
        }

        sort_files(&mut self.files, self.sort_column, self.sort_ascending);
    }

    fn load_path(&mut self) {
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

        let (tx, rx) = unbounded();
        scan_dir_async(self.current_nav().current.clone(), tx);
        self.rx = Some(rx);

        let (size_req_tx, size_req_rx) = unbounded::<PathBuf>();
        let (size_done_tx, size_done_rx) = unbounded::<(PathBuf, u64, bool)>();
        self.size_req_tx = Some(size_req_tx);
        self.size_rx = Some(size_done_rx);

        std::thread::spawn(move || {
            while let Ok(path) = size_req_rx.recv() {
                calculate_folder_size_fast_progress(path, size_done_tx.clone());
            }
        });
    }

    fn create_new_folder(&mut self) {
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
            self.load_path();
        }
    }

    fn add_favorite(&mut self) {
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

    fn persist_favorites(&self) {
        let items: Vec<String> = self
            .favorites
            .iter()
            .map(|fav| fav.path.display().to_string())
            .collect();
        save_favorites('C', &items);
    }

    fn handle_context_action(&mut self, action: ItemViewerContextAction) {
        match action {
            ItemViewerContextAction::Cut(path) => {
                let _ = set_clipboard_files(&[path.clone()], true);
                self.selected_path = Some(path);
            }
            ItemViewerContextAction::Copy(path) => {
                let _ = set_clipboard_files(&[path.clone()], false);
                self.selected_path = Some(path);
            }
            ItemViewerContextAction::Paste => {
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
                todo!("Undo not implemented yet");
                // self.undo(); // implement this method or your undo logic here
            }
            ItemViewerContextAction::Redo => {
                todo!("Redo not implemented yet");
                // self.redo(); // implement this method or your redo logic here
            }
        }
    }

    fn paste_clipboard(&mut self) {
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

            let mut dest = dest_dir.join(&name);
            let mut counter = 1;
            while dest.exists() {
                counter += 1;
                let new_name = format!("{} ({})", name, counter);
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

        self.load_path();
    }

    fn delete_path(&self, path: &PathBuf) {
        if !shell_delete_to_recycle_bin(path) {
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(path);
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    fn open_properties(&self, path: &PathBuf) {
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
}

impl eframe::App for ExplorerApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if self.theme_dirty {
            apply_theme(ctx, self.theme);
            self.theme_dirty = false;
        }

        // Increase scroll speed for the explorer view.
        ctx.input_mut(|i| {
            i.raw_scroll_delta *= 8.0;
            i.smooth_scroll_delta *= 8.0;
        });

        // Init once
        if self.icon_cache.is_none() {
            self.icon_cache = Some(IconCache::new(ctx.clone()));
        }

        // 🔥 TAKE ownership (fixes borrow issues)
        let icon_cache = self.icon_cache.take().unwrap();

        // Batch receive
        if let Some(rx) = &self.rx {
            let mut batch = Vec::with_capacity(128);

            for _ in 0..128 {
                match rx.try_recv() {
                    Ok(item) => batch.push(item),
                    Err(_) => break,
                }
            }

            if !batch.is_empty() {
                for item in batch.iter() {
                    if item.is_dir {
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

                self.files.extend(batch);
                sort_files(&mut self.files, self.sort_column, self.sort_ascending);
                ctx.request_repaint();
            }
        }

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

        // Toolbar (left column)
        let palette = palette(self.theme);
        let mut topbar_action = None;

        // Throttle size requests to keep UI responsive
        if let Some(size_req_tx) = &self.size_req_tx {
            let should_pause =
                ctx.input(|i| i.pointer.any_down() || i.raw_scroll_delta.y.abs() > 0.0);
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

        // Main layout: sidebar + tabs column
        let sidebar_palette = SidebarPalette {
            hover: palette.sidebar_hover,
            active: palette.sidebar_active,
        };
        let mut sidebar_action: Option<SidebarAction> = None;
        let mut tabs_action = None;
        let mut tabbar_action = None;
        let nav_snapshot = self.current_nav().clone();
        let mut search_snapshot = self.search_query.clone();
        let mut pending_action: Option<ItemViewerAction> = None;

        egui::CentralPanel::default().show(ctx, |ui| {
            let avail = ui.available_size();
            ui.allocate_ui_with_layout(
                avail,
                egui::Layout::left_to_right(egui::Align::Min),
                |ui| {
                    // Sidebar column with resizable width
                    let sidebar_width = self.sidebar_width.max(140.0).min(280.0);
                    let sidebar_frame =
                        egui::Frame::NONE.inner_margin(egui::Margin::symmetric(12, 0));

                    // Left panel (sidebar)
                    ui.allocate_ui_with_layout(
                        egui::vec2(sidebar_width, ui.available_height()),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            egui::Frame::NONE.show(ui, |ui| {
                                topbar_action =
                                    Some(draw_topbar(ui, self.theme == ThemeMode::Dark));
                            });
                            sidebar_frame.show(ui, |ui| {
                                let drives = get_drive_infos();
                                sidebar_action = Some(draw_sidebar(
                                    ui,
                                    &icon_cache,
                                    &mut self.favorites,
                                    self.sidebar_selected.as_ref(),
                                    &drives,
                                    &sidebar_palette,
                                    &mut self.dragging_favorite,
                                ));

                                // Apply reorder
                                if let Some((from, to)) = sidebar_action.as_ref().unwrap().reorder {
                                    self.favorites.swap(from, to);
                                }
                            });
                        },
                    );

                    // Resize handle
                    let separator_response = ui.allocate_rect(
                        egui::Rect::from_min_size(
                            ui.cursor().min,
                            egui::vec2(4.0, ui.available_height()),
                        ),
                        egui::Sense::click_and_drag(),
                    );

                    if separator_response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }

                    if separator_response.dragged() {
                        self.sidebar_width = (self.sidebar_width
                            + separator_response.drag_delta().x)
                            .max(140.0)
                            .min(560.0);
                    }

                    // Tabs column
                    let right_size = egui::vec2(ui.available_width(), ui.available_height());
                    ui.allocate_ui_with_layout(
                        right_size,
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            let old_spacing = ui.spacing().item_spacing;
                            ui.spacing_mut().item_spacing.y = 0.0;

                            let tab_infos: Vec<TabInfo> = self
                                .tabs
                                .iter()
                                .map(|tab| TabInfo {
                                    id: tab.id,
                                    title: tab_title_for(&tab.nav),
                                })
                                .collect();
                            let active_id = self.tabs[self.active_tab].id;

                            tabs_action = Some(draw_tabs(ui, &tab_infos, active_id));

                            let container_stroke = ui.visuals().widgets.active.bg_stroke;
                            let container = egui::Frame::NONE
                                .stroke(container_stroke)
                                .inner_margin(egui::Margin::symmetric(10, 8))
                                .corner_radius(egui::CornerRadius {
                                    nw: 0,
                                    ne: 0,
                                    sw: 8,
                                    se: 8,
                                });

                            container.show(ui, |ui| {
                                tabbar_action = Some(draw_tabbar(
                                    ui,
                                    &icon_cache,
                                    &nav_snapshot,
                                    &mut search_snapshot,
                                ));

                                ui.add_space(4.0);

                                let display_files = if self.search_active {
                                    &self.search_results
                                } else {
                                    &self.files
                                };

                                egui::ScrollArea::vertical().auto_shrink([false; 2]).show(
                                    ui,
                                    |ui| {
                                        pending_action = draw_item_viewer(
                                            ui,
                                            display_files,
                                            &self.folder_sizes,
                                            self.selected_path.as_ref(),
                                            clipboard_has_files(),
                                            self.sort_column,
                                            self.sort_ascending,
                                            &icon_cache,
                                            self.rename_state.as_mut(),
                                            &palette,
                                            &mut self.file_type_cache,
                                        );
                                    },
                                );
                            });

                            ui.spacing_mut().item_spacing = old_spacing;
                        },
                    );
                },
            );
        });

        if let Some(action) = topbar_action {
            if action.toggle_theme {
                self.theme = match self.theme {
                    ThemeMode::Dark => ThemeMode::Light,
                    ThemeMode::Light => ThemeMode::Dark,
                };
                self.theme_dirty = true;
            }
        }

        if let Some(action) = sidebar_action {
            if let Some(path) = action.nav_to {
                self.current_nav_mut().go_to(path);
                self.load_path();
            }
            if let Some(path) = action.open_new_tab {
                self.open_new_tab(path);
                self.load_path();
            }
            if let Some(path) = action.select_favorite {
                self.sidebar_selected = Some(path);
            }
            if let Some(path) = action.remove_favorite {
                self.favorites.retain(|fav| fav.path != path);
                self.persist_favorites();
                if self
                    .sidebar_selected
                    .as_ref()
                    .map(|p| p == &path)
                    .unwrap_or(false)
                {
                    self.sidebar_selected = None;
                }
            }
        }

        if let Some(action) = tabs_action {
            if let Some(id) = action.activate {
                if let Some(idx) = self.tabs.iter().position(|t| t.id == id) {
                    self.active_tab = idx;
                    self.load_path();
                }
            }
            if action.open_new {
                let cloned_nav = self.current_nav().clone();
                let id = self.next_tab_id;
                self.next_tab_id += 1;
                self.tabs.push(TabState {
                    id,
                    nav: cloned_nav,
                });
                self.active_tab = self.tabs.len() - 1;
                self.load_path();
            }
            if let Some(id) = action.close {
                if self.tabs.len() > 1 {
                    if let Some(idx) = self.tabs.iter().position(|t| t.id == id) {
                        self.tabs.remove(idx);
                        if self.active_tab >= self.tabs.len() {
                            self.active_tab = self.tabs.len() - 1;
                        }
                        self.load_path();
                    }
                } else {
                    self.tabs[0].nav = Navigation::new();
                    self.active_tab = 0;
                    self.load_path();
                }
            }
        }

        if let Some(action) = tabbar_action {
            self.search_query = search_snapshot;
            if let Some(nav_action) = action.nav {
                match nav_action {
                    TabbarNavAction::Back => self.current_nav_mut().go_back(),
                    TabbarNavAction::Forward => self.current_nav_mut().go_forward(),
                    TabbarNavAction::Up => self.current_nav_mut().go_up(),
                }
                self.load_path();
            }
            if let Some(path) = action.nav_to {
                self.current_nav_mut().go_to(path);
                self.load_path();
            }
            if action.create_folder {
                self.create_new_folder();
            }
            if action.add_favorite {
                self.add_favorite();
            }
            if action.search_changed {
                let query = self.search_query.trim().to_string();
                if query.is_empty() {
                    self.search_active = false;
                    self.search_results.clear();
                } else if self.indexer.status() == IndexStatus::Ready {
                    let results = self.indexer.search(&query);
                    self.search_results = results
                        .into_iter()
                        .filter_map(|rec| self.record_to_file_item(rec))
                        .collect();
                    self.search_active = true;
                }
            }
        }

        if let Some(action) = pending_action {
            match action {
                ItemViewerAction::Sort(col) => self.toggle_sort(col),
                ItemViewerAction::Select(path) => {
                    self.selected_path = Some(path);
                }
                ItemViewerAction::Open(path) => {
                    self.selected_path = Some(path.clone());
                    self.current_nav_mut().go_to(path);
                    self.load_path();
                }
                ItemViewerAction::OpenInNewTab(path) => {
                    self.open_new_tab(path);
                    self.load_path();
                }
                ItemViewerAction::Context(action) => {
                    self.handle_context_action(action);
                }
                ItemViewerAction::StartEdit(path) => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string());
                    self.rename_state = Some(RenameState {
                        path,
                        new_name: name,
                    });
                }
                ItemViewerAction::RenameRequest(path, new_name) => {
                    if let Some(parent) = path.parent() {
                        let target = parent.join(new_name);
                        let _ = std::fs::rename(&path, &target);
                        self.rename_state = None;
                        self.load_path();
                    }
                }
                ItemViewerAction::RenameCancel => {
                    self.rename_state = None;
                }
            }
        }

        // 🔥 PUT IT BACK
        self.icon_cache = Some(icon_cache);
    }
}

fn tab_title_for(nav: &Navigation) -> String {
    if nav.is_root() {
        return "This PC".to_string();
    }

    nav.current
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| nav.current.display().to_string())
}

impl ExplorerApp {
    fn record_to_file_item(&self, rec: crate::indexer::FileRecord) -> Option<FileItem> {
        let path = self.indexer.get_path(rec.file_ref)?;
        let file_size = if rec.is_dir {
            self.indexer.get_folder_size(&path)
        } else {
            Some(rec.size)
        };

        Some(FileItem::new(rec.name, path, rec.is_dir, file_size, None, None))
    }
}
