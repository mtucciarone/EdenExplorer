use crate::core::drives::{DriveInfo, get_drive_infos};
use crate::core::fs::MY_PC_PATH;
use crate::core::networkdevices::NetworkDevicesState;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::utils::{draw_object_drag_ghost, drive_usage_gradient, truncate_item_text};
use crate::gui::windows::containers::structs::SidebarAction;
use crate::gui::windows::structs::SidebarState;
use eframe::egui;
use egui::{FontFamily, FontId, ScrollArea};
use egui_phosphor::regular;
use std::path::PathBuf;

pub fn draw_sidebar_item(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    path: &PathBuf,
    label: &str,
    is_dir: bool,
    palette: &ThemePalette,
    draggable: bool,
    rect_and_resp: Option<(egui::Rect, egui::Response)>,
) -> egui::Response {
    let height = 18.0;

    // --- Get rect + response ---
    let (rect, resp) = if let Some((rect, resp)) = rect_and_resp {
        (rect, resp)
    } else {
        let available_width = ui.available_width();
        ui.allocate_exact_size(
            egui::vec2(available_width, height),
            if draggable {
                egui::Sense::click_and_drag()
            } else {
                egui::Sense::click()
            },
        )
    };

    // --- Hover background + cursor ---
    if resp.hovered() {
        ui.ctx().set_cursor_icon(if draggable {
            egui::CursorIcon::Grab
        } else {
            egui::CursorIcon::Default
        });

        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(palette.medium_radius),
            palette.primary_hover,
        );

        if draggable {
            let handle_width = 12.0;
            let handle_rect = egui::Rect::from_min_size(
                egui::pos2(rect.right() - handle_width - 4.0, rect.top()),
                egui::vec2(handle_width, rect.height()),
            );

            ui.painter().text(
                handle_rect.center(),
                egui::Align2::CENTER_CENTER,
                regular::DOTS_SIX_VERTICAL,
                egui::FontId::new(14.0, egui::FontFamily::Proportional),
                palette.icon_color,
            );
        }
    }

    // --- Icon ---
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

        palette.text_size + icon_size.x + icon_padding
    } else {
        palette.text_size + 20.0 + icon_padding
    };

    // --- Text ---
    let text_width = rect.width() - text_offset_x;
    let font_id = egui::FontId::new(palette.text_size, egui::FontFamily::Proportional);
    let color = ui.visuals().text_color();

    let (display_name, truncated) = truncate_item_text(ui, label, text_width, &font_id, color);

    let text_pos = egui::pos2(rect.min.x + text_offset_x, rect.center().y - 2.0);

    ui.painter().text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        display_name,
        font_id,
        color,
    );

    // --- Tooltip + cursor ---
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);

    if truncated {
        resp.on_hover_text(
            egui::RichText::new(label)
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        )
    } else {
        resp.on_hover_text(
            egui::RichText::new(path.to_string_lossy())
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        )
    }
}

fn favorites_item_layout(ui: &mut egui::Ui) -> (egui::Rect, egui::Response) {
    let available_width = ui.available_width();
    let height = 18.0;

    ui.allocate_exact_size(
        egui::vec2(available_width, height),
        egui::Sense::click_and_drag(),
    )
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
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
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

        palette.text_size + icon_size.x + icon_padding
    } else {
        palette.text_size + 20.0 + icon_padding
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
            egui::pos2(rect.min.x + 4.0, rect.bottom() - bar_height),
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

        let gb = 1024.0 * 1024.0 * 1024.0;
        let used_gb = (total - free) as f64 / gb;
        let total_gb = total as f64 / gb;

        resp = resp.on_hover_text(
            egui::RichText::new(format!("{:.1}/{:.1}GB", used_gb, total_gb))
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        );
    }

    resp
}

