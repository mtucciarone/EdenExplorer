use eframe::egui;
use std::path::{Path, PathBuf};

use crate::app::icons::IconCache;
use crate::state::Navigation;
use egui_phosphor::regular;

#[derive(Clone)]
pub struct TabInfo {
    pub id: u64,
    pub title: String,
}

#[derive(Default)]
pub struct TabsAction {
    pub activate: Option<u64>,
    pub close: Option<u64>,
    pub open_new: bool,
}

pub fn draw_tabs(ui: &mut egui::Ui, tabs: &[TabInfo], active_id: u64) -> TabsAction {
    let mut action = TabsAction::default();

    ui.horizontal(|ui| {
        for tab in tabs {
            let is_active = tab.id == active_id;
            let corner = if is_active {
                egui::CornerRadius {
                    nw: 8,
                    ne: 8,
                    sw: 0,
                    se: 0,
                }
            } else {
                egui::CornerRadius {
                    nw: 6,
                    ne: 6,
                    sw: 0,
                    se: 0,
                }
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
                .stroke(if is_active {
                    egui::Stroke::new(1.0, ui.visuals().widgets.active.bg_stroke.color)
                } else {
                    egui::Stroke::NONE
                });

            let resp = tab_frame
                .show(ui, |ui| {
                    ui.set_min_width(160.0);
                    ui.set_max_width(200.0);
                    ui.horizontal(|ui| {
                        ui.add(egui::Label::new(regular::FOLDER_SIMPLE).selectable(false));

                        if ui
                            .add(
                                egui::Label::new(&tab.title)
                                    .selectable(false)
                                    .sense(egui::Sense::click()),
                            )
                            .clicked()
                        {
                            action.activate = Some(tab.id);
                        }
                    });
                })
                .response;

            let close_resp = tab_close_button(ui, resp.rect, tab.id);
            if close_resp.clicked() {
                action.close = Some(tab.id);
            }

            let _ = resp;
        }

        let add_frame = egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(6, 6))
            .corner_radius(egui::CornerRadius {
                nw: 6,
                ne: 6,
                sw: 0,
                se: 0,
            })
            .stroke(egui::Stroke::new(
                1.0,
                ui.visuals().widgets.noninteractive.bg_stroke.color,
            ));

        let add_resp = add_frame.show(ui, |ui| {
            ui.set_min_width(25.0);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
            (rect, resp)
        });
        let add_resp = tab_add_button(ui, add_resp.inner.0, add_resp.inner.1);
        if add_resp.clicked() {
            action.open_new = true;
        }
    });

    action
}

fn tab_close_button(ui: &mut egui::Ui, tab_rect: egui::Rect, tab_id: u64) -> egui::Response {
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
        egui::Color32::from_rgb(200, 52, 52)
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 4.0, bg);

    let color = if hovered {
        egui::Color32::WHITE
    } else {
        ui.visuals().widgets.noninteractive.fg_stroke.color
    };

    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        regular::X,
        egui::FontId::proportional(12.0),
        color,
    );

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    resp
}

fn tab_add_button(ui: &mut egui::Ui, rect: egui::Rect, resp: egui::Response) -> egui::Response {
    let hovered = resp.hovered();
    let bg = if hovered {
        egui::Color32::from_rgb(54, 168, 82)
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 4.0, bg);

    let color = if hovered {
        egui::Color32::WHITE
    } else {
        ui.visuals().widgets.noninteractive.fg_stroke.color
    };

    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        regular::PLUS,
        egui::FontId::proportional(12.0),
        color,
    );

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    resp
}

#[derive(Default)]
pub struct TabbarAction {
    pub nav: Option<TabbarNavAction>,
    pub create_folder: bool,
    pub add_favorite: bool,
    pub search_changed: bool,
    pub nav_to: Option<PathBuf>,
}

pub enum TabbarNavAction {
    Back,
    Forward,
    Up,
}

pub fn draw_tabbar(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    nav: &Navigation,
    search_query: &mut String,
) -> TabbarAction {
    let mut action = TabbarAction::default();

    ui.horizontal(|ui| {
        if ui.button(regular::ARROW_LEFT).clicked() {
            action.nav = Some(TabbarNavAction::Back);
        }
        if ui.button(regular::ARROW_RIGHT).clicked() {
            action.nav = Some(TabbarNavAction::Forward);
        }
        if ui.button(regular::ARROW_UP).clicked() {
            action.nav = Some(TabbarNavAction::Up);
        }

        if ui.button(regular::FOLDER_PLUS).clicked() {
            action.create_folder = true;
        }

        if ui.button(regular::STAR).clicked() {
            action.add_favorite = true;
        }

        ui.separator();

        if nav.is_root() {
            let pc_icon_path = PathBuf::from("C:\\");
            if let Some(icon) = icon_cache.get(&pc_icon_path, true) {
                ui.add(egui::Image::new(&icon).fit_to_exact_size(egui::vec2(14.0, 14.0)));
            }
            if ui
                .add(
                    egui::Label::new(egui::RichText::new("This PC").size(13.0))
                        .selectable(false)
                        .sense(egui::Sense::click()),
                )
                .clicked()
            {
                action.nav_to = Some(PathBuf::from("::MY_PC::"));
            }
        } else {
            let segments = build_breadcrumbs(&nav.current);
            let mut first = true;
            for (idx, (label, path)) in segments.into_iter().enumerate() {
                if !first {
                    let old_spacing = ui.spacing().item_spacing;
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(
                        egui::RichText::new(">")
                            .size(13.0)
                            .color(ui.visuals().widgets.noninteractive.fg_stroke.color),
                    );
                    ui.spacing_mut().item_spacing = old_spacing;
                }
                first = false;
                let base = ui.visuals().widgets.hovered.bg_fill;
                let tint = if idx % 2 == 0 { 0.12 } else { 0.22 };
                let crumb_bg = egui::Color32::from_rgba_premultiplied(
                    base.r(),
                    base.g(),
                    base.b(),
                    (255.0 * tint) as u8,
                );

                let inner = egui::Frame::NONE
                    .fill(crumb_bg)
                    .inner_margin(egui::Margin::symmetric(6, 2))
                    .corner_radius(egui::CornerRadius::same(4))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(label).size(13.0))
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

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let search_resp = ui.add(
                egui::TextEdit::singleline(search_query)
                    .hint_text("Search")
                    .desired_width(220.0),
            );
            if search_resp.changed() {
                action.search_changed = true;
            }
        });
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
