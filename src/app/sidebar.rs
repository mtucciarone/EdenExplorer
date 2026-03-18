use eframe::egui;
use std::path::PathBuf;

use crate::app::icons::IconCache;
use crate::app::utils::drive_usage_gradient;
use crate::drives::DriveInfo;

#[derive(Clone)]
pub struct FavoriteItem {
    pub path: PathBuf,
    pub label: String,
}

#[derive(Default)]
pub struct SidebarAction {
    pub nav_to: Option<PathBuf>,
    pub open_new_tab: Option<PathBuf>,
    pub remove_favorite: Option<PathBuf>,
    pub select_favorite: Option<PathBuf>,
    pub reorder: Option<(usize, usize)>, // from_idx, to_idx
}

#[derive(Clone)]
pub struct SidebarPalette {
    pub hover: egui::Color32,
    pub active: egui::Color32,
}

/// Draw a single sidebar item (favorite or folder)
fn sidebar_item(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    path: &PathBuf,
    label: &str,
    is_dir: bool,
    palette: &SidebarPalette,
    selected: bool,
) -> egui::Response {
    let available_width = ui.available_width();
    let height = ui.text_style_height(&egui::TextStyle::Button) + 4.0; // vertical padding

    let (rect, item_resp) =
        ui.allocate_exact_size(egui::vec2(available_width, height), egui::Sense::click());

    // Hover background first
    if item_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(4), palette.hover);
    }

    // Icon
    let icon_size = egui::vec2(16.0, 16.0);
    let icon_padding = 4.0;
    let text_offset_x = if let Some(icon) = icon_cache.get(path, is_dir) {
        let icon_pos = egui::pos2(rect.min.x + 4.0, rect.center().y - icon_size.y / 2.0);
        ui.painter().image(
            (&icon).into(),
            egui::Rect::from_min_size(icon_pos, icon_size),
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        8.0 + icon_size.x + icon_padding
    } else {
        8.0 + 16.0 + icon_padding
    };

    // Text
    let text_pos = egui::pos2(rect.min.x + text_offset_x, rect.center().y);
    ui.painter().text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        label,
        egui::TextStyle::Button.resolve(ui.style()),
        ui.style().visuals.text_color(),
    );

    item_resp
}

/// Draw a drive item with usage bar and size on hover
fn sidebar_drive_item(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    drive: &DriveInfo,
    palette: &SidebarPalette,
    selected: bool,
) -> egui::Response {
    let available_width = ui.available_width();
    let height = 32.0;

    let (rect, mut resp) =
        ui.allocate_exact_size(egui::vec2(available_width, height), egui::Sense::click());

    // Background (selected > active click > hover)
    let fill_color = if selected {
        palette.active
    } else if resp.is_pointer_button_down_on() {
        palette.active
    } else if resp.hovered() {
        palette.hover
    } else {
        egui::Color32::TRANSPARENT
    };

    if fill_color != egui::Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(4), fill_color);
    }

    // Cursor
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // --- Top row: icon + label ---
    let icon_size = egui::vec2(16.0, 16.0);
    let icon_padding = 4.0;

    let text_offset_x = if let Some(icon) = icon_cache.get(&drive.path, true) {
        let icon_pos = egui::pos2(rect.min.x + 4.0, rect.min.y + 4.0);

        ui.painter().image(
            (&icon).into(),
            egui::Rect::from_min_size(icon_pos, icon_size),
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1.0, 1.0)),
            egui::Color32::WHITE,
        );

        8.0 + icon_size.x + icon_padding
    } else {
        8.0 + 16.0 + icon_padding
    };

    let text_y = rect.min.y + 4.0 + icon_size.y / 2.0;
    let text_pos = egui::pos2(rect.min.x + text_offset_x, text_y);

    ui.painter().text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        &drive.display,
        egui::TextStyle::Button.resolve(ui.style()),
        ui.style().visuals.text_color(),
    );

    // --- Bottom row: progress bar ---
    if let (Some(total), Some(free)) = (drive.total_space, drive.free_space) {
        let bar_height = 6.0;

        let bar_rect = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + 4.0, rect.bottom() - bar_height - 4.0),
            egui::vec2(available_width - 8.0, bar_height),
        );

        let bar_bg = palette.hover.gamma_multiply(0.5);
        let bar_fill = drive_usage_gradient((total - free) as f32 / total as f32).0;

        ui.painter()
            .rect_filled(bar_rect, egui::CornerRadius::same(2), bar_bg);

        let used_ratio = (total - free) as f32 / total as f32;
        let fill_width = bar_rect.width() * used_ratio;

        let fill_rect = egui::Rect::from_min_size(bar_rect.min, egui::vec2(fill_width, bar_height));

        ui.painter()
            .rect_filled(fill_rect, egui::CornerRadius::same(2), bar_fill);

        // Tooltip (FIXED)
        let gb = 1024.0 * 1024.0 * 1024.0;
        let used_gb = (total - free) as f64 / gb;
        let total_gb = total as f64 / gb;

        resp = resp.on_hover_text(format!("{:.1}/{:.1}GB", used_gb, total_gb));
    }

    resp
}

