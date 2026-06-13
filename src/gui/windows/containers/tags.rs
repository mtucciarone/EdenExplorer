use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::utils::{clickable_icon, draw_object_drag_ghost, rgba_color_edit_button};
use crate::gui::windows::containers::sidebar::draw_sidebar_item;
use crate::gui::windows::containers::structs::TagsState;
use crate::gui::windows::windowsoverrides::handle_draw_windows_buttons;
use eframe::egui;
use egui::FontId;
use egui::ScrollArea;
use egui::containers::{Popup, PopupCloseBehavior};
use egui_phosphor::regular;
use std::path::PathBuf;
use windows::Win32::Foundation::HWND;

pub fn draw_tags(
    ui: &mut egui::Ui,
    i18n: &I18n,
    icon_cache: &IconCache,
    palette: &ThemePalette,
    hwnd: Option<HWND>,
    tags_state: &mut TagsState,
) -> bool {
    let mut changed = false;
    let mut drag_state = tags_state.drag_state.take();
    let mut rename_state = tags_state.rename_state.take();
    let mut delete_confirmation = tags_state.delete_confirmation.take();
    let pointer_pos = ui.ctx().input(|input| input.pointer.hover_pos());
    let pointer_released = ui.ctx().input(|input| input.pointer.primary_released());
    let groups_len = tags_state.groups.len();

    let tabs_width = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(tabs_width, ui.available_height()),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            let old_spacing = ui.spacing().item_spacing;
            ui.spacing_mut().item_spacing.y = 0.0;

            egui::Frame::NONE.show(ui, |ui| {
                ui.add_space(8.0);
                draw_container_header(
                    ui,
                    i18n,
                    palette,
                    hwnd
                );
            });

                if tags_state.groups.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(i18n.tr("tag_empty_state"));
                    });
                    return;
                }

                ui.vertical(|ui| {
                    ui.add_space(-4.0); // align the first group vertically with the side bar border
                    ui.spacing_mut().item_spacing.y = 8.0;

                    for group_index in 0..groups_len {
                        let group = &mut tags_state.groups[group_index];
                        let group_id = group.id;
                        let group_name = group.name.clone();
                        let group_items = group.items.clone();
                        let group_color = group.color;
                        let editing_this_group = rename_state
                            .as_ref()
                            .map(|state| state.group_id == group_id)
                            .unwrap_or(false);
                        let drag_source_index = drag_state
                            .as_ref()
                            .filter(|drag| drag.group_id == group_id && drag.active)
                            .map(|drag| drag.source_index);
                        let mut drop_index: Option<usize> = None;
                        let mut should_clear_drag = false;
                        let mut clear_rename_state = false;

                        let group_frame = egui::Frame::NONE
                            .stroke(egui::Stroke::NONE)
                            .fill(egui::Color32::TRANSPARENT)
                            .inner_margin(egui::Margin::symmetric(10, 10));

                        group_frame.show(ui, |ui| {
                            ui.set_width(ui.available_width());

                            let header_id = ui.make_persistent_id(("tag_group", group_id));
                            let header_state =
                                egui::collapsing_header::CollapsingState::load_with_default_open(
                                    ui.ctx(),
                                    header_id,
                                    true,
                                );

                            let header_response = header_state.show_header(ui, |ui| {
                                if editing_this_group {
                                    if let Some(rename) = rename_state
                                        .as_mut()
                                        .filter(|state| state.group_id == group_id)
                                    {
                                        let edit_id = ui.id().with("tag_rename_input").with(group_id);
                                        let edit_response = ui.add(
                                            egui::TextEdit::singleline(&mut rename.buffer)
                                                .id(edit_id)
                                                .desired_width(150.0)
                                                .font(egui::FontId::new(
                                                    palette.text_size,
                                                    egui::FontFamily::Proportional,
                                                )),
                                        );

                                        if rename.should_focus {
                                            ui.memory_mut(|mem| mem.request_focus(edit_id));
                                            edit_response.request_focus();
                                            if edit_response.has_focus() {
                                                rename.should_focus = false;
                                            }
                                        }

                                        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                                        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));

                                        if enter || edit_response.lost_focus() {
                                            let new_name = rename.buffer.trim().to_string();
                                            if !new_name.is_empty() && group.name != new_name {
                                                group.name = new_name;
                                                changed = true;
                                            }
                                            clear_rename_state = true;
                                        } else if escape {
                                            clear_rename_state = true;
                                        }
                                    }
                                } else {
                                    ui.label(
                                        egui::RichText::new(&group_name)
                                            .size(palette.text_size)
                                            .color(ui.visuals().text_color())
                                            .strong(),
                                    );
                                    if clickable_icon(ui, regular::PENCIL_SIMPLE, palette.primary)
                                        .on_hover_text(i18n.tr("inputs_rename"))
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        rename_state = Some(
                                            crate::gui::windows::containers::structs::TagRenameState {
                                                group_id,
                                                buffer: group_name.clone(),
                                                should_focus: true,
                                            },
                                        );
                                    }
                                    if clickable_icon(ui, regular::TRASH, palette.primary)
                                        .on_hover_text(i18n.tr("tag_delete_group"))
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        delete_confirmation = Some(group_id);
                                    }
                                }

                                ui.label(
                                    egui::RichText::new(format!("({})", group_items.len()))
                                        .size(palette.text_size)
                                        .color(palette.tooltip_text_color),
                                );

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if rgba_color_edit_button(ui, &mut group.color).changed() {
                                            changed = true;
                                        }
                                    },
                                );
                            });

                            let _ = header_response.body(|ui| {
                                let collapsible_body_frame = egui::Frame::NONE
                                    .stroke(egui::Stroke::new(1.0, group_color.gamma_multiply(0.8)))
                                    .fill(palette.row_bg.linear_multiply(0.18))
                                    .inner_margin(egui::Margin::symmetric(10, 10));

                                collapsible_body_frame.show(ui, |ui| {
                                    if group_items.is_empty() {
                                        // ui.add_space(4.0);
                                        ui.label(
                                            egui::RichText::new(i18n.tr("tag_empty_group"))
                                                .size(palette.text_size)
                                                .color(palette.text_normal),
                                        );
                                        return;
                                    }

                                    let drag_is_active = drag_source_index.is_some();
                                    let mut item_rects = Vec::with_capacity(group_items.len());
                                    let mut item_responses = Vec::with_capacity(group_items.len());

                                    if drag_is_active {
                                        for _ in &group_items {
                                            let (rect, resp) = tag_item_layout(ui);
                                            item_rects.push(rect);
                                            item_responses.push(resp);
                                        }

                                        if let Some(drag_source_index) = drag_source_index {
                                            if let Some(pointer) = pointer_pos {
                                                drop_index = compute_drop_index(
                                                    &item_rects,
                                                    pointer.y,
                                                    drag_source_index,
                                                );
                                            }
                                        }
                                    }

                                    for (item_index, path) in group_items.iter().enumerate() {
                                        let label = tag_item_label(path);
                                        let is_dir = path.is_dir();
                                        let resp = if drag_is_active {
                                            let rect = item_rects[item_index];
                                            let item_resp = item_responses[item_index].clone();
                                            draw_sidebar_item(
                                                ui,
                                                icon_cache,
                                                path,
                                                &label,
                                                is_dir,
                                                palette,
                                                true,
                                                Some((rect, item_resp)),
                                            )
                                        } else {
                                            draw_sidebar_item(
                                                ui,
                                                icon_cache,
                                                path,
                                                &label,
                                                is_dir,
                                                palette,
                                                true,
                                                None,
                                            )
                                        };

                                        if drag_state.is_none() && resp.drag_started() {
                                            drag_state = Some(crate::gui::windows::containers::structs::TagDragState {
                                                group_id,
                                                source_index: item_index,
                                                active: true,
                                            });
                                        }

                                        Popup::context_menu(&resp)
                                            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                                            .show(|ui| {
                                                apply_context_menu_style(ui, palette);
                                                if ui.button(i18n.tr("tag_remove")).clicked() {
                                                    if item_index < group.items.len() {
                                                        group.items.remove(item_index);
                                                        changed = true;
                                                    }
                                                    ui.close();
                                                }
                                            });
                                    }

                                    if let Some(drag_source_index) = drag_source_index {
                                        if let Some(drop) = drop_index {
                                            if drop < item_rects.len() {
                                                let rect = item_rects[drop];
                                                draw_insert_line(
                                                    ui,
                                                    palette,
                                                    rect.top(),
                                                    rect.left(),
                                                    rect.right(),
                                                );
                                            } else if let Some(last) = item_rects.last().copied() {
                                                draw_insert_line(
                                                    ui,
                                                    palette,
                                                    last.bottom(),
                                                    last.left(),
                                                    last.right(),
                                                );
                                            }
                                        }

                                        if pointer_released {
                                            if let Some(drop) = drop_index {
                                                if drag_source_index < group.items.len()
                                                    && drag_source_index != drop
                                                {
                                                    let item = group.items.remove(drag_source_index);
                                                    let mut target = drop;

                                                    if drop > drag_source_index {
                                                        target -= 1;
                                                    }

                                                    target = target.min(group.items.len());
                                                    group.items.insert(target, item);
                                                    changed = true;
                                                }
                                            }

                                            should_clear_drag = true;
                                        }

                                        if let Some(label_path) = group_items.get(drag_source_index) {
                                            draw_object_drag_ghost(
                                                ui,
                                                palette,
                                                &tag_item_label(label_path),
                                                true,
                                            );
                                        }
                                    }
                                });
                            });

                            if clear_rename_state
                                && rename_state
                                    .as_ref()
                                    .map(|state| state.group_id == group_id)
                                    .unwrap_or(false)
                            {
                                rename_state = None;
                            }
                        });

                        if should_clear_drag {
                            drag_state = None;
                        }

                        if group_index + 1 < groups_len {
                            ui.add_space(-12.0); // remove vertical margins between tag groups
                        }
                    }
                });
            });

    if pointer_released && drag_state.is_some() {
        drag_state = None;
    }

    tags_state.drag_state = drag_state;
    tags_state.rename_state = rename_state;
    tags_state.delete_confirmation = delete_confirmation;
    changed
}

