use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use crate::gui::utils::{clickable_icon, truncate_item_text};
use crate::gui::windows::containers::structs::{TabInfo, TabsAction};
use crate::gui::windows::windowsoverrides::{
    handle_draw_windows_buttons, toggle_window_fullscreen,
};
use eframe::egui;
use egui::{FontFamily, FontId};
use egui_phosphor::{fill, regular};
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::HWND;

pub fn draw_tabs(
    ui: &mut egui::Ui,
    i18n: &I18n,
    tabs: &[TabInfo],
    active_id: u64,
    palette: &ThemePalette,
    hwnd: Option<HWND>,
    scroll_to_id: Option<u64>,
    drag_active: bool,
    drag_hover_target: Option<PathBuf>,
    has_split: bool,
) -> TabsAction {
    let mut action: TabsAction = TabsAction::default();
    let pointer_pos = ui.input(|i| i.pointer.interact_pos().or_else(|| i.pointer.hover_pos()));
    let pointer_released =
        ui.input(|i| i.pointer.any_released() && i.pointer.interact_pos().is_some());
    let hovered_target_ref = drag_hover_target.as_ref();
    let mut tab_drop_target: Option<PathBuf> = None;
    let controls_width = 90.0;
    let full_width = ui.available_width();
    let tabs_width = (full_width - controls_width).max(0.0);

    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 32.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(tabs_width, 32.0),
                egui::Layout::left_to_right(egui::Align::Min),
                |ui| {
                    let tabs_rect = ui.available_rect_before_wrap();
                    egui::ScrollArea::horizontal()
                        .id_salt("tabs_scroll")
                        .auto_shrink([false, true])
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.add_space(-2.0);
                            ui.horizontal(|ui| {
                                for tab in tabs {
                                    let tab_width = 140.0;
                                    let (rect, resp) = ui.allocate_exact_size(
                                        egui::vec2(tab_width, 28.0),
                                        egui::Sense::click(),
                                    );
                                    if Some(tab.id) == scroll_to_id {
                                        resp.scroll_to_me(Some(egui::Align::Center));
                                    }

                                    if drag_active && tab.id != active_id {
                                        let hovered = hovered_target_ref
                                            .map(|target| target == &tab.full_path)
                                            .unwrap_or_else(|| {
                                                pointer_pos
                                                    .map(|pointer| rect.contains(pointer))
                                                    .unwrap_or(false)
                                            });
                                        if hovered {
                                            let painter = ui
                                                .ctx()
                                                .layer_painter(egui::LayerId::new(
                                                    egui::Order::Background,
                                                    ui.id().with("tab_drop_bg").with(tab.id),
                                                ))
                                                .with_clip_rect(ui.clip_rect());
                                            painter.rect_filled(
                                                rect,
                                                egui::CornerRadius::same(palette.medium_radius),
                                                palette.primary_hover,
                                            );

                                            if pointer_released {
                                                tab_drop_target = Some(tab.full_path.clone());
                                                action.move_files_to_tab_dir_rect = Some(rect);
                                            }
                                        }
                                    }

                                    handle_draw_tab_new_allocated(
                                        ui,
                                        i18n,
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
                            if resp.double_clicked() {
                                if let Some(hwnd) = hwnd {
                                    toggle_window_fullscreen(hwnd);
                                }
                            }
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
                    let (icon, tooltip_key) = if has_split {
                        (regular::ARROWS_MERGE, "tooltip_close_split")
                    } else {
                        (regular::COLUMNS_PLUS_RIGHT, "tooltip_open_split")
                    };
                    if clickable_icon(ui, icon, palette.primary)
                        .on_hover_text(
                            egui::RichText::new(i18n.tr(tooltip_key))
                                .size(palette.tooltip_text_size)
                                .color(palette.tooltip_text_color),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        action.toggle_split = true;
                    }
                },
            );
        },
    );
    action.move_files_to_tab_dir = tab_drop_target;
    action
}

fn handle_draw_tab_new_allocated(
    ui: &mut egui::Ui,
    i18n: &I18n,
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
    let icon_font_id = FontId::new(palette.tab_icon_size, FontFamily::Proportional);
    let label_color = if is_active {
        palette.tab_text_selected
    } else {
        palette.text_normal
    };
    let icon_color = if tab.is_pinned {
        palette.pinned_tab_color
    } else {
        label_color
    };

    // --- Layout parameters ---
    let icon_size = palette.tab_icon_size;
    let spacing = 6.0;
    let padding = 8.0;
    let close_button_width = 20.0;
    let icon_top_left = egui::pos2(rect.left() + padding, rect.center().y - icon_size * 0.5);
    let icon_rect = egui::Rect::from_min_size(icon_top_left, egui::vec2(icon_size, icon_size));

    let icon_resp = ui.interact(
        icon_rect,
        ui.id().with(("tab_pin", tab.id)),
        egui::Sense::click(),
    );
    let icon_resp = icon_resp.on_hover_text(
        egui::RichText::new(if tab.is_pinned {
            i18n.tr("tooltip_tab_unpin")
        } else {
            i18n.tr("tooltip_tab_pin")
        })
        .size(palette.tooltip_text_size)
        .color(palette.tooltip_text_color),
    );
    let icon_glyph = if tab.is_pinned {
        if icon_resp.hovered() {
            regular::FOLDER_SIMPLE
        } else {
            fill::PUSH_PIN
        }
    } else if icon_resp.hovered() {
        regular::PUSH_PIN
    } else {
        regular::FOLDER_SIMPLE
    };

    let icon_glyph_width = ui
        .painter()
        .layout_no_wrap(icon_glyph.to_string(), icon_font_id.clone(), icon_color)
        .size()
        .x;
    let icon_draw_width = icon_size.max(icon_glyph_width);
    let text_pos = egui::pos2(
        rect.left() + padding + icon_draw_width + spacing,
        rect.center().y,
    );
    let text_width = rect.width() - icon_draw_width - spacing - 2.0 * padding - close_button_width;

    let (display_title, truncated) =
        truncate_item_text(ui, &tab.title, text_width, &font_id, label_color);

    // --- NOW safe to use painter ---
    let painter = ui.painter();

    // --- Paint background ---
    let rect = egui::Rect::from_min_max(rect.min.round(), rect.max.round());
    let bg_rect = rect.expand(5.0);

    let rounding = egui::CornerRadius {
        nw: corner.nw,
        ne: corner.ne,
        sw: 0,
        se: 0,
    };

    painter.rect_filled(rect, rounding, tab_fill);

    painter.text(
        icon_rect.left_center(),
        egui::Align2::LEFT_CENTER,
        icon_glyph,
        icon_font_id,
        icon_color,
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

    painter.rect_stroke(rect, rounding, stroke, egui::StrokeKind::Inside);

    if is_active {
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            egui::Stroke::new(2.0, ui.visuals().panel_fill),
        );
    }

    let close_resp = tab_close_button(ui, rect, tab.id, is_active, palette);
    if close_resp.clicked() {
        action.close = Some(tab.id);
    } else if icon_resp.clicked() {
        action.toggle_pin = Some(tab.full_path.clone());
    } else if resp.clicked() {
        action.activate = Some(tab.id);
    }

    if icon_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
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
        .inner_margin(egui::Margin::same(0))
        .corner_radius(palette.tab_inactive_radius)
        .stroke(egui::Stroke::new(1.0, palette.tab_border_default));

    let response = frame
        .show(ui, |ui| {
            let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
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
    is_active: bool,
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
    } else if is_active {
        palette.tab_close_active
    } else {
        palette.tab_close_normal
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
