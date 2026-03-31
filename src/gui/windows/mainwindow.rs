use crate::core::fs::{FileItem, MY_PC_PATH};
use crate::core::indexer::{load_app_settings, load_favorites, load_theme_settings};
use crate::gui::icons::IconCache;
use crate::gui::theme::{ThemeMode, apply_theme, get_palette, set_palette};
use crate::gui::utils::SortColumn;
use crate::gui::windows::containers::enums::ItemViewerAction;
use crate::gui::windows::containers::itemviewer::draw_item_viewer;
use crate::gui::windows::containers::sidebar::draw_sidebar;
use crate::gui::windows::containers::structs::{
    DragState, ExplorerState, FavoriteItem, FilterState, ItemViewerFolderSizeState, RenameState,
    SidebarAction, TabInfo, TabState,
};
use crate::gui::windows::containers::tabs::{draw_tabbar, draw_tabs};
use crate::gui::windows::containers::topbar::draw_topbar;
use crate::gui::windows::mainwindow_imp::{
    handle_draw_customizetheme_window, handle_pending_actions, tab_title_for,
};
use crate::gui::windows::structs::{
    AboutWindow, AppSettings, Navigation, SettingsWindow, SidebarState, ThemeCustomizer,
};
use crate::gui::windows::windowsoverrides::{
    apply_window_override, consume_clipboard_dirty, install_wndproc,
};
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use eframe::egui;
use std::collections::HashMap;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use windows::Win32::Foundation::HWND;

pub struct MainWindow {
    // MainWindow General Variables
    pub(crate) theme: ThemeMode,
    pub(crate) theme_dirty: bool,
    pub(crate) window_override_set: bool,
    pub(crate) theme_customizer: ThemeCustomizer,
    pub(crate) settings_window: SettingsWindow,
    pub(crate) about_window: AboutWindow,
    pub(crate) dropped_files: Vec<PathBuf>,
    pub(crate) external_drag_to_internal_hover: bool,
    pub(crate) dropped_files_pending_ui_refresh: bool,
    pub(crate) shutdown: Arc<AtomicBool>,
    pub(crate) size_threads: Vec<std::thread::JoinHandle<()>>,
    pub(crate) hwnd: Option<HWND>,

    // File Explorer Variables
    pub(crate) tabs: Vec<TabState>,
    pub(crate) active_tab: usize,
    pub(crate) next_tab_id: u64,
    pub(crate) files: Vec<FileItem>,
    pub(crate) folder_sizes: HashMap<PathBuf, ItemViewerFolderSizeState>,
    pub(crate) rename_state: Option<RenameState>,
    pub(crate) sort_column: SortColumn,
    pub(crate) drag_state: DragState,
    pub(crate) sort_ascending: bool,
    pub(crate) file_type_cache: HashMap<String, String>,
    pub(crate) explorer_state: ExplorerState,
    pub(crate) item_viewer_filter_state: FilterState,
    pub(crate) clipboard_paths: Vec<PathBuf>,
    pub(crate) clipboard_set: HashSet<PathBuf>,
    pub(crate) clipboard_is_cut: bool,
    pub(crate) clipboard_has_files: bool,
    pub(crate) tab_infos_cache: Vec<TabInfo>,
    pub(crate) tab_infos_dirty: bool,
    pub(crate) file_size_text_cache: HashMap<PathBuf, (u64, String)>,
    pub(crate) folder_size_text_cache: HashMap<PathBuf, (u64, bool, String)>,
    pub(crate) drive_size_text_cache: HashMap<PathBuf, (u64, u64, String)>,
    pub(crate) is_loading: bool,
    pub(crate) pending_tab_scroll_id: Option<u64>,

    // Sidebar Variables
    pub(crate) sidebar_state: SidebarState,

    // Misc. Variables
    pub(crate) rx: Option<Receiver<FileItem>>,
    pub(crate) size_req_tx: Option<Sender<PathBuf>>,
    pub(crate) size_rx: Option<Receiver<(PathBuf, u64, bool)>>,
    pub(crate) pending_size_queue: VecDeque<PathBuf>,
    pub(crate) pending_size_set: HashSet<PathBuf>,
    pub(crate) icon_cache: Option<IconCache>,
}

