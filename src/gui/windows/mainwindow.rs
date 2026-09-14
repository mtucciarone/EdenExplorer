use crate::core::fs::MY_PC_PATH;
use crate::core::indexer::WindowSizeMode;
use crate::core::indexer::{
    SessionTabEntry, SessionTabsSnapshot, load_app_settings, load_favorites, load_session_tabs,
    load_tags, load_theme_settings, save_app_settings, save_session_tabs,
};
use crate::core::launch::take_forwarded_paths;
use crate::core::utils::tabs::update_tab_infos_cache;
use crate::gui::dragdrop::{DragDropBackend, DropTargets};
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::{
    ThemeMode, apply_font_to_context, apply_theme, get_default_palette, get_palette, set_palette,
};
use crate::gui::windows::containers::enums::ItemViewerAction;
use crate::gui::windows::containers::explorer::draw_tab_content;
use crate::gui::windows::containers::notifications::{draw_notifications_button, draw_toast, NotificationsState};
use crate::gui::windows::containers::sidebar::draw_sidebar;
use crate::gui::windows::containers::structs::{
    ItemViewerColumnState, ItemViewerFolderSizeState, ItemViewerNavBarAction,
    RenameState, SavedSearchesState, SidebarAction, SplitSide, TabInfo, TabState, TabView,
    TabsAction, TagsState,
};
use crate::gui::windows::containers::tabs::{TAB_HEIGHT, draw_tabs, tab_row_count};
use crate::gui::windows::containers::tags::draw_tag_picker_popup;
use crate::gui::windows::containers::topbar::draw_topbar;
use crate::gui::windows::mainwindow_imp::{
    DisplayModeFallback, apply_directory_settings_to_view, directory_settings_snapshot_for_view,
    handle_pending_actions, persist_directory_settings_snapshot,
};
use crate::gui::windows::structs::{
    AboutWindow, AppSettings, Navigation, SettingsWindow, SidebarState, ThemeCustomizer,
};
use crate::gui::windows::windowsoverrides::{
    apply_window_override, consume_clipboard_dirty, handle_draw_windows_buttons, install_wndproc,
};
use eframe::egui;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetCursorPos};

/// Thickness of the top-line indicator on the currently active pane in dual-
/// pane mode. Kept as one named constant, not two independent literals -
/// the primary and secondary pane each draw their own `hline`, and a past
/// bump (2.0 -> 4.0) touched both call sites by hand, which is an easy way
/// for the two to silently drift apart.
const ACTIVE_PANE_INDICATOR_THICKNESS: f32 = 6.0;

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
    pub(crate) hwnd: Option<HWND>,
    pub(crate) last_window_size: Option<(f32, f32)>,
    pub(crate) last_window_position: Option<(f32, f32)>,
    pub(crate) sidebar_collapsed: bool,
    /// Whether the OS window had keyboard focus as of the last frame - used
    /// to detect the window regaining focus (e.g. after running an external
    /// script/program from a custom context menu command) so the current
    /// folder can be refreshed automatically, since the app has no other way
    /// to know that command finished changing files on disk.
    pub(crate) was_window_focused: bool,
    /// Times (queued after launching a custom context menu command) at
    /// which the current folder gets refreshed automatically. The launched
    /// program runs detached (fire-and-forget via `ShellExecuteW`), so this
    /// is the only way to notice it finished changing files; several
    /// staggered times are queued per launch since there's no way to know
    /// how long it'll take, and window-focus regain (see
    /// `was_window_focused`) is a second, independent trigger for the same
    /// refresh in case the launched program never takes focus at all (e.g.
    /// a script that finishes before its console window would even show).
    pub(crate) pending_command_refreshes: Vec<std::time::Instant>,
    /// Paste (or cut-move) operations currently running via the
    /// robocopy-backed engine (`core::robocopy`), keyed by the notification
    /// id `paste_clipboard_native` created for each - see `PendingPaste`'s
    /// doc comment in `mainwindow_imp.rs`.
    pub(crate) pending_robocopy_pastes:
        HashMap<u64, crate::gui::windows::mainwindow_imp::PendingPaste>,
    /// A paste held back for the user to resolve a name collision - see
    /// `PasteConflictPrompt`'s doc comment in `mainwindow_imp.rs`. Drawn as
    /// a modal from the update loop.
    pub(crate) pending_paste_conflict:
        Option<crate::gui::windows::mainwindow_imp::PasteConflictPrompt>,
    /// A bulk-rename dialog open over a multi-item selection - see
    /// `bulk_rename::BulkRenameState`'s doc comment. Drawn as a modal from
    /// the update loop, alongside `pending_paste_conflict`.
    pub(crate) pending_bulk_rename:
        Option<crate::gui::windows::containers::bulk_rename::BulkRenameState>,
    /// Compress (zip) jobs running on a background thread, keyed by the
    /// notification id `handle_context_action`'s `Compress` arm created for
    /// each - polled once per frame by `poll_pending_compress`.
    pub(crate) pending_compress_jobs:
        HashMap<u64, (crossbeam_channel::Receiver<Result<(), String>>, PathBuf)>,
    /// The "Checksums" modal's state while it's open - see
    /// `ChecksumDialogState`'s doc comment in `mainwindow_imp.rs`. Drawn as
    /// a modal from the update loop, alongside `pending_paste_conflict`.
    pub(crate) pending_checksum:
        Option<crate::gui::windows::mainwindow_imp::ChecksumDialogState>,
    /// Tracked copy/move/delete operations shown by the top-right
    /// notification bell - see `containers::notifications`.
    pub(crate) notifications_state: NotificationsState,
    /// Completed, reversible Rename/BulkRename/Move/Copy operations, most
    /// recent last - see `UndoableOperation`'s doc comment in
    /// `mainwindow_imp.rs`. Deliberately in-memory only (never persisted),
    /// capped at `MAX_UNDO_STACK`.
    pub(crate) undo_stack:
        std::collections::VecDeque<crate::gui::windows::mainwindow_imp::UndoableOperation>,
    /// Operations undone via `MainWindow::undo`, available to redo via
    /// Ctrl+Y/Ctrl+Shift+Z - cleared whenever a fresh Rename/BulkRename/
    /// Move/Copy is pushed onto `undo_stack`.
    pub(crate) redo_stack:
        std::collections::VecDeque<crate::gui::windows::mainwindow_imp::UndoableOperation>,

    // File Explorer Variables (per-tab/per-view state lives on TabState/TabView)
    pub(crate) tabs: Vec<TabState>,
    pub(crate) active_tab: usize,
    pub(crate) tab_infos_cache: Vec<TabInfo>,
    pub(crate) tab_infos_dirty: bool,
    pub(crate) pending_tab_scroll_id: Option<u64>,
    /// Index (into `tab_infos_cache`/`tabs`) of the tab currently being dragged to
    /// reorder it, if any.
    pub(crate) dragging_tab_index: Option<usize>,
    pub(crate) focused_split: SplitSide,
    pub(crate) next_tab_id: u64,
    pub(crate) folder_sizes: HashMap<PathBuf, ItemViewerFolderSizeState>,
    pub(crate) rename_state: Option<RenameState>,
    pub(crate) dragdrop: Option<Box<dyn DragDropBackend>>,
    pub(crate) file_type_cache: HashMap<String, String>,
    pub(crate) tags_state: TagsState,
    pub(crate) saved_searches_state: SavedSearchesState,
    pub(crate) recent_locations_state:
        crate::gui::windows::containers::structs::RecentLocationsState,
    pub(crate) clipboard_paths: Vec<PathBuf>,
    pub(crate) clipboard_set: HashSet<PathBuf>,
    pub(crate) clipboard_is_cut: bool,
    pub(crate) clipboard_has_files: bool,
    pub(crate) file_size_text_cache: HashMap<PathBuf, (u64, String)>,
    pub(crate) folder_size_text_cache: HashMap<PathBuf, (u64, bool, String)>,
    pub(crate) drive_size_text_cache: HashMap<PathBuf, (u64, u64, String)>,

    // Sidebar Variables
    pub(crate) sidebar_state: SidebarState,

    // Misc. Variables
    pub(crate) icon_cache: Option<IconCache>,

    // i18n
    pub(crate) i18n: I18n,
}

