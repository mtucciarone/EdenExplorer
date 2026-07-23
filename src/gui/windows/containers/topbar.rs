use crate::gui::i18n::I18n;
use crate::gui::utils::{clickable_active_icon, clickable_icon};
use crate::gui::windows::containers::structs::TopbarAction;
use crate::gui::windows::windowsoverrides::toggle_window_fullscreen;
use eframe::egui;
use egui::Pos2;
use egui_phosphor::regular;
use windows::Win32::Foundation::HWND;

pub fn draw_topbar(
    ui: &mut egui::Ui,
    i18n: &I18n,
    is_dark: bool,
    is_file_explorer: bool,
    sidebar_collapsed: bool,
    hwnd: Option<HWND>,
    palette: &crate::gui::theme::ThemePalette,
) -> TopbarAction {
    let mut action = TopbarAction::default();

    if sidebar_collapsed {
        // Collapsed: icons stack in a narrow vertical rail, centered.
        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
            draw_hamburger_menu(ui, i18n, palette, &mut action);
            draw_mode_icons(
                ui,
                i18n,
                is_dark,
                is_file_explorer,
                sidebar_collapsed,
                hwnd,
                palette,
                &mut action,
            );
            draw_drag_region(ui, hwnd);
        });
    } else {
        // Expanded: original horizontal topbar row.
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            draw_hamburger_menu(ui, i18n, palette, &mut action);

            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                draw_mode_icons(
                    ui,
                    i18n,
                    is_dark,
                    is_file_explorer,
                    sidebar_collapsed,
                    hwnd,
                    palette,
                    &mut action,
                );
            });

            draw_drag_region(ui, hwnd);
        });
    }

    action
}

fn draw_hamburger_menu(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &crate::gui::theme::ThemePalette,
    action: &mut TopbarAction,
) {
    let menu_id = egui::Id::new("topbar_hamburger_menu");

    let menu_open = ui.memory(|mem| mem.data.get_temp::<bool>(menu_id).unwrap_or(false));

    if clickable_icon(ui, regular::LIST, palette.primary)
        .on_hover_text(
            egui::RichText::new(&i18n.tr("tooltip_mainmenu"))
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        ui.memory_mut(|mem| mem.data.insert_temp(menu_id, !menu_open));
    }

    if menu_open {
        let icon_rect = ui.min_rect();
        let popup_pos = Pos2::new(icon_rect.min.x, icon_rect.max.y);

        let area_response = egui::Area::new(egui::Id::new("topbar_hamburger_menu_area"))
            .fixed_pos(popup_pos)
            .show(ui.ctx(), |ui| {
                egui::containers::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(120.0);

                    if menu_item(ui, regular::PALETTE, &i18n.tr("theme"), palette).clicked() {
                        action.customize_theme = true;
                        ui.memory_mut(|mem| mem.data.insert_temp(menu_id, false));
                    }

                    if menu_item(ui, regular::SLIDERS, &i18n.tr("settings"), palette).clicked() {
                        action.open_settings = true;
                        ui.memory_mut(|mem| mem.data.insert_temp(menu_id, false));
                    }

                    if menu_item(ui, regular::QUESTION, &i18n.tr("about"), palette).clicked() {
                        action.about = true;
                        ui.memory_mut(|mem| mem.data.insert_temp(menu_id, false));
                    }

                    if menu_item(ui, regular::X, &i18n.tr("exit"), palette).clicked() {
                        action.exit = true;
                        ui.memory_mut(|mem| mem.data.insert_temp(menu_id, false));
                    }
                });
            })
            .response;

        if area_response.clicked_elsewhere() {
            ui.memory_mut(|mem| mem.data.insert_temp(menu_id, false));
        }
    }
}

fn draw_mode_icons(
    ui: &mut egui::Ui,
    i18n: &I18n,
    is_dark: bool,
    is_file_explorer: bool,
    sidebar_collapsed: bool,
    _hwnd: Option<HWND>,
    palette: &crate::gui::theme::ThemePalette,
    action: &mut TopbarAction,
) {
    let icon = if is_dark { regular::SUN } else { regular::MOON };

    if clickable_icon(ui, icon, palette.primary)
        .on_hover_text(
            egui::RichText::new(&i18n.tr("tooltip_theme"))
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        action.toggle_theme = true;
    }

    let file_explorer_icon = if is_file_explorer {
        regular::FOLDER_OPEN
    } else {
        regular::FOLDER
    };

    if clickable_active_icon(
        ui,
        file_explorer_icon,
        ui.visuals().text_color(),
        is_file_explorer,
        palette.primary,
    )
    .on_hover_text(
        egui::RichText::new(&i18n.tr("tooltip_show_file_explorer"))
            .size(palette.tooltip_text_size)
            .color(palette.tooltip_text_color),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .clicked()
    {
        action.toggle_file_explorer = true;
    }

    if clickable_active_icon(
        ui,
        regular::TAG,
        ui.visuals().text_color(),
        !is_file_explorer,
        palette.primary,
    )
    .on_hover_text(
        egui::RichText::new(&i18n.tr("tooltip_show_tags"))
            .size(palette.tooltip_text_size)
            .color(palette.tooltip_text_color),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .clicked()
    {
        action.toggle_file_explorer = true;
    }

    let sidebar_tooltip_key = if sidebar_collapsed {
        "tooltip_show_sidebar"
    } else {
        "tooltip_hide_sidebar"
    };

    if clickable_active_icon(
        ui,
        regular::SIDEBAR_SIMPLE,
        ui.visuals().text_color(),
        !sidebar_collapsed,
        palette.primary,
    )
    .on_hover_text(
        egui::RichText::new(&i18n.tr(sidebar_tooltip_key))
            .size(palette.tooltip_text_size)
            .color(palette.tooltip_text_color),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .clicked()
    {
        action.toggle_sidebar = true;
    }
}

fn draw_drag_region(ui: &mut egui::Ui, hwnd: Option<HWND>) {
    // Remaining empty space: lets the user drag the window from the topbar.
    let drag_rect = ui.available_rect_before_wrap();
    if drag_rect.width() > 0.0 && drag_rect.height() > 0.0 {
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

fn menu_item(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    palette: &crate::gui::theme::ThemePalette,
) -> egui::Response {
    let text_galley = ui.fonts_mut(|fonts| {
        fonts.layout_no_wrap(
            label.to_owned(),
            egui::FontId::proportional(palette.text_size),
            ui.visuals().text_color(),
        )
    });
    let text_width = text_galley.size().x;
    let row_height = 18.0;
    let icon_width = 18.0;
    let spacing = 4.0;

    let total_size = egui::vec2(icon_width + spacing + text_width, row_height);

    let (rect, response) = ui.allocate_exact_size(total_size, egui::Sense::click());
    let mut x = rect.min.x;
    let center_y = rect.center().y;

    let icon_color = if response.hovered() {
        palette.primary
    } else {
        ui.visuals().text_color()
    };
    ui.painter().text(
        egui::pos2(x, center_y),
        egui::Align2::LEFT_CENTER,
        icon,
        egui::FontId::default(),
        icon_color,
    );
    x += icon_width + spacing;

    let text_color = if response.hovered() {
        palette.primary
    } else {
        ui.visuals().text_color()
    };
    ui.painter().text(
        egui::pos2(x, center_y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(palette.text_size),
        text_color,
    );

    response
}
