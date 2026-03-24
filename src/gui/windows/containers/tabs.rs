use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::utils::clickable_icon;
use crate::gui::windows::containers::enums::TabbarNavAction;
use crate::gui::windows::containers::structs::{TabInfo, TabState, TabbarAction, TabsAction};
use eframe::egui;
use egui::{FontFamily, FontId};
use egui_phosphor::regular;
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};

pub fn draw_tabs(
    ui: &mut egui::Ui,
    tabs: &[TabInfo],
    active_id: u64,
    palette: &ThemePalette,
    hwnd: Option<HWND>,
) -> TabsAction {
    let mut action: TabsAction = TabsAction::default();
    let controls_width = 64.0;
    let full_width = ui.available_width();
    let tabs_width = full_width - controls_width;

    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 32.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            // --- TABS AREA (CLIPPED) ---
            ui.allocate_ui_with_layout(
                egui::vec2(tabs_width, 32.0),
                egui::Layout::left_to_right(egui::Align::Min),
                |ui| {
                    // 🔥 THIS is where overflow must be handled
                    egui::ScrollArea::horizontal()
                        .id_salt("tabs_scroll")
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for tab in tabs {
                                    handle_draw_tab_new(ui, tab, active_id, palette, &mut action);
                                }
                            });
                            handle_draw_add_new_tab_button(ui, palette, &mut action);
                        });
                },
            );

            // --- RIGHT SIDE ---
            ui.allocate_ui_with_layout(
                egui::vec2(controls_width, 32.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    handle_draw_windows_buttons(ui, hwnd, palette);
                },
            );
        },
    );
    action
}

fn handle_draw_tab(
    ui: &mut egui::Ui,
    tab: &TabInfo,
    active_id: u64,
    palette: &ThemePalette,
    action: &mut TabsAction,
) {
    let is_active = tab.id == active_id;
    let corner = if is_active {
        palette.tab_active_radius
    } else {
        palette.tab_inactive_radius
    };
    let tab_fill = if is_active {
        ui.visuals().widgets.active.bg_fill
    } else {
        egui::Color32::TRANSPARENT
    };

    let tab_frame = egui::Frame::NONE
        .fill(tab_fill)
        .corner_radius(corner)
        .stroke(egui::Stroke::NONE);

    let resp = tab_frame
        .show(ui, |ui| {
            ui.set_min_width(160.0);
            ui.set_max_width(200.0);
            ui.set_min_height(28.0);

            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.add_space(2.0); // fine-tune
                    ui.add(egui::Label::new(regular::FOLDER_SIMPLE).selectable(false));
                });

                let label_color = if is_active {
                    palette.row_label_selected
                } else {
                    ui.visuals().widgets.noninteractive.fg_stroke.color
                };

                let font_id = FontId::new(palette.text_size, FontFamily::Proportional);

                // --- DISPLAY TEXT WITH TRUNCATION ---
                let available_width = ui.available_width() - 24.0; // Account for spacing
                let max_chars = (available_width / 7.0) as usize; // Approximate character width

                let display_title = if tab.title.len() > max_chars && max_chars > 3 {
                    // Use character boundaries instead of byte indices
                    let mut char_count = 0;
                    let mut byte_end = 0;
                    for (i, _) in tab.title.char_indices() {
                        if char_count >= max_chars - 3 {
                            break;
                        }
                        char_count += 1;
                        byte_end = i;
                    }
                    format!("{}...", &tab.title[..byte_end])
                } else {
                    tab.title.clone()
                };

                let resp = ui.add(egui::Label::new(
                    egui::RichText::new(display_title)
                        .color(label_color)
                        .font(font_id),
                ));

                // Add tooltip showing full path
                let resp = resp.on_hover_text(
                    egui::RichText::new(format!("{}", tab.full_path.display()))
                        .size(palette.tooltip_text_size)
                        .color(palette.tooltip_text_color),
                );

                // Change cursor depending on hover state
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
                } else {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
                }
            });
        })
        .response
        .interact(egui::Sense::click());

    let rect = resp.rect;

    // 👇 Allow space for outside stroke, but still clip bottom
    let clip_rect = egui::Rect::from_min_max(
        egui::pos2(rect.min.x - 10.0, rect.min.y - 1.0),
        egui::pos2(rect.max.x + 10.0, rect.max.y + 0.5),
    );

    let painter = ui.painter().with_clip_rect(clip_rect);

    let rounding = egui::CornerRadius {
        nw: corner.nw,
        ne: corner.ne,
        sw: 0,
        se: 0,
    };

    let stroke = if is_active {
        egui::Stroke::new(1.0, palette.tab_border_active)
    } else {
        egui::Stroke::new(1.0, palette.tab_border_default)
    };

    // ✅ Draw border
    painter.rect_stroke(rect, rounding, stroke, egui::StrokeKind::Outside);

    // 🎯 Active tab blends into panel
    if is_active {
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            egui::Stroke::new(2.0, ui.visuals().panel_fill),
        );
    }

    if resp.clicked() {
        action.activate = Some(tab.id);
    }

    let close_resp = tab_close_button(ui, resp.rect, tab.id, palette);
    if close_resp.clicked() {
        action.close = Some(tab.id);
    }

    let _ = resp;
}