impl Default for MainWindow {
    fn default() -> Self {
        // Load saved settings
        let (
            folder_scanning_enabled,
            show_hidden_files_folders,
            show_item_viewer_icons,
            windows_context_menu_enabled,
            window_size_mode,
            start_path,
            saved_theme,
            pinned_tabs,
            time_format_24h,
            sort_column,
            sort_ascending,
            language,
            date_style,
            custom_date_format,
            item_viewer_file_column_order,
            item_viewer_drive_column_order,
            recycle_bin_column_order,
            item_viewer_file_column_sizes,
            item_viewer_drive_column_sizes,
            recycle_bin_column_sizes,
            directory_settings,
            double_click_navigates_up,
            show_selection_checkboxes,
            middle_click_opens_new_tab,
            restore_last_session_tabs,
            default_display_mode,
            default_search_scope,
            search_engine,
        ) = load_app_settings();
        let loaded_settings = AppSettings {
            folder_scanning_enabled,
            show_hidden_files_folders,
            show_item_viewer_icons,
            windows_context_menu_enabled,
            window_size_mode: window_size_mode.clone(),
            start_path: Some(start_path.clone()), // important
            pinned_tabs: pinned_tabs.clone(),
            time_format_24h,
            date_style,
            custom_date_format,
            sort_column,
            sort_ascending,
            language,
            item_viewer_file_column_order,
            item_viewer_drive_column_order,
            recycle_bin_column_order,
            item_viewer_file_column_sizes,
            item_viewer_drive_column_sizes,
            recycle_bin_column_sizes,
            directory_settings,
            double_click_navigates_up,
            show_selection_checkboxes,
            middle_click_opens_new_tab,
            restore_last_session_tabs,
            default_display_mode,
            default_search_scope,
            search_engine,
            custom_context_menu: crate::core::context_menu_settings::load_custom_context_menu(),
            tab_groups: crate::core::tab_groups::load_tab_groups(),
        };

        let system_locale = sys_locale::get_locale().unwrap_or_else(|| "en-US".to_string());

        let default_locale = match system_locale.as_str() {
            l if l.starts_with("ja") => "ja-JP",
            l if l.starts_with("id") => "id-ID",
            l if l.starts_with("zh") => "zh-CN",
            l if l.starts_with("zh-HK") => "zh-HK",
            l if l.starts_with("zh-TW") => "zh-TW",
            _ => "en-US",
        };

        let pinned_tabs = pinned_tabs;

        // Restoring the last session takes priority over pinned tabs/start path when
        // enabled and a previous session was actually saved.
        let restored_session = if loaded_settings.restore_last_session_tabs {
            load_session_tabs().filter(|snapshot| !snapshot.tabs.is_empty())
        } else {
            None
        };

        let (tab_entries, initial_active_tab): (Vec<SessionTabEntry>, usize) = match restored_session
        {
            Some(snapshot) => {
                let active = snapshot
                    .active_index
                    .min(snapshot.tabs.len().saturating_sub(1));
                (snapshot.tabs, active)
            }
            None if pinned_tabs.is_empty() => (
                vec![SessionTabEntry {
                    path: start_path,
                    split_path: None,
                }],
                0,
            ),
            None => (
                pinned_tabs
                    .iter()
                    .map(|path| SessionTabEntry {
                        path: path.clone(),
                        split_path: None,
                    })
                    .collect(),
                0,
            ),
        };

        let mut tabs = Vec::new();
        let mut next_tab_id = 1;

        for entry in &tab_entries {
            let mut tab = TabState::new(
                next_tab_id,
                Navigation::new(entry.path.clone()),
                loaded_settings.sort_column,
                loaded_settings.sort_ascending,
            );
            tab.primary_view.column_state = ItemViewerColumnState::from_orders(
                loaded_settings.item_viewer_file_column_order.clone(),
                loaded_settings.item_viewer_drive_column_order.clone(),
                loaded_settings.recycle_bin_column_order.clone(),
                loaded_settings.item_viewer_file_column_sizes.clone(),
                loaded_settings.item_viewer_drive_column_sizes.clone(),
                loaded_settings.recycle_bin_column_sizes.clone(),
            );
            if let Some(split_path) = &entry.split_path {
                let mut split_view = TabView::new(
                    Navigation::new(split_path.clone()),
                    loaded_settings.sort_column,
                    loaded_settings.sort_ascending,
                );
                split_view.column_state = ItemViewerColumnState::from_orders(
                    loaded_settings.item_viewer_file_column_order.clone(),
                    loaded_settings.item_viewer_drive_column_order.clone(),
                    loaded_settings.recycle_bin_column_order.clone(),
                    loaded_settings.item_viewer_file_column_sizes.clone(),
                    loaded_settings.item_viewer_drive_column_sizes.clone(),
                    loaded_settings.recycle_bin_column_sizes.clone(),
                );
                tab.split_view = Some(split_view);
            }
            tabs.push(tab);
            next_tab_id += 1;
        }

        let mut app = Self {
            tabs,
            active_tab: initial_active_tab,
            tab_infos_cache: Vec::new(),
            tab_infos_dirty: true,

            pending_tab_scroll_id: None,
            dragging_tab_index: None,
            focused_split: SplitSide::Primary,
            next_tab_id,
            folder_sizes: HashMap::new(),

            sidebar_state: {
                let sections = crate::core::indexer::load_sidebar_sections();
                SidebarState {
                    places_expanded: sections.places,
                    storage_expanded: sections.storage,
                    favorites_expanded: sections.favorites,
                    tags_expanded: sections.tags,
                    shared_network_expanded: sections.shared_network,
                    saved_searches_expanded: sections.saved_searches,
                    recent_locations_expanded: sections.recent_locations,
                    sidebar_default_width: sections.sidebar_width,
                    ..SidebarState::default()
                }
            },

            rename_state: None,
            theme: match saved_theme.as_deref() {
                Some("light") => ThemeMode::Light,
                Some("dark") | _ => ThemeMode::Dark,
            },
            sidebar_collapsed: false,
            was_window_focused: true,
            pending_command_refreshes: Vec::new(),
            pending_robocopy_pastes: HashMap::new(),
            pending_paste_conflict: None,
            pending_bulk_rename: None,
            pending_compress_jobs: HashMap::new(),
            pending_checksum: None,
            notifications_state: NotificationsState::default(),
            undo_stack: std::collections::VecDeque::new(),
            redo_stack: std::collections::VecDeque::new(),
            theme_dirty: true,
            window_override_set: false,
            dragdrop: None,
            icon_cache: None,

            file_type_cache: HashMap::new(),
            tags_state: load_tags()
                .map(TagsState::from_snapshot)
                .unwrap_or_default(),
            saved_searches_state: crate::core::indexer::load_saved_searches()
                .map(SavedSearchesState::from_snapshot)
                .unwrap_or_default(),
            recent_locations_state: crate::core::indexer::load_recent_locations()
                .map(
                    crate::gui::windows::containers::structs::RecentLocationsState::from_snapshot,
                )
                .unwrap_or_default(),
            theme_customizer: Default::default(),
            settings_window: Default::default(),
            about_window: Default::default(),
            dropped_files: Vec::new(), // Files dropped from external drag and drop
            external_drag_to_internal_hover: false, // Whether external drag is hovering over the item viewer
            dropped_files_pending_ui_refresh: false,
            shutdown: Arc::new(AtomicBool::new(false)),

            hwnd: None,
            last_window_size: None,
            last_window_position: None,
            clipboard_paths: Vec::new(),
            clipboard_set: HashSet::new(),
            clipboard_is_cut: false,
            clipboard_has_files: false,
            file_size_text_cache: HashMap::new(),
            folder_size_text_cache: HashMap::new(),
            drive_size_text_cache: HashMap::new(),

            i18n: I18n::new(default_locale),
        };

        let lang = if loaded_settings.language.is_empty() {
            default_locale
        } else {
            loaded_settings.language.as_str()
        };

        app.i18n.set_locale(lang);
        app.settings_window.current_settings = loaded_settings;

        let current_settings = app.settings_window.current_settings.clone();
        for tab in &mut app.tabs {
            apply_directory_settings_to_view(
                &mut tab.primary_view,
                &current_settings,
                DisplayModeFallback::Default,
            );
            if let Some(split) = tab.split_view.as_mut() {
                apply_directory_settings_to_view(
                    split,
                    &current_settings,
                    DisplayModeFallback::Default,
                );
            }
        }

        match load_theme_settings() {
            Some((light, dark)) => {
                set_palette(ThemeMode::Light, light);
                set_palette(ThemeMode::Dark, dark);
            }
            None => {
                let light = get_default_palette(ThemeMode::Light);
                let dark = get_default_palette(ThemeMode::Dark);

                set_palette(ThemeMode::Light, light);
                set_palette(ThemeMode::Dark, dark);
            }
        }

        app.theme_customizer.light_palette = get_palette(ThemeMode::Light);
        app.theme_customizer.dark_palette = get_palette(ThemeMode::Dark);
        let stored = load_favorites('C');
        if stored.is_empty() {
            app.sidebar_state.favorites = app.default_favorites();
            app.persist_favorites();
        } else {
            app.sidebar_state.favorites = stored;
        }
        app.load_path();
        app
    }
}

