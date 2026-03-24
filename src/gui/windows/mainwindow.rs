use crate::core::drives::get_drive_infos;
use crate::core::indexer::{load_app_settings, load_favorites};
use crate::core::networkdevices::NetworkDevicesState;
use crate::core::state::{FileItem, History, Navigation};
use crate::gui::icons::IconCache;
use crate::gui::theme::{ThemeMode, apply_theme, get_palette};
use crate::gui::utils::SortColumn;
use crate::gui::utils::clipboard_has_files;
use crate::gui::windows::containers::enums::ItemViewerAction;
use crate::gui::windows::containers::itemviewer::draw_item_viewer;
use crate::gui::windows::containers::sidebar::draw_sidebar;
use crate::gui::windows::containers::structs::{
    FavoriteItem, ItemViewerFolderSizeState, RenameState, SidebarAction, TabInfo, TabState,
};
use crate::gui::windows::containers::tabs::{draw_tabbar, draw_tabs};
use crate::gui::windows::containers::topbar::draw_topbar;
use crate::gui::windows::customizetheme::ThemeCustomizer;
use crate::gui::windows::mainwindow_imp::{
    apply_window_override, handle_draw_customizetheme_window, handle_pending_actions,
    install_wndproc, tab_title_for,
};
use crate::gui::windows::settings::SettingsWindow;
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
    pub(crate) tabs: Vec<TabState>,
    pub(crate) active_tab: usize,
    pub(crate) next_tab_id: u64,
    pub(crate) files: Vec<FileItem>,
    pub(crate) rx: Option<Receiver<FileItem>>,
    pub(crate) size_req_tx: Option<Sender<PathBuf>>,
    pub(crate) size_rx: Option<Receiver<(PathBuf, u64, bool)>>,
    pub(crate) folder_sizes: HashMap<PathBuf, ItemViewerFolderSizeState>,
    pub(crate) search_query: String,
    pub(crate) search_results: Vec<FileItem>,
    pub(crate) search_active: bool,
    pub(crate) favorites: Vec<FavoriteItem>,
    pub(crate) dragging_favorite: Option<usize>,
    pub(crate) sidebar_selected: Option<PathBuf>,
    pub(crate) selected_path: Option<PathBuf>,
    pub(crate) rename_state: Option<RenameState>,
    pub(crate) pending_size_queue: VecDeque<PathBuf>,
    pub(crate) pending_size_set: HashSet<PathBuf>,
    pub(crate) theme: ThemeMode,
    pub(crate) theme_dirty: bool,
    pub(crate) sort_column: SortColumn,
    pub(crate) sort_ascending: bool,
    pub(crate) icon_cache: Option<IconCache>,
    pub(crate) sidebar_default_width: f32,
    pub(crate) file_type_cache: HashMap<String, String>,
    pub(crate) selected_paths: HashSet<PathBuf>,
    // pub(crate) box_selection_start: Option<egui::Pos2>,
    // pub(crate) box_selection_active: bool,
    pub(crate) theme_customizer: ThemeCustomizer,
    pub(crate) settings_window: SettingsWindow,
    pub(crate) dropped_files: Vec<PathBuf>,
    pub(crate) drag_hover: bool,
    pub(crate) dropped_files_pending_ui_refresh: bool,
    pub(crate) action_history: History,
    pub(crate) selection_anchor: Option<usize>,
    pub(crate) selection_focus: Option<usize>,
    pub(crate) shutdown: Arc<AtomicBool>,
    pub(crate) size_threads: Vec<std::thread::JoinHandle<()>>,
    pub(crate) network_state: crate::core::networkdevices::NetworkDevicesState,
    pub(crate) hwnd: Option<HWND>,
}