impl Default for MainWindow {
    fn default() -> Self {
        // Load saved settings
        let (folder_scanning_enabled, window_size_mode, start_path) = load_app_settings();
        let loaded_settings = AppSettings {
            folder_scanning_enabled,
            window_size_mode: window_size_mode.clone(),
            start_path: Some(start_path.clone()), // 👈 important
        };

        let mut app = Self {
            tabs: vec![TabState {
                id: 1,
                nav: Navigation::new(start_path),
                breadcrumb_path_editing: false,
                breadcrumb_path_buffer: String::new(),
                breadcrumb_just_started_editing: false,
                breadcrumb_path_error: false,
                breadcrumb_path_error_animation_time: 0.0,
            }],
            active_tab: 0,
            next_tab_id: 2,
            files: vec![],
            rx: None,
            size_req_tx: None,
            size_rx: None,
            folder_sizes: HashMap::new(),

            sidebar_state: SidebarState::default(),

            rename_state: None,
            pending_size_queue: VecDeque::new(),
            pending_size_set: HashSet::new(),
            theme: ThemeMode::Dark,
            theme_dirty: true,
            window_override_set: false,
            drag_state: DragState::default(),
            sort_column: SortColumn::Name,
            sort_ascending: true,
            icon_cache: None,

            file_type_cache: HashMap::new(),
            explorer_state: Default::default(),
            theme_customizer: Default::default(),
            settings_window: Default::default(),
            about_window: Default::default(),
            dropped_files: Vec::new(), // Files dropped from external drag and drop
            external_drag_to_internal_hover: false, // Whether external drag is hovering over the item viewer
            dropped_files_pending_ui_refresh: false,
            shutdown: Arc::new(AtomicBool::new(false)),
            size_threads: Vec::new(),

            hwnd: None,
            item_viewer_filter_state: FilterState::default(),
            clipboard_paths: Vec::new(),
            clipboard_set: HashSet::new(),
            clipboard_is_cut: false,
            clipboard_has_files: false,
            tab_infos_cache: Vec::new(),
            tab_infos_dirty: true,
            file_size_text_cache: HashMap::new(),
            folder_size_text_cache: HashMap::new(),
            drive_size_text_cache: HashMap::new(),
            is_loading: false,
            pending_tab_scroll_id: None,
        };

        // Initialize settings window with loaded values
        app.settings_window.current_settings = loaded_settings;

        if let Some((light, dark)) = load_theme_settings() {
            set_palette(ThemeMode::Light, light);
            set_palette(ThemeMode::Dark, dark);
        }

        app.theme_customizer.light_palette = get_palette(ThemeMode::Light);
        app.theme_customizer.dark_palette = get_palette(ThemeMode::Dark);
        let stored = load_favorites('C');
        if stored.is_empty() {
            app.sidebar_state.favorites = app.default_favorites();
            app.persist_favorites();
        } else {
            app.sidebar_state.favorites = stored
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

impl MainWindow {
    pub fn new(hwnd: Option<HWND>) -> Self {
        let mut app = Self::default();
        if let Some(hwnd) = hwnd {
            unsafe {
                install_wndproc(hwnd);
            }
        }

        app.hwnd = hwnd;
        app
    }

    pub fn mark_tab_infos_dirty(&mut self) {
        self.tab_infos_dirty = true;
    }

    fn rebuild_tab_infos(&mut self) {
        self.tab_infos_cache = self
            .tabs
            .iter()
            .map(|tab| TabInfo {
                id: tab.id,
                title: tab_title_for(&tab.nav),
                full_path: if tab.nav.is_root() {
                    PathBuf::from(MY_PC_PATH)
                } else {
                    tab.nav.current.clone()
                },
            })
            .collect();
        self.tab_infos_dirty = false;
    }
}

impl Drop for MainWindow {
    fn drop(&mut self) {
        self.cleanup();
    }
}

impl eframe::App for MainWindow {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let palette = get_palette(self.theme);

        if !self.window_override_set {
            if let Some(hwnd) = self.hwnd {
                apply_window_override(hwnd, &palette);
                self.window_override_set = true;
            }
        }

        // Main layout: sidebar + tabs column
        let mut pending_action: Option<ItemViewerAction> = None;

        // 🔥 Detect external drag-over (files hovering)
        self.external_drag_to_internal_hover = ctx.input(|i| !i.raw.hovered_files.is_empty());

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

        if consume_clipboard_dirty() {
            self.clipboard_paths = crate::gui::utils::get_clipboard_files().unwrap_or_default();
            self.clipboard_is_cut = crate::gui::utils::is_clipboard_cut();
            self.clipboard_has_files = !self.clipboard_paths.is_empty();
            self.clipboard_set = self.clipboard_paths.iter().cloned().collect();
        }

        // Increase scroll speed for the explorer view.
        ctx.input_mut(|i| {
            // i.raw_scroll_delta *= 8.0;
            i.smooth_scroll_delta *= 6.0;
        });

        if self.icon_cache.is_none() {
            self.icon_cache = Some(IconCache::new(ctx.clone()));
        }

        let icon_cache = self.icon_cache.take().unwrap();

        // Main layout: sidebar + tabs column
        let mut topbar_action = None;
        let mut sidebar_action: Option<SidebarAction> = None;
        let mut tabs_action = None;
        let mut tabbar_action = None;

        let offset = egui::vec2(8.0, 8.0);

        egui::CentralPanel::default().show(ctx, |ui| {
            // CentralPanel available rect
            let rect = ui.min_rect();

            // Shift it to compensate for Windows inset
            let rect = rect.translate(-offset);

            ui.allocate_ui_at_rect(rect, |ui| {
                let avail = ui.available_size();

                ui.allocate_ui_with_layout(
                    avail,
                    egui::Layout::left_to_right(egui::Align::Min),
                    |ui| {
                        // --- Sidebar column ---
                        let sidebar_width_min = 140.0;
                        let sidebar_width_max = 280.0;
                        let sidebar_width = self
                            .sidebar_state
                            .sidebar_default_width
                            .max(sidebar_width_min)
                            .min(sidebar_width_max);

                        let sidebar_frame = egui::Frame::NONE
                            .stroke(egui::Stroke::new(1.0, palette.tab_border_default));

                        ui.allocate_ui_with_layout(
                            egui::vec2(sidebar_width, ui.available_height() + 15.5),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                egui::Frame::NONE.show(ui, |ui| {
                                    ui.add_space(8.0);
                                    topbar_action = Some(draw_topbar(
                                        ui,
                                        self.theme == ThemeMode::Dark,
                                        &palette,
                                    ));
                                });
                                sidebar_frame.show(ui, |ui| {
                                    sidebar_action = Some(draw_sidebar(
                                        ui,
                                        &icon_cache,
                                        &mut self.sidebar_state,
                                        &palette,
                                    ));
                                });
                            },
                        );

                        // --- Separator handle (drawn on top, no extra allocation) ---
                        let separator_width = 6.0;
                        let separator_rect = egui::Rect::from_min_size(
                            egui::pos2(
                                self.sidebar_state.sidebar_default_width - separator_width / 2.0,
                                0.0,
                            ),
                            egui::vec2(separator_width, ui.available_height()),
                        );

                        let separator_response =
                            ui.allocate_rect(separator_rect, egui::Sense::click_and_drag());

                        if separator_response.hovered() || separator_response.dragged() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);

                            let handle_height = 25.0;
                            let handle_width = 6.0;
                            let center_y = ui.available_height() / 2.0;

                            let handle_rect = egui::Rect::from_center_size(
                                egui::pos2(self.sidebar_state.sidebar_default_width, center_y), // exactly on sidebar right edge
                                egui::vec2(handle_width, handle_height),
                            );

                            ui.painter().rect_filled(
                                handle_rect,
                                handle_width / 2.0,
                                palette.button_seperator_handle_fill,
                            );
                        }

                        if separator_response.dragged() {
                            self.sidebar_state.sidebar_default_width =
                                (self.sidebar_state.sidebar_default_width
                                    + separator_response.drag_delta().x)
                                    .max(sidebar_width_min)
                                    .min(sidebar_width_max);
                        }

                        // --- Tabs column ---
                        let tabs_width = ui.available_width();
                        ui.allocate_ui_with_layout(
                            egui::vec2(tabs_width, ui.available_height() - 16.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                let old_spacing = ui.spacing().item_spacing;
                                ui.spacing_mut().item_spacing.y = 0.0;

                                if self.tab_infos_dirty
                                    || self.tab_infos_cache.len() != self.tabs.len()
                                {
                                    self.rebuild_tab_infos();
                                }
                                let active_id = self.tabs[self.active_tab].id;

                                egui::Frame::NONE.show(ui, |ui| {
                                    ui.add_space(8.0);
                                    let scroll_to_id = self.pending_tab_scroll_id;
                                    tabs_action = Some(draw_tabs(
                                        ui,
                                        &self.tab_infos_cache,
                                        active_id,
                                        &palette,
                                        self.hwnd,
                                        scroll_to_id,
                                    ));
                                    if scroll_to_id.is_some() {
                                        self.pending_tab_scroll_id = None;
                                    }
                                });

                                let container = egui::Frame::NONE
                                    .stroke(egui::Stroke::NONE)
                                    .fill(egui::Color32::TRANSPARENT)
                                    .inner_margin(egui::Margin::symmetric(10, 8));

                                let active_index = self.active_tab;
                                let is_drive_view = self.current_nav().is_root();
                                let display_files = &self.files;

                                container.show(ui, |ui| {
                                    tabbar_action = {
                                        let tab = &mut self.tabs[active_index];
                                        let is_favorited = self
                                            .sidebar_state
                                            .favorites
                                            .iter()
                                            .any(|fav| fav.path == tab.nav.current);

                                        Some(draw_tabbar(
                                            ui,
                                            &icon_cache,
                                            tab,
                                            &palette,
                                            is_favorited,
                                        ))
                                    };

                                    ui.add_space(4.0);

                                    pending_action = draw_item_viewer(
                                        ui,
                                        display_files,
                                        &self.folder_sizes,
                                        self.clipboard_has_files,
                                        &self.clipboard_set,
                                        self.clipboard_is_cut,
                                        is_drive_view,
                                        self.sort_column,
                                        self.sort_ascending,
                                        &icon_cache,
                                        &mut self.rename_state,
                                        &palette,
                                        &mut self.file_type_cache,
                                        &mut self.file_size_text_cache,
                                        &mut self.folder_size_text_cache,
                                        &mut self.drive_size_text_cache,
                                        &mut self.external_drag_to_internal_hover,
                                        &mut tabbar_action,
                                        &mut self.drag_state,
                                        &mut self.item_viewer_filter_state,
                                        self.is_loading,
                                        &mut self.explorer_state,
                                    );

                                    ui.add_space(16.0);
                                });

                                ui.spacing_mut().item_spacing = old_spacing;
                            },
                        );
                    },
                );
            });
        });

        self.handle_directory_batch_recieve(ctx);
        self.handle_directory_size_updates(ctx);
        self.handle_throttle_size_requests(ctx);
        self.handle_topbar_action(topbar_action);
        self.handle_sidebar_action(sidebar_action);
        self.handle_tabs_action(tabs_action);
        self.handle_tabbar_action(tabbar_action);
        handle_pending_actions(pending_action, self);
        handle_draw_customizetheme_window(
            ctx,
            &mut self.theme_customizer,
            &palette,
            self.theme,
            &mut self.theme_dirty,
        );
        self.handle_draw_settings_window(ctx, &palette);
        self.handle_draw_about_window(ctx, &palette);

        // ✅ Step 5: Apply Deferred Refresh (IMPORTANT)
        if self.dropped_files_pending_ui_refresh {
            self.load_path();
            self.dropped_files_pending_ui_refresh = false;
        }

        self.icon_cache = Some(icon_cache);
    }
}