impl MainWindow {
    pub fn new_with_paths(paths: Vec<PathBuf>) -> Self {
        let mut app = Self::default();
        if !paths.is_empty() {
            app.open_startup_paths(&paths);
        }
        app
    }

    pub fn mark_tab_infos_dirty(&mut self) {
        self.tab_infos_dirty = true;
    }

    pub fn active_tab(&self) -> &TabState {
        &self.tabs[self.active_tab]
    }

    pub fn active_tab_mut(&mut self) -> &mut TabState {
        &mut self.tabs[self.active_tab]
    }
}

impl eframe::App for MainWindow {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Lets the item viewer's own keyboard handling (Ctrl+A, Delete, ...)
        // know a blocking modal is on top of it this frame, so those
        // shortcuts don't fire on the file list underneath a modal that's
        // visually on top of it but doesn't otherwise suppress the view's
        // own per-frame input handling - see `BLOCKING_MODAL_MEMORY_ID`'s
        // doc comment. Reflects state from the *start* of this frame
        // (before any modal's own Close button runs), which is exactly
        // "was a modal open when this frame's keyboard input arrived."
        {
            let blocking_modal_open = self.pending_paste_conflict.is_some()
                || self.pending_bulk_rename.is_some()
                || self.pending_checksum.is_some();
            ui.ctx().memory_mut(|mem| {
                mem.data.insert_temp(
                    egui::Id::new(
                        crate::gui::windows::containers::itemviewer_helper::BLOCKING_MODAL_MEMORY_ID,
                    ),
                    blocking_modal_open,
                );
            });
        }
        {
            let forwarded_paths = take_forwarded_paths();
            for path in &forwarded_paths {
                self.open_new_tab(path.clone());
            }
            if !forwarded_paths.is_empty() {
                self.load_path();
            }
        }

        // Refresh the active tab's folder(s) when the window regains focus -
        // e.g. after running an external script/program from a custom
        // context menu command that created/moved files. The app has no
        // other way to notice a change like that made outside itself.
        let is_focused = ui.input(|i| i.focused);
        if is_focused && !self.was_window_focused {
            self.load_view(SplitSide::Primary);
            if self.active_tab().split_view.is_some() {
                self.load_view(SplitSide::Secondary);
            }
        }
        self.was_window_focused = is_focused;

        // Second, independent trigger for the same refresh: a custom
        // context menu command was launched recently. `ShellExecuteW` runs
        // it detached, so these timers (rather than the launched program's
        // exit or window focus, which it may never actually take) are what
        // catch it finishing. Several are queued per launch, staggered, since
        // there's no way to know how long the program will actually take.
        if !self.pending_command_refreshes.is_empty() {
            let now = std::time::Instant::now();
            let (due, still_pending): (Vec<_>, Vec<_>) = self
                .pending_command_refreshes
                .drain(..)
                .partition(|deadline| now >= *deadline);
            self.pending_command_refreshes = still_pending;

            if !due.is_empty() {
                self.load_view(SplitSide::Primary);
                if self.active_tab().split_view.is_some() {
                    self.load_view(SplitSide::Secondary);
                }
            }
            if let Some(next) = self.pending_command_refreshes.iter().min() {
                ui.ctx()
                    .request_repaint_after(next.saturating_duration_since(now));
            }
        }

        let palette = get_palette(self.theme);