impl Default for MainWindow {
    fn default() -> Self {
        // Load saved settings
        let (folder_scanning_enabled, window_size_mode) = load_app_settings();

        let mut app = Self {
            tabs: vec![TabState {
                id: 1,
                nav: Navigation::new(),
                is_editing_path: false,
                path_buffer: String::new(),
            }],
            active_tab: 0,
            next_tab_id: 2,
            files: vec![],
            rx: None,
            size_req_tx: None,
            size_rx: None,
            folder_sizes: HashMap::new(),
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
            sort_column: SortColumn::Name,
            sort_ascending: true,
            icon_cache: None,
            sidebar_default_width: 250.0,
            file_type_cache: HashMap::new(),
            selected_paths: HashSet::new(), // Multi-selection state
            // box_selection_start: None,      // Box selection start position
            // box_selection_active: false,    // Whether box selection is currently active
            theme_customizer: Default::default(),
            settings_window: Default::default(),
            dropped_files: Vec::new(), // Files dropped from external drag and drop
            drag_hover: false,         // Whether external drag is hovering over the item viewer
            dropped_files_pending_ui_refresh: false,
            action_history: History::new(),
            selection_anchor: None, // Anchor index for extended selection
            selection_focus: None,  // Focus index for extended selection
            shutdown: Arc::new(AtomicBool::new(false)),
            size_threads: Vec::new(),
            network_state: NetworkDevicesState::default(),
            hwnd: None,
        };

        // Initialize settings window with loaded values
        app.settings_window.current_settings.folder_scanning_enabled = folder_scanning_enabled;
        app.settings_window.current_settings.window_size_mode = window_size_mode;
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

impl MainWindow {
    pub fn new(hwnd: Option<HWND>) -> Self {
        let mut app = Self::default();

        // if let Some(hwnd) = hwnd {
        //     apply_window_style(hwnd, &get_palette()); // 👈 apply once
        // }

        if let Some(hwnd) = hwnd {
            unsafe {
                install_wndproc(hwnd); // 👈 THIS is the key
            }
        }

        app.hwnd = hwnd;
        app
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

        if let Some(hwnd) = self.hwnd {
            apply_window_override(hwnd, &palette);
        }
        apply_theme(ctx, self.theme);

        // Main layout: sidebar + tabs column
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
                        .sidebar_default_width
                        .max(sidebar_width_min)
                        .min(sidebar_width_max);

                    let sidebar_frame =
                        // egui::Frame::NONE.inner_margin(egui::Margin::symmetric(12, 0))
                        egui::Frame::NONE.stroke(egui::Stroke::new(1.0, palette.tab_border_default));

                    ui.allocate_ui_with_layout(
                        egui::vec2(sidebar_width, ui.available_height() + 32.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            egui::Frame::NONE.show(ui, |ui| {
                                ui.add_space(8.0);
                                topbar_action =
                                    Some(draw_topbar(ui, self.theme == ThemeMode::Dark, palette));
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
                                    &mut self.network_state,
                                ));

                                if let Some(action) = &sidebar_action {
                                    if let Some((from, to)) = action.reorder {
                                        self.favorites.swap(from, to);
                                    }
                                }
                            });
                        },
                    );

                    // --- Separator handle (drawn on top, no extra allocation) ---
                    let separator_width = 6.0;
                    let separator_rect = egui::Rect::from_min_size(
                        egui::pos2(self.sidebar_default_width - separator_width / 2.0, 0.0),
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
                            egui::pos2(self.sidebar_default_width, center_y), // exactly on sidebar right edge
                            egui::vec2(handle_width, handle_height),
                        );

                        ui.painter().rect_filled(
                            handle_rect,
                            handle_width / 2.0,
                            if self.theme == ThemeMode::Dark {
                                egui::Color32::from_gray(120)
                            } else {
                                egui::Color32::from_gray(180)
                            },
                        );
                    }

                    if separator_response.dragged() {
                        self.sidebar_default_width = (self.sidebar_default_width
                            + separator_response.drag_delta().x)
                            .max(sidebar_width_min)
                            .min(sidebar_width_max);
                    }

                    // --- Tabs column ---
                    let tabs_width = ui.available_width();
                    ui.allocate_ui_with_layout(
                        egui::vec2(tabs_width, ui.available_height()),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            let old_spacing = ui.spacing().item_spacing;
                            ui.spacing_mut().item_spacing.y = 0.0;
                            // ui.add_space(6.0); // vertical spacing above tabs

                            let tab_infos: Vec<TabInfo> = self
                                .tabs
                                .iter()
                                .map(|tab| TabInfo {
                                    id: tab.id,
                                    title: tab_title_for(&tab.nav),
                                    full_path: if tab.nav.is_root() {
                                        PathBuf::from("::MY_PC::")
                                    } else {
                                        tab.nav.current.clone()
                                    },
                                })
                                .collect();
                            let active_id = self.tabs[self.active_tab].id;

                            let tab_bar_height = 35.0;

                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), tab_bar_height),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    tabs_action =
                                        Some(draw_tabs(ui, &tab_infos, active_id, &palette, self.hwnd));
                                },
                            );

                            let container = egui::Frame::NONE
                                .stroke(egui::Stroke::NONE)
                                .fill(egui::Color32::TRANSPARENT)
                                .inner_margin(egui::Margin::symmetric(10, 8));

                            let active_index = self.active_tab;
                            let search_active = self.search_active;

                            let display_files = if search_active {
                                &self.search_results
                            } else {
                                &self.files
                            };

                            container.show(ui, |ui| {
                                tabbar_action = {
                                    let tab = &mut self.tabs[active_index];

                                    Some(draw_tabbar(ui, &icon_cache, tab, &palette))
                                };

                                ui.add_space(4.0);

                                egui::ScrollArea::vertical().auto_shrink([false; 2]).show(
                                    ui,
                                    |ui| {
                                        pending_action = draw_item_viewer(
                                            ui,
                                            display_files,
                                            &self.folder_sizes,
                                            &mut self.selected_path,
                                            &self.selected_paths,
                                            clipboard_has_files(),
                                            self.sort_column,
                                            self.sort_ascending,
                                            &icon_cache,
                                            &mut self.rename_state,
                                            &palette,
                                            &mut self.file_type_cache,
                                            &mut self.drag_hover,
                                            &mut self.selection_anchor,
                                            &mut self.selection_focus,
                                        );
                                        ui.add_space(6.0);
                                    },
                                );
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
        handle_draw_customizetheme_window(ctx, &mut self.theme_customizer);
        self.handle_draw_settings_window(ctx, &palette);

        // ✅ Step 5: Apply Deferred Refresh (IMPORTANT)
        if self.dropped_files_pending_ui_refresh {
            self.load_path();
            self.dropped_files_pending_ui_refresh = false;
        }

        self.icon_cache = Some(icon_cache);
    }
}
