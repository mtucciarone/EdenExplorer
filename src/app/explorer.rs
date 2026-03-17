use eframe::egui;
use std::collections::HashMap;
use std::path::PathBuf;

use crossbeam_channel::{unbounded, Receiver, Sender};

use crate::state::{FileItem, Navigation};
use crate::fs::{scan_dir_async, get_drive_space, calculate_folder_size_fast};
use crate::drives::get_drives;
use crate::app::icons::IconCache; // 🔥 FIXED PATH

use super::sorting::{sort_files, SortColumn};
use super::table::{draw_table, TableAction};

#[derive(Clone, Copy, PartialEq)]
enum ThemeMode {
    Light,
    Dark,
}

struct ThemePalette {
    topbar_bg: egui::Color32,
    sidebar_bg: egui::Color32,
    sidebar_hover: egui::Color32,
    sidebar_active: egui::Color32,
}

fn palette(mode: ThemeMode) -> ThemePalette {
    match mode {
        ThemeMode::Dark => ThemePalette {
            topbar_bg: egui::Color32::from_rgb(24, 27, 31),
            sidebar_bg: egui::Color32::from_rgb(28, 32, 37),
            sidebar_hover: egui::Color32::from_rgb(38, 44, 52),
            sidebar_active: egui::Color32::from_rgb(46, 54, 64),
        },
        ThemeMode::Light => ThemePalette {
            topbar_bg: egui::Color32::from_rgb(247, 248, 250),
            sidebar_bg: egui::Color32::from_rgb(235, 239, 245),
            sidebar_hover: egui::Color32::from_rgb(224, 232, 242),
            sidebar_active: egui::Color32::from_rgb(214, 224, 236),
        },
    }
}

fn apply_theme(ctx: &egui::Context, mode: ThemeMode) {
    let mut style = (*ctx.style()).clone();
    style.visuals = match mode {
        ThemeMode::Dark => egui::Visuals::dark(),
        ThemeMode::Light => egui::Visuals::light(),
    };

    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(10);
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::proportional(18.0),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::proportional(14.0),
    );
    style.visuals.window_corner_radius = egui::CornerRadius::same(10);
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(6);
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(6);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(6);

    match mode {
        ThemeMode::Dark => {
            style.visuals.panel_fill = egui::Color32::from_rgb(20, 22, 26);
            style.visuals.faint_bg_color = egui::Color32::from_rgb(26, 30, 36);
            style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(30, 34, 40);
            style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(38, 44, 52);
            style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(46, 54, 64);
            style.visuals.selection.bg_fill = egui::Color32::from_rgb(60, 90, 130);
            style.visuals.selection.stroke.color = egui::Color32::from_rgb(120, 160, 210);
            style.visuals.widgets.inactive.fg_stroke.color = egui::Color32::from_rgb(220, 226, 232);
            style.visuals.widgets.hovered.fg_stroke.color = egui::Color32::from_rgb(235, 240, 246);
            style.visuals.widgets.active.fg_stroke.color = egui::Color32::from_rgb(245, 248, 252);
            style.visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::from_rgb(160, 170, 180);
        }
        ThemeMode::Light => {
            style.visuals.panel_fill = egui::Color32::from_rgb(250, 251, 253);
            style.visuals.faint_bg_color = egui::Color32::from_rgb(244, 246, 249);
            style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(247, 248, 250);
            style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(236, 240, 246);
            style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(224, 231, 242);
            style.visuals.selection.bg_fill = egui::Color32::from_rgb(210, 225, 245);
            style.visuals.selection.stroke.color = egui::Color32::from_rgb(60, 90, 130);
            style.visuals.widgets.inactive.fg_stroke.color = egui::Color32::from_rgb(35, 41, 47);
            style.visuals.widgets.hovered.fg_stroke.color = egui::Color32::from_rgb(25, 29, 33);
            style.visuals.widgets.active.fg_stroke.color = egui::Color32::from_rgb(15, 18, 22);
            style.visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::from_rgb(70, 78, 86);
        }
    }

    ctx.set_style(style);
}