/// Draw the sidebar, supporting favorites reordering
pub fn draw_sidebar(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    sidebar_state: &mut SidebarState,
    palette: &ThemePalette,
) -> SidebarAction {
    let mut action = SidebarAction::default();
    let mut drop_index: Option<usize> = None;
    let pointer_pos = ui.ctx().input(|i| i.pointer.hover_pos());
    let pointer_released = ui.ctx().input(|i| i.pointer.primary_released());

    ScrollArea::vertical()
        .id_salt("sidebar_scroll")
        .auto_shrink([false; 2]) // don't shrink horizontally or vertically
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.add_space(8.0);
                    ui.spacing_mut().item_spacing.y *= 0.5;

                    ui.add(egui::Label::new(
                        egui::RichText::new("Places")
                            .size(palette.text_size)
                            .strong(),
                    ));
                    ui.add_space(8.0);

                    let pc_icon_path = PathBuf::from("C:\\");
                    let resp = draw_sidebar_item(
                        ui,
                        icon_cache,
                        &pc_icon_path,
                        "This PC",
                        true,
                        palette,
                        false,
                        None,
                    );
                    if resp.clicked() {
                        action.nav_to = Some(PathBuf::from(MY_PC_PATH));
                    }
                    if resp.middle_clicked() {
                        action.open_new_tab = Some(PathBuf::from(MY_PC_PATH));
                    }

                    if let Some(home) = dirs::home_dir() {
                        let resp = draw_sidebar_item(
                            ui,
                            icon_cache,
                            &home,
                            "My User Home",
                            true,
                            palette,
                            false,
                            None,
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
                            .size(palette.text_size)
                            .strong(),
                    ));
                    ui.add_space(4.0);

                    let mut item_layouts = Vec::new();

                    for (i, _favorite) in sidebar_state.favorites.iter().enumerate() {
                        let (rect, resp) = favorites_item_layout(ui);

                        if resp.drag_started() {
                            sidebar_state.dragging_favorite = Some(i);
                        }

                        item_layouts.push((rect, resp));
                    }

                    if let (Some(pos), Some(drag_idx)) =
                        (pointer_pos, sidebar_state.dragging_favorite)
                    {
                        drop_index = None;

                        for (i, (rect, _)) in item_layouts.iter().enumerate() {
                            let mid_y = rect.center().y;

                            let new_index = if pos.y < mid_y { i } else { i + 1 };

                            if new_index != drag_idx && new_index != drag_idx + 1 {
                                drop_index = Some(new_index);
                            }

                            if pos.y < rect.bottom() {
                                break;
                            }
                        }

                        if let Some(last) = item_layouts.last() {
                            if pos.y > last.0.bottom() {
                                drop_index = Some(item_layouts.len());
                            }
                        }
                    }

                    for (i, favorite) in sidebar_state.favorites.iter().enumerate() {
                        let (rect, resp) = &item_layouts[i];

                        draw_sidebar_item(
                            ui,
                            icon_cache,
                            &favorite.path,
                            &favorite.label,
                            true,
                            palette,
                            true,
                            Some((*rect, resp.clone())),
                        );

                        if resp.clicked() {
                            action.nav_to = Some(favorite.path.clone());
                        }
                        if resp.secondary_clicked() {
                            action.select_favorite = Some(favorite.path.clone());
                        }
                        if resp.middle_clicked() {
                            action.open_new_tab = Some(favorite.path.clone());
                        }

                        resp.context_menu(|ui| {
                            if ui.button("Remove Favorite").clicked() {
                                action.remove_favorite = Some(favorite.path.clone());
                                ui.close();
                            }
                        });

                        if let Some(drop) = drop_index {
                            if drop == i {
                                let painter = ui.ctx().layer_painter(egui::LayerId::new(
                                    egui::Order::Background,
                                    egui::Id::new(format!("insert_line_{}", i)),
                                ));

                                let y = resp.rect.top();
                                let left = resp.rect.left() + 6.0;
                                let right = resp.rect.right() - 6.0;

                                painter.line_segment(
                                    [egui::pos2(left, y), egui::pos2(right, y)],
                                    egui::Stroke::new(2.0, palette.primary_active),
                                );
                            }
                        }
                    }

                    if let Some(drop) = drop_index {
                        if drop == sidebar_state.favorites.len() {
                            if let Some(rect) = item_layouts.last().map(|(r, _)| r) {
                                let painter = ui.ctx().layer_painter(egui::LayerId::new(
                                    egui::Order::Background,
                                    egui::Id::new("insert_line_end"),
                                ));

                                let y = rect.bottom();
                                let left = rect.left() + 6.0;
                                let right = rect.right() - 6.0;

                                painter.line_segment(
                                    [egui::pos2(left, y), egui::pos2(right, y)],
                                    egui::Stroke::new(2.0, palette.primary_active),
                                );
                            }
                        }
                    }

                    if let Some(drag_idx) = sidebar_state.dragging_favorite {
                        draw_object_drag_ghost(
                            ui,
                            palette,
                            &sidebar_state.favorites[drag_idx].label,
                            true,
                        );
                    }

                    if let Some(from) = sidebar_state.dragging_favorite {
                        if pointer_released {
                            if let Some(to) = drop_index {
                                action.reorder = Some((from, to));
                            }

                            sidebar_state.dragging_favorite = None;
                            drop_index = None;
                        }
                    }

                    ui.add_space(6.0);
                    ui.add(egui::Label::new(
                        egui::RichText::new("Storage")
                            .size(palette.text_size)
                            .strong(),
                    ));
                    ui.add_space(4.0);

                    let drives = get_drive_infos();
                    for drive in drives {
                        let is_selected = sidebar_state
                            .item_clicked
                            .as_ref()
                            .map(|p| p == &drive.path)
                            .unwrap_or(false);

                        let resp = sidebar_drive_item(ui, icon_cache, &drive, palette, is_selected);
                        if resp.clicked() {
                            action.nav_to = Some(drive.path.clone());
                        }
                        if resp.middle_clicked() {
                            action.open_new_tab = Some(drive.path.clone());
                        }
                    }

                    // ui.add_space(6.0);
                    // ui.add(egui::Label::new(
                    //     egui::RichText::new("Network")
                    //         .size(palette.text_size)
                    //         .strong(),
                    // ));
                    // ui.add_space(4.0);

                    // network_state.update();
                    // network_state.start_loading();

                    // if network_state.loading {
                    //     ui.add(egui::Label::new(
                    //         egui::RichText::new("Scanning...")
                    //             .size(palette.text_size),
                    //     ));
                    // }

                    // for device in &network_state.devices {
                    //     let device_path = PathBuf::from(format!("\\\\{}", device.name));

                    //     let resp = draw_sidebar_item(
                    //         ui,
                    //         icon_cache,
                    //         &device_path,
                    //         &device.name,
                    //         true,
                    //         palette,
                    //         false,
                    //         None,
                    //     );

                    //     if resp.clicked() {
                    //         action.nav_to = Some(device_path.clone());
                    //     }
                    //     if resp.middle_clicked() {
                    //         action.open_new_tab = Some(device_path);
                    //     }
                    // }
                });
                ui.add_space(2.0);
            });
        });

    action
}
