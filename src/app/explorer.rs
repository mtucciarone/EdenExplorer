use eframe::egui;
use std::collections::HashMap;
use std::collections::{HashSet, VecDeque};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

use crossbeam_channel::{Receiver, Sender, unbounded};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{SW_SHOW, SW_SHOWNORMAL};
use windows::core::PCWSTR;

use crate::app::customizetheme;
use crate::app::icons::IconCache; // 🔥 FIXED PATH
use crate::app::sidebar::{FavoriteItem, SidebarAction, draw_sidebar};
use crate::app::tabs::{TabInfo, TabbarAction, TabbarNavAction, draw_tabbar, draw_tabs};
use crate::app::topbar::draw_topbar;
use crate::app::utils::{
    clipboard_has_files, copy_dir_recursive, get_clipboard_files, set_clipboard_files,
    shell_delete_to_recycle_bin,
};
use crate::drives::{get_drive_infos, get_drives, parse_drive_display};
use crate::fs::{calculate_folder_size_fast_progress, get_drive_space, scan_dir_async};
use crate::indexer::{IndexStatus, Indexer, load_favorites, save_favorites};
use crate::state::{FileItem, Navigation};

use super::features::{ThemeMode, apply_theme};
use super::itemviewer::{
    ItemViewerAction, ItemViewerContextAction, ItemViewerFolderSizeState, RenameState,
    draw_item_viewer,
};
use super::sorting::{SortColumn, sort_files};
use windows::Win32::UI::Shell::{SEE_MASK_INVOKEIDLIST, SHELLEXECUTEINFOW, ShellExecuteExW};

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
    indexer: std::sync::Arc<crate::indexer::Indexer>,
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
    show_hamburger_menu: bool,
    sort_column: SortColumn,
    sort_ascending: bool,
    icon_cache: Option<IconCache>, // 🔥 FIX: lazy init
    sidebar_width: f32,
    file_type_cache: HashMap<String, String>,
    selected_paths: HashSet<PathBuf>,        // Multi-selection state
    box_selection_start: Option<egui::Pos2>, // Box selection start position
    box_selection_active: bool,              // Whether box selection is currently active
    theme_customizer: customizetheme::ThemeCustomizer,
    dropped_files: Vec<PathBuf>, // Files dropped from external drag and drop
    drag_hover: bool,            // Whether external drag is hovering over the item viewer
    pending_refresh: bool,
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
            search_results: vec![],
            search_active: false,
            favorites: vec![],
            dragging_favorite: None,
            sidebar_selected: None,
            selected_path: None,
            rename_state: None,
            pending_size_queue: VecDeque::new(),
            pending_size_set: HashSet::new(),
            theme: ThemeMode::Dark,
            theme_dirty: false,
            show_hamburger_menu: false,
            sort_column: SortColumn::Name,
            sort_ascending: true,
            icon_cache: None, //
            sidebar_width: 250.0,
            file_type_cache: HashMap::new(),
            selected_paths: HashSet::new(), // Multi-selection state
            box_selection_start: None,      // Box selection start position
            box_selection_active: false,    // Whether box selection is currently active
            theme_customizer: Default::default(),
            dropped_files: Vec::new(), // Files dropped from external drag and drop
            drag_hover: false,         // Whether external drag is hovering over the item viewer
            pending_refresh: false,
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
        // 🔥 Step 4: Force continuous repaint (dev mode)
        ctx.request_repaint(); // keeps UI live

        apply_theme(ctx, self.theme);

        // Main layout: sidebar + tabs column
        let sidebar_action: Option<SidebarAction> = None;
        let tabs_action: Option<crate::app::tabs::TabsAction> = None;
        let tabbar_action: Option<crate::app::tabs::TabbarNavAction> = None;
        let nav_snapshot = self.current_nav().clone();
        let mut search_snapshot = self.search_query.clone();
        let mut pending_action: Option<ItemViewerAction> = None;

        // 🔥 Detect external drag-over (files hovering)
        self.drag_hover = ctx.input(|i| !i.raw.hovered_files.is_empty());

        // 🔥 Detect dropped files from OS
        let dropped_files: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });

        if !dropped_files.is_empty() {
            // Send into your existing system
            pending_action = Some(ItemViewerAction::FilesDropped(dropped_files));
        }

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
        let palette = crate::app::features::get_palette(self.theme);
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
        let mut sidebar_action: Option<SidebarAction> = None;
        let mut tabs_action = None;
        let mut tabbar_action = None;

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
                                    &palette,
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

                            tabs_action = Some(draw_tabs(ui, &tab_infos, active_id, &palette));

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
                                    &palette,
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
                                            &self.selected_paths,
                                            clipboard_has_files(),
                                            self.sort_column,
                                            self.sort_ascending,
                                            &icon_cache,
                                            self.rename_state.as_mut(),
                                            &palette,
                                            &mut self.file_type_cache,
                                            &mut self.drag_hover,
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

            if action.customize_theme {
                self.theme_customizer.open = true;
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
                    self.selected_path = Some(path.clone());
                    self.selected_paths.insert(path);
                }
                ItemViewerAction::Deselect(path) => {
                    self.selected_paths.remove(&path);
                }
                ItemViewerAction::SelectAll => {
                    self.selected_paths.clear();
                    for file in &self.files {
                        self.selected_paths.insert(file.path.clone());
                    }
                }
                ItemViewerAction::DeselectAll => {
                    self.selected_paths.clear();
                }
                ItemViewerAction::BoxSelect(paths) => {
                    // Clear current selection and add box-selected files
                    self.selected_paths.clear();
                    for path in paths {
                        self.selected_paths.insert(path);
                    }
                }
                ItemViewerAction::RangeSelect(paths) => {
                    // Clear current selection and add range-selected files
                    self.selected_paths.clear();
                    for path in paths {
                        self.selected_paths.insert(path);
                    }
                }
                ItemViewerAction::Open(path) => {
                    self.selected_path = Some(path.clone());
                    self.current_nav_mut().go_to(path);
                    self.load_path();
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
                        should_focus: true,
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
                ItemViewerAction::ReplaceSelection(path) => {
                    self.selected_paths.clear();
                    self.selected_paths.insert(path.clone());
                    self.selected_path = Some(path);
                }
                ItemViewerAction::FilesDropped(dropped_files) => {
                    let valid_files: Vec<PathBuf> =
                        dropped_files.into_iter().filter(|p| p.exists()).collect();

                    if valid_files.is_empty() {
                        return;
                    }

                    self.dropped_files = valid_files.clone();

                    let current_path = self.current_nav().current.clone();

                    if let Err(e) =
                        crate::app::utils::show_copy_move_dialog(valid_files, &current_path)
                    {
                        eprintln!("Failed to show copy/move dialog: {}", e);
                    }

                    // ✅ Defer refresh (important)
                    self.pending_refresh = true;
                }
            }
        }

        if let Some(action) = customizetheme::draw_theme_customizer(ctx, &mut self.theme_customizer)
        {
            match action {
                customizetheme::ThemeCustomizerAction::ApplyTheme => {
                    self.theme_dirty = true;
                }
                customizetheme::ThemeCustomizerAction::ResetToDefaults => {
                    self.theme_customizer.current_theme = Default::default();
                    self.theme_dirty = true;
                }
                customizetheme::ThemeCustomizerAction::SaveTheme => {
                    // implement later
                }
                customizetheme::ThemeCustomizerAction::LoadTheme => {
                    // implement later
                }
                customizetheme::ThemeCustomizerAction::ExportTheme => {
                    // implement later
                }
                customizetheme::ThemeCustomizerAction::ImportTheme => {
                    // implement later
                }
            }
        }

        // ✅ Step 5: Apply Deferred Refresh (IMPORTANT)
        if self.pending_refresh {
            self.load_path();
            self.pending_refresh = false;
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

        Some(FileItem::new(
            rec.name, path, rec.is_dir, file_size, None, None,
        ))
    }
}
