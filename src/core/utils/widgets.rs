use crate::core::utils::colors::drive_usage_color;
use crate::core::utils::text::apply_eden_text_overrides;
use crate::gui::theme::ThemePalette;
use eframe::egui::*;
use egui_phosphor::regular::DOTS_SIX_VERTICAL;

pub fn clickable_active_icon(
    ui: &mut Ui,
    icon: &str,
    default_color: Color32,
    is_active: bool,
    palette: &ThemePalette,
) -> Response {
    let font_id = FontId::default();

    let galley = ui
        .painter()
        .layout_no_wrap(icon.to_string(), font_id.clone(), default_color);

    let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::click());

    let color = if is_active {
        palette.primary
    } else {
        if resp.hovered() {
            palette.primary
        } else {
            default_color
        }
    };

    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, icon, font_id, color);

    resp
}

pub fn clickable_icon(ui: &mut Ui, icon: &str, palette: &ThemePalette) -> Response {
    let font_id = FontId::default();

    let galley =
        ui.painter()
            .layout_no_wrap(icon.to_string(), font_id.clone(), ui.visuals().text_color());

    let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::click());

    let color = if resp.hovered() {
        palette.primary
    } else {
        ui.visuals().text_color()
    };

    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, icon, font_id, color);

    resp
}

pub fn clickable_windows_icon(
    ui: &mut Ui,
    icon: &str,
    hover_bg: Color32,
    palette: &ThemePalette,
) -> Response {
    const BUTTON_WIDTH: f32 = 45.0;
    const BUTTON_HEIGHT: f32 = 26.0;
    const ICON_SIZE: f32 = 14.0;

    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(BUTTON_WIDTH, BUTTON_HEIGHT), Sense::click());

    if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, hover_bg);
    }

    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon,
        FontId::proportional(ICON_SIZE),
        palette.icon_color,
    );

    resp
}

pub fn rgba_color_edit_button(ui: &mut Ui, color: &mut Color32) -> Response {
    let mut rgba = egui::Rgba::from(*color);

    let response = egui::widgets::color_picker::color_edit_button_rgba(
        ui,
        &mut rgba,
        egui::widgets::color_picker::Alpha::OnlyBlend,
    );

    if response.changed() {
        *color = rgba.into();
    }

    response
}

pub fn drive_usage_bar(ui: &mut Ui, total: u64, free: u64, height: f32, palette: &ThemePalette) {
    let used = total.saturating_sub(free);

    let target_ratio = if total == 0 {
        0.0
    } else {
        used as f32 / total as f32
    };

    let id = ui.id().with("drive_usage_anim");
    let animated_ratio = ui.ctx().animate_value_with_time(
        id,
        target_ratio,
        1.5, // animation speed (lower = faster)
    );

    let max_bar_width = 180.0;
    let bar_width = (ui.available_width() - 8.0).min(max_bar_width);
    let (outer_rect, _) = ui.allocate_exact_size(vec2(bar_width, height), Sense::hover());
    let painter = ui.painter();

    let bar_height = outer_rect.height() * 0.65;
    let y_offset = (outer_rect.height() - bar_height) / 2.0;

    let rect = Rect::from_min_size(
        pos2(outer_rect.min.x, outer_rect.min.y + y_offset),
        vec2(outer_rect.width(), bar_height),
    );
    painter.rect_filled(
        rect,
        CornerRadius::same(palette.small_radius),
        palette.drive_usage_background,
    );

    let fill_width = rect.width() * animated_ratio;

    if fill_width > 0.0 {
        let fill_rect = Rect::from_min_size(rect.min, vec2(fill_width, rect.height()));
        let fill_color = drive_usage_color(target_ratio, palette);

        let radius = palette.small_radius;

        let fill_rounding = if animated_ratio >= 0.999 {
            CornerRadius::same(radius)
        } else {
            CornerRadius {
                nw: radius,
                sw: radius,
                ne: 0,
                se: 0,
            }
        };

        painter.rect_filled(fill_rect, fill_rounding, fill_color);
    }

    let percent = format!("{:.0}%", target_ratio * 100.0);

    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        percent,
        TextStyle::Small.resolve(ui.style()),
        palette.drive_usage_text,
    );
}

pub fn draw_object_drag_ghost(
    ui: &Ui,
    palette: &ThemePalette,
    label: &str,
    show_reordering_handle: bool,
) {
    if let Some(pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
        let painter = ui
            .ctx()
            .layer_painter(LayerId::new(Order::Foreground, Id::new("drag_ghost")));

        let ui_rect = ui.min_rect();
        let ghost_width = ui_rect.width();

        let ghost_rect = Rect::from_center_size(pos, vec2(ghost_width, 18.0));

        painter.rect_filled(
            ghost_rect,
            CornerRadius::same(palette.medium_radius),
            palette.borders_default,
        );

        let font_id = FontId::new(palette.text_size, FontFamily::Proportional);

        painter.text(
            pos2(ghost_rect.left() + 8.0, ghost_rect.center().y),
            Align2::LEFT_CENTER,
            label,
            font_id,
            palette.icon_color.gamma_multiply(0.2),
        );

        ui.ctx().set_cursor_icon(CursorIcon::Grab);

        if show_reordering_handle {
            let handle_width = 12.0;

            let handle_rect = Rect::from_min_size(
                pos2(ghost_rect.right() - handle_width - 4.0, ghost_rect.top()),
                vec2(handle_width, ghost_rect.height()),
            );

            painter.text(
                handle_rect.center(),
                Align2::CENTER_CENTER,
                DOTS_SIX_VERTICAL,
                FontId::new(14.0, FontFamily::Proportional),
                palette.icon_color,
            );
        }
    }
}

