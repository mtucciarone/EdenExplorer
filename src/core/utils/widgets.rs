use crate::core::indexer::TagIconStyle;
use crate::core::utils::colors::{drive_usage_color, hsl_to_color32, rgb_to_hsl};
use crate::core::utils::text::apply_eden_text_overrides;
use crate::gui::theme::ThemePalette;
use eframe::egui::*;
use egui_phosphor::regular::DOTS_SIX_VERTICAL;

/// Resolves a tag's own colored glyph + font family for the current
/// `TagIconStyle` setting - the single place every tag-icon call site
/// (sidebar list, tab strip, Settings > Tags list) should read this from, so
/// they can't drift out of sync with each other. `regular::TAG`/`fill::TAG`
/// are the exact same Unicode codepoint; which weight actually renders
/// depends entirely on the font *family* requested (Fill is registered under
/// its own named family, not merged into `Proportional`) - painting the fill
/// codepoint with the default family silently renders the outline glyph
/// anyway, which is why this always returns a matched glyph+family pair
/// rather than letting a call site pick one without the other.
pub fn tag_glyph(style: TagIconStyle) -> (&'static str, FontFamily) {
    match style {
        TagIconStyle::Filled => (
            egui_phosphor::fill::TAG,
            FontFamily::Name("phosphor_fill".into()),
        ),
        TagIconStyle::Outline => (egui_phosphor::regular::TAG, FontFamily::Proportional),
    }
}

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

    let padded_size = galley.size() + Vec2::splat(ICON_CLICK_PADDING * 2.0);
    let (rect, resp) = ui.allocate_exact_size(padded_size, Sense::click());

    let color = if is_active || resp.hovered() {
        palette.toolbar_icon_active_color
    } else {
        default_color
    };

    if (is_active || resp.hovered()) && ui.is_rect_visible(rect) {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(palette.small_radius),
            palette
                .search_active_icon_bg
                .linear_multiply(if is_active { 0.7 } else { 0.4 }),
        );
    }

    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, icon, font_id, color);

    resp
}

pub fn clickable_icon(ui: &mut Ui, icon: &str, palette: &ThemePalette) -> Response {
    clickable_icon_sized(ui, icon, palette, FontId::default().size)
}

pub fn clickable_icon_sized(
    ui: &mut Ui,
    icon: &str,
    palette: &ThemePalette,
    size: f32,
) -> Response {
    clickable_icon_sized_with_base_color(ui, icon, palette, size, ui.visuals().text_color())
}

/// Same as [`clickable_icon_sized`] but with an explicit default (non-hover)
/// color instead of the generic text color - e.g. the toolbar uses its own
/// accent-tinted `palette.toolbar_icon_color`.
/// Breathing room reserved around every icon glyph's own rendered size, on
/// all four sides - without this, `allocate_exact_size` sized the clickable
/// rect to *exactly* the glyph's ink, so adjacent icon buttons (the
/// toolbar, breadcrumb icons, context-menu glyphs) had no gap of their own
/// beyond whatever `item_spacing` a parent layout happened to add, and each
/// one's click target was only as forgiving as its own thin glyph shape.
const ICON_CLICK_PADDING: f32 = 4.0;

pub fn clickable_icon_sized_with_base_color(
    ui: &mut Ui,
    icon: &str,
    palette: &ThemePalette,
    size: f32,
    base_color: Color32,
) -> Response {
    let font_id = FontId::proportional(size);

    let galley = ui
        .painter()
        .layout_no_wrap(icon.to_string(), font_id.clone(), base_color);

    let padded_size = galley.size() + Vec2::splat(ICON_CLICK_PADDING * 2.0);
    let (rect, resp) = ui.allocate_exact_size(padded_size, Sense::click());

    let color = if resp.hovered() {
        palette.primary
    } else {
        base_color
    };

    if resp.hovered() && ui.is_rect_visible(rect) {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(palette.small_radius),
            palette.primary_hover.linear_multiply(0.5),
        );
    }

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

