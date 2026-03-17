use eframe::egui;
use std::path::PathBuf;

use crate::app::icons::IconCache;
use crate::app::utils::drive_usage_bar_sidebar;
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
}

#[derive(Clone)]
pub struct SidebarPalette {
    pub hover: egui::Color32,
    pub active: egui::Color32,
}

fn sidebar_item(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    path: &PathBuf,
    label: &str,
    is_dir: bool,
    palette: &SidebarPalette,
    selected: bool,
) -> egui::Response {
    let width = ui.available_width();
    let height = 22.0;
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(width, height),
        egui::Sense::click(),
    );
    let mut combined_resp = resp.clone();

    if ui.is_rect_visible(rect) {
        let fill = if selected {
            Some(palette.active)
        } else if combined_resp.is_pointer_button_down_on() {
            Some(palette.active)
        } else if combined_resp.hovered() {
            Some(palette.hover)
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
        child.add_space(4.0);

        if let Some(icon) = icon_cache.get(path, is_dir) {
            let icon_resp = child.add(
                egui::Image::new(&icon)
                    .fit_to_exact_size(egui::vec2(16.0, 16.0)),
            );
            combined_resp = combined_resp.union(icon_resp);
        } else {
            child.add_space(16.0);
        }

        child.add_space(6.0);
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

fn sidebar_drive_item(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    drive: &DriveInfo,
    palette: &SidebarPalette,
) -> egui::Response {
    let width = ui.available_width();
    let height = if drive.total_space.is_some() { 44.0 } else { 24.0 };
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(width, height),
        egui::Sense::click(),
    );
    let mut combined_resp = resp.clone();

    if ui.is_rect_visible(rect) {
        let fill = if combined_resp.is_pointer_button_down_on() {
            Some(palette.active)
        } else if combined_resp.hovered() {
            Some(palette.hover)
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
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.add_space(2.0);

        child.horizontal(|ui| {
            if let Some(icon) = icon_cache.get(&drive.path, true) {
                let icon_resp = ui.add(
                    egui::Image::new(&icon)
                        .fit_to_exact_size(egui::vec2(16.0, 16.0)),
                );
                combined_resp = combined_resp.union(icon_resp);
            } else {
                ui.add_space(16.0);
            }

            ui.add_space(6.0);
            let label_resp = ui.add(
                egui::Label::new(
                    egui::RichText::new(&drive.display)
                        .text_style(egui::TextStyle::Button),
                )
                .sense(egui::Sense::click()),
            );
            combined_resp = combined_resp.union(label_resp);
        });

        if let (Some(total), Some(free)) = (drive.total_space, drive.free_space) {
            child.add_space(2.0);
            child.spacing_mut().item_spacing.y = 2.0;
            drive_usage_bar_sidebar(&mut child, total, free);
        }
    }

    if combined_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    combined_resp
}

pub fn draw_sidebar(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    favorites: &[FavoriteItem],
    sidebar_selected: Option<&PathBuf>,
    drives: &[DriveInfo],
    palette: &SidebarPalette,
) -> SidebarAction {
    let mut action = SidebarAction::default();

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y *= 0.5;
        ui.add(egui::Label::new(
            egui::RichText::new("Places")
                .size(13.0)
                .strong(),
        ));
        ui.add_space(8.0);

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

        if let Some(home) = dirs::home_dir() {
            let resp = sidebar_item(
                ui,
                icon_cache,
                &home,
                "Home",
                true,
                palette,
                false,
            );
            if resp.clicked() {
                action.nav_to = Some(home.clone());
            }
            if resp.middle_clicked() {
                action.open_new_tab = Some(home);
            }
        }

        ui.add_space(6.0);
        ui.add(egui::Label::new(
            egui::RichText::new("Favorites")
                .size(13.0)
                .strong(),
        ));
        ui.add_space(4.0);

        for favorite in favorites {
            let fav_path = favorite.path.clone();
            let resp = sidebar_item(
                ui,
                icon_cache,
                &fav_path,
                &favorite.label,
                true,
                palette,
                sidebar_selected
                    .map(|p| p == &fav_path)
                    .unwrap_or(false),
            );
            if resp.clicked() {
                action.nav_to = Some(favorite.path.clone());
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
        }

        ui.add_space(6.0);
        ui.add(egui::Label::new(
            egui::RichText::new("Storage")
                .size(13.0)
                .strong(),
        ));
        ui.add_space(4.0);

        for drive in drives {
            let resp = sidebar_drive_item(ui, icon_cache, drive, palette);
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