        if self.hwnd.is_none() {
            if let Some(hwnd) = crate::gui::windows::windowsoverrides::get_hwnd_from_frame(frame) {
                self.hwnd = Some(hwnd);

                unsafe {
                    if let Err(e) = install_wndproc(hwnd) {
                        eprintln!("Failed to install wndproc: {}", e);
                    }
                }

                self.dragdrop = Some(Box::new(
                    crate::gui::windows::dragdrop::WindowsDragDropBackend::new(Some(hwnd)),
                ));
            } else {
                eprintln!("Failed to get HWND on first frame");
            }
        }

        if !self.window_override_set {
            if let Some(hwnd) = self.hwnd {
                apply_window_override(hwnd, &palette);
                self.window_override_set = true;
            }
        }

        // Main layout: sidebar + tabs column
        let mut pending_action: Option<ItemViewerAction> = None;
        let mut drop_targets = DropTargets::default();
        let dragdrop = self.dragdrop.as_deref();
        let native_drag_active = dragdrop
            .map(|backend| backend.is_drag_active())
            .unwrap_or(false);
        let native_inbound_drag_active = dragdrop
            .map(|backend| backend.is_inbound_drag_active())
            .unwrap_or(false);
        let drag_hover_target = dragdrop.and_then(|backend| backend.hovered_drop_target());
        let drag_active = {
            let tab = self.active_tab();
            tab.primary_view.drag_state.active
                || tab
                    .split_view
                    .as_ref()
                    .map(|v| v.drag_state.active)
                    .unwrap_or(false)
        } || native_drag_active;
        if let Some(backend) = dragdrop {
            backend.set_scale_factor(ui.ctx().pixels_per_point());
        }

        if self.theme_dirty {
            apply_theme(ui.ctx(), self.theme);
            apply_font_to_context(ui.ctx(), &palette);
            if let Some(hwnd) = self.hwnd {
                apply_window_override(hwnd, &palette);
            }
            self.theme_dirty = false;
        }

        // Auto-save window size when it changes (including maximize/restore)
        if let Some(viewport_rect) = ui.ctx().input(|i| i.viewport().inner_rect) {
            let current_size = (viewport_rect.width(), viewport_rect.height());

            // Check if window size changed from last recorded size
            if let Some(last_size) = self.last_window_size {
                let size_changed = (current_size.0 - last_size.0).abs() > 1.0
                    || (current_size.1 - last_size.1).abs() > 1.0;

                if size_changed {
                    // Update the window size mode in settings
                    match &mut self.settings_window.current_settings.window_size_mode {
                        WindowSizeMode::Custom { width, height } => {
                            *width = current_size.0;
                            *height = current_size.1;
                        }
                        WindowSizeMode::FullScreen => {
                            // Keep the mode as FullScreen.
                            // Don't overwrite it just because the window was resized.
                        }
                    }

                    // Save the updated settings
                    save_app_settings(
                        self.settings_window
                            .current_settings
                            .folder_scanning_enabled,
                        self.settings_window
                            .current_settings
                            .show_hidden_files_folders,
                        self.settings_window.current_settings.show_item_viewer_icons,
                        self.settings_window
                            .current_settings
                            .windows_context_menu_enabled,
                        &self.settings_window.current_settings.window_size_mode,
                        &self.settings_window.current_settings.start_path,
                        Some(match self.theme {
                            crate::gui::theme::ThemeMode::Dark => "dark",
                            crate::gui::theme::ThemeMode::Light => "light",
                        }),
                        &self.settings_window.current_settings.pinned_tabs,
                        self.settings_window.current_settings.time_format_24h,
                        self.settings_window.current_settings.sort_column,
                        self.settings_window.current_settings.sort_ascending,
                        &self.settings_window.current_settings.language,
                        self.settings_window.current_settings.date_style,
                        &self.settings_window.current_settings.custom_date_format,
                        &self
                            .settings_window
                            .current_settings
                            .item_viewer_file_column_order,
                        &self
                            .settings_window
                            .current_settings
                            .item_viewer_drive_column_order,
                        &self
                            .settings_window
                            .current_settings
                            .recycle_bin_column_order,
                        &self
                            .settings_window
                            .current_settings
                            .item_viewer_file_column_sizes,
                        &self
                            .settings_window
                            .current_settings
                            .item_viewer_drive_column_sizes,
                        &self
                            .settings_window
                            .current_settings
                            .recycle_bin_column_sizes,
                        &self.settings_window.current_settings.directory_settings,
                        self.settings_window
                            .current_settings
                            .double_click_navigates_up,
                        self.settings_window
                            .current_settings
                            .show_selection_checkboxes,
                        self.settings_window
                            .current_settings
                            .middle_click_opens_new_tab,
                        self.settings_window
                            .current_settings
                            .restore_last_session_tabs,
                        self.settings_window.current_settings.default_display_mode,
                        self.settings_window.current_settings.default_search_scope,
                        self.settings_window.current_settings.search_engine,
                    );

                    self.last_window_size = Some(current_size);
                }
            } else {
                // First time, just record the size
                self.last_window_size = Some(current_size);
            }
        }

        // Auto-save window position when it changes (including dragging the
        // window to a new spot), the same way the size above is auto-saved.
        if let Some(viewport_rect) = ui.ctx().input(|i| i.viewport().inner_rect) {
            let current_position = (viewport_rect.min.x, viewport_rect.min.y);

            if let Some(last_position) = self.last_window_position {
                let moved = (current_position.0 - last_position.0).abs() > 1.0
                    || (current_position.1 - last_position.1).abs() > 1.0;

                if moved {
                    crate::core::indexer::save_window_position(
                        current_position.0,
                        current_position.1,
                    );
                    self.last_window_position = Some(current_position);
                }
            } else {
                self.last_window_position = Some(current_position);
            }
        }

        if consume_clipboard_dirty() {
            self.clipboard_paths = crate::gui::utils::get_clipboard_files().unwrap_or_default();
            self.clipboard_is_cut = crate::gui::utils::is_clipboard_cut();
            self.clipboard_has_files = !self.clipboard_paths.is_empty();
            self.clipboard_set = self.clipboard_paths.iter().cloned().collect();
        }

        // Increase scroll speed for the explorer view.
        ui.ctx().input_mut(|i| {
            i.smooth_scroll_delta *= 6.0;
        });

        if self.icon_cache.is_none() {
            self.icon_cache = Some(IconCache::new(ui.ctx().clone()));
        }

        let icon_cache = self.icon_cache.take().unwrap();

        // Main layout: sidebar + tabs column
        let mut topbar_action = None;
        let mut sidebar_action: Option<SidebarAction> = None;
        let mut tabs_action: Option<TabsAction> = None;
        let mut tabbar_action = None;
        let mut secondary_tabbar_action: Option<ItemViewerNavBarAction> = None;
        let mut secondary_pending_action: Option<ItemViewerAction> = None;
        let mut tags_changed = false;
        let has_split = self.tabs[self.active_tab].split_view.is_some();

        let offset = egui::vec2(8.0, 8.0);

