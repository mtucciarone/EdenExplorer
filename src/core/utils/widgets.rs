use crate::core::utils::colors::drive_usage_color;
use crate::gui::theme::ThemePalette;
use eframe::egui::*;
use egui_phosphor::regular::DOTS_SIX_VERTICAL;

pub fn clickable_active_icon(
    ui: &mut Ui,
    icon: &str,
    default_color: Color32,
    is_active: bool,
    is_active_color: Color32,
) -> Response {
    let font_id = FontId::default();

    let galley = ui
        .painter()
        .layout_no_wrap(icon.to_string(), font_id.clone(), default_color);

    let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::click());

    let color = if is_active {
        is_active_color
    } else {
        default_color
    };

    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, icon, font_id, color);

    resp
}

pub fn clickable_icon(ui: &mut Ui, icon: &str, hover_color: Color32) -> Response {
    let font_id = FontId::default();

    let galley =
        ui.painter()
            .layout_no_wrap(icon.to_string(), font_id.clone(), ui.visuals().text_color());

    let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::click());

    let color = if resp.hovered() {
        hover_color
    } else {
        ui.visuals().text_color()
    };

    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, icon, font_id, color);

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
            palette.primary_subtle,
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

pub fn styled_button(ui: &mut Ui, label: impl Into<String>, palette: &ThemePalette) -> Response {
    let label = label.into();
    let font_id = FontId::new(palette.text_size, FontFamily::Proportional);

    let desired_height = ui.spacing().interact_size.y;
    let desired_width = ui.available_width();
    let size = vec2(desired_width, desired_height);

    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    ui.painter().rect(
        rect,
        CornerRadius::same(palette.medium_radius),
        palette.button_background,
        Stroke::new(1.0, palette.tab_border_default),
        StrokeKind::Inside,
    );

    ui.centered_and_justified(|ui| {
        let text_label = Label::new(
            RichText::new(label)
                .color(palette.button_stroke)
                .font(font_id),
        );
        ui.add(text_label);
    });

    response
}