/// A color-swatch button that opens a popup combining egui's own visual
/// picker (the saturation/value square + hue slider + RGB drag values it
/// already provides) with a hex field and HSL drag values egui's own picker
/// doesn't have - typing a hex code or an HSL triple is a much faster way to
/// match a specific published color than dragging a 2D square, and egui's
/// built-in picker offers no way to do either.
pub fn rgba_color_edit_button(ui: &mut Ui, color: &mut Color32) -> Response {
    let desired_size = ui.spacing().interact_size;
    let (rect, mut response) = ui.allocate_exact_size(desired_size, Sense::click());

    let popup_id = response.id.with("eden_color_popup");
    let is_open = egui::containers::Popup::is_id_open(ui.ctx(), popup_id);

    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, is_open);
        let rect = rect.expand(visuals.expansion);
        egui::widgets::color_picker::show_color_at(ui.painter(), *color, rect);
        let rounding = visuals.corner_radius.at_most(2);
        ui.painter()
            .rect_stroke(rect, rounding, visuals.bg_stroke, egui::StrokeKind::Outside);
    }
    response = response.on_hover_text("Click to edit color");

    egui::containers::Popup::from_toggle_button_response(&response)
        .id(popup_id)
        .close_behavior(egui::containers::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.spacing_mut().slider_width = 220.0;
            ui.set_width(240.0);

            if egui::widgets::color_picker::color_picker_color32(
                ui,
                color,
                egui::widgets::color_picker::Alpha::OnlyBlend,
            ) {
                response.mark_changed();
            }

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);

            // Hex - "RRGGBB", with or without a leading '#'; alpha untouched.
            ui.horizontal(|ui| {
                ui.label("Hex");
                let mut hex = format!("{:02X}{:02X}{:02X}", color.r(), color.g(), color.b());
                let hex_resp = ui.add(TextEdit::singleline(&mut hex).desired_width(70.0));
                if hex_resp.lost_focus() || hex_resp.changed() {
                    if let Some(rgb) = parse_hex_rgb(&hex) {
                        let parsed = Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], color.a());
                        if parsed != *color {
                            *color = parsed;
                            response.mark_changed();
                        }
                    }
                }
            });

            // HSL - recomputed from the color every frame (so it always
            // reflects whatever the square/hue slider/RGB fields/hex field
            // just did), only written back to `color` on an actual edit so
            // idle frames can't drift from float rounding.
            let (mut h, s01, l01) = rgb_to_hsl(*color);
            let mut s_pct = s01 * 100.0;
            let mut l_pct = l01 * 100.0;
            ui.horizontal(|ui| {
                ui.label("HSL");
                let rh = ui.add(DragValue::new(&mut h).range(0.0..=360.0).suffix("°"));
                let rs = ui.add(DragValue::new(&mut s_pct).range(0.0..=100.0).suffix("%"));
                let rl = ui.add(DragValue::new(&mut l_pct).range(0.0..=100.0).suffix("%"));
                if rh.changed() || rs.changed() || rl.changed() {
                    let new_rgb = hsl_to_color32(h, s_pct / 100.0, l_pct / 100.0);
                    *color = Color32::from_rgba_unmultiplied(
                        new_rgb.r(),
                        new_rgb.g(),
                        new_rgb.b(),
                        color.a(),
                    );
                    response.mark_changed();
                }
            });
        });

    response
}

