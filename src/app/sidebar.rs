use eframe::egui;
use std::path::PathBuf;

use crate::app::features::ThemePalette;
use crate::app::icons::IconCache;
use crate::app::utils::drive_usage_gradient;
use crate::drives::DriveInfo;
use egui::{FontFamily, FontId, ScrollArea};

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

/// Draw a single sidebar item (favorite or folder)
fn sidebar_item(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    path: &PathBuf,
    label: &str,
    is_dir: bool,
    palette: &ThemePalette,
    _selected: bool,
) -> egui::Response {
    let available_width = ui.available_width();
    let height = 18.0;

    let (rect, item_resp) =
        ui.allocate_exact_size(egui::vec2(available_width, height), egui::Sense::click());

    // Hover background first
    if item_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(palette.medium_radius),
            palette.primary_hover,
        );
    }

    // Icon
    let icon_size = egui::vec2(20.0, 20.0);
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

    // --- DISPLAY TEXT ---
    let text_width = available_width - text_offset_x;
    let max_chars = (text_width / 7.0) as usize;

    let display_name = if label.len() > max_chars && max_chars > 3 {
        // Use character boundaries instead of byte indices
        let mut char_count = 0;
        let mut byte_end = 0;
        for (i, _) in label.char_indices() {
            if char_count >= max_chars - 3 {
                break;
            }
            char_count += 1;
            byte_end = i;
        }
        format!("{}...", &label[..byte_end])
    } else {
        label.to_string()
    };

    // Text
    let text_pos = egui::pos2(rect.min.x + text_offset_x, rect.center().y);
    let font_id = FontId::new(palette.text_size, FontFamily::Proportional);
    ui.painter().text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        display_name,
        font_id,
        ui.visuals().text_color(),
    );

    item_resp
}

/// Draw a drive item with usage bar and size on hover
fn sidebar_drive_item(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    drive: &DriveInfo,
    palette: &ThemePalette,
    selected: bool,
) -> egui::Response {
    let available_width = ui.available_width();
    let height = 32.0;

    let (rect, mut resp) =
        ui.allocate_exact_size(egui::vec2(available_width, height), egui::Sense::click());

    // Background (selected > active click > hover)
    let fill_color = if selected {
        palette.primary_active
    } else if resp.is_pointer_button_down_on() {
        palette.primary_active
    } else if resp.hovered() {
        palette.primary_hover
    } else {
        egui::Color32::TRANSPARENT
    };

    if fill_color != egui::Color32::TRANSPARENT {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(palette.medium_radius),
            fill_color,
        );
    }

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
    }

    // --- Top row: icon + label ---
    let icon_size = egui::vec2(20.0, 20.0);
    let icon_padding = 4.0;

    let text_offset_x = if let Some(icon) = icon_cache.get(&drive.path, true) {
        let icon_pos = egui::pos2(rect.min.x + 4.0, rect.min.y + 4.0);

        ui.painter().image(
            (&icon).into(),
            egui::Rect::from_min_size(icon_pos, icon_size),
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1.0, 1.0)),
            palette.icon_color,
        );

        8.0 + icon_size.x + icon_padding
    } else {
        8.0 + 16.0 + icon_padding
    };

    // --- DISPLAY TEXT ---
    let text_width = available_width - text_offset_x;
    let max_chars = (text_width / 7.0) as usize;

    let display_name = if drive.display.len() > max_chars && max_chars > 3 {
        // Use character boundaries instead of byte indices
        let mut char_count = 0;
        let mut byte_end = 0;
        for (i, _) in drive.display.char_indices() {
            if char_count >= max_chars - 3 {
                break;
            }
            char_count += 1;
            byte_end = i;
        }
        format!("{}...", &drive.display[..byte_end])
    } else {
        drive.display.clone()
    };

    let text_y = rect.min.y + 4.0 + icon_size.y / 2.0;
    let text_pos = egui::pos2(rect.min.x + text_offset_x, text_y);
    let font_id = FontId::new(palette.text_size, FontFamily::Proportional);

    ui.painter().text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        display_name,
        font_id,
        ui.visuals().text_color(),
    );

    // --- Bottom row: progress bar ---
    if let (Some(total), Some(free)) = (drive.total_space, drive.free_space) {
        let bar_height = 6.0;
        let max_bar_width = 180.0;
        let bar_width = (available_width - 8.0).min(max_bar_width);

        let bar_rect = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + 4.0, rect.bottom() - bar_height - 4.0),
            egui::vec2(bar_width, bar_height),
        );

        let bar_bg = palette.drive_usage_background;
        let bar_fill = drive_usage_gradient((total - free) as f32 / total as f32, palette).0;

        ui.painter().rect_filled(
            bar_rect,
            egui::CornerRadius::same(palette.small_radius),
            bar_bg,
        );

        let used_ratio = (total - free) as f32 / total as f32;
        let fill_width = bar_rect.width() * used_ratio;

        let fill_rect = egui::Rect::from_min_size(bar_rect.min, egui::vec2(fill_width, bar_height));

        ui.painter().rect_filled(
            fill_rect,
            egui::CornerRadius::same(palette.small_radius),
            bar_fill,
        );

        // Tooltip (FIXED)
        let gb = 1024.0 * 1024.0 * 1024.0;
        let used_gb = (total - free) as f64 / gb;
        let total_gb = total as f64 / gb;

        resp = resp.on_hover_text(
            egui::RichText::new(format!("{:.1}/{:.1}GB", used_gb, total_gb))
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color)
        );
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
    palette: &ThemePalette,
    dragging_favorite: &mut Option<usize>, // track dragged item globally
) -> SidebarAction {
    let mut action = SidebarAction::default();
    let mut drop_index: Option<usize> = None;

    ScrollArea::vertical()
        .id_salt("sidebar_scroll")
        .auto_shrink([false; 2]) // don't shrink horizontally or vertically
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y *= 0.5;

                    ui.add(egui::Label::new(
                        egui::RichText::new("Places")
                            .size(palette.text_size)
                            .strong(),
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
                        let resp =
                            sidebar_item(ui, icon_cache, &home, "My User Home", true, palette, false);
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
                        egui::RichText::new("Favorites")
                            .size(palette.text_size)
                            .strong(),
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

                        if resp.hovered()
                            && dragging_favorite.is_some()
                            && dragging_favorite.unwrap() != i
                        {
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
                                        egui::CornerRadius::same(palette.large_radius),
                                        palette.primary_active,
                                    );

                                    let font_id =
                                        FontId::new(palette.text_size, FontFamily::Proportional);

                                    ui.painter().text(
                                        pos,
                                        egui::Align2::CENTER_CENTER,
                                        &favorites[*drag_idx].label,
                                        font_id,
                                        palette.icon_color,
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
                        egui::RichText::new("Storage")
                            .size(palette.text_size)
                            .strong(),
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
                ui.add_space(2.0);
            });
        });

    action
}