pub fn draw_delete_confirmation_popup(
    ctx: &egui::Context,
    i18n: &I18n,
    palette: &ThemePalette,
    tags_state: &mut TagsState,
) -> bool {
    let Some(group_id) = tags_state.delete_confirmation else {
        return false;
    };

    let mut changed = false;
    let mut close_requested = false;
    let mut confirmed = false;

    let group_name = tags_state
        .groups
        .iter()
        .find(|g| g.id == group_id)
        .map(|g| g.name.clone())
        .unwrap_or_else(|| "Unknown".to_string());

    egui::Area::new(egui::Id::new("tag_delete_modal_bg"))
        .order(egui::Order::Middle)
        .interactable(true)
        .show(ctx, |ui| {
            let rect = ctx.content_rect();
            ui.painter()
                .rect_filled(rect, 0.0, palette.modal_background_effect_color);
        });

    egui::Window::new(i18n.tr("tag_delete_group"))
        .collapsible(false)
        .resizable(false)
        .default_width(360.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(egui::Frame::popup(&ctx.style()).corner_radius(egui::CornerRadius::same(8)))
        .show(ctx, |ui| {
            let mut style = (*ui.ctx().style()).clone();
            style.text_styles = [
                (egui::TextStyle::Heading, egui::FontId::proportional(14.0)),
                (
                    egui::TextStyle::Body,
                    egui::FontId::proportional(palette.text_size),
                ),
                (
                    egui::TextStyle::Button,
                    egui::FontId::proportional(palette.text_size),
                ),
                (
                    egui::TextStyle::Small,
                    egui::FontId::proportional(palette.text_size),
                ),
            ]
            .into();
            ui.set_style(style);

            ui.label(egui::RichText::new(i18n.tr("tag_delete_confirm")).strong());
            ui.add_space(8.0);

            ui.label(
                egui::RichText::new(format!("{}: \"{}\"", i18n.tr("tag_group_name"), group_name))
                    .color(ui.visuals().text_color().linear_multiply(0.8)),
            );

            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if ui.button(i18n.tr("ok")).clicked() {
                    confirmed = true;
                    close_requested = true;
                }

                if ui.button(i18n.tr("close")).clicked() {
                    close_requested = true;
                }
            });
        });

    if close_requested {
        if confirmed {
            tags_state.groups.retain(|g| g.id != group_id);
            changed = true;
        }
        tags_state.delete_confirmation = None;
    } else {
        tags_state.delete_confirmation = Some(group_id);
    }

    changed
}