/// Parses a "RRGGBB" (optionally "#RRGGBB") hex string into `[r, g, b]`.
/// Returns `None` for anything that isn't exactly 6 valid hex digits, so a
/// still-being-typed value (e.g. "6E5") just doesn't update the color yet
/// rather than erroring.
fn parse_hex_rgb(input: &str) -> Option<[u8; 3]> {
    let hex = input.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    Some([
        u8::from_str_radix(&hex[0..2], 16).ok()?,
        u8::from_str_radix(&hex[2..4], 16).ok()?,
        u8::from_str_radix(&hex[4..6], 16).ok()?,
    ])
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
        // A fixed `width` that exceeds the available space (e.g. a narrow
        // split pane, or Settings opened in one) overflows straight past
        // the ui's own clip rect - `ComboBox` doesn't shrink to fit on its
        // own. Clamping here fixes every call site at once rather than
        // each one needing its own bound.
        let width = width.min(ui.available_width().max(60.0));
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

/// Paints a themed, fixed-size button by hand - allocate a rect once, then
/// only ever repaint its fill/stroke/text color depending on hover/disabled
/// state, never its size.
///
/// This exists because egui 0.35's built-in `Button` visibly grows/shrinks
/// by a couple of pixels on hover even when every color passed to it is
/// held constant across states. `Button::atom_ui` computes its frame's
/// `inner_margin` from `button_padding + state.expansion - state.bg_stroke.
/// width` using each interaction state's own *default* `bg_stroke.width`
/// from `Widgets::dark()`/`light()` (`inactive` = 0.0, `hovered` = 1.0) -
/// and it reads that default *before* this app's own `.stroke(...)` call
/// ever gets applied, so the override changes what's *painted* but not the
/// margin math that already ran. The net effect: switching from `inactive`
/// to `hovered` shrinks the button's own inner margin by ~1px per side
/// purely because of that ambient default, with no way to cancel it out
/// through the builder API - reported by the user as buttons visibly
/// resizing on hover across every themed button in the app. Building the
/// button by hand instead (the same technique `clickable_icon_sized_with_
/// base_color` above already uses successfully) sidesteps this class of
/// bug entirely: the rect is allocated once from the text's own measured
/// size, and hovering only ever swaps which precomputed colors get painted
/// into that same rect.
#[allow(clippy::too_many_arguments)]
fn draw_themed_button(
    ui: &mut Ui,
    palette: &ThemePalette,
    text: &str,
    fill: Color32,
    hover_fill: Color32,
    text_color: Color32,
    hover_text_color: Color32,
    stroke: Stroke,
) -> Response {
    let font_id = FontId::proportional(palette.text_size);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font_id.clone(), text_color);

    let padding = ui.spacing().button_padding;
    let min_height = ui.spacing().interact_size.y;
    let desired_size = egui::vec2(
        galley.size().x + padding.x * 2.0,
        (galley.size().y + padding.y * 2.0).max(min_height),
    );

    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());

    if ui.is_rect_visible(rect) {
        let hovered = ui.is_enabled() && response.hovered();
        let (fill, text_color) = if hovered {
            (hover_fill, hover_text_color)
        } else {
            (fill, text_color)
        };

        ui.painter().rect(
            rect,
            CornerRadius::same(palette.medium_radius),
            fill,
            stroke,
            StrokeKind::Inside,
        );
        ui.painter()
            .text(rect.center(), Align2::CENTER_CENTER, text, font_id, text_color);
    }

    response
}

/// The app's general-purpose button (Export/Import/Reset theme, and every
/// other plain action button that isn't a dialog's primary/secondary/ghost
/// choice). Explicitly filled/stroked from `palette.button_background`/
/// `button_stroke` rather than left to the ambient style - those two fields
/// are editable in the Appearance page's "Checkboxes & Buttons" section and
/// a prebuilt theme preset, but previously had no call site actually reading
/// them, so editing them (or switching presets) had no visible effect at
/// all. `regenerate_base_derived_colors` accent-tints both from a preset's
/// `primary`, so this button's resting color now genuinely follows the
/// active accent/preset instead of only reacting to it on hover. On hover,
/// only the fill/text color change (to the same accent-tinted pair used
/// elsewhere for "this is clickable") - see `draw_themed_button`'s own doc
/// comment for why the button's size never does.
pub fn eden_button(ui: &mut egui::Ui, palette: &ThemePalette, text: &str) -> egui::Response {
    apply_eden_visual_overrides(ui, palette);
    // `ui.is_enabled()` reflects a caller wrapping this in `ui.add_enabled(_ui)`
    // (e.g. a "Create" button gated on a non-empty name) - egui's own
    // automatic disabled-opacity fade still applies on top of whichever pair
    // is picked here, same as every other disabled control in the app.
    let (fill, text_color) = if ui.is_enabled() {
        (palette.button_background, palette.button_text_color)
    } else {
        (palette.button_disabled_bg, palette.button_disabled_text)
    };
    draw_themed_button(
        ui,
        palette,
        text,
        fill,
        palette.primary_hover,
        text_color,
        palette.primary_button_text_color,
        Stroke::new(1.0, palette.button_stroke),
    )
}

/// A dialog's single highlighted/recommended action - filled with the
/// user's accent color so it reads as the default choice at a glance.
pub fn primary_dialog_button(ui: &mut Ui, palette: &ThemePalette, text: &str) -> Response {
    let (fill, text_color) = if ui.is_enabled() {
        (palette.primary, palette.primary_button_text_color)
    } else {
        (palette.button_disabled_bg, palette.button_disabled_text)
    };
    draw_themed_button(
        ui,
        palette,
        text,
        fill,
        palette.primary_hover,
        text_color,
        palette.primary_button_text_color,
        Stroke::NONE,
    )
}