pub fn draw_checkbox(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    checked: &mut bool,
    id: impl std::hash::Hash + std::fmt::Debug,
) -> egui::Response {
    let size = ui.available_rect_before_wrap().height().min(12.0);

    // The entire table cell
    let cell = ui.available_rect_before_wrap();

    // Center the checkbox inside the cell
    let rect = egui::Rect::from_center_size(cell.center(), egui::vec2(size, size));

    let response = ui.interact(rect, ui.id().with(id), egui::Sense::click());

    if response.clicked() {
        *checked = !*checked;
    }

    let bg = if *checked {
        palette.checkbox_bg_active
    } else if response.hovered() {
        palette.checkbox_bg_hover
    } else {
        palette.checkbox_bg_default
    };

    let border = if response.hovered() {
        palette.checkbox_bg_hover
    } else {
        bg
    };

    ui.painter().rect(
        rect,
        egui::CornerRadius::same(5),
        bg,
        egui::Stroke::new(1.0, border),
        egui::StrokeKind::Middle,
    );

    if *checked {
        let p1 = egui::pos2(rect.left() + 3.0, rect.center().y);
        let p2 = egui::pos2(rect.left() + 6.0, rect.bottom() - 3.0);
        let p3 = egui::pos2(rect.right() - 3.0, rect.top() + 3.0);

        let stroke = egui::Stroke::new(2.0, palette.checkbox_checkmark_color);

        ui.painter().line_segment([p1, p2], stroke);
        ui.painter().line_segment([p2, p3], stroke);
    }

    response
}

pub fn draw_dropdown(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    id: impl std::hash::Hash + std::fmt::Debug,
    width: f32,
    selected_text: impl Into<egui::WidgetText>,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    ui.scope(|ui| {
        // Closed combo styling...
        let visuals = ui.visuals_mut();

        visuals.widgets.hovered.bg_fill = palette.primary_hover;
        visuals.widgets.active.bg_fill = palette.primary_active;
        apply_eden_visual_overrides(ui, palette);
        apply_eden_dropdown_visual_color_overrides(ui, palette);
        egui::ComboBox::from_id_salt(id)
            .width(width)
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                apply_eden_text_overrides(ui, palette);
                apply_eden_visual_overrides(ui, palette);
                // Popup styling
                ui.style_mut().text_styles.insert(
                    egui::TextStyle::Body,
                    egui::FontId::proportional(palette.text_size),
                );

                ui.style_mut().text_styles.insert(
                    egui::TextStyle::Button,
                    egui::FontId::proportional(palette.text_size),
                );

                ui.style_mut().visuals.selection.stroke.color = palette.text_header_section;

                add_contents(ui);
            });
    });
}

pub fn apply_eden_visual_overrides(ui: &mut egui::Ui, palette: &ThemePalette) {
    let style = ui.style_mut();

    style.spacing.button_padding = egui::vec2(4.0, 2.0);
    style.spacing.item_spacing = egui::vec2(6.0, 2.0);
    style.spacing.menu_margin = egui::Margin::same(4);
    style.spacing.interact_size.y = palette.text_size + 6.0;
}

pub fn apply_eden_dropdown_visual_color_overrides(ui: &mut egui::Ui, palette: &ThemePalette) {
    let visuals = &mut ui.style_mut().visuals;

    visuals.widgets.active.bg_fill = egui::Color32::TRANSPARENT;
    visuals.widgets.active.weak_bg_fill = egui::Color32::TRANSPARENT;
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(palette.medium_radius);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(palette.medium_radius);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(palette.medium_radius);
    visuals.widgets.open.corner_radius = egui::CornerRadius::same(palette.medium_radius);
}

pub fn apply_eden_visual_color_overrides(ui: &mut egui::Ui, palette: &ThemePalette) {
    let visuals = &mut ui.style_mut().visuals;

    visuals.widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
    visuals.widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;

    visuals.widgets.hovered.bg_fill = palette.primary;
    visuals.widgets.hovered.weak_bg_fill = palette.primary;

    visuals.widgets.active.bg_fill = palette.primary;
    visuals.widgets.active.weak_bg_fill = palette.primary;
}

pub fn eden_text_label(ui: &mut egui::Ui, palette: &ThemePalette, text: &str) -> egui::Response {
    apply_eden_text_overrides(ui, palette);
    ui.label(
        egui::RichText::new(text)
            .font(FontId::proportional(palette.text_size))
            .color(palette.text_normal),
    )
}

pub fn eden_button(ui: &mut egui::Ui, palette: &ThemePalette, text: &str) -> egui::Response {
    apply_eden_visual_overrides(ui, palette);
    apply_eden_text_overrides(ui, palette);
    ui.button(egui::RichText::new(text).color(palette.text_normal))
}

pub fn eden_toggle_button(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    selected: bool,
    text: &str,
) -> egui::Response {
    apply_eden_visual_overrides(ui, palette);
    apply_eden_text_overrides(ui, palette);
    ui.add(
        egui::Button::new(egui::RichText::new(text).color(if selected {
            palette.text_header_section
        } else {
            palette.text_normal
        }))
        .selected(selected),
    )
}