fn sidebar_item(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    path: &PathBuf,
    label: &str,
    is_dir: bool,
    palette: &ThemePalette,
) -> egui::Response {
    let width = ui.available_width();
    let height = 26.0;
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(width, height),
        egui::Sense::click(),
    );
    let mut combined_resp = resp.clone();

    if ui.is_rect_visible(rect) {
        let fill = if combined_resp.is_pointer_button_down_on() {
            Some(palette.sidebar_active)
        } else if combined_resp.hovered() {
            Some(palette.sidebar_hover)
        } else {
            None
        };

        if let Some(color) = fill {
            ui.painter().rect_filled(
                rect,
                egui::CornerRadius::same(6),
                color,
            );
        }

        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.add_space(6.0);

        if let Some(icon) = icon_cache.get(path, is_dir) {
            let icon_resp = child.add(
                egui::Image::new(&icon)
                    .fit_to_exact_size(egui::vec2(16.0, 16.0)),
            );
            combined_resp = combined_resp.union(icon_resp);
        } else {
            child.add_space(16.0);
        }

        child.add_space(8.0);
        let label_resp = child.add(
            egui::Label::new(
                egui::RichText::new(label)
                    .text_style(egui::TextStyle::Button),
            )
            .sense(egui::Sense::click()),
        );
        combined_resp = combined_resp.union(label_resp);
    }

    if combined_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    combined_resp
}

pub struct ExplorerApp {
    nav: Navigation,
    files: Vec<FileItem>,
    rx: Option<Receiver<FileItem>>,
    size_req_tx: Option<Sender<PathBuf>>,
    size_rx: Option<Receiver<(PathBuf, u64)>>,
    folder_sizes: HashMap<PathBuf, u64>,
    theme: ThemeMode,
    theme_dirty: bool,
    sort_column: SortColumn,
    sort_ascending: bool,
    icon_cache: Option<IconCache>, // 🔥 FIX: lazy init
}

impl Default for ExplorerApp {
    fn default() -> Self {
        let mut app = Self {
            nav: Navigation::new(),
            files: vec![],
            rx: None,
            size_req_tx: None,
            size_rx: None,
            folder_sizes: HashMap::new(),
            theme: ThemeMode::Dark,
            theme_dirty: true,
            sort_column: SortColumn::Name,
            sort_ascending: true,
            icon_cache: None, // 🔥 FIX
        };

        app.load_path();
        app
    }
}

impl ExplorerApp {
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

        if self.nav.is_root() {
            for d in get_drives() {
                let drive_letter = d.chars().take(3).collect::<String>();
                let path = PathBuf::from(&drive_letter);

                if let Some((total, free)) = get_drive_space(&path) {
                    self.files.push(FileItem::with_drive_info(
                        d.clone(),
                        path,
                        true,
                        None,
                        None,
                        total,
                        free,
                    ));
                } else {
                    self.files.push(FileItem::new(
                        d.clone(),
                        path,
                        true,
                        None,
                        None,
                    ));
                }
            }

            sort_files(&mut self.files, self.sort_column, self.sort_ascending);
            return;
        }

        let (tx, rx) = unbounded();
        scan_dir_async(self.nav.current.clone(), tx);
        self.rx = Some(rx);

        let (size_req_tx, size_req_rx) = unbounded::<PathBuf>();
        let (size_done_tx, size_done_rx) = unbounded::<(PathBuf, u64)>();
        self.size_req_tx = Some(size_req_tx);
        self.size_rx = Some(size_done_rx);

        std::thread::spawn(move || {
            while let Ok(path) = size_req_rx.recv() {
                let size = calculate_folder_size_fast(path.clone());
                let _ = size_done_tx.send((path, size));
            }
        });
    }
}

