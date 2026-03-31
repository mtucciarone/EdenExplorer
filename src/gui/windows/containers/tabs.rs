use crate::core::fs::MY_PC_PATH;
use crate::core::portable;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::utils::{clear_clipboard_files, clickable_icon, truncate_item_text};
use crate::gui::windows::containers::enums::TabbarNavAction;
use crate::gui::windows::containers::structs::{TabInfo, TabState, TabbarAction, TabsAction};
use crate::gui::windows::windowsoverrides::handle_draw_windows_buttons;
use eframe::egui;
use egui::{FontFamily, FontId};
use egui_phosphor::{fill, regular};
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::HWND;

pub fn draw_tabs(
    ui: &mut egui::Ui,
    tabs: &[TabInfo],
    active_id: u64,
    palette: &ThemePalette,
    hwnd: Option<HWND>,
    scroll_to_id: Option<u64>,
) -> TabsAction {
    let mut action: TabsAction = TabsAction::default();
    let controls_width = 64.0;
    let full_width = ui.available_width();
    let tabs_width = (full_width - controls_width).max(0.0);

    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 32.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            // --- TABS AREA (CLIPPED) ---
            ui.allocate_ui_with_layout(
                egui::vec2(tabs_width, 32.0),
                egui::Layout::left_to_right(egui::Align::Min),
                |ui| {
                    let tabs_rect = ui.available_rect_before_wrap();
                    // 🔥 THIS is where overflow must be handled
                    egui::ScrollArea::horizontal()
                        .id_salt("tabs_scroll")
                        .auto_shrink([false, true])
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for tab in tabs {
                                    // Compute tab width dynamically
                                    let tab_width = 140.0;
                                    let (rect, resp) = ui.allocate_exact_size(
                                        egui::vec2(tab_width, 28.0),
                                        egui::Sense::click(),
                                    );
                                    if Some(tab.id) == scroll_to_id {
                                        resp.scroll_to_me(Some(egui::Align::Center));
                                    }
                                    handle_draw_tab_new_allocated(
                                        ui,
                                        tab,
                                        rect,
                                        resp.clone(),
                                        active_id,
                                        palette,
                                        &mut action,
                                    );
                                }
                            });
                            handle_draw_add_new_tab_button(ui, palette, &mut action);
                        });

                    // Drag region: empty space to the right of the last tab
                    let tab_width = 140.0;
                    let add_tab_width = 28.0;
                    let spacing = ui.spacing().item_spacing.x;
                    let tab_count = tabs.len() as f32;
                    let gaps = if tab_count > 0.0 {
                        tab_count - 1.0
                    } else {
                        0.0
                    };
                    let content_width =
                        tab_count * tab_width + gaps * spacing + spacing + add_tab_width;

                    if content_width < tabs_rect.width() {
                        let empty_left = tabs_rect.min.x + content_width;
                        let drag_rect = egui::Rect::from_min_max(
                            egui::pos2(empty_left, tabs_rect.min.y),
                            tabs_rect.max,
                        );
                        if drag_rect.width() > 4.0 {
                            let resp = ui.allocate_rect(drag_rect, egui::Sense::click_and_drag());
                            if resp.drag_started() || resp.dragged() {
                                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                            }
                            if resp.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                            }
                        }
                    }
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

fn handle_draw_tab_new_allocated(
    ui: &mut egui::Ui,
    tab: &TabInfo,
    rect: egui::Rect,
    resp: egui::Response,
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

    // --- Font and colors ---
    let font_id = FontId::new(palette.text_size, FontFamily::Proportional);
    let label_color = if is_active {
        palette.tab_text_selected
    } else {
        palette.tab_text_normal
    };

    // --- Layout parameters ---
    let icon_size = 16.0;
    let spacing = 6.0;
    let padding = 8.0;
    let close_button_width = 20.0;
    let icon_pos = egui::pos2(rect.left() + padding, rect.center().y);
    let text_pos = egui::pos2(rect.left() + padding + icon_size + spacing, rect.center().y);
    let text_width = rect.width() - icon_size - spacing - 2.0 * padding - close_button_width;

    let (display_title, truncated) =
        truncate_item_text(ui, &tab.title, text_width, &font_id, label_color);

    // --- NOW safe to use painter ---
    let painter = ui.painter();

    // --- Paint background ---
    painter.rect_filled(rect, corner, tab_fill);

    // --- Draw folder icon ---
    painter.text(
        icon_pos,
        egui::Align2::LEFT_CENTER,
        regular::FOLDER_SIMPLE,
        font_id.clone(),
        label_color,
    );

    painter.text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        display_title,
        font_id.clone(),
        label_color,
    );

    // --- Draw border ---
    let stroke = if is_active {
        egui::Stroke::new(1.0, palette.tab_border_active)
    } else {
        egui::Stroke::new(1.0, palette.tab_border_default)
    };

    let rounding = egui::CornerRadius {
        nw: corner.nw,
        ne: corner.ne,
        sw: 0,
        se: 0,
    };

    painter.rect_stroke(rect, rounding, stroke, egui::StrokeKind::Outside);

    if is_active {
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            egui::Stroke::new(2.0, ui.visuals().panel_fill),
        );
    }

    let close_resp = tab_close_button(ui, rect, tab.id, palette);
    if close_resp.clicked() {
        action.close = Some(tab.id);
    } else if resp.clicked() {
        action.activate = Some(tab.id);
    }

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);

        let tooltip_text = if truncated {
            tab.title.clone()
        } else {
            tab.full_path.to_string_lossy().to_string()
        };

        resp.on_hover_text(
            egui::RichText::new(tooltip_text)
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        );
    }
}