fn tag_item_layout(ui: &mut egui::Ui) -> (egui::Rect, egui::Response) {
    ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 18.0),
        egui::Sense::click_and_drag(),
    )
}

fn apply_context_menu_style(ui: &mut egui::Ui, palette: &ThemePalette) {
    let mut style = (*ui.ctx().style()).clone();
    style.text_styles = [
        (
            egui::TextStyle::Body,
            FontId::proportional(palette.context_menu_text_size),
        ),
        (
            egui::TextStyle::Button,
            FontId::proportional(palette.context_menu_text_size),
        ),
        (
            egui::TextStyle::Small,
            FontId::proportional(palette.context_menu_text_size),
        ),
        (
            egui::TextStyle::Heading,
            FontId::proportional(palette.context_menu_text_size + 2.0),
        ),
    ]
    .into();
    style.spacing.button_padding = egui::vec2(4.0, 2.0);
    style.spacing.item_spacing = egui::vec2(6.0, 2.0);
    style.spacing.menu_margin = egui::Margin::same(4);
    style.spacing.interact_size = egui::vec2(
        style.spacing.interact_size.x,
        palette.context_menu_text_size + 6.0,
    );
    style.visuals.widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
    style.visuals.widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
    style.visuals.widgets.hovered.bg_fill = palette.primary;
    style.visuals.widgets.hovered.weak_bg_fill = palette.primary;
    style.visuals.widgets.active.bg_fill = palette.primary;
    style.visuals.widgets.active.weak_bg_fill = palette.primary;
    ui.set_style(style);
}