fn handle_draw_tab_new(
    ui: &mut egui::Ui,
    tab: &TabInfo,
    active_id: u64,
    palette: &ThemePalette,
    action: &mut TabsAction,
) {
    let is_active = tab.id == active_id;
    let corner = if is_active {
        palette.tab_active_radius
    } else {
        palette.tab_inactive_radius
    };
    let tab_fill = if is_active {
        ui.visuals().widgets.active.bg_fill
    } else {
        egui::Color32::TRANSPARENT
    };

    // 🔥 1. Allocate clickable region FIRST
    let width = 200.0;
    let desired_size = egui::vec2(width, 28.0);
    let (rect, resp) = ui.allocate_exact_size(desired_size, egui::Sense::click());

    // 🔥 2. Paint background manually
    let painter = ui.painter();
    painter.rect_filled(rect, corner, tab_fill);

    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.add_space(2.0); // fine-tune
                ui.add(egui::Label::new(regular::FOLDER_SIMPLE).selectable(false));
            });

            let label_color = if is_active {
                palette.row_label_selected
            } else {
                ui.visuals().widgets.noninteractive.fg_stroke.color
            };

            let font_id = FontId::new(palette.text_size, FontFamily::Proportional);

            // --- DISPLAY TEXT WITH TRUNCATION ---
            let available_width = ui.available_width() - 24.0; // Account for spacing
            let max_chars = (available_width / 7.0) as usize; // Approximate character width

            let display_title = if tab.title.len() > max_chars && max_chars > 3 {
                // Use character boundaries instead of byte indices
                let mut char_count = 0;
                let mut byte_end = 0;
                for (i, _) in tab.title.char_indices() {
                    if char_count >= max_chars - 3 {
                        break;
                    }
                    char_count += 1;
                    byte_end = i;
                }
                format!("{}...", &tab.title[..byte_end])
            } else {
                tab.title.clone()
            };

            let label_resp = ui.add(egui::Label::new(
                egui::RichText::new(display_title)
                    .color(label_color)
                    .font(font_id),
            ));

            // Add tooltip showing full path
            let label_resp = label_resp.on_hover_text(
                egui::RichText::new(format!("{}", tab.full_path.display()))
                    .size(palette.tooltip_text_size)
                    .color(palette.tooltip_text_color),
            );

            // Change cursor depending on hover state
            if label_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
            } else {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
            }
        });
    });

    // let rect = resp.rect;

    // 👇 Allow space for outside stroke, but still clip bottom
    let clip_rect = egui::Rect::from_min_max(
        egui::pos2(rect.min.x - 10.0, rect.min.y - 1.0),
        egui::pos2(rect.max.x + 10.0, rect.max.y + 0.5),
    );

    let clipped = ui.painter().with_clip_rect(clip_rect);

    let rounding = egui::CornerRadius {
        nw: corner.nw,
        ne: corner.ne,
        sw: 0,
        se: 0,
    };

    let stroke = if is_active {
        egui::Stroke::new(1.0, palette.tab_border_active)
    } else {
        egui::Stroke::new(1.0, palette.tab_border_default)
    };

    // ✅ Draw border
    clipped.rect_stroke(rect, rounding, stroke, egui::StrokeKind::Outside);

    // 🎯 Active tab blends into panel
    if is_active {
        clipped.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            egui::Stroke::new(2.0, ui.visuals().panel_fill),
        );
    }

    let close_resp = tab_close_button(ui, resp.rect, tab.id, palette);
    if close_resp.clicked() {
        action.close = Some(tab.id);
    } else if resp.clicked() {
        action.activate = Some(tab.id);
    }
}