impl eframe::App for ExplorerApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if self.theme_dirty {
            apply_theme(ctx, self.theme);
            self.theme_dirty = false;
        }

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
                if let Some(size_req_tx) = &self.size_req_tx {
                    for item in batch.iter() {
                        if item.is_dir {
                            let _ = size_req_tx.send(item.path.clone());
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
                    Ok((path, size)) => {
                        self.folder_sizes.insert(path.clone(), size);
                        if let Some(item) =
                            self.files.iter_mut().find(|f| f.path == path)
                        {
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

        // Toolbar
        let palette = palette(self.theme);

        egui::TopBottomPanel::top("toolbar")
            .frame(
                egui::Frame::NONE
                    .fill(palette.topbar_bg)
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Back").clicked() {
                    self.nav.go_back();
                    self.load_path();
                }
                if ui.button("Forward").clicked() {
                    self.nav.go_forward();
                    self.load_path();
                }
                if ui.button("Up").clicked() {
                    self.nav.go_up();
                    self.load_path();
                }

                ui.separator();
                if self.nav.is_root() {
                    let pc_icon_path = PathBuf::from("C:\\");
                    if let Some(icon) = icon_cache.get(&pc_icon_path, true) {
                        ui.add(
                            egui::Image::new(&icon)
                                .fit_to_exact_size(egui::vec2(16.0, 16.0)),
                        );
                    }
                    ui.label("This PC");
                } else {
                    ui.label(self.nav.current.display().to_string());
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (icon, label) = match self.theme {
                        ThemeMode::Dark => ("☀", "Light"),
                        ThemeMode::Light => ("🌙", "Dark"),
                    };

                    if ui.button(format!("{} {}", icon, label)).clicked() {
                        self.theme = match self.theme {
                            ThemeMode::Dark => ThemeMode::Light,
                            ThemeMode::Light => ThemeMode::Dark,
                        };
                        self.theme_dirty = true;
                    }
                });
            });
        });

        // Sidebar
        egui::SidePanel::left("sidebar")
            .default_width(220.0)
            .frame(
                egui::Frame::NONE
                    .fill(palette.sidebar_bg)
                    .inner_margin(egui::Margin::symmetric(12, 12)),
            )
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.heading("Places");
                    ui.add_space(8.0);

                    let pc_icon_path = PathBuf::from("C:\\");
                    if sidebar_item(
                        ui,
                        &icon_cache,
                        &pc_icon_path,
                        "This PC",
                        true,
                        &palette,
                    )
                    .clicked()
                    {
                        self.nav.go_to(PathBuf::from("::MY_PC::"));
                        self.load_path();
                    }

                    if let Some(home) = dirs::home_dir() {
                        if sidebar_item(
                            ui,
                            &icon_cache,
                            &home,
                            "Home",
                            true,
                            &palette,
                        )
                        .clicked()
                        {
                            self.nav.go_to(home);
                            self.load_path();
                        }
                    }

                    ui.add_space(12.0);
                    ui.heading("Favorites");
                    ui.add_space(8.0);

                    if let Some(home) = dirs::home_dir() {
                        let desktop = home.join("Desktop");
                        if sidebar_item(
                            ui,
                            &icon_cache,
                            &desktop,
                            "Desktop",
                            true,
                            &palette,
                        )
                        .clicked()
                        {
                            self.nav.go_to(desktop);
                            self.load_path();
                        }

                        let documents = home.join("Documents");
                        if sidebar_item(
                            ui,
                            &icon_cache,
                            &documents,
                            "Documents",
                            true,
                            &palette,
                        )
                        .clicked()
                        {
                            self.nav.go_to(documents);
                            self.load_path();
                        }

                        let downloads = home.join("Downloads");
                        if sidebar_item(
                            ui,
                            &icon_cache,
                            &downloads,
                            "Downloads",
                            true,
                            &palette,
                        )
                        .clicked()
                        {
                            self.nav.go_to(downloads);
                            self.load_path();
                        }

                        let pictures = home.join("Pictures");
                        if sidebar_item(
                            ui,
                            &icon_cache,
                            &pictures,
                            "Pictures",
                            true,
                            &palette,
                        )
                        .clicked()
                        {
                            self.nav.go_to(pictures);
                            self.load_path();
                        }
                    }
                });
            });

        // Main table
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(action) = draw_table(
                ui,
                &self.files,
                &self.folder_sizes,
                self.sort_column,
                self.sort_ascending,
                &icon_cache, // ✅ works now
            ) {
                match action {
                    TableAction::Sort(col) => self.toggle_sort(col),
                    TableAction::Open(path) => {
                        self.nav.go_to(path);
                        self.load_path();
                    }
                }
            }
        });

        // 🔥 PUT IT BACK
        self.icon_cache = Some(icon_cache);
    }
}