pub fn draw_tag_picker_popup(
    ctx: &egui::Context,
    i18n: &I18n,
    palette: &ThemePalette,
    tags_state: &mut TagsState,
) -> bool {
    let Some(mut picker) = tags_state.picker.take() else {
        return false;
    };

    let mut changed = false;
    let mut close_requested = false;

    egui::Area::new(egui::Id::new("tag_modal_bg"))
        .order(egui::Order::Middle)
        .interactable(true)
        .show(ctx, |ui| {
            let rect = ctx.content_rect();
            ui.painter()
                .rect_filled(rect, 0.0, palette.modal_background_effect_color);
        });

    egui::Window::new(i18n.tr("tag_add"))
        .collapsible(false)
        .resizable(false)
        .default_width(360.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(egui::Frame::popup(&ctx.style()).corner_radius(egui::CornerRadius::same(8)))
        .show(ctx, |ui| {
            let mut style = (*ui.ctx().style()).clone();
            style.text_styles = [
                (egui::TextStyle::Heading, egui::FontId::proportional(14.0)),
                (
                    egui::TextStyle::Body,
                    egui::FontId::proportional(palette.text_size),
                ),
                (
                    egui::TextStyle::Button,
                    egui::FontId::proportional(palette.text_size),
                ),
                (
                    egui::TextStyle::Small,
                    egui::FontId::proportional(palette.text_size),
                ),
            ]
            .into();
            ui.set_style(style);

            ui.label(egui::RichText::new(i18n.tr("tag_add_to_existing_group")).strong());
            ui.add_space(6.0);

            let group_choices: Vec<(u64, String, egui::Color32, usize)> = tags_state
                .groups
                .iter()
                .map(|group| (group.id, group.name.clone(), group.color, group.items.len()))
                .collect();

            if group_choices.is_empty() {
                ui.label(
                    egui::RichText::new(i18n.tr("tag_no_groups"))
                        .color(ui.visuals().text_color().linear_multiply(0.8)),
                );
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);

                    for (group_id, group_name, group_color, item_count) in &group_choices {
                        let button_label = format!("{} ({})", group_name, item_count);
                        let button = egui::Button::new(button_label)
                            .fill(group_color.gamma_multiply(0.25))
                            .stroke(egui::Stroke::new(1.0, group_color.gamma_multiply(0.6)));

                        if ui.add(button).clicked() {
                            if tags_state.add_paths_to_group(*group_id, &picker.paths) {
                                changed = true;
                            }
                            close_requested = true;
                        }
                    }
                });
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            ui.label(egui::RichText::new(i18n.tr("tag_create_new_group")).strong());
            ui.add_space(6.0);

            ui.label(i18n.tr("tag_group_name"));
            let name_id = ui.id().with("tag_group_name_input");
            let name_response = ui.add(
                egui::TextEdit::singleline(&mut picker.new_group_name)
                    .id(name_id)
                    .desired_width(240.0)
                    .font(egui::FontId::new(
                        palette.text_size,
                        egui::FontFamily::Proportional,
                    )),
            );

            if picker.focus_requested {
                ui.memory_mut(|mem| mem.request_focus(name_id));
                name_response.request_focus();
                if name_response.has_focus() {
                    picker.focus_requested = false;
                }
            }

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(i18n.tr("tag_group_color"));
                if rgba_color_edit_button(ui, &mut picker.new_group_color).changed() {
                    changed = true;
                }
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !picker.new_group_name.trim().is_empty(),
                        egui::Button::new(i18n.tr("tag_create_group")),
                    )
                    .clicked()
                {
                    if tags_state.create_group_and_add(
                        picker.new_group_name.clone(),
                        picker.new_group_color,
                        &picker.paths,
                    ) {
                        changed = true;
                    }
                    close_requested = true;
                }

                if ui.button(i18n.tr("close")).clicked() {
                    close_requested = true;
                }
            });
        });

    if !close_requested {
        tags_state.picker = Some(picker);
    } else {
        changed = true;
    }

    changed
}