/// Draw the sidebar, supporting favorites reordering
pub fn draw_sidebar(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    favorites: &mut [FavoriteItem],
    sidebar_selected: Option<&PathBuf>,
    drives: &[DriveInfo],
    palette: &SidebarPalette,
    dragging_favorite: &mut Option<usize>, // track dragged item globally
) -> SidebarAction {
    let mut action = SidebarAction::default();
    let mut drop_index: Option<usize> = None;

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y *= 0.5;

        ui.add(egui::Label::new(
            egui::RichText::new("Places").size(13.0).strong(),
        ));
        ui.add_space(8.0);

        // "This PC"
        let pc_icon_path = PathBuf::from("C:\\");
        let resp = sidebar_item(
            ui,
            icon_cache,
            &pc_icon_path,
            "This PC",
            true,
            palette,
            false,
        );
        if resp.clicked() {
            action.nav_to = Some(PathBuf::from("::MY_PC::"));
        }
        if resp.middle_clicked() {
            action.open_new_tab = Some(PathBuf::from("::MY_PC::"));
        }

        // User Home
        if let Some(home) = dirs::home_dir() {
            let resp = sidebar_item(ui, icon_cache, &home, "My User Home", true, palette, false);
            if resp.clicked() {
                action.nav_to = Some(home.clone());
            }
            if resp.middle_clicked() {
                action.open_new_tab = Some(home);
            }
        }

        // Favorites
        ui.add_space(6.0);
        ui.add(egui::Label::new(
            egui::RichText::new("Favorites").size(13.0).strong(),
        ));
        ui.add_space(4.0);

        for (i, favorite) in favorites.iter().enumerate() {
            let fav_path = favorite.path.clone();
            let is_selected = sidebar_selected.map(|p| p == &fav_path).unwrap_or(false);
            let resp = sidebar_item(
                ui,
                icon_cache,
                &fav_path,
                &favorite.label,
                true,
                palette,
                is_selected,
            );

            if resp.clicked() {
                action.nav_to = Some(fav_path.clone());
            }
            if resp.secondary_clicked() {
                action.select_favorite = Some(fav_path.clone());
            }
            if resp.middle_clicked() {
                action.open_new_tab = Some(fav_path.clone());
            }

            resp.context_menu(|ui| {
                if ui.button("Remove Favorite").clicked() {
                    action.remove_favorite = Some(fav_path.clone());
                    ui.close();
                }
            });

            // --- Drag logic ---
            if resp.drag_started() {
                *dragging_favorite = Some(i);
            }

            if resp.hovered() && dragging_favorite.is_some() && dragging_favorite.unwrap() != i {
                drop_index = Some(i);
            }

            // Draw drag ghost
            if let Some(drag_idx) = dragging_favorite {
                if *drag_idx == i {
                    let pointer = ui.ctx().input(|i| i.pointer.hover_pos());
                    if let Some(pos) = pointer {
                        ui.painter().rect_filled(
                            egui::Rect::from_center_size(
                                pos,
                                egui::vec2(ui.available_width(), 22.0),
                            ),
                            egui::CornerRadius::same(6),
                            palette.active,
                        );
                        // Get the FontId for the desired text style
                        let font_id: egui::FontId =
                            ui.style().text_styles[&egui::TextStyle::Button].clone();

                        ui.painter().text(
                            pos,
                            egui::Align2::CENTER_CENTER,
                            &favorites[*drag_idx].label,
                            font_id,
                            egui::Color32::WHITE,
                        );
                    }
                }
            }

            // Commit reorder on release
            if ui.ctx().input(|i| i.pointer.any_released()) {
                if let (Some(from), Some(to)) = (*dragging_favorite, drop_index) {
                    action.reorder = Some((from, to));
                }
                *dragging_favorite = None;
                drop_index = None;
            }
        }

        // Storage drives
        ui.add_space(6.0);
        ui.add(egui::Label::new(
            egui::RichText::new("Storage").size(13.0).strong(),
        ));
        ui.add_space(4.0);

        for drive in drives {
            let is_selected = sidebar_selected.map(|p| p == &drive.path).unwrap_or(false);

            let resp = sidebar_drive_item(ui, icon_cache, drive, palette, is_selected);
            if resp.clicked() {
                action.nav_to = Some(drive.path.clone());
            }
            if resp.middle_clicked() {
                action.open_new_tab = Some(drive.path.clone());
            }
        }
    });

    action
}