fn handle_draw_add_new_tab_button(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    action: &mut TabsAction,
) {
    let size = egui::vec2(28.0, 28.0);

    let frame = egui::Frame::NONE
        .inner_margin(egui::Margin::same(0)) // 👈 controls padding inside border
        .corner_radius(palette.tab_inactive_radius)
        .stroke(egui::Stroke::new(1.0, palette.tab_border_default));

    let response = frame
        .show(ui, |ui| {
            let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());

            // ✅ shrink AFTER allocation
            let rect = rect.shrink(1.5);

            let resp = tab_add_button(ui, rect, resp, palette);
            resp
        })
        .inner;

    if response.clicked() {
        action.open_new = true;
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
        palette.icon_colored_hover
    } else {
        palette.icon_color
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

    let bg = if hovered {
        palette.tab_add_hover
    } else {
        egui::Color32::TRANSPARENT
    };

    let visual_rect = rect.shrink(4.0);

    ui.painter()
        .rect_filled(visual_rect, palette.tab_button_radius, bg);

    let color = if hovered {
        palette.icon_colored_hover
    } else {
        //ui.visuals().widgets.noninteractive.fg_stroke.color
        palette.icon_color
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

fn toolbar_buttons(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    is_favorited: bool,
    is_root: bool,
) -> TabbarAction {
    let mut action = TabbarAction::default();

    // Navigation buttons
    if clickable_icon(ui, regular::ARROW_LEFT, palette.primary)
        .on_hover_text(
            egui::RichText::new("Navigate to previous directory")
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        action.nav = Some(TabbarNavAction::Back);
    }

    if clickable_icon(ui, regular::ARROW_RIGHT, palette.primary)
        .on_hover_text(
            egui::RichText::new("Navigate to next directory")
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        action.nav = Some(TabbarNavAction::Forward);
    }

    if clickable_icon(ui, regular::ARROW_UP, palette.primary)
        .on_hover_text(
            egui::RichText::new("Navigate to parent directory")
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        action.nav = Some(TabbarNavAction::Up);
    }

    if clickable_icon(ui, regular::ARROWS_CLOCKWISE, palette.primary)
        .on_hover_text(
            egui::RichText::new("Refresh current directory")
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        action.refresh_current_directory = true;
        clear_clipboard_files();
    }

    // Action buttons
    if clickable_icon(ui, regular::FOLDER_PLUS, palette.primary)
        .on_hover_text(
            egui::RichText::new("Create new folder")
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        action.create_folder = true;
    }

    if clickable_icon(ui, regular::FILE_PLUS, palette.primary)
        .on_hover_text(
            egui::RichText::new("Create new file")
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        action.create_file = true;
    }

    let star_icon = if is_favorited {
        fill::STAR
    } else {
        regular::STAR
    };
    let star_color = if is_favorited {
        palette.button_favorite_fill
    } else {
        palette.tooltip_text_color
    };

    let star_font = if is_favorited {
        FontId::new(palette.text_size, FontFamily::Name("phosphor_fill".into()))
    } else {
        FontId::new(palette.text_size, FontFamily::Proportional)
    };

    let star_resp = ui.add_enabled(
        !is_root,
        egui::Label::new(
            egui::RichText::new(star_icon)
                .font(star_font)
                .color(star_color),
        )
        .selectable(false)
        .sense(egui::Sense::click()),
    );

    let star_resp = if is_root {
        star_resp.on_hover_text(
            egui::RichText::new("Favorites are disabled on This PC")
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        )
    } else {
        star_resp
            .on_hover_text(
                egui::RichText::new(if is_favorited {
                    "Remove current directory from favorites"
                } else {
                    "Add current directory to favorites"
                })
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
    };

    if star_resp.clicked() {
        if is_favorited {
            action.remove_favorite = true;
        } else {
            action.add_favorite = true;
        }
    }

    action
}

pub fn draw_tabbar(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    tab: &mut TabState,
    palette: &ThemePalette,
    is_favorited: bool,
) -> TabbarAction {
    let mut action = TabbarAction::default();

    ui.horizontal(|ui| {
        let toolbar_action = toolbar_buttons(ui, palette, is_favorited, tab.nav.is_root());

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
        if toolbar_action.remove_favorite {
            action.remove_favorite = true;
        }

        ui.separator();

        // 👇 SWITCH BETWEEN MODES
        if tab.breadcrumb_path_editing {
            let text_edit_id = ui.id().with("breadcrumbs_path_edit");

            // --- 🔥 Shake animation ---
            let mut offset_x = 0.0;
            if tab.breadcrumb_path_error {
                let t = (ui.input(|i| i.time) - tab.breadcrumb_path_error_animation_time) as f32;

                if t < 0.4 {
                    let frequency = 30.0_f32;
                    let amplitude = 4.0 * (1.0 - t / 0.4); // decay
                    offset_x = (t * frequency).sin() * amplitude;
                }
            }

            ui.add_space(offset_x);

            let time_since_error = ui.input(|i| i.time) - tab.breadcrumb_path_error_animation_time;
            let error_strength = (1.0 - time_since_error * 2.0).clamp(0.0, 1.0);

            let stroke_color = if tab.breadcrumb_path_error {
                egui::Color32::from_rgba_premultiplied(255, 80, 80, (255.0 * error_strength) as u8)
            } else {
                ui.visuals().widgets.inactive.bg_stroke.color
            };

            let frame = egui::Frame::NONE
                .stroke(egui::Stroke::new(1.5, stroke_color))
                .corner_radius(egui::CornerRadius::same(4));

            let mut resp = frame
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut tab.breadcrumb_path_buffer)
                            .id(text_edit_id)
                            .frame(!tab.breadcrumb_path_error)
                            .desired_width(ui.available_width() - 40.0)
                            .font(FontId::new(
                                palette.text_size,
                                egui::FontFamily::Proportional,
                            )),
                    )
                })
                .inner;

            if resp.changed() {
                tab.breadcrumb_path_error = false;
            }

            if tab.breadcrumb_path_error && resp.hovered() {
                resp = resp.on_hover_text(
                    egui::RichText::new("Path does not exist")
                        .size(palette.tooltip_text_size)
                        .color(palette.tooltip_text_color),
                );
            }

            if !tab.breadcrumb_just_started_editing || tab.breadcrumb_path_error {
                resp.request_focus();
                tab.breadcrumb_just_started_editing = true;
            }

            // ✅ Track focus for global shortcut blocking
            action.is_breadcrumb_path_edit_active = resp.has_focus();

            // ✅ Handle input behavior (DON’T REMOVE THIS)
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));

            let mut exit_edit_mode = false;

            if enter {
                let input = tab.breadcrumb_path_buffer.trim().trim_matches('"');
                let new_path = PathBuf::from(input);

                if new_path.exists() {
                    action.nav_to = Some(new_path);
                    exit_edit_mode = true;
                } else {
                    println!("Invalid path: {}", tab.breadcrumb_path_buffer);
                    tab.breadcrumb_path_error = true;
                    tab.breadcrumb_path_error_animation_time = ui.input(|i| i.time);
                }
            } else if escape || resp.lost_focus() {
                tab.breadcrumb_path_buffer = tab.nav.current.to_string_lossy().to_string();
                exit_edit_mode = true;
            }

            if exit_edit_mode {
                tab.breadcrumb_path_editing = false;
                tab.breadcrumb_just_started_editing = false;

                // 🔥 Reset error state
                tab.breadcrumb_path_error = false;
                tab.breadcrumb_path_error_animation_time = 0.0;

                ui.memory_mut(|mem| mem.surrender_focus(text_edit_id));
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
                action.nav_to = Some(PathBuf::from(MY_PC_PATH));
            }
        } else {
            let segments =
                build_breadcrumbs(&tab.nav.current, ui.available_width(), palette.text_size);
            let mut first = true;

            let mut breadcrumbs_right = 0.0;
            let font_id = egui::FontId::new(palette.text_size, egui::FontFamily::Proportional);
            let total_width = ui.available_width();

            for (label, path) in segments.iter() {
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
                        let color = ui.visuals().text_color();

                        let max_width = (total_width / segments.len() as f32).clamp(60.0, 180.0);

                        let (label_text, truncated) =
                            truncate_item_text(ui, label, max_width, &font_id, color);

                        let resp = ui.add(
                            egui::Label::new(
                                egui::RichText::new(label_text).size(palette.text_size),
                            )
                            .selectable(false)
                            .sense(egui::Sense::click()),
                        );

                        // ✅ RETURN THIS
                        if truncated {
                            resp.on_hover_text(label)
                        } else {
                            resp
                        }
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
                tab.breadcrumb_path_editing = true;
                tab.breadcrumb_path_buffer = tab.nav.current.to_string_lossy().to_string();
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
    let all_segments = if portable::is_portable_path(&path.to_path_buf()) {
        portable::build_breadcrumb_segments(&path.to_path_buf()).unwrap_or_default()
    } else {
        let mut segments = Vec::new();
        let mut current = PathBuf::new();
        for comp in path.components() {
            match comp {
                Component::Prefix(prefix) => {
                    current.push(prefix.as_os_str());
                    segments.push((
                        prefix.as_os_str().to_string_lossy().to_string(),
                        current.clone(),
                    ));
                }
                Component::RootDir => {
                    current.push(Path::new("\\"));
                }
                Component::Normal(name) => {
                    current.push(name);
                    segments.push((name.to_string_lossy().to_string(), current.clone()));
                }
                _ => {}
            }
        }
        segments
    };

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