fn tag_item_label(path: &PathBuf) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.display().to_string())
}

fn compute_drop_index(
    item_rects: &[egui::Rect],
    pointer_y: f32,
    source_index: usize,
) -> Option<usize> {
    if item_rects.is_empty() {
        return Some(0);
    }

    let mut drop_index: Option<usize> = None;

    for (index, rect) in item_rects.iter().enumerate() {
        let midpoint = rect.center().y;
        let new_index = if pointer_y < midpoint {
            index
        } else {
            index + 1
        };

        if new_index != source_index && new_index != source_index + 1 {
            drop_index = Some(new_index);
        }

        if pointer_y < rect.bottom() {
            break;
        }
    }

    if let Some(last) = item_rects.last() {
        if pointer_y > last.bottom() {
            drop_index = Some(item_rects.len());
        }
    }

    drop_index
}

fn draw_insert_line(ui: &mut egui::Ui, palette: &ThemePalette, y: f32, left: f32, right: f32) {
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("tag_insert_line"),
    ));

    painter.line_segment(
        [egui::pos2(left + 6.0, y), egui::pos2(right - 6.0, y)],
        egui::Stroke::new(2.0, palette.primary_active),
    );
}

pub fn draw_container_header(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    hwnd: Option<HWND>,
) {
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
                    // egui::ScrollArea::horizontal()
                    // .id_salt("tabs_scroll")
                    // .auto_shrink([false, true])
                    // .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    // .show(ui, |ui| {
                    //     ui.horizontal(|ui| {
                    //         ui.add_space(4.0);
                    //         let label_text = i18n.tr("tags");
                    //         let label_color = palette.tab_text_selected;
                    //         let label_font_id =
                    //             egui::FontId::proportional(palette.text_size + 2.0);
                    //         let label_galley = ui.fonts_mut(|fonts| {
                    //             fonts.layout_no_wrap(
                    //                 label_text.clone(),
                    //                 label_font_id.clone(),
                    //                 label_color,
                    //             )
                    //         });
                    //         let label_padding = egui::vec2(20.0, 4.0);
                    //         let (rect, _) = ui.allocate_exact_size(
                    //             label_galley.size() + label_padding * 2.0,
                    //             egui::Sense::hover(),
                    //         );
                    //         let painter = ui.painter();
                    //         painter.rect_filled(
                    //             rect,
                    //             egui::CornerRadius::same(palette.medium_radius),
                    //             palette.primary,
                    //         );
                    //         painter.text(
                    //             rect.left_center() + egui::vec2(label_padding.x, 0.0),
                    //             egui::Align2::LEFT_CENTER,
                    //             label_text,
                    //             label_font_id,
                    //             label_color,
                    //         );
                    //     });
                    // });

                    // Drag region: empty space to the right of the last tab
                    let content_width = 0.0;

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
}
