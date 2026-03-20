use eframe::egui;
use std::path::{Path, PathBuf};

use crate::app::features::ThemePalette;
use crate::app::icons::IconCache;
use crate::app::utils::clickable_icon;
use crate::state::Navigation;
use egui::{FontFamily, FontId};
use egui_phosphor::regular;

#[derive(Clone)]
pub struct TabInfo {
    pub id: u64,
    pub title: String,
    pub full_path: PathBuf,
}

#[derive(Default)]
pub struct TabsAction {
    pub activate: Option<u64>,
    pub close: Option<u64>,
    pub open_new: bool,
}

pub struct TabState {
    pub id: u64,
    pub nav: Navigation,
    pub is_editing_path: bool,
    pub path_buffer: String,
}

pub fn draw_tabs(
    ui: &mut egui::Ui,
    tabs: &[TabInfo],
    active_id: u64,
    palette: &ThemePalette,
) -> TabsAction {
    let mut action = TabsAction::default();

    ui.horizontal(|ui| {
        for tab in tabs {
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
                .inner_margin(egui::Margin::symmetric(10, 6))
                .corner_radius(corner)
                .stroke(egui::Stroke::NONE);

            let resp = tab_frame
                .show(ui, |ui| {
                    ui.set_min_width(160.0);
                    ui.set_max_width(200.0);

                    ui.horizontal(|ui| {
                        // Add folder icon with vertical centering
                        ui.add(egui::Label::new(regular::FOLDER_SIMPLE).selectable(false));

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

            let stroke_width = 1.0;

            // 👇 Allow space for outside stroke, but still clip bottom
            let clip_rect = egui::Rect::from_min_max(
                egui::pos2(rect.min.x - stroke_width, rect.min.y - stroke_width),
                egui::pos2(rect.max.x + stroke_width, rect.max.y + 0.5), // only trim bottom
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

    action
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

#[derive(Default)]
pub struct TabbarAction {
    pub nav: Option<TabbarNavAction>,
    pub create_folder: bool,
    pub create_file: bool,
    pub add_favorite: bool,
    pub nav_to: Option<PathBuf>,
    pub refresh_current_directory: bool,
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

pub enum TabbarNavAction {
    Back,
    Forward,
    Up,
}

pub fn draw_tabbar(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    tab: &mut TabState,
    search_query: &mut String,
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
            let segments = build_breadcrumbs(&tab.nav.current);
            let mut first = true;

            for (_idx, (label, path)) in segments.into_iter().enumerate() {
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
                        ui.add(
                            egui::Label::new(egui::RichText::new(label).size(palette.text_size))
                                .selectable(false)
                                .sense(egui::Sense::click()),
                        )
                    });

                let resp = inner.response.union(inner.inner);

                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }

                if resp.clicked() {
                    action.nav_to = Some(path);
                }
            }
        }

        if !tab.nav.is_root() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if tab.is_editing_path {
                    if clickable_icon(ui, regular::X, palette.primary).clicked() {
                        tab.is_editing_path = false;
                    }
                } else {
                    if clickable_icon(ui, regular::PENCIL_SIMPLE, palette.primary).clicked() {
                        tab.is_editing_path = true;
                        tab.path_buffer = tab.nav.current.to_string_lossy().to_string();
                    }
                }
            });
        }
    });

    action
}

fn build_breadcrumbs(path: &Path) -> Vec<(String, PathBuf)> {
    use std::path::Component;
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
}