        egui::CentralPanel::default().show(ui, |ui| {
            // CentralPanel available rect
            let rect = ui.min_rect();

            // Shift it to compensate for Windows inset
            let rect = rect.translate(-offset);

            ui.scope_builder(
                egui::UiBuilder::new().max_rect(rect),
                |ui| {
                let avail = ui.available_size();

                ui.allocate_ui_with_layout(
                    avail,
                    egui::Layout::left_to_right(egui::Align::Min),
                    |ui| {
                        // --- Sidebar column ---
                        let collapsed_width = 38.0;
                        let sidebar_width_min = 140.0;
                        let explorer_min_width = 200.0;
                        let sidebar_width_max =
                            (avail.x - explorer_min_width).max(sidebar_width_min);
                        let sidebar_width = if self.sidebar_collapsed {
                            collapsed_width
                        } else {
                            self.sidebar_state
                                .sidebar_default_width
                                .max(sidebar_width_min)
                                .min(sidebar_width_max)
                        };

                        let sidebar_frame = egui::Frame::NONE
                            .fill(palette.sidebar_bg_color)
                            .stroke(egui::Stroke::new(1.5, palette.borders_default));

                        ui.allocate_ui_with_layout(
                            egui::vec2(sidebar_width, ui.available_height()),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                egui::Frame::NONE.show(ui, |ui| {
                                    ui.add_space(8.0);
                                    topbar_action = Some(draw_topbar(
                                        ui,
                                        &self.i18n,
                                        self.theme == ThemeMode::Dark,
                                        self.sidebar_collapsed,
                                        self.hwnd,
                                        &palette,
                                        has_split,
                                    ));
                                });
                                if !self.sidebar_collapsed {
                                    let sidebar_box_size = ui.available_size();
                                    sidebar_frame.show(ui, |ui| {
                                        ui.allocate_ui_with_layout(
                                            sidebar_box_size,
                                            egui::Layout::top_down(egui::Align::Min),
                                            |ui| {
                                                sidebar_action = Some(draw_sidebar(
                                                    ui,
                                                    &self.i18n,
                                                    &icon_cache,
                                                    &mut self.sidebar_state,
                                                    &palette,
                                                    drag_active,
                                                    drag_hover_target.clone(),
                                                    &self.tags_state,
                                                    &mut self.saved_searches_state,
                                                    &self.recent_locations_state,
                                                ));
                                            },
                                        );
                                    });
                                }
                            },
                        );

                        // --- Separator handle (drawn on top, no extra allocation), only when expanded ---
                        if !self.sidebar_collapsed {
                            let separator_width = 6.0;
                            let separator_rect = egui::Rect::from_min_size(
                                egui::pos2(sidebar_width - separator_width / 2.0, 0.0),
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
                                    egui::pos2(sidebar_width, center_y), // exactly on sidebar right edge
                                    egui::vec2(handle_width, handle_height),
                                );

                                ui.painter().rect_filled(
                                    handle_rect,
                                    handle_width / 2.0,
                                    palette.resize_handle,
                                );
                            }

                            if separator_response.dragged() {
                                self.sidebar_state.sidebar_default_width =
                                    (self.sidebar_state.sidebar_default_width
                                        + separator_response.drag_delta().x)
                                        .max(sidebar_width_min)
                                        .min(sidebar_width_max);
                            }

                            if separator_response.drag_stopped() {
                                crate::core::indexer::save_sidebar_sections(
                                    &crate::core::indexer::SidebarSectionsSnapshot {
                                        places: self.sidebar_state.places_expanded,
                                        storage: self.sidebar_state.storage_expanded,
                                        favorites: self.sidebar_state.favorites_expanded,
                                        tags: self.sidebar_state.tags_expanded,
                                        shared_network: self.sidebar_state.shared_network_expanded,
                                        saved_searches: self.sidebar_state.saved_searches_expanded,
                                        recent_locations: self
                                            .sidebar_state
                                            .recent_locations_expanded,
                                        sidebar_width: self.sidebar_state.sidebar_default_width,
                                    },
                                );
                            }
                        }

                        // --- Explorer column ---
                        {
                            update_tab_infos_cache(
                                &self.tabs,
                                &mut self.tab_infos_cache,
                                &mut self.tab_infos_dirty,
                                &self.settings_window,
                                &self.i18n,
                                &self.tags_state,
                            );

                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), ui.available_height()),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    let active_id = self.tabs[self.active_tab].id;

                                    egui::Frame::NONE.show(ui, |ui| {
                                        // Keeps the first tab from sitting flush against
                                        // the sidebar/window edge.
                                        let topbar_left_padding = 6.0;
                                        // Tabs that don't fit on one row wrap onto extra
                                        // rows below, so the topbar's height must grow
                                        // to fit however many rows are needed.
                                        let spacing = ui.spacing().item_spacing.x;
                                        let tab_rows = tab_row_count(
                                            self.tab_infos_cache.len(),
                                            ui.available_width() - topbar_left_padding,
                                            spacing,
                                        ) as f32;
                                        let tabs_content_height =
                                            tab_rows * TAB_HEIGHT + (tab_rows - 1.0).max(0.0) * spacing;
                                        // A bit more breathing room above the first tab
                                        // row and the window control buttons than below,
                                        // so neither sits flush against the app window's
                                        // own top border.
                                        let topbar_top_padding = 8.0;
                                        let topbar_bottom_padding = 4.0;
                                        let topbar_height =
                                            topbar_top_padding + tabs_content_height + topbar_bottom_padding;

                                        let topbar_rect = ui.allocate_exact_size(
                                            egui::vec2(ui.available_width(), topbar_height),
                                            egui::Sense::hover(),
                                        ).0;

                                        // Separates the tab strip from the file/folder view below it.
                                        ui.painter().hline(
                                            topbar_rect.x_range(),
                                            topbar_rect.bottom(),
                                            egui::Stroke::new(1.5, palette.borders_default),
                                        );

                                        // -------------------------
                                        // Tabs
                                        // -------------------------

                                        let tabs_rect = egui::Rect::from_min_size(
                                            topbar_rect.min
                                                + egui::vec2(topbar_left_padding, topbar_top_padding),
                                            egui::vec2(
                                                topbar_rect.width() - topbar_left_padding,
                                                tabs_content_height,
                                            ),
                                        );

                                        let scroll_to_id = self.pending_tab_scroll_id;

                                        ui.scope_builder(
                                            egui::UiBuilder::new().max_rect(tabs_rect),
                                            |ui| {
                                                tabs_action = Some(draw_tabs(
                                                    ui,
                                                    &self.i18n,
                                                    &self.tab_infos_cache,
                                                    active_id,
                                                    &palette,
                                                    self.hwnd,
                                                    scroll_to_id,
                                                    drag_active,
                                                    drag_hover_target.clone(),
                                                    &icon_cache,
                                                    &mut self.dragging_tab_index,
                                                    &self.settings_window.current_settings.tab_groups,
                                                    &self.tags_state.groups,
                                                    &self.sidebar_state.favorites,
                                                    self.saved_searches_state.items.len(),
                                                ));
                                            },
                                        );

                                        if scroll_to_id.is_some() {
                                            self.pending_tab_scroll_id = None;
                                        }

                                        // -------------------------
                                        // Window controls
                                        // -------------------------

                                        let controls_width = 45.0 * 3.0;

                                        let viewport_rect = ui.ctx().viewport_rect();

                                        let controls_rect = egui::Rect::from_min_size(
                                            egui::pos2(
                                                viewport_rect.right() - controls_width,
                                                topbar_rect.top() + topbar_top_padding,
                                            ),
                                            egui::vec2(controls_width, 32.0),
                                        );

                                        // Notification bell - sits just left of the window
                                        // controls, in the same row, so it's genuinely in the
                                        // app's top-right corner rather than buried in the
                                        // sidebar's own topbar. `bell_gap` keeps its own
                                        // clickable area (and the in-progress badge, which
                                        // extends a few px past the icon's own glyph) from
                                        // overlapping the minimize button immediately to its
                                        // right.
                                        let bell_gap = 10.0;
                                        let bell_width = 40.0;
                                        let bell_rect = egui::Rect::from_min_size(
                                            egui::pos2(
                                                controls_rect.min.x - bell_width - bell_gap,
                                                controls_rect.min.y,
                                            ),
                                            egui::vec2(bell_width, controls_rect.height()),
                                        );
                                        ui.scope_builder(
                                            egui::UiBuilder::new().max_rect(bell_rect),
                                            |ui| {
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(egui::Align::Center),
                                                    |ui| {
                                                        draw_notifications_button(
                                                            ui,
                                                            &self.i18n,
                                                            &palette,
                                                            &mut self.notifications_state,
                                                        );
                                                    },
                                                );
                                            },
                                        );

                                        ui.scope_builder(
                                            egui::UiBuilder::new().max_rect(controls_rect),
                                            |ui| {
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(egui::Align::Min),
                                                    |ui| {
                                                        handle_draw_windows_buttons(&self.i18n, ui, self.hwnd, &palette);
                                                    },
                                                );
                                            },
                                        );
                                    });
                                    let container = egui::Frame::NONE
                                        .stroke(egui::Stroke::NONE)
                                        .fill(egui::Color32::TRANSPARENT)
                                        .inner_margin(egui::Margin {
                                            left: 3,
                                            right: 0,
                                            top: 0,
                                            bottom: 0,
                                        });

                                    container.show(ui, |ui| {
                                        if has_split {
                                            let split_rect = ui.available_rect_before_wrap();
                                            let (split_rect, _) = ui.allocate_exact_size(
                                                split_rect.size(),
                                                egui::Sense::hover(),
                                            );
                                            let split_width = split_rect.width();
                                            let split_height = split_rect.height();
                                            const SPLIT_GAP: f32 = 8.0;
                                            let primary_width = ((split_width - SPLIT_GAP) * 0.5).floor();
                                            let secondary_width =
                                                (split_width - primary_width - SPLIT_GAP).max(0.0);
                                            let primary_rect = egui::Rect::from_min_size(
                                                split_rect.min,
                                                egui::vec2(primary_width, split_height),
                                            );

                                            let secondary_rect = egui::Rect::from_min_size(
                                                egui::pos2(
                                                    primary_rect.right() + SPLIT_GAP,
                                                    split_rect.top(),
                                                ),
                                                egui::vec2(secondary_width, split_height),
                                            );

                                            // A subtle divider between the two split panes.
                                            let divider_x = primary_rect.right() + SPLIT_GAP * 0.5;
                                            ui.painter().vline(
                                                divider_x,
                                                split_rect.y_range(),
                                                egui::Stroke::new(1.5, palette.borders_default),
                                            );

                                            let (pointer_pos, primary_clicked) = ui.ctx().input(|i| {
                                                (
                                                    i.pointer.interact_pos(),
                                                    i.pointer.primary_clicked(),
                                                )
                                            });

                                            if primary_clicked {
                                                if let Some(pos) = pointer_pos {
                                                    let in_primary = primary_rect.contains(pos);
                                                    let in_secondary = secondary_rect.contains(pos);

                                                    if in_primary {
                                                        self.focused_split = SplitSide::Primary;
                                                    } else if in_secondary {
                                                        self.focused_split = SplitSide::Secondary;
                                                    }
                                                }
                                            }

                                            let primary_focused = self.focused_split == SplitSide::Primary;
                                            let secondary_focused = !primary_focused;

                                            ui.scope_builder(
                                                egui::UiBuilder::new().max_rect(primary_rect),
                                                |ui| {
                                                ui.set_clip_rect(primary_rect);
                                                ui.push_id("pane_0", |ui| {
                                                    ui.allocate_ui_with_layout(
                                                        ui.available_size(),
                                                        egui::Layout::top_down(egui::Align::Min),
                                                        |ui| {
                                                            if primary_focused {
                                                                let accent_rect =
                                                                    ui.available_rect_before_wrap();
                                                                ui.painter().hline(
                                                                    accent_rect.x_range(),
                                                                    accent_rect.top(),
                                                                    egui::Stroke::new(
                                                                        ACTIVE_PANE_INDICATOR_THICKNESS,
                                                                        palette.borders_active,
                                                                    ),
                                                                );
                                                            }
                                                            ui.add_space(2.0);

                                                            let tab_id = self.tabs[self.active_tab].id;
                                                            let is_favorited = self
                                                                .sidebar_state
                                                                .favorites
                                                                .iter()
                                                                .any(|fav| {
                                                                    fav.path
                                                                        == self.tabs
                                                                            [self.active_tab]
                                                                            .primary_view
                                                                            .nav
                                                                            .current
                                                                });
                                                            let (a, b) = draw_tab_content(
                                                                ui,
                                                                &mut self.i18n,
                                                                &icon_cache,
                                                                &palette,
                                                                self.hwnd,
                                                                &mut self.tabs[self.active_tab]
                                                                    .primary_view,
                                                                tab_id,
                                                                is_favorited,
                                                                &self.folder_sizes,
                                                                self.clipboard_has_files,
                                                                &self.clipboard_set,
                                                                self.clipboard_is_cut,
                                                                self.settings_window
                                                                    .current_settings
                                                                    .show_hidden_files_folders,
                                                                self.settings_window
                                                                    .current_settings
                                                                    .show_item_viewer_icons,
                                                                &mut self.rename_state,
                                                                &mut self.file_type_cache,
                                                                &mut self.file_size_text_cache,
                                                                &mut self.folder_size_text_cache,
                                                                &mut self.drive_size_text_cache,
                                                                &mut self.external_drag_to_internal_hover,
                                                                drag_active,
                                                                native_inbound_drag_active,
                                                                drag_hover_target.clone(),
                                                                &mut self.tags_state,
                                                                &mut self.theme_customizer,
                                                                &mut self.settings_window,
                                                                &mut self.sidebar_state.favorites,
                                                                &mut drop_targets,
                                                                primary_focused,
                                                                true,
                                                                self.saved_searches_state.items.len(),
                                                            );
                                                            tabbar_action = a;
                                                            pending_action = b;
                                                            let snapshot =
                                                                directory_settings_snapshot_for_view(
                                                                    &self.tabs[self.active_tab]
                                                                        .primary_view,
                                                                );
                                                            let _ =
                                                                persist_directory_settings_snapshot(
                                                                    &mut self
                                                                        .settings_window
                                                                        .current_settings
                                                                        .directory_settings,
                                                                    snapshot,
                                                                );
                                                        },
                                                    );
                                                });
                                            });

                                            ui.scope_builder(
                                                egui::UiBuilder::new().max_rect(secondary_rect),
                                                |ui| {
                                                ui.set_clip_rect(secondary_rect);
                                                ui.push_id("pane_1", |ui| {
                                                    ui.allocate_ui_with_layout(
                                                        ui.available_size(),
                                                        egui::Layout::top_down(egui::Align::Min),
                                                        |ui| {
                                                            if secondary_focused {
                                                                let accent_rect =
                                                                    ui.available_rect_before_wrap();
                                                                ui.painter().hline(
                                                                    accent_rect.x_range(),
                                                                    accent_rect.top(),
                                                                    egui::Stroke::new(
                                                                        ACTIVE_PANE_INDICATOR_THICKNESS,
                                                                        palette.borders_active,
                                                                    ),
                                                                );
                                                            }
                                                            ui.add_space(2.0);

                                                            let tab_id = self.tabs[self.active_tab].id;
                                                            let is_favorited = self.tabs
                                                                [self.active_tab]
                                                                .split_view
                                                                .as_ref()
                                                                .map(|v| {
                                                                    self.sidebar_state
                                                                        .favorites
                                                                        .iter()
                                                                        .any(|fav| {
                                                                            fav.path
                                                                                == v.nav.current
                                                                        })
                                                                })
                                                                .unwrap_or(false);
                                                            let (a, b) = draw_tab_content(
                                                                ui,
                                                                &mut self.i18n,
                                                                &icon_cache,
                                                                &palette,
                                                                self.hwnd,
                                                                self.tabs[self.active_tab]
                                                                    .split_view
                                                                    .as_mut()
                                                                    .unwrap(),
                                                                tab_id,
                                                                is_favorited,
                                                                &self.folder_sizes,
                                                                self.clipboard_has_files,
                                                                &self.clipboard_set,
                                                                self.clipboard_is_cut,
                                                                self.settings_window
                                                                    .current_settings
                                                                    .show_hidden_files_folders,
                                                                self.settings_window
                                                                    .current_settings
                                                                    .show_item_viewer_icons,
                                                                &mut self.rename_state,
                                                                &mut self.file_type_cache,
                                                                &mut self.file_size_text_cache,
                                                                &mut self.folder_size_text_cache,
                                                                &mut self.drive_size_text_cache,
                                                                &mut self.external_drag_to_internal_hover,
                                                                drag_active,
                                                                native_inbound_drag_active,
                                                                drag_hover_target.clone(),
                                                                &mut self.tags_state,
                                                                &mut self.theme_customizer,
                                                                &mut self.settings_window,
                                                                &mut self.sidebar_state.favorites,
                                                                &mut drop_targets,
                                                                secondary_focused,
                                                                true,
                                                                self.saved_searches_state.items.len(),
                                                            );
                                                            secondary_tabbar_action = a;
                                                            secondary_pending_action = b;
                                                            let snapshot =
                                                                directory_settings_snapshot_for_view(
                                                                    self.tabs[self.active_tab]
                                                                        .split_view
                                                                        .as_ref()
                                                                        .unwrap(),
                                                                );
                                                            let _ =
                                                                persist_directory_settings_snapshot(
                                                                    &mut self
                                                                        .settings_window
                                                                        .current_settings
                                                                        .directory_settings,
                                                                    snapshot,
                                                                );
                                                        },
                                                    );
                                                });
                                            });
                                        } else {
                                            let tab_id = self.tabs[self.active_tab].id;
                                            let is_favorited =
                                                self.sidebar_state.favorites.iter().any(|fav| {
                                                    fav.path
                                                        == self.tabs[self.active_tab]
                                                            .primary_view
                                                            .nav
                                                            .current
                                                });
                                            let (a, b) = draw_tab_content(
                                                ui,
                                                &mut self.i18n,
                                                &icon_cache,
                                                &palette,
                                                self.hwnd,
                                                &mut self.tabs[self.active_tab].primary_view,
                                                tab_id,
                                                is_favorited,
                                                &self.folder_sizes,
                                                self.clipboard_has_files,
                                                &self.clipboard_set,
                                                self.clipboard_is_cut,
                                                self.settings_window
                                                    .current_settings
                                                    .show_hidden_files_folders,
                                                self.settings_window
                                                    .current_settings
                                                    .show_item_viewer_icons,
                                                &mut self.rename_state,
                                                &mut self.file_type_cache,
                                                &mut self.file_size_text_cache,
                                                &mut self.folder_size_text_cache,
                                                &mut self.drive_size_text_cache,
                                                &mut self.external_drag_to_internal_hover,
                                                drag_active,
                                                native_inbound_drag_active,
                                                drag_hover_target.clone(),
                                                &mut self.tags_state,
                                                &mut self.theme_customizer,
                                                &mut self.settings_window,
                                                &mut self.sidebar_state.favorites,
                                                &mut drop_targets,
                                                true,
                                                false,
                                                self.saved_searches_state.items.len(),
                                            );
                                            tabbar_action = a;
                                            pending_action = b;
                                            let snapshot =
                                                directory_settings_snapshot_for_view(
                                                    &self.tabs[self.active_tab].primary_view,
                                                );
                                            let _ = persist_directory_settings_snapshot(
                                                &mut self
                                                    .settings_window
                                                    .current_settings
                                                    .directory_settings,
                                                snapshot,
                                            );
                                        }
                                    });
                                },
                            );
                        }
                    },
                );
            });
        });

        if let Some(backend) = self.dragdrop.as_ref() {
            backend.update_drop_targets(drop_targets);
        }

        if pending_action.is_none() {
            let dropped_paths: Vec<PathBuf> = ui.ctx().input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .filter_map(|file| file.path.clone())
                    .collect()
            });
            if !dropped_paths.is_empty() {
                pending_action = Some(ItemViewerAction::FilesDropped(dropped_paths));
            }
        }

        let primary_drag_active = self.tabs[self.active_tab].primary_view.drag_state.active;
        if pending_action.is_none() && primary_drag_active && !native_drag_active {
            let pointer_inside = if let Some(hwnd) = self.hwnd {
                unsafe {
                    let mut screen_pt = POINT::default();
                    if GetCursorPos(&mut screen_pt).is_err() {
                        false
                    } else {
                        let mut client_rect = RECT::default();
                        if GetClientRect(hwnd, &mut client_rect).is_err() {
                            false
                        } else {
                            let mut top_left = POINT {
                                x: client_rect.left,
                                y: client_rect.top,
                            };
                            let mut bottom_right = POINT {
                                x: client_rect.right,
                                y: client_rect.bottom,
                            };
                            let _ = ClientToScreen(hwnd, &mut top_left);
                            let _ = ClientToScreen(hwnd, &mut bottom_right);
                            screen_pt.x >= top_left.x
                                && screen_pt.x < bottom_right.x
                                && screen_pt.y >= top_left.y
                                && screen_pt.y < bottom_right.y
                        }
                    }
                }
            } else {
                true
            };

            if !pointer_inside {
                if let Some(backend) = self.dragdrop.as_ref() {
                    if backend.begin_file_drag(
                        &self.tabs[self.active_tab]
                            .primary_view
                            .drag_state
                            .source_items,
                    ) {
                        let view = &mut self.tabs[self.active_tab].primary_view;
                        view.drag_state.active = false;
                        view.drag_state.start_pos = None;
                        view.drag_state.source_items.clear();
                        self.load_path();
                    }
                }
            }
        }

        let primary_drag_sources = if self.tabs[self.active_tab]
            .primary_view
            .drag_state
            .source_items
            .is_empty()
        {
            None
        } else {
            Some(
                self.tabs[self.active_tab]
                    .primary_view
                    .drag_state
                    .source_items
                    .clone(),
            )
        };
        let secondary_drag_sources = self.tabs[self.active_tab]
            .split_view
            .as_ref()
            .and_then(|v| {
                if v.drag_state.source_items.is_empty() {
                    None
                } else {
                    Some(v.drag_state.source_items.clone())
                }
            });
        let tabs_drag_sources = primary_drag_sources
            .clone()
            .or_else(|| secondary_drag_sources.clone());
        let sidebar_drag_sources = primary_drag_sources
            .clone()
            .or_else(|| secondary_drag_sources.clone());
        let primary_move_target = tabbar_action
            .as_ref()
            .and_then(|a| a.move_files_to_breadcrumb_dir.as_ref())
            .is_some()
            || tabs_action
                .as_ref()
                .and_then(|a| a.move_files_to_tab_dir.as_ref())
                .is_some()
            || sidebar_action
                .as_ref()
                .and_then(|a| a.move_files_to_sidebar_dir.as_ref())
                .is_some();
        let secondary_move_target = secondary_tabbar_action
            .as_ref()
            .and_then(|a| a.move_files_to_breadcrumb_dir.as_ref())
            .is_some();

        self.handle_directory_batch_recieve(ui.ctx());
        self.handle_directory_size_updates(ui.ctx());
        self.handle_throttle_size_requests(ui.ctx());
        self.handle_topbar_action(topbar_action);
        self.handle_sidebar_action(sidebar_action, sidebar_drag_sources.as_deref());
        self.handle_tabs_action(tabs_action, tabs_drag_sources.as_deref());

        // Route each view's breadcrumb actions with focus pinned to that view for the
        // duration of the call, since the shared handlers resolve via self.focused_split.
        let restore_focus = self.focused_split;
        if tabbar_action.is_some() {
            self.focused_split = SplitSide::Primary;
            self.handle_tabbar_action(tabbar_action, primary_drag_sources.as_deref());
        }
        if secondary_tabbar_action.is_some() {
            self.focused_split = SplitSide::Secondary;
            self.handle_tabbar_action(secondary_tabbar_action, secondary_drag_sources.as_deref());
        }
        self.focused_split = restore_focus;

        if primary_move_target {
            let view = &mut self.tabs[self.active_tab].primary_view;
            view.drag_state.active = false;
            view.drag_state.start_pos = None;
            view.drag_state.source_items.clear();
        }
        if secondary_move_target {
            if let Some(split) = self.tabs[self.active_tab].split_view.as_mut() {
                split.drag_state.active = false;
                split.drag_state.start_pos = None;
                split.drag_state.source_items.clear();
            }
        }

        if pending_action.is_none() {
            if let Some(command) = self
                .dragdrop
                .as_ref()
                .and_then(|backend| backend.poll_command())
            {
                pending_action = Some(match command {
                    crate::gui::dragdrop::NativeDropCommand::ImportFiles(paths) => {
                        ItemViewerAction::FilesDropped(paths)
                    }
                    crate::gui::dragdrop::NativeDropCommand::MoveFiles {
                        sources,
                        target_dir,
                    } => ItemViewerAction::MoveItems {
                        sources,
                        target_dir,
                    },
                });
            }
        }

        let restore_focus = self.focused_split;
        if pending_action.is_some() {
            self.focused_split = SplitSide::Primary;
            handle_pending_actions(pending_action, self);
        }
        if secondary_pending_action.is_some() {
            self.focused_split = SplitSide::Secondary;
            handle_pending_actions(secondary_pending_action, self);
        }
        self.focused_split = restore_focus;
        // If either call above just queued delayed post-command refreshes,
        // make sure a frame actually runs to check them later - the app may
        // otherwise sit fully idle (no input, nothing else asking to
        // repaint) until the user happens to interact with it again.
        if let Some(next) = self.pending_command_refreshes.iter().min() {
            ui.ctx().request_repaint_after(
                next.saturating_duration_since(std::time::Instant::now()),
            );
        }
        if draw_tag_picker_popup(ui.ctx(), &self.i18n, &palette, &mut self.tags_state) {
            tags_changed = true;
        }
        self.poll_pending_paste();
        self.poll_pending_compress();
        self.poll_pending_checksum();
        self.draw_paste_conflict_modal(ui.ctx(), &palette);
        self.draw_bulk_rename_modal(ui.ctx(), &palette);
        self.draw_checksum_modal(ui.ctx(), &palette);
        draw_toast(ui.ctx(), &self.i18n, &palette, &mut self.notifications_state);
        self.handle_pending_settings_action(ui.ctx());
        self.handle_draw_about_window(ui.ctx(), &palette);
        self.handle_global_shortcuts(ui.ctx());

        if tags_changed {
            self.persist_tags();
        }

        // ✅ Step 5: Apply Deferred Refresh (IMPORTANT)
        if self.dropped_files_pending_ui_refresh {
            self.load_path();
            self.dropped_files_pending_ui_refresh = false;
        }

        // Window decorations are disabled, so paint a border around the
        // whole window to make it distinguishable from other open windows.
        ui.painter().rect_stroke(
            ui.ctx().viewport_rect().shrink(1.5),
            egui::CornerRadius::ZERO,
            egui::Stroke::new(3.0, palette.borders_default),
            egui::StrokeKind::Inside,
        );

        self.icon_cache = Some(icon_cache);
    }

    fn on_exit(&mut self) {
        if !self
            .settings_window
            .current_settings
            .restore_last_session_tabs
        {
            return;
        }

        let tab_path = |nav: &Navigation| {
            if nav.is_root() {
                PathBuf::from(MY_PC_PATH)
            } else {
                nav.current.clone()
            }
        };

        let tabs: Vec<SessionTabEntry> = self
            .tabs
            .iter()
            .map(|tab| SessionTabEntry {
                path: tab_path(&tab.primary_view.nav),
                split_path: tab.split_view.as_ref().map(|split| tab_path(&split.nav)),
            })
            .collect();

        if tabs.is_empty() {
            return;
        }

        save_session_tabs(&SessionTabsSnapshot {
            tabs,
            active_index: self.active_tab,
        });
    }
}