/// A dialog action that's valid but not the recommended one - outlined
/// rather than filled, so it doesn't compete with the primary button.
pub fn secondary_dialog_button(ui: &mut Ui, palette: &ThemePalette, text: &str) -> Response {
    draw_themed_button(
        ui,
        palette,
        text,
        Color32::TRANSPARENT,
        palette.primary_hover.linear_multiply(0.15),
        palette.text_normal,
        palette.text_normal,
        Stroke::new(1.0, palette.borders_default),
    )
}

/// A dialog's lowest-emphasis action (Cancel) - text only, no fill or
/// border, so it doesn't visually compete with the real choices.
pub fn ghost_dialog_button(ui: &mut Ui, palette: &ThemePalette, text: &str) -> Response {
    draw_themed_button(
        ui,
        palette,
        text,
        Color32::TRANSPARENT,
        palette.primary_hover.linear_multiply(0.12),
        palette.text_normal.gamma_multiply(0.7),
        palette.text_normal,
        Stroke::NONE,
    )
}

/// The consistent chrome every `egui::Area`+`Frame::popup` modal in this app
/// uses (Checksums, Paste Conflict, Bulk Rename) - rounded corners, a soft
/// drop shadow, and an accent-tinted border so a modal visibly belongs to
/// the app's current theme/preset rather than reading as a flat gray box.
/// `ctx.style_of(ctx.theme())` still has to be read at the call site (it
/// needs a live `&egui::Context`, not just a palette), so this only factors
/// out the part that's identical everywhere.
pub fn modal_frame(style: &Style, palette: &ThemePalette) -> Frame {
    Frame::popup(style)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(18))
        .stroke(Stroke::new(1.5, palette.borders_active.gamma_multiply(0.55)))
        .shadow(epaint::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(90),
        })
}

/// A modal's title row: a tinted icon badge (a soft-filled circle behind a
/// glyph) followed by a bold title and an optional muted subtitle - the
/// exact shape the Checksums and Paste Conflict modals already hand-wrote
/// independently, pulled out so every modal gets it for free and stays
/// visually consistent with the others.
pub fn modal_icon_header(
    ui: &mut Ui,
    palette: &ThemePalette,
    icon: &str,
    icon_color: Color32,
    title: &str,
    subtitle: Option<&str>,
) {
    ui.horizontal(|ui| {
        let icon_diameter = 34.0;
        let (icon_rect, _) =
            ui.allocate_exact_size(vec2(icon_diameter, icon_diameter), Sense::hover());
        ui.painter().circle_filled(
            icon_rect.center(),
            icon_diameter / 2.0,
            icon_color.linear_multiply(0.18),
        );
        ui.painter().text(
            icon_rect.center(),
            Align2::CENTER_CENTER,
            icon,
            FontId::proportional(18.0),
            icon_color,
        );
        ui.add_space(10.0);
        ui.vertical(|ui| {
            ui.label(
                RichText::new(title)
                    .strong()
                    .size(palette.text_size + 2.0)
                    .color(ui.visuals().text_color()),
            );
            if let Some(subtitle) = subtitle {
                ui.add_space(2.0);
                ui.label(
                    RichText::new(subtitle)
                        .size(palette.text_size)
                        .color(palette.text_normal.gamma_multiply(0.75)),
                );
            }
        });
    });
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

#[cfg(test)]
mod color_hex_tests {
    use super::parse_hex_rgb;

    #[test]
    fn parses_with_and_without_leading_hash() {
        assert_eq!(parse_hex_rgb("6E55A0"), Some([0x6E, 0x55, 0xA0]));
        assert_eq!(parse_hex_rgb("#6E55A0"), Some([0x6E, 0x55, 0xA0]));
    }

    #[test]
    fn parses_case_insensitively_and_trims_whitespace() {
        assert_eq!(parse_hex_rgb(" #6e55a0 "), Some([0x6E, 0x55, 0xA0]));
    }

    #[test]
    fn rejects_anything_that_isnt_exactly_six_hex_digits() {
        assert_eq!(parse_hex_rgb("6E5"), None);
        assert_eq!(parse_hex_rgb("6E55A0FF"), None);
        assert_eq!(parse_hex_rgb("GGGGGG"), None);
        assert_eq!(parse_hex_rgb(""), None);
    }
}