fn handle_draw_add_new_tab_button(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    action: &mut TabsAction,
) {
    let desired_size = egui::vec2(32.0, 32.0); // give it a bit more vertical room
    let (rect, _outer_resp) = ui.allocate_exact_size(desired_size, egui::Sense::hover());

    // 🔥 Shrink the rect from the top by 12px
    let inner_rect = rect.shrink2(egui::vec2(0.0, 4.0));

    ui.scope_builder(egui::UiBuilder::new().max_rect(inner_rect), |ui| {
        let add_frame = egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(6, 6))
            .corner_radius(palette.tab_inactive_radius)
            .stroke(egui::Stroke::new(1.0, palette.tab_border_default));

        let add_resp = add_frame.show(ui, |ui| {
            ui.set_min_width(25.0);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
            (rect, resp)
        });

        let add_resp = tab_add_button(ui, add_resp.inner.0, add_resp.inner.1, palette);

        if add_resp.clicked() {
            action.open_new = true;
        }
    });
}

fn handle_draw_windows_buttons(ui: &mut egui::Ui, hwnd: Option<HWND>, palette: &ThemePalette) {
    if let Some(hwnd) = hwnd {
        if clickable_icon(ui, regular::X, palette.primary).clicked() {
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::*;
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }

        if clickable_icon(ui, regular::SQUARE, palette.primary).clicked() {
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::*;
                let mut placement = WINDOWPLACEMENT::default();
                placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;

                if GetWindowPlacement(hwnd, &mut placement).is_ok() {
                    if placement.showCmd == SW_SHOWMAXIMIZED.0 as u32 {
                        let _ = ShowWindow(hwnd, SW_RESTORE);
                    } else {
                        let _ = ShowWindow(hwnd, SW_MAXIMIZE);
                    }
                }
            }
        }

        if clickable_icon(ui, regular::MINUS, palette.primary).clicked() {
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::*;
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            }
        }
    }
}

fn tab_close_button(
    ui: &mut egui::Ui,
    tab_rect: egui::Rect,
    tab_id: u64,
    palette: &ThemePalette,
) -> egui::Response {
    let size = egui::vec2(18.0, 18.0);
    let rect = egui::Rect::from_min_size(
        egui::pos2(
            tab_rect.right() - size.x - 6.0,
            tab_rect.center().y - size.y * 0.5,
        ),
        size,
    );
    let resp = ui.interact(
        rect,
        ui.id().with(("tab_close", tab_id)),
        egui::Sense::click(),
    );
    let hovered = resp.hovered();
    let bg = if hovered {
        palette.tab_close_hover
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, palette.tab_button_radius, bg);

    let color = if hovered {
        palette.icon_color
    } else {
        ui.visuals().widgets.noninteractive.fg_stroke.color
    };

    let font_id = FontId::new(palette.text_size, FontFamily::Proportional);

    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        regular::X,
        font_id,
        color,
    );

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    resp
}

fn tab_add_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    resp: egui::Response,
    palette: &ThemePalette,
) -> egui::Response {
    let hovered = resp.hovered();
    let nudge = egui::vec2(4.0, 0.0);
    let rect = rect.translate(nudge);

    let bg = if hovered {
        palette.tab_add_hover
    } else {
        egui::Color32::TRANSPARENT
    };

    ui.painter()
        .rect_filled(rect, palette.tab_button_radius, bg);

    let color = if hovered {
        palette.icon_color
    } else {
        ui.visuals().widgets.noninteractive.fg_stroke.color
    };

    let font_id = FontId::new(palette.text_size, FontFamily::Proportional);

    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        regular::PLUS,
        font_id,
        color,
    );

    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    resp
}

fn toolbar_buttons(ui: &mut egui::Ui, palette: &ThemePalette) -> TabbarAction {
    let mut action = TabbarAction::default();

    // Navigation buttons
    if clickable_icon(ui, regular::ARROW_LEFT, palette.primary).clicked() {
        action.nav = Some(TabbarNavAction::Back);
    }

    if clickable_icon(ui, regular::ARROW_RIGHT, palette.primary).clicked() {
        action.nav = Some(TabbarNavAction::Forward);
    }

    if clickable_icon(ui, regular::ARROW_UP, palette.primary).clicked() {
        action.nav = Some(TabbarNavAction::Up);
    }

    if clickable_icon(ui, regular::ARROWS_CLOCKWISE, palette.primary).clicked() {
        action.refresh_current_directory = true;
    }

    // Action buttons
    if clickable_icon(ui, regular::FOLDER_PLUS, palette.primary).clicked() {
        action.create_folder = true;
    }

    if clickable_icon(ui, regular::FILE_PLUS, palette.primary).clicked() {
        action.create_file = true;
    }

    if clickable_icon(ui, regular::STAR, palette.primary).clicked() {
        action.add_favorite = true;
    }

    action
}

pub fn draw_tabbar(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    tab: &mut TabState,
    palette: &ThemePalette,
) -> TabbarAction {
    let mut action = TabbarAction::default();

    ui.horizontal(|ui| {
        let toolbar_action = toolbar_buttons(ui, palette);

        // Merge toolbar actions
        if toolbar_action.nav.is_some() {
            action.nav = toolbar_action.nav;
        }
        if toolbar_action.refresh_current_directory {
            action.refresh_current_directory = true;
        }
        if toolbar_action.create_folder {
            action.create_folder = true;
        }
        if toolbar_action.create_file {
            action.create_file = true;
        }
        if toolbar_action.add_favorite {
            action.add_favorite = true;
        }

        ui.separator();

        // 👇 SWITCH BETWEEN MODES
        if tab.is_editing_path {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut tab.path_buffer)
                    .desired_width(ui.available_width() - 40.0)
                    .font(FontId::new(
                        palette.text_size,
                        egui::FontFamily::Proportional,
                    )),
            );

            resp.request_focus();

            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));

            if enter {
                action.nav_to = Some(PathBuf::from(tab.path_buffer.clone()));
                tab.is_editing_path = false;
            } else if escape {
                tab.is_editing_path = false;
            } else if resp.lost_focus() {
                tab.is_editing_path = false;
            }
        } else if tab.nav.is_root() {
            let pc_icon_path = PathBuf::from("C:\\");
            if let Some(icon) = icon_cache.get(&pc_icon_path, true) {
                ui.add(egui::Image::new(&icon).fit_to_exact_size(egui::vec2(14.0, 14.0)));
            }

            if ui
                .add(
                    egui::Label::new(egui::RichText::new("This PC").size(palette.text_size))
                        .selectable(false)
                        .sense(egui::Sense::click()),
                )
                .clicked()
            {
                action.nav_to = Some(PathBuf::from("::MY_PC::"));
            }
        } else {
            let segments =
                build_breadcrumbs(&tab.nav.current, ui.available_width(), palette.text_size);
            let mut first = true;

            // Track right-most x-coordinate of breadcrumbs
            let mut breadcrumbs_right = 0.0;

            for (_idx, (label, path)) in segments.iter().enumerate() {
                if !first {
                    let old_spacing = ui.spacing().item_spacing;
                    ui.spacing_mut().item_spacing.x = 10.0;

                    ui.label(
                        egui::RichText::new(">")
                            .size(palette.text_size)
                            .color(ui.visuals().widgets.noninteractive.fg_stroke.color),
                    );

                    ui.spacing_mut().item_spacing = old_spacing;
                }
                first = false;

                let inner = egui::Frame::NONE
                    .fill(egui::Color32::TRANSPARENT)
                    .inner_margin(egui::Margin::symmetric(6, 2))
                    .corner_radius(egui::CornerRadius::same(palette.medium_radius))
                    .show(ui, |ui| {
                        let mut label_text = label.clone();

                        // Optional: truncate the label if too long
                        let max_label_len = 15;
                        if label_text.len() > max_label_len {
                            label_text = format!("{}...", &label_text[..max_label_len - 3]);
                        }

                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(label_text).size(palette.text_size),
                            )
                            .selectable(false)
                            .sense(egui::Sense::click()),
                        )
                    });

                let resp = inner.response.union(inner.inner);

                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }

                if resp.clicked() {
                    // If "..." clicked, navigate to root
                    if label == "..." {
                        if let Some((_, root_path)) = segments.first() {
                            action.nav_to = Some(root_path.clone());
                        }
                    } else {
                        action.nav_to = Some(path.clone());
                    }
                }

                breadcrumbs_right = resp.rect.right();
            }

            // ---- Detect click in empty area to enter path editing ----
            let available_rect = ui.available_rect_before_wrap();
            let empty_area = egui::Rect::from_min_max(
                egui::pos2(breadcrumbs_right, available_rect.top()),
                available_rect.right_bottom(),
            );

            if ui
                .interact(
                    empty_area,
                    ui.id().with("breadcrumb_empty"),
                    egui::Sense::click(),
                )
                .clicked()
            {
                tab.is_editing_path = true;
                tab.path_buffer = tab.nav.current.to_string_lossy().to_string();
            }
        }
    });

    action
}

fn build_breadcrumbs(path: &Path, available_width: f32, font_size: f32) -> Vec<(String, PathBuf)> {
    use std::path::Component;

    // Approximate width per character
    let char_width = font_size * 0.55;
    let separator_width = char_width * 4.0; // width for '>' separator

    // Collect all segments
    let mut all_segments = Vec::new();
    let mut current = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(prefix) => {
                current.push(prefix.as_os_str());
                all_segments.push((
                    prefix.as_os_str().to_string_lossy().to_string(),
                    current.clone(),
                ));
            }
            Component::RootDir => {
                current.push(Path::new("\\"));
            }
            Component::Normal(name) => {
                current.push(name);
                all_segments.push((name.to_string_lossy().to_string(), current.clone()));
            }
            _ => {}
        }
    }

    // Compute total width
    let total_width: f32 = all_segments
        .iter()
        .map(|(label, _)| label.len() as f32 * char_width + separator_width)
        .sum();

    // If everything fits, show all segments
    if total_width <= available_width {
        return all_segments;
    }

    // Otherwise, truncate dynamically
    let mut segments = Vec::new();

    // Always show "..." pointing to root
    if let Some((_, root_path)) = all_segments.first() {
        segments.push(("...".to_string(), root_path.clone()));
    }

    // Start from the end and add segments until we fill the available space
    let mut used_width = 0.0;
    let mut included_segments = Vec::new();

    // Reverse iterator for trailing segments
    for (label, path) in all_segments.iter().rev() {
        let label_width = label.len() as f32 * char_width + separator_width;
        if used_width + label_width > available_width - char_width * 3.0 {
            // Leave room for "..." and a little buffer
            break;
        }
        used_width += label_width;
        included_segments.push((label.clone(), path.clone()));
    }

    // Reverse to keep the correct order
    included_segments.reverse();

    // Append trailing segments after "..."
    segments.extend(included_segments);

    segments
}
