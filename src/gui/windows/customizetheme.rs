use crate::core::indexer::CustomThemeEntry;
use crate::core::utils::fonts::get_font_list;
use crate::gui::i18n::I18n;
use crate::gui::theme::{
    PALETTE_PRESETS, ThemeMode, ThemePalette, apply_font_to_context, apply_theme_preset,
    get_default_palette, regenerate_base_derived_colors,
};
use crate::gui::utils::rgba_color_edit_button;
use crate::gui::utils::widgets::{
    apply_eden_visual_overrides, draw_dropdown, eden_button, eden_text_label, eden_toggle_button,
    ghost_dialog_button, primary_dialog_button,
};
use crate::gui::windows::enums::ThemeCustomizerAction;
use crate::gui::windows::structs::ThemeCustomizer;
use eframe::egui;
use egui_phosphor::regular;

/// Named accent-color presets - a quick way to retint the app (primary +
/// its derived hover/active/border shades) without hand-picking RGB values,
/// the same convenience Files Community's "color scheme" swatch picker
/// offers. Selecting one only changes `primary`; every other color a user
/// has customized is left alone.
const COLOR_SCHEME_PRESETS: &[(&str, egui::Color32)] = &[
    ("theme_scheme_blue", egui::Color32::from_rgb(0, 120, 215)),
    ("theme_scheme_teal", egui::Color32::from_rgb(0, 130, 114)),
    ("theme_scheme_green", egui::Color32::from_rgb(16, 137, 62)),
    (
        "theme_scheme_yellow_gold",
        egui::Color32::from_rgb(218, 165, 32),
    ),
    ("theme_scheme_orange", egui::Color32::from_rgb(202, 80, 16)),
    ("theme_scheme_red", egui::Color32::from_rgb(196, 43, 28)),
    ("theme_scheme_pink", egui::Color32::from_rgb(196, 69, 105)),
    ("theme_scheme_magenta", egui::Color32::from_rgb(191, 0, 119)),
    ("theme_scheme_purple", egui::Color32::from_rgb(136, 23, 152)),
    ("theme_scheme_cyan", egui::Color32::from_rgb(0, 153, 188)),
    ("theme_scheme_gray", egui::Color32::from_rgb(93, 90, 88)),
    ("theme_scheme_indigo", egui::Color32::from_rgb(61, 51, 224)),
    ("theme_scheme_lime", egui::Color32::from_rgb(122, 165, 25)),
    ("theme_scheme_brown", egui::Color32::from_rgb(139, 94, 60)),
    ("theme_scheme_amber", egui::Color32::from_rgb(255, 185, 0)),
    ("theme_scheme_slate", egui::Color32::from_rgb(81, 92, 107)),
    ("theme_scheme_coral", egui::Color32::from_rgb(250, 115, 90)),
    ("theme_scheme_mint", egui::Color32::from_rgb(0, 178, 148)),
    ("theme_scheme_lavender", egui::Color32::from_rgb(150, 130, 230)),
];

fn selectable_mode(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    current: ThemeMode,
    target: ThemeMode,
    label: &str,
) -> bool {
    eden_toggle_button(ui, palette, current == target, label).clicked()
}

/// Draws the theme editor's fields (typography, color-scheme swatches, core
/// colors, export/import/reset) directly into `ui` - embedded as the
/// Settings page's Appearance category rather than a floating window.
pub fn draw_theme_customizer_content(
    ui: &mut egui::Ui,
    i18n: &I18n,
    ctx: &egui::Context,
    customizer: &mut ThemeCustomizer,
    palette: &ThemePalette,
) -> Option<ThemeCustomizerAction> {
    let mut action = None;

    {
        {
            ui.set_width(ui.available_width());
            // TOP SECTION: select which palette to edit - also switches the
            // live app theme to match (see `ThemeCustomizerAction::
            // SetLiveMode`'s doc comment), so clicking "Light mode" here
            // both edits the light palette *and* actually shows it.
            ui.horizontal(|ui| {
                if selectable_mode(
                    ui,
                    palette,
                    customizer.selected_mode,
                    ThemeMode::Dark,
                    &i18n.tr("theme_dark"),
                ) {
                    customizer.selected_mode = ThemeMode::Dark;
                    action = Some(ThemeCustomizerAction::SetLiveMode(ThemeMode::Dark));
                }

                if selectable_mode(
                    ui,
                    palette,
                    customizer.selected_mode,
                    ThemeMode::Light,
                    &i18n.tr("theme_light"),
                ) {
                    customizer.selected_mode = ThemeMode::Light;
                    action = Some(ThemeCustomizerAction::SetLiveMode(ThemeMode::Light));
                }
            });
            // Real gap before the scrollable content below, rather than the
            // row's own item spacing being the only separation.
            ui.add_space(10.0);

            // Captured before `editing_palette` below borrows `customizer`
            // mutably (through a specific field) - `tab_gap` is a plain,
            // unrelated field, but reading it *after* that borrow starts
            // would conflict with it for as long as `editing_palette` is
            // still alive, which is the whole rest of this function.
            let tab_gap_for_preview = customizer.tab_gap;
            let min_tab_width_for_preview = customizer.min_tab_width;

            let editing_palette = match customizer.selected_mode {
                ThemeMode::Dark => &mut customizer.dark_palette,
                ThemeMode::Light => &mut customizer.light_palette,
            };

            let mut changed = false;
            let mut sidebar_width_changed = false;
            let mut tab_gap_changed = false;
            let mut min_tab_width_changed = false;
            let mut custom_themes_changed = false;

            // Two-column layout: every picker/section scrolls independently
            // on the left, while the live mockup of every major surface
            // stays put on the right - previously the preview sat pinned
            // *above* one single scrolling column, which meant it scrolled
            // out of view together with everything below it on a short
            // window. Splitting the row like this needs `editing_palette`
            // (a `&mut ThemePalette`) borrowed mutably by the left column's
            // pickers first, then immutably by the right column's preview -
            // valid because the two `allocate_ui_with_layout` closures run
            // one fully after the other, not concurrently, so this is a
            // sequential reborrow rather than an aliasing one.
            // Equal-width columns rather than a narrow fixed preview strip -
            // the mockup lays out several sub-columns side by side (Sidebar/
            // Toolbar/Preview Pane/Buttons/Notifications), so giving it the
            // same room as the settings list keeps its own labels from
            // clipping instead of just being "wide enough to not look tiny."
            // Reserves a trailing gap after the right column so its own content
            // (the mockup boxes, the "Live Preview" label) doesn't sit flush
            // against the item viewer's own content border - every other edge
            // in this app keeps a small margin from its container's border,
            // this row had none of its own.
            const RIGHT_COLUMN_TRAILING_GAP: f32 = 12.0;
            let half_width = ((ui.available_width()
                - ui.spacing().item_spacing.x
                - RIGHT_COLUMN_TRAILING_GAP)
                / 2.0)
                .max(280.0);
            let settings_width = half_width;
            let preview_width = half_width;
            // Captured *before* entering `ui.horizontal` below - a plain
            // `horizontal` layout's row has no inherent height bound of its own
            // (it grows to fit whatever's tallest inside), so re-querying
            // `ui.available_height()` from inside it returns something far
            // larger/unbounded rather than this page's real remaining height,
            // which silently broke both columns' `ScrollArea`/`allocate_ui_
            // with_layout` sizing (Typography rendered as a ~100px sliver with
            // hundreds of px of dead space below it).
            // Reserves room for the FOOTER row (Export/Import/Reset Theme
            // buttons, drawn after both columns below) *before* handing the
            // rest of the available height to those columns - otherwise
            // `column_height` claims the page's entire remaining height for
            // itself, leaving the footer (and its own trailing `add_space`)
            // to render past that budget with no margin from the window's
            // own bottom border, since nothing reserved space for it.
            const FOOTER_RESERVED_HEIGHT: f32 = 44.0;
            let column_height = (ui.available_height() - FOOTER_RESERVED_HEIGHT).max(0.0);

            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(settings_width, column_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        // SCROLLABLE CONTENT
                        egui::ScrollArea::vertical()
                            .id_salt("theme_settings_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.group(|ui| {
                                    apply_eden_visual_overrides(ui, palette);
                                    eden_text_label(ui, palette, &i18n.tr("theme_typography"));

                                    ui.add_space(6.0);
                                    egui::Grid::new("typography_settings")
                                        .num_columns(2)
                                        .spacing([12.0, 6.0])
                                        .show(ui, |ui| {
                                            eden_text_label(
                                                ui,
                                                palette,
                                                &i18n.tr("theme_textsize"),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    apply_eden_visual_overrides(ui, palette);
                                                    changed |= ui
                                                        .add_sized(
                                                            egui::vec2(90.0, 0.0),
                                                            egui::DragValue::new(
                                                                &mut editing_palette.text_size,
                                                            )
                                                            .range(8.0..=24.0)
                                                            .speed(0.2),
                                                        )
                                                        .changed();
                                                },
                                            );
                                            ui.end_row();

                                            eden_text_label(
                                                ui,
                                                palette,
                                                &i18n.tr("theme_tooltip_textsize"),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    apply_eden_visual_overrides(ui, palette);
                                                    changed |= ui
                                                        .add_sized(
                                                            egui::vec2(90.0, 0.0),
                                                            egui::DragValue::new(
                                                                &mut editing_palette
                                                                    .tooltip_text_size,
                                                            )
                                                            .range(8.0..=24.0)
                                                            .speed(0.2),
                                                        )
                                                        .changed();
                                                },
                                            );
                                            ui.end_row();

                                            eden_text_label(
                                                ui,
                                                palette,
                                                &i18n.tr("theme_explorer_rowheight"),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    apply_eden_visual_overrides(ui, palette);
                                                    changed |= ui
                                                        .add_sized(
                                                            egui::vec2(90.0, 0.0),
                                                            egui::DragValue::new(
                                                                &mut editing_palette.row_height,
                                                            )
                                                            .range(8.0..=32.0)
                                                            .speed(0.5),
                                                        )
                                                        .changed();
                                                },
                                            );
                                            ui.end_row();

                                            eden_text_label(
                                                ui,
                                                palette,
                                                &i18n.tr("theme_sidebar_iconsize"),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    apply_eden_visual_overrides(ui, palette);
                                                    changed |= ui
                                                        .add_sized(
                                                            egui::vec2(90.0, 0.0),
                                                            egui::DragValue::new(
                                                                &mut editing_palette
                                                                    .sidebar_icon_size,
                                                            )
                                                            .range(8.0..=32.0)
                                                            .speed(0.2),
                                                        )
                                                        .changed();
                                                },
                                            );
                                            ui.end_row();

                                            eden_text_label(
                                                ui,
                                                palette,
                                                &i18n.tr("theme_tab_iconsize"),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    apply_eden_visual_overrides(ui, palette);
                                                    changed |= ui
                                                        .add_sized(
                                                            egui::vec2(90.0, 0.0),
                                                            egui::DragValue::new(
                                                                &mut editing_palette.tab_icon_size,
                                                            )
                                                            .range(8.0..=32.0)
                                                            .speed(0.2),
                                                        )
                                                        .changed();
                                                },
                                            );
                                            ui.end_row();

                                            eden_text_label(
                                                ui,
                                                palette,
                                                &i18n.tr("theme_sidebar_item_spacing_y"),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    apply_eden_visual_overrides(ui, palette);
                                                    changed |= ui
                                                        .add_sized(
                                                            egui::vec2(90.0, 0.0),
                                                            egui::DragValue::new(
                                                                &mut editing_palette
                                                                    .sidebar_item_spacing_y,
                                                            )
                                                            .range(-0.3..=2.0)
                                                            .speed(0.1),
                                                        )
                                                        .changed();
                                                },
                                            );
                                            ui.end_row();
                                        });

                                    ui.add_space(8.0);

                                    egui::Grid::new("typography_font_settings")
                                        .num_columns(2)
                                        .spacing([12.0, 6.0])
                                        .show(ui, |ui| {
                                            eden_text_label(ui, palette, &i18n.tr("theme_font"));
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if let Some(new_font) = font_selector(
                                                        ui,
                                                        palette,
                                                        "theme_font_selector",
                                                        &editing_palette.font_name,
                                                    ) {
                                                        editing_palette.font_name = new_font;
                                                        apply_font_to_context(
                                                            ctx,
                                                            &editing_palette,
                                                        );
                                                        changed = true;
                                                    }
                                                },
                                            );
                                            ui.end_row();

                                            eden_text_label(
                                                ui,
                                                palette,
                                                &i18n.tr("theme_mono_font"),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if let Some(new_font) = font_selector(
                                                        ui,
                                                        palette,
                                                        "theme_mono_font_selector",
                                                        &editing_palette.mono_font_name,
                                                    ) {
                                                        editing_palette.mono_font_name = new_font;
                                                        apply_font_to_context(
                                                            ctx,
                                                            &editing_palette,
                                                        );
                                                        changed = true;
                                                    }
                                                },
                                            );
                                            ui.end_row();
                                        });

                                    ui.add_space(6.0);
                                    ui.separator();

                                    eden_text_label(ui, palette, &i18n.tr("theme_layout"));
                                    ui.add_space(6.0);

                                    eden_text_label(ui, palette, &i18n.tr("theme_layout_density"));
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        // (label key, row_height, sidebar_item_spacing_y) - pure
                                        // convenience presets over the two existing fields above;
                                        // no new schema needed since both fields already exist.
                                        const DENSITY_PRESETS: &[(&str, f32, f32)] = &[
                                            ("theme_density_compact", 14.0, 0.2),
                                            ("theme_density_comfortable", 16.0, 0.5),
                                            ("theme_density_spacious", 20.0, 0.8),
                                        ];
                                        for &(label_key, row_height, spacing_y) in DENSITY_PRESETS {
                                            let is_selected =
                                                (editing_palette.row_height - row_height).abs()
                                                    < 0.01
                                                    && (editing_palette.sidebar_item_spacing_y
                                                        - spacing_y)
                                                        .abs()
                                                        < 0.01;
                                            if eden_toggle_button(
                                                ui,
                                                palette,
                                                is_selected,
                                                &i18n.tr(label_key),
                                            )
                                            .clicked()
                                            {
                                                editing_palette.row_height = row_height;
                                                editing_palette.sidebar_item_spacing_y = spacing_y;
                                                changed = true;
                                            }
                                        }
                                    });

                                    ui.add_space(8.0);

                                    egui::Grid::new("theme_layout_grid")
                                        .num_columns(2)
                                        .spacing([12.0, 6.0])
                                        .show(ui, |ui| {
                                            eden_text_label(
                                                ui,
                                                palette,
                                                &i18n.tr("theme_sidebar_width"),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    apply_eden_visual_overrides(ui, palette);
                                                    let resp = ui.add_sized(
                                                        egui::vec2(90.0, 0.0),
                                                        egui::DragValue::new(
                                                            &mut customizer.sidebar_width,
                                                        )
                                                        .range(140.0..=600.0)
                                                        .speed(1.0),
                                                    );
                                                    if resp.changed() {
                                                        sidebar_width_changed = true;
                                                    }
                                                },
                                            );
                                            ui.end_row();

                                            eden_text_label(ui, palette, &i18n.tr("theme_tab_gap"));
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    apply_eden_visual_overrides(ui, palette);
                                                    let resp = ui.add_sized(
                                                        egui::vec2(90.0, 0.0),
                                                        egui::DragValue::new(
                                                            &mut customizer.tab_gap,
                                                        )
                                                        .range(0.0..=24.0)
                                                        .speed(0.2),
                                                    );
                                                    if resp.changed() {
                                                        tab_gap_changed = true;
                                                    }
                                                },
                                            );
                                            ui.end_row();

                                            eden_text_label(
                                                ui,
                                                palette,
                                                &i18n.tr("theme_min_tab_width"),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    apply_eden_visual_overrides(ui, palette);
                                                    let resp = ui.add_sized(
                                                        egui::vec2(90.0, 0.0),
                                                        egui::DragValue::new(
                                                            &mut customizer.min_tab_width,
                                                        )
                                                        .range(60.0..=300.0)
                                                        .speed(1.0),
                                                    );
                                                    if resp.changed() {
                                                        min_tab_width_changed = true;
                                                    }
                                                },
                                            );
                                            ui.end_row();

                                            eden_text_label(
                                                ui,
                                                palette,
                                                &i18n.tr("theme_tab_corner_radius"),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    apply_eden_visual_overrides(ui, palette);
                                                    // One picker drives both the active and
                                                    // inactive tab's top corners together
                                                    // (`nw`/`ne` - `sw`/`se` stay 0, a tab never
                                                    // rounds its bottom edge) rather than
                                                    // exposing all four `CornerRadius` fields
                                                    // separately, matching how the user asked
                                                    // for one "tab corner radius" setting, not
                                                    // an active/inactive distinction.
                                                    let mut radius = editing_palette
                                                        .tab_active_radius
                                                        .nw;
                                                    let resp = ui.add_sized(
                                                        egui::vec2(90.0, 0.0),
                                                        egui::DragValue::new(&mut radius)
                                                            .range(0..=20)
                                                            .speed(0.2),
                                                    );
                                                    if resp.changed() {
                                                        editing_palette.tab_active_radius.nw =
                                                            radius;
                                                        editing_palette.tab_active_radius.ne =
                                                            radius;
                                                        editing_palette.tab_inactive_radius.nw =
                                                            radius;
                                                        editing_palette.tab_inactive_radius.ne =
                                                            radius;
                                                        changed = true;
                                                    }
                                                },
                                            );
                                            ui.end_row();
                                        });

                                    ui.add_space(6.0);
                                    ui.separator();

                                    eden_text_label(ui, palette, &i18n.tr("theme_prebuilt_themes"));
                                    ui.add_space(6.0);

                                    ui.horizontal_wrapped(|ui| {
                                        for preset in PALETTE_PRESETS {
                                            let is_selected = editing_palette.primary
                                                == preset.accent
                                                && editing_palette.secondary_accent
                                                    == preset.secondary;

                                            let (rect, resp) = ui.allocate_exact_size(
                                                egui::vec2(64.0, 32.0),
                                                egui::Sense::click(),
                                            );
                                            if ui.is_rect_visible(rect) {
                                                let radius = palette.small_radius;
                                                let painter = ui.painter();
                                                painter.rect_filled(
                                                    rect,
                                                    egui::CornerRadius::same(radius),
                                                    preset.accent,
                                                );
                                                let strip_w = rect.width() / 2.0;
                                                let strip = egui::Rect::from_min_size(
                                                    egui::pos2(rect.min.x + strip_w, rect.min.y),
                                                    egui::vec2(strip_w, rect.height()),
                                                );
                                                painter.rect_filled(
                                                    strip,
                                                    egui::CornerRadius {
                                                        nw: 0,
                                                        sw: 0,
                                                        ne: radius,
                                                        se: radius,
                                                    },
                                                    preset.secondary,
                                                );
                                                painter.rect_stroke(
                                                    rect,
                                                    egui::CornerRadius::same(radius),
                                                    if is_selected {
                                                        egui::Stroke::new(2.0, palette.text_normal)
                                                    } else {
                                                        egui::Stroke::new(
                                                            1.0,
                                                            palette.borders_default,
                                                        )
                                                    },
                                                    egui::StrokeKind::Outside,
                                                );
                                            }
                                            let resp = resp.on_hover_text(i18n.tr(preset.name_key));
                                            if resp.clicked() {
                                                apply_theme_preset(
                                                    editing_palette,
                                                    preset,
                                                    customizer.selected_mode == ThemeMode::Dark,
                                                );
                                                // A prebuilt preset isn't a custom theme -
                                                // leaving the name field/"Update Theme"
                                                // button pointed at whichever custom theme
                                                // was last selected would offer to save
                                                // this preset's colors over it by mistake.
                                                // Clears the persisted selection too (see
                                                // `SelectedCustomThemeSnapshot`'s own doc
                                                // comment) so a restart doesn't restore it.
                                                customizer.new_custom_theme_name.clear();
                                                customizer.selected_custom_theme_id = None;
                                                crate::core::indexer::save_selected_custom_theme(
                                                    &crate::core::indexer::SelectedCustomThemeSnapshot {
                                                        id: None,
                                                    },
                                                );
                                                changed = true;
                                            }
                                        }
                                    });

                                    ui.add_space(10.0);
                                    ui.separator();

                                    eden_text_label(ui, palette, &i18n.tr("theme_color_scheme"));
                                    ui.add_space(6.0);

                                    ui.horizontal_wrapped(|ui| {
                                        for &(name_key, color) in COLOR_SCHEME_PRESETS {
                                            let is_selected = editing_palette.primary == color;
                                            let resp = ui.add(
                                                egui::Button::new("")
                                                    .fill(color)
                                                    .stroke(if is_selected {
                                                        egui::Stroke::new(2.0, palette.text_normal)
                                                    } else {
                                                        egui::Stroke::new(
                                                            1.0,
                                                            palette.borders_default,
                                                        )
                                                    })
                                                    .corner_radius(egui::CornerRadius::same(
                                                        palette.small_radius,
                                                    ))
                                                    .min_size(egui::vec2(26.0, 26.0)),
                                            );
                                            let resp = resp.on_hover_text(i18n.tr(name_key));
                                            if resp.clicked() {
                                                editing_palette.primary = color;
                                                regenerate_base_derived_colors(
                                                    editing_palette,
                                                    customizer.selected_mode == ThemeMode::Dark,
                                                );
                                                changed = true;
                                            }
                                        }
                                    });

                                    ui.add_space(10.0);
                                    ui.separator();

                                    eden_text_label(ui, palette, &i18n.tr("theme_custom_themes"));
                                    ui.add_space(6.0);

                                    ui.horizontal_wrapped(|ui| {
                                        // Each entry is allocated as one atomic rect
                                        // (swatch + delete button together), not a
                                        // nested `ui.horizontal` - a composite child ui
                                        // inside `horizontal_wrapped` throws off its
                                        // running-width bookkeeping, which is what made
                                        // the gap between entries visibly uneven
                                        // (reported directly: "the list is not align").
                                        // Every other wrapped swatch row in this file
                                        // (Prebuilt Themes, Color Scheme) is a single
                                        // flat widget per entry for the same reason.
                                        //
                                        // Wider spacing than the ambient default - this
                                        // is the only wrapped swatch row with a name
                                        // label under each entry, so the default gap
                                        // (tuned for the plain, label-less Prebuilt
                                        // Themes/Color Scheme swatches above) reads as
                                        // cramped against that extra line of text, both
                                        // between entries in a row and between wrapped
                                        // rows themselves (reported directly: "gap
                                        // between list" / "gap between first row and
                                        // second row" - then again, more: "so user can
                                        // differentiate the delete button... is for that
                                        // particular custom theme"). Set on this
                                        // closure's own child `ui` only, so it doesn't
                                        // leak spacing into anything drawn after this
                                        // block.
                                        ui.spacing_mut().item_spacing = egui::vec2(24.0, 20.0);
                                        const SWATCH_W: f32 = 64.0;
                                        const GAP: f32 = 6.0;
                                        const TRASH_W: f32 = 24.0;
                                        const ROW_H: f32 = 32.0;
                                        const NAME_GAP: f32 = 3.0;
                                        const NAME_H: f32 = 15.0;
                                        const MIN_ENTRY_W: f32 = SWATCH_W + GAP + TRASH_W;
                                        let name_font =
                                            egui::FontId::proportional(palette.tooltip_text_size);

                                        for entry in &customizer.custom_themes {
                                            let is_selected = editing_palette.primary
                                                == entry.accent
                                                && editing_palette.secondary_accent
                                                    == entry.secondary;

                                            // The card widens to fit the theme's full
                                            // name (left-aligned under the swatch+trash
                                            // row) rather than truncating it - measured
                                            // directly rather than through `truncate_
                                            // item_text`, since that helper's whole job
                                            // is finding the longest string that fits a
                                            // *fixed* width, the opposite of what's
                                            // needed here.
                                            let name_width = ui
                                                .painter()
                                                .layout_no_wrap(
                                                    entry.name.clone(),
                                                    name_font.clone(),
                                                    palette.text_normal,
                                                )
                                                .size()
                                                .x;
                                            let entry_w = MIN_ENTRY_W.max(name_width);

                                            // Full block (swatch+trash row, plus the
                                            // theme's own name underneath) is one atomic
                                            // allocation for the same reason as above -
                                            // the name was previously only a hover
                                            // tooltip, with nothing shown at a glance.
                                            let (rect, _resp) = ui.allocate_exact_size(
                                                egui::vec2(entry_w, ROW_H + NAME_GAP + NAME_H),
                                                egui::Sense::hover(),
                                            );

                                            let swatch_rect = egui::Rect::from_min_size(
                                                rect.min,
                                                egui::vec2(SWATCH_W, ROW_H),
                                            );
                                            let trash_rect = egui::Rect::from_min_size(
                                                egui::pos2(rect.min.x + SWATCH_W + GAP, rect.min.y),
                                                egui::vec2(TRASH_W, ROW_H),
                                            );

                                            let swatch_resp = ui.interact(
                                                swatch_rect,
                                                ui.id().with(("custom_theme_swatch", entry.id)),
                                                egui::Sense::click(),
                                            );
                                            if ui.is_rect_visible(swatch_rect) {
                                                let radius = palette.small_radius;
                                                let painter = ui.painter();
                                                painter.rect_filled(
                                                    swatch_rect,
                                                    egui::CornerRadius::same(radius),
                                                    entry.accent,
                                                );
                                                let strip_w = swatch_rect.width() / 2.0;
                                                let strip = egui::Rect::from_min_size(
                                                    egui::pos2(
                                                        swatch_rect.min.x + strip_w,
                                                        swatch_rect.min.y,
                                                    ),
                                                    egui::vec2(strip_w, swatch_rect.height()),
                                                );
                                                painter.rect_filled(
                                                    strip,
                                                    egui::CornerRadius {
                                                        nw: 0,
                                                        sw: 0,
                                                        ne: radius,
                                                        se: radius,
                                                    },
                                                    entry.secondary,
                                                );
                                                painter.rect_stroke(
                                                    swatch_rect,
                                                    egui::CornerRadius::same(radius),
                                                    if is_selected {
                                                        egui::Stroke::new(2.0, palette.text_normal)
                                                    } else {
                                                        egui::Stroke::new(
                                                            1.0,
                                                            palette.borders_default,
                                                        )
                                                    },
                                                    egui::StrokeKind::Outside,
                                                );
                                            }
                                            let swatch_resp = swatch_resp
                                                .on_hover_text(entry.name.clone())
                                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                                            if swatch_resp.clicked() {
                                                if let Some(saved_palette) = &entry.palette {
                                                    // Full restore - every field the user
                                                    // had when they saved this theme, not
                                                    // just accent/secondary re-derived
                                                    // from scratch (see `CustomThemeEntry`'s
                                                    // doc comment for why that was the bug).
                                                    *editing_palette = saved_palette.clone();
                                                } else {
                                                    // Legacy entry (saved before `palette`
                                                    // existed) - only ever had accent/
                                                    // secondary to begin with, so this is
                                                    // the most it can restore.
                                                    editing_palette.primary = entry.accent;
                                                    editing_palette.secondary_accent =
                                                        entry.secondary;
                                                    // `pinned_tab_color` is re-derived from
                                                    // `secondary_accent` below;
                                                    // `button_favorite_fill` is deliberately
                                                    // NOT secondary-derived (fixed default,
                                                    // user-editable independently), so it's
                                                    // left untouched here.
                                                    regenerate_base_derived_colors(
                                                        editing_palette,
                                                        customizer.selected_mode == ThemeMode::Dark,
                                                    );
                                                }
                                                changed = true;
                                                // Pre-fill the name field with this theme's
                                                // own name, so tweaking a color and clicking
                                                // the button (now reading "Update Theme")
                                                // saves back into this same entry instead of
                                                // requiring the name to be retyped from
                                                // scratch. Secondary needs no equivalent
                                                // pre-fill - the swatch click above (either
                                                // branch) already set `editing_palette.
                                                // secondary_accent` directly, which is what
                                                // its own picker now edits live.
                                                customizer.new_custom_theme_name =
                                                    entry.name.clone();
                                                // Persisted immediately (not routed through
                                                // the action system below, which only
                                                // returns one action per frame and this
                                                // click already needs `ThemeUpdated` for the
                                                // palette change) so the next launch can
                                                // restore this same selection - see
                                                // `SelectedCustomThemeSnapshot`'s own doc
                                                // comment.
                                                customizer.selected_custom_theme_id =
                                                    Some(entry.id);
                                                crate::core::indexer::save_selected_custom_theme(
                                                    &crate::core::indexer::SelectedCustomThemeSnapshot {
                                                        id: Some(entry.id),
                                                    },
                                                );
                                            }

                                            let trash_resp = ui.interact(
                                                trash_rect,
                                                ui.id().with(("custom_theme_delete", entry.id)),
                                                egui::Sense::click(),
                                            );
                                            if ui.is_rect_visible(trash_rect) {
                                                let painter = ui.painter();
                                                if trash_resp.hovered() {
                                                    painter.rect_filled(
                                                        trash_rect,
                                                        egui::CornerRadius::same(
                                                            palette.small_radius,
                                                        ),
                                                        palette.primary_hover.linear_multiply(0.4),
                                                    );
                                                }
                                                painter.text(
                                                    trash_rect.center(),
                                                    egui::Align2::CENTER_CENTER,
                                                    regular::TRASH,
                                                    egui::FontId::default(),
                                                    palette.icon_color,
                                                );
                                            }
                                            if trash_resp
                                                .on_hover_text(i18n.tr("theme_custom_theme_delete"))
                                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                .clicked()
                                            {
                                                customizer.custom_theme_delete_confirm =
                                                    Some(entry.id);
                                            }

                                            // The full name, left-aligned under the
                                            // swatch's own left edge - `entry_w` above
                                            // already widened the card to fit it, so
                                            // there's nothing left to truncate.
                                            if ui.is_rect_visible(rect) {
                                                ui.painter().text(
                                                    egui::pos2(
                                                        rect.min.x,
                                                        rect.min.y
                                                            + ROW_H
                                                            + NAME_GAP
                                                            + NAME_H / 2.0,
                                                    ),
                                                    egui::Align2::LEFT_CENTER,
                                                    &entry.name,
                                                    name_font.clone(),
                                                    palette.text_normal,
                                                );
                                            }
                                        }
                                    });

                                    ui.add_space(14.0);
                                    // A 2-column grid rather than three separate
                                    // horizontal rows - exact layout spelled out by the
                                    // user after two earlier attempts (a standalone row
                                    // with tuned spacing, then merging the button into
                                    // Secondary's own row) both still read as
                                    // ambiguous: row 1 is Primary's label + picker, row
                                    // 2 is blank + the Swap Colors button (so it sits
                                    // directly between the two color swatches, aligned
                                    // under them rather than under either label), row 3
                                    // is Secondary's label + picker.
                                    // No `right_to_left` wrapper in column 2 (unlike
                                    // this file's own `color_row` helper used by the
                                    // full-width settings-list sections below) - that
                                    // layout claims the rest of the row's available
                                    // width and right-aligns within it, which is exactly
                                    // what pushed the picker/button far from column 1's
                                    // labels per direct user feedback. Left in normal
                                    // flow, each column sizes to its own content's
                                    // natural width instead, so the whole grid stays
                                    // compact instead of spanning the panel.
                                    egui::Grid::new("custom_theme_primary_secondary_grid")
                                        .num_columns(2)
                                        .spacing([12.0, 6.0])
                                        .show(ui, |ui| {
                                            apply_eden_visual_overrides(ui, palette);
                                            // Editing primary right here (not just up in
                                            // Core Colors) means both halves of a new
                                            // custom theme can be chosen together in one
                                            // place. Mirrors Core Colors' own Primary row
                                            // exactly - same field, same
                                            // `regenerate_base_derived_colors` call on
                                            // change - so the Live Preview updates
                                            // immediately and stays in sync no matter
                                            // which of the two pickers was actually used.
                                            eden_text_label(
                                                ui,
                                                palette,
                                                &i18n.tr("theme_colors_primary"),
                                            );
                                            if color_picker_control(
                                                ui,
                                                &mut editing_palette.primary,
                                            ) {
                                                regenerate_base_derived_colors(
                                                    editing_palette,
                                                    customizer.selected_mode
                                                        == ThemeMode::Dark,
                                                );
                                                changed = true;
                                            }
                                            ui.end_row();

                                            ui.label("");
                                            // Swaps the two colors right above/below it
                                            // rather than requiring the user to re-pick
                                            // both by hand to try the opposite pairing -
                                            // re-derives immediately afterward, same as
                                            // either picker.
                                            if eden_button(
                                                ui,
                                                palette,
                                                &i18n.tr("theme_custom_theme_swap"),
                                            )
                                            .clicked()
                                            {
                                                std::mem::swap(
                                                    &mut editing_palette.primary,
                                                    &mut editing_palette.secondary_accent,
                                                );
                                                regenerate_base_derived_colors(
                                                    editing_palette,
                                                    customizer.selected_mode
                                                        == ThemeMode::Dark,
                                                );
                                                changed = true;
                                            }
                                            ui.end_row();

                                            // `secondary_accent` has no Core Colors
                                            // picker of its own (see its doc comment in
                                            // `theme.rs`) - this is its only editable
                                            // home. Edits `editing_palette` directly (not
                                            // a separate staging value) and re-derives
                                            // immediately, the same way choosing a whole
                                            // prebuilt preset already does via
                                            // `apply_theme_preset`.
                                            eden_text_label(
                                                ui,
                                                palette,
                                                &i18n.tr("theme_custom_theme_secondary"),
                                            );
                                            if color_picker_control(
                                                ui,
                                                &mut editing_palette.secondary_accent,
                                            ) {
                                                // `pinned_tab_color`/
                                                // `toolbar_icon_color`/the notification+
                                                // toast border colors are all re-derived
                                                // from `secondary_accent` inside
                                                // `regenerate_base_derived_colors` itself
                                                // now - `button_favorite_fill` is
                                                // deliberately NOT secondary-derived
                                                // (fixed default, user-editable
                                                // independently per its own row below).
                                                regenerate_base_derived_colors(
                                                    editing_palette,
                                                    customizer.selected_mode
                                                        == ThemeMode::Dark,
                                                );
                                                changed = true;
                                            }
                                            ui.end_row();
                                        });
                                    ui.add_space(14.0);
                                    ui.horizontal(|ui| {
                                        apply_eden_visual_overrides(ui, palette);
                                        ui.add(
                                            egui::TextEdit::singleline(
                                                &mut customizer.new_custom_theme_name,
                                            )
                                            .hint_text(i18n.tr("theme_custom_theme_name_hint"))
                                            .desired_width(180.0),
                                        );
                                        let trimmed_name =
                                            customizer.new_custom_theme_name.trim().to_string();
                                        let can_create = !trimmed_name.is_empty();
                                        // Saving under a name that already exists in
                                        // this list updates that entry in place instead
                                        // of creating a duplicate - matched case-
                                        // insensitively so "midnight"/"Midnight" are
                                        // treated as the same theme. The button's own
                                        // label reflects which one is about to happen.
                                        let existing_id = customizer
                                            .custom_themes
                                            .iter()
                                            .find(|e| e.name.eq_ignore_ascii_case(&trimmed_name))
                                            .map(|e| e.id);
                                        let save_label = if existing_id.is_some() {
                                            i18n.tr("theme_custom_theme_update")
                                        } else {
                                            i18n.tr("theme_custom_theme_save")
                                        };
                                        let create_resp = ui.add_enabled_ui(can_create, |ui| {
                                            primary_dialog_button(ui, palette, &save_label)
                                        });
                                        if create_resp.inner.clicked() {
                                            // Captures the *entire* currently-edited
                                            // palette, not just accent/secondary - the
                                            // button reads "Save Current Colors", so it
                                            // should actually save all of them (see
                                            // `CustomThemeEntry`'s doc comment). Primary/
                                            // secondary (and their derived fields) are
                                            // already live-correct in `editing_palette` by
                                            // this point - both pickers above apply
                                            // immediately - so no separate override is
                                            // needed here the way there used to be.
                                            let snapshot_palette = editing_palette.clone();

                                            let saved_id = if let Some(id) = existing_id {
                                                if let Some(existing) = customizer
                                                    .custom_themes
                                                    .iter_mut()
                                                    .find(|e| e.id == id)
                                                {
                                                    existing.accent = snapshot_palette.primary;
                                                    existing.secondary =
                                                        snapshot_palette.secondary_accent;
                                                    existing.palette = Some(snapshot_palette);
                                                }
                                                id
                                            } else {
                                                let id = customizer.custom_themes_next_id;
                                                customizer.custom_themes_next_id += 1;
                                                customizer.custom_themes.push(CustomThemeEntry {
                                                    id,
                                                    name: trimmed_name,
                                                    accent: snapshot_palette.primary,
                                                    secondary: snapshot_palette.secondary_accent,
                                                    palette: Some(snapshot_palette),
                                                });
                                                id
                                            };
                                            customizer.new_custom_theme_name.clear();
                                            // The name field above clears immediately (existing
                                            // behavior, unrelated to this), but the theme just
                                            // saved/updated is still what a restart should
                                            // restore - see `SelectedCustomThemeSnapshot`'s own
                                            // doc comment.
                                            customizer.selected_custom_theme_id = Some(saved_id);
                                            crate::core::indexer::save_selected_custom_theme(
                                                &crate::core::indexer::SelectedCustomThemeSnapshot {
                                                    id: Some(saved_id),
                                                },
                                            );
                                            custom_themes_changed = true;
                                        }
                                    });

                                    ui.add_space(10.0);
                                    ui.separator();

                                    eden_text_label(ui, palette, &i18n.tr("theme_core_colors"));

                                    ui.add_space(6.0);

                                    egui::Grid::new("theme_corecolors")
                                        .num_columns(2)
                                        .spacing([12.0, 6.0])
                                        .show(ui, |ui| {
                                            eden_text_label(
                                                ui,
                                                palette,
                                                &i18n.tr("theme_colors_primary"),
                                            );

                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    apply_eden_visual_overrides(ui, palette);

                                                    let primary_changed = color_picker_control(
                                                        ui,
                                                        &mut editing_palette.primary,
                                                    );

                                                    if primary_changed {
                                                        regenerate_base_derived_colors(
                                                            editing_palette,
                                                            customizer.selected_mode
                                                                == ThemeMode::Dark,
                                                        );
                                                        changed = true;
                                                    }
                                                },
                                            );
                                            ui.end_row();

                                            eden_text_label(
                                                ui,
                                                palette,
                                                &i18n.tr("theme_colors_primary_hover"),
                                            );

                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    apply_eden_visual_overrides(ui, palette);

                                                    changed |= color_picker_control(
                                                        ui,
                                                        &mut editing_palette.primary_hover,
                                                    );
                                                },
                                            );
                                            ui.end_row();

                                            eden_text_label(
                                                ui,
                                                palette,
                                                &i18n.tr("theme_colors_primary_active"),
                                            );

                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    apply_eden_visual_overrides(ui, palette);

                                                    changed |= color_picker_control(
                                                        ui,
                                                        &mut editing_palette.primary_active,
                                                    );
                                                },
                                            );
                                            ui.end_row();

                                            eden_text_label(
                                                ui,
                                                palette,
                                                &i18n.tr("theme_colors_borders_default"),
                                            );

                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    apply_eden_visual_overrides(ui, palette);

                                                    changed |= color_picker_control(
                                                        ui,
                                                        &mut editing_palette.borders_default,
                                                    );
                                                },
                                            );
                                            ui.end_row();

                                            eden_text_label(
                                                ui,
                                                palette,
                                                &i18n.tr("theme_colors_borders_active"),
                                            );

                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    apply_eden_visual_overrides(ui, palette);

                                                    changed |= color_picker_control(
                                                        ui,
                                                        &mut editing_palette.borders_active,
                                                    );
                                                },
                                            );
                                            ui.end_row();

                                            eden_text_label(
                                                ui,
                                                palette,
                                                &i18n.tr("theme_colors_application_background"),
                                            );

                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    apply_eden_visual_overrides(ui, palette);

                                                    changed |= color_picker_control(
                                                        ui,
                                                        &mut editing_palette.application_bg_color,
                                                    );
                                                },
                                            );
                                            ui.end_row();
                                        });

                                    ui.add_space(10.0);
                                    ui.separator();

                                    eden_text_label(
                                        ui,
                                        palette,
                                        &i18n.tr("theme_colors_text_rows"),
                                    );
                                    ui.add_space(6.0);
                                    egui::Grid::new("theme_text_rows")
                                        .num_columns(2)
                                        .spacing([12.0, 6.0])
                                        .show(ui, |ui| {
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_text_normal",
                                                &mut editing_palette.text_normal,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_text_header_section",
                                                &mut editing_palette.text_header_section,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_row_selected_bg",
                                                &mut editing_palette.row_selected_bg,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_row_bg",
                                                &mut editing_palette.row_bg,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_item_viewer_row_text_selected",
                                                &mut editing_palette.item_viewer_row_text_selected,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_tooltip_text_color",
                                                &mut editing_palette.tooltip_text_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_tab_text_selected",
                                                &mut editing_palette.tab_text_selected,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_widget_text_inactive",
                                                &mut editing_palette.widget_text_inactive,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_widget_text_hovered",
                                                &mut editing_palette.widget_text_hovered,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_widget_text_active",
                                                &mut editing_palette.widget_text_active,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_selection_text_color",
                                                &mut editing_palette.selection_text_color,
                                            );
                                        });

                                    ui.add_space(10.0);
                                    ui.separator();

                                    eden_text_label(
                                        ui,
                                        palette,
                                        &i18n.tr("theme_colors_panels_icons"),
                                    );
                                    ui.add_space(6.0);
                                    egui::Grid::new("theme_panels_icons")
                                        .num_columns(2)
                                        .spacing([12.0, 6.0])
                                        .show(ui, |ui| {
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_faint_bg",
                                                &mut editing_palette.faint_bg_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_input_field_bg",
                                                &mut editing_palette.input_field_bg,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_sidebar_bg",
                                                &mut editing_palette.sidebar_bg_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_tab_inactive_bg",
                                                &mut editing_palette.tab_inactive_bg_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_modal_background",
                                                &mut editing_palette.modal_background_effect_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_icon_color",
                                                &mut editing_palette.icon_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_icon_windows",
                                                &mut editing_palette.icon_windows,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_icon_colored_hover",
                                                &mut editing_palette.icon_colored_hover,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_toolbar_icon_color",
                                                &mut editing_palette.toolbar_icon_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_resize_handle",
                                                &mut editing_palette.resize_handle,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_separator",
                                                &mut editing_palette.separator_color,
                                            );
                                        });

                                    ui.add_space(10.0);
                                    ui.separator();

                                    eden_text_label(ui, palette, &i18n.tr("theme_colors_tabs"));
                                    ui.add_space(6.0);
                                    egui::Grid::new("theme_tabs")
                                        .num_columns(2)
                                        .spacing([12.0, 6.0])
                                        .show(ui, |ui| {
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_tab_close_hover",
                                                &mut editing_palette.tab_close_hover,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_tab_close_active",
                                                &mut editing_palette.tab_close_active,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_tab_close_normal",
                                                &mut editing_palette.tab_close_normal,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_tab_add_hover",
                                                &mut editing_palette.tab_add_hover,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_pinned_tab",
                                                &mut editing_palette.pinned_tab_color,
                                            );
                                        });

                                    ui.add_space(10.0);
                                    ui.separator();

                                    eden_text_label(
                                        ui,
                                        palette,
                                        &i18n.tr("theme_colors_drive_usage"),
                                    );
                                    ui.add_space(6.0);
                                    egui::Grid::new("theme_drive_usage")
                                        .num_columns(2)
                                        .spacing([12.0, 6.0])
                                        .show(ui, |ui| {
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_drive_usage_critical",
                                                &mut editing_palette.drive_usage_critical,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_drive_usage_warning",
                                                &mut editing_palette.drive_usage_warning,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_drive_usage_normal",
                                                &mut editing_palette.drive_usage_normal,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_drive_usage_background",
                                                &mut editing_palette.drive_usage_background,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_drive_usage_text",
                                                &mut editing_palette.drive_usage_text,
                                            );
                                        });

                                    ui.add_space(10.0);
                                    ui.separator();

                                    eden_text_label(
                                        ui,
                                        palette,
                                        &i18n.tr("theme_colors_checkboxes_buttons"),
                                    );
                                    ui.add_space(6.0);
                                    egui::Grid::new("theme_checkboxes_buttons")
                                        .num_columns(2)
                                        .spacing([12.0, 6.0])
                                        .show(ui, |ui| {
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_checkbox_bg_default",
                                                &mut editing_palette.checkbox_bg_default,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_checkbox_checkmark",
                                                &mut editing_palette.checkbox_checkmark_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_checkbox_bg_hover",
                                                &mut editing_palette.checkbox_bg_hover,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_checkbox_bg_active",
                                                &mut editing_palette.checkbox_bg_active,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_button_background",
                                                &mut editing_palette.button_background,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_button_stroke",
                                                &mut editing_palette.button_stroke,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_button_text",
                                                &mut editing_palette.button_text_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_button_favorite_fill",
                                                &mut editing_palette.button_favorite_fill,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_primary_button_text",
                                                &mut editing_palette.primary_button_text_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_button_disabled_bg",
                                                &mut editing_palette.button_disabled_bg,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_button_disabled_text",
                                                &mut editing_palette.button_disabled_text,
                                            );
                                        });

                                    ui.add_space(10.0);
                                    ui.separator();

                                    eden_text_label(
                                        ui,
                                        palette,
                                        &i18n.tr("theme_colors_corner_radius"),
                                    );
                                    ui.add_space(6.0);
                                    egui::Grid::new("theme_corner_radius")
                                        .num_columns(2)
                                        .spacing([12.0, 6.0])
                                        .show(ui, |ui| {
                                            for (label_key, radius) in [
                                                (
                                                    "theme_radius_small",
                                                    &mut editing_palette.small_radius,
                                                ),
                                                (
                                                    "theme_radius_medium",
                                                    &mut editing_palette.medium_radius,
                                                ),
                                                (
                                                    "theme_radius_large",
                                                    &mut editing_palette.large_radius,
                                                ),
                                            ] {
                                                eden_text_label(ui, palette, &i18n.tr(label_key));
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(
                                                        egui::Align::Center,
                                                    ),
                                                    |ui| {
                                                        apply_eden_visual_overrides(ui, palette);
                                                        let mut value = *radius as f32;
                                                        let resp = ui.add_sized(
                                                            egui::vec2(90.0, 0.0),
                                                            egui::DragValue::new(&mut value)
                                                                .range(0.0..=20.0)
                                                                .speed(0.2),
                                                        );
                                                        if resp.changed() {
                                                            *radius = value.round() as u8;
                                                            changed = true;
                                                        }
                                                    },
                                                );
                                                ui.end_row();
                                            }
                                        });

                                    ui.add_space(10.0);
                                    ui.separator();

                                    eden_text_label(ui, palette, &i18n.tr("theme_colors_sidebar"));
                                    ui.add_space(6.0);
                                    egui::Grid::new("theme_sidebar")
                                        .num_columns(2)
                                        .spacing([12.0, 6.0])
                                        .show(ui, |ui| {
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_sidebar_text",
                                                &mut editing_palette.sidebar_text_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_settings_nav_inactive",
                                                &mut editing_palette.settings_nav_inactive_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_settings_nav_selected_text",
                                                &mut editing_palette
                                                    .settings_nav_selected_text_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_settings_nav_selected_icon",
                                                &mut editing_palette
                                                    .settings_nav_selected_icon_color,
                                            );
                                        });

                                    ui.add_space(10.0);
                                    ui.separator();

                                    eden_text_label(
                                        ui,
                                        palette,
                                        &i18n.tr("theme_colors_status_bar"),
                                    );
                                    ui.add_space(6.0);
                                    egui::Grid::new("theme_status_bar")
                                        .num_columns(2)
                                        .spacing([12.0, 6.0])
                                        .show(ui, |ui| {
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_status_bar_bg",
                                                &mut editing_palette.status_bar_bg_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_status_bar_text",
                                                &mut editing_palette.status_bar_text_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_status_bar_icon",
                                                &mut editing_palette.status_bar_icon_color,
                                            );
                                        });

                                    ui.add_space(10.0);
                                    ui.separator();

                                    eden_text_label(
                                        ui,
                                        palette,
                                        &i18n.tr("theme_colors_address_search"),
                                    );
                                    ui.add_space(6.0);
                                    egui::Grid::new("theme_address_search")
                                        .num_columns(2)
                                        .spacing([12.0, 6.0])
                                        .show(ui, |ui| {
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_address_bar_bg",
                                                &mut editing_palette.address_bar_bg_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_search_active_icon_bg",
                                                &mut editing_palette.search_active_icon_bg,
                                            );
                                        });

                                    ui.add_space(10.0);
                                    ui.separator();

                                    eden_text_label(ui, palette, &i18n.tr("theme_colors_toolbar"));
                                    ui.add_space(6.0);
                                    egui::Grid::new("theme_toolbar")
                                        .num_columns(2)
                                        .spacing([12.0, 6.0])
                                        .show(ui, |ui| {
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_toolbar_bg",
                                                &mut editing_palette.toolbar_bg_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_toolbar_icon_disabled",
                                                &mut editing_palette.toolbar_icon_disabled_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_toolbar_icon_active",
                                                &mut editing_palette.toolbar_icon_active_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_toolbar_icon_hover",
                                                &mut editing_palette.toolbar_icon_hover_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_toolbar_icon_hover_bg",
                                                &mut editing_palette.toolbar_icon_hover_bg_color,
                                            );
                                        });

                                    ui.add_space(10.0);
                                    ui.separator();

                                    eden_text_label(
                                        ui,
                                        palette,
                                        &i18n.tr("theme_colors_preview_pane"),
                                    );
                                    ui.add_space(6.0);
                                    egui::Grid::new("theme_preview_pane")
                                        .num_columns(2)
                                        .spacing([12.0, 6.0])
                                        .show(ui, |ui| {
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_preview_pane_bg",
                                                &mut editing_palette.preview_pane_bg_color,
                                            );
                                        });

                                    ui.add_space(10.0);
                                    ui.separator();

                                    eden_text_label(
                                        ui,
                                        palette,
                                        &i18n.tr("theme_colors_notifications"),
                                    );
                                    ui.add_space(6.0);
                                    egui::Grid::new("theme_notifications")
                                        .num_columns(2)
                                        .spacing([12.0, 6.0])
                                        .show(ui, |ui| {
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_notification_border",
                                                &mut editing_palette.notification_border_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_notification_bg",
                                                &mut editing_palette.notification_bg_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_notification_header_text",
                                                &mut editing_palette.notification_header_text_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_notification_status_success",
                                                &mut editing_palette.notification_status_success,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_notification_status_warning",
                                                &mut editing_palette.notification_status_warning,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_notification_status_error",
                                                &mut editing_palette.notification_status_error,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_notification_status_info",
                                                &mut editing_palette.notification_status_info,
                                            );
                                        });

                                    ui.add_space(10.0);
                                    ui.separator();

                                    eden_text_label(
                                        ui,
                                        palette,
                                        &i18n.tr("theme_colors_navigation_toast"),
                                    );
                                    ui.add_space(6.0);
                                    egui::Grid::new("theme_navigation_toast")
                                        .num_columns(2)
                                        .spacing([12.0, 6.0])
                                        .show(ui, |ui| {
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_navigation_toast_border",
                                                &mut editing_palette.navigation_toast_border_color,
                                            );
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_navigation_toast_bg",
                                                &mut editing_palette.navigation_toast_bg_color,
                                            );
                                        });

                                    ui.add_space(10.0);
                                    ui.separator();

                                    eden_text_label(ui, palette, &i18n.tr("theme_colors_badge"));
                                    ui.add_space(6.0);
                                    egui::Grid::new("theme_badge")
                                        .num_columns(2)
                                        .spacing([12.0, 6.0])
                                        .show(ui, |ui| {
                                            changed |= color_row(
                                                ui,
                                                palette,
                                                i18n,
                                                "theme_colors_badge_color",
                                                &mut editing_palette.badge_color,
                                            );
                                        });
                                });
                            });
                    },
                );

                ui.allocate_ui_with_layout(
                    egui::vec2(preview_width, column_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        // Live mockup of every major surface, built directly from
                        // `editing_palette`'s own field values rather than the live
                        // `ui.visuals()` - so it reflects a color edit the instant it's
                        // made, regardless of whether the mode being edited is also the
                        // one currently live/rendered elsewhere in the app. Static on
                        // the right rather than scrolling with the pickers, so it stays
                        // visible no matter how far down the left column is scrolled.
                        egui::ScrollArea::vertical()
                            .id_salt("theme_preview_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                draw_theme_preview(
                                    ui,
                                    editing_palette,
                                    i18n,
                                    tab_gap_for_preview,
                                    min_tab_width_for_preview,
                                );
                            });
                    },
                );
            });

            // FOOTER
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if eden_button(ui, palette, &i18n.tr("theme_export")).clicked() {
                    action = Some(ThemeCustomizerAction::ExportTheme(customizer.selected_mode));
                }

                if eden_button(ui, palette, &i18n.tr("theme_import")).clicked() {
                    action = Some(ThemeCustomizerAction::ImportTheme(customizer.selected_mode));
                }
                if eden_button(ui, palette, &i18n.tr("theme_reset")).clicked() {
                    let default = get_default_palette(customizer.selected_mode);
                    match customizer.selected_mode {
                        ThemeMode::Dark => customizer.dark_palette = default,
                        ThemeMode::Light => customizer.light_palette = default,
                    }
                    action = Some(ThemeCustomizerAction::ResetToDefaults(
                        customizer.selected_mode,
                    ));
                }
            });
            ui.add_space(8.0);

            if changed && action.is_none() {
                action = Some(ThemeCustomizerAction::ThemeUpdated(
                    customizer.selected_mode,
                ));
            }

            if sidebar_width_changed {
                action = Some(ThemeCustomizerAction::SidebarWidthChanged(
                    customizer.sidebar_width,
                ));
            }

            if tab_gap_changed {
                action = Some(ThemeCustomizerAction::TabGapChanged(customizer.tab_gap));
            }

            if min_tab_width_changed {
                action = Some(ThemeCustomizerAction::MinTabWidthChanged(
                    customizer.min_tab_width,
                ));
            }

            if let Some(pending_id) = customizer.custom_theme_delete_confirm {
                if let Some(entry) = customizer.custom_themes.iter().find(|e| e.id == pending_id) {
                    let theme_name = entry.name.clone();
                    let mut close = false;
                    let mut confirmed = false;
                    egui::Window::new(i18n.tr("theme_custom_theme_delete_title"))
                        .collapsible(false)
                        .resizable(false)
                        .default_width(300.0)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .frame(
                            egui::Frame::popup(&ui.ctx().style_of(ui.ctx().theme()))
                                .corner_radius(egui::CornerRadius::same(8)),
                        )
                        .show(ui.ctx(), |ui| {
                            ui.label(format!(
                                "{}: \"{theme_name}\"",
                                i18n.tr("theme_custom_theme_delete_confirm")
                            ));
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if primary_dialog_button(ui, palette, &i18n.tr("ok")).clicked() {
                                    confirmed = true;
                                    close = true;
                                }
                                if ghost_dialog_button(ui, palette, &i18n.tr("close")).clicked() {
                                    close = true;
                                }
                            });
                        });
                    if confirmed {
                        customizer.custom_themes.retain(|e| e.id != pending_id);
                        // Don't leave the persisted selection pointing at a
                        // theme that no longer exists - falls back to the
                        // existing empty-field behavior on the next launch
                        // instead of silently resolving to nothing.
                        if customizer.selected_custom_theme_id == Some(pending_id) {
                            customizer.selected_custom_theme_id = None;
                            crate::core::indexer::save_selected_custom_theme(
                                &crate::core::indexer::SelectedCustomThemeSnapshot { id: None },
                            );
                        }
                        custom_themes_changed = true;
                    }
                    if close {
                        customizer.custom_theme_delete_confirm = None;
                    }
                } else {
                    customizer.custom_theme_delete_confirm = None;
                }
            }

            if custom_themes_changed {
                action = Some(ThemeCustomizerAction::CustomThemesChanged);
            }
        }
    }

    action
}

fn color_picker_control(ui: &mut egui::Ui, color: &mut egui::Color32) -> bool {
    rgba_color_edit_button(ui, color).changed()
}

/// Small rounded rect with centered text, painted directly with the given
/// fill/border/text colors - the one repeated shape the live preview below
/// is built out of, standing in for a themed surface (a button, a chip, a
/// panel) without needing the real widget behind it.
fn preview_swatch(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    fill: egui::Color32,
    stroke: egui::Stroke,
    radius: u8,
    label: &str,
    text_color: egui::Color32,
) -> egui::Rect {
    let (rect, _resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(radius), fill);
        if stroke.width > 0.0 {
            ui.painter().rect_stroke(
                rect,
                egui::CornerRadius::same(radius),
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        if !label.is_empty() {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(11.0),
                text_color,
            );
        }
    }
    rect
}

/// A small muted caption above one preview cluster - every cluster in
/// `draw_theme_preview` uses the exact same caption style/size, so their
/// content rows all start at the same y regardless of how many stacked
/// elements sit inside them.
fn preview_caption(ui: &mut egui::Ui, p: &ThemePalette, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(10.0)
            .color(p.text_normal.gamma_multiply(0.7)),
    );
    ui.add_space(3.0);
}

/// A genuinely interactive toolbar-icon-button mockup - unlike
/// `preview_swatch` (a static painted rect), this actually responds to the
/// mouse, so hovering it in the Live Preview panel shows the real
/// `toolbar_icon_hover_color`/`toolbar_icon_hover_bg_color` the moment
/// they're edited, without needing to go find the real toolbar to check.
/// Mirrors `nav_icon_button_active` (`itemviewer_navbar.rs`) - kept as its
/// own small copy here rather than sharing code across modules, matching
/// how every other cluster in this preview is already a self-contained
/// mockup rather than the real widget.
fn preview_toolbar_icon(ui: &mut egui::Ui, p: &ThemePalette, icon: &str, is_active: bool) {
    let font_id = egui::FontId::proportional(15.0);
    let color = if is_active {
        p.toolbar_icon_active_color
    } else {
        p.toolbar_icon_color
    };
    let galley = ui
        .painter()
        .layout_no_wrap(icon.to_string(), font_id.clone(), color);
    let padded = galley.size() + egui::Vec2::splat(6.0);
    let (rect, resp) = ui.allocate_exact_size(padded, egui::Sense::hover());

    if resp.hovered() {
        if p.toolbar_icon_hover_bg_color != egui::Color32::TRANSPARENT {
            ui.painter().rect_filled(
                rect,
                egui::CornerRadius::same(p.small_radius),
                p.toolbar_icon_hover_bg_color,
            );
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            icon,
            font_id,
            p.toolbar_icon_hover_color,
        );
    } else {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            icon,
            font_id,
            color,
        );
    }
}

/// A mockup tab shape mirroring the real paint in `tabs.rs::
/// handle_draw_tab_new_allocated` - rounded top corners only (`sw`/`se`
/// forced to 0, matching how a real tab never rounds its bottom edge, since
/// that's where it meets the content below it), using the same active/
/// inactive radius and fill fields so the Tab Corner Radius picker has
/// genuine visual feedback instead of needing several real tabs open to see.
fn preview_tab(ui: &mut egui::Ui, p: &ThemePalette, title: &str, active: bool, min_width: f32) {
    let radius = if active {
        p.tab_active_radius
    } else {
        p.tab_inactive_radius
    };
    let rounding = egui::CornerRadius {
        nw: radius.nw,
        ne: radius.ne,
        sw: 0,
        se: 0,
    };
    let fill = if active {
        p.primary
    } else {
        p.tab_inactive_bg_color
    };
    let text_color = if active {
        p.tab_text_selected
    } else {
        p.text_normal
    };

    let font_id = egui::FontId::proportional(11.0);
    let galley = ui
        .painter()
        .layout_no_wrap(title.to_string(), font_id.clone(), text_color);
    // Scaled down from the real tab strip's own min width (`tabs.rs`'s
    // `min_tab_width`) to fit this compact mockup, but still proportional -
    // widening the real setting visibly widens these preview tabs too, once
    // it exceeds what the title text alone would need.
    let mockup_min_width = (min_width * 0.55).max(1.0);
    let natural_size = galley.size() + egui::vec2(20.0, 12.0);
    let size = egui::vec2(natural_size.x.max(mockup_min_width), natural_size.y);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, rounding, fill);
    ui.painter().galley(
        rect.center() - galley.size() / 2.0,
        galley,
        text_color,
    );
}

/// Live mockup of the app's major themed surfaces, redrawn every frame
/// straight from `p`'s current field values - so any color edit below shows
/// up here instantly, without depending on whether the mode being edited is
/// also the one currently applied to the rest of the running app (see the
/// `ThemeCustomizerAction::SetLiveMode` doc comment for why those two can
/// otherwise disagree). Laid out as two label-then-content rows: every
/// cluster in a row is wrapped in `ui.vertical` with the same caption style
/// first, so - since a plain `ui.horizontal` top-aligns its children - every
/// cluster's content box starts at the same y, and a shared, explicit
/// content height per row keeps them ending at the same y too, rather than
/// each cluster's box floating at whatever size its own content happened to
/// need.
fn draw_theme_preview(
    ui: &mut egui::Ui,
    p: &ThemePalette,
    i18n: &I18n,
    tab_gap: f32,
    min_tab_width: f32,
) {
    const ROW1_HEIGHT: f32 = 52.0;
    const ROW2_HEIGHT: f32 = 26.0;

    egui::Frame::NONE
        .fill(p.application_bg_color)
        .stroke(egui::Stroke::new(1.0, p.borders_default))
        .corner_radius(egui::CornerRadius::same(p.medium_radius))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            eden_text_label(ui, p, &i18n.tr("theme_preview_title"));
            ui.add_space(8.0);

            // --- Row 1: Sidebar | Toolbar & Address Bar | Preview Pane ---
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    preview_caption(ui, p, "Sidebar");
                    egui::Frame::NONE
                        .fill(p.sidebar_bg_color)
                        .corner_radius(egui::CornerRadius::same(p.small_radius))
                        .inner_margin(egui::Margin::same(6))
                        .show(ui, |ui| {
                            ui.set_width(112.0);
                            ui.set_height(ROW1_HEIGHT - 12.0);
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(regular::FOLDER)
                                            .color(p.sidebar_text_color)
                                            .size(12.0),
                                    );
                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new("Downloads")
                                            .color(p.sidebar_text_color)
                                            .size(11.0),
                                    );
                                });
                                ui.add_space(4.0);
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(ui.available_width(), 20.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(
                                    rect,
                                    egui::CornerRadius::same(p.small_radius),
                                    p.primary_hover,
                                );
                                let icon_pos = rect.left_center() + egui::vec2(6.0, 0.0);
                                let icon_galley = ui.painter().layout_no_wrap(
                                    format!("{} ", regular::TAG),
                                    egui::FontId::proportional(11.0),
                                    p.settings_nav_selected_icon_color,
                                );
                                ui.painter().galley(
                                    egui::pos2(
                                        icon_pos.x,
                                        rect.center().y - icon_galley.size().y / 2.0,
                                    ),
                                    icon_galley.clone(),
                                    p.settings_nav_selected_icon_color,
                                );
                                ui.painter().text(
                                    icon_pos + egui::vec2(icon_galley.size().x, 0.0),
                                    egui::Align2::LEFT_CENTER,
                                    "Tags",
                                    egui::FontId::proportional(11.0),
                                    p.settings_nav_selected_text_color,
                                );
                            });
                        });
                });

                ui.add_space(10.0);

                ui.vertical(|ui| {
                    preview_caption(ui, p, "Toolbar & Address Bar");
                    ui.vertical(|ui| {
                        // Toolbar: normal / disabled / active icon states.
                        egui::Frame::NONE
                            .fill(p.toolbar_bg_color)
                            .corner_radius(egui::CornerRadius::same(p.small_radius))
                            .inner_margin(egui::Margin::symmetric(6, 4))
                            .show(ui, |ui| {
                                ui.set_width(160.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(regular::ARROW_LEFT)
                                            .color(p.toolbar_icon_color)
                                            .size(13.0),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        egui::RichText::new(regular::ARROW_RIGHT)
                                            .color(p.toolbar_icon_disabled_color)
                                            .size(13.0),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let (rect, _) = ui.allocate_exact_size(
                                                egui::vec2(22.0, 18.0),
                                                egui::Sense::hover(),
                                            );
                                            ui.painter().rect_filled(
                                                rect,
                                                egui::CornerRadius::same(p.small_radius),
                                                p.search_active_icon_bg,
                                            );
                                            ui.painter().text(
                                                rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                regular::MAGNIFYING_GLASS,
                                                egui::FontId::proportional(13.0),
                                                p.toolbar_icon_active_color,
                                            );
                                        },
                                    );
                                });
                            });
                        ui.add_space(4.0);
                        // Address bar pill.
                        preview_swatch(
                            ui,
                            egui::vec2(160.0, 22.0),
                            p.address_bar_bg_color,
                            egui::Stroke::new(1.0, p.borders_default),
                            p.medium_radius,
                            "D:\\Downloads",
                            p.text_normal,
                        );
                    });
                });

                ui.add_space(10.0);

                ui.vertical(|ui| {
                    preview_caption(ui, p, "Preview Pane");
                    preview_swatch(
                        ui,
                        egui::vec2(70.0, ROW1_HEIGHT - 12.0),
                        p.preview_pane_bg_color,
                        egui::Stroke::new(1.0, p.borders_default),
                        p.medium_radius,
                        "Aa",
                        p.text_normal,
                    );
                });
            });

            ui.add_space(12.0);

            // --- View Layout: hover this row to preview the real
            // toolbar_icon_hover_color/toolbar_icon_hover_bg_color live.
            ui.vertical(|ui| {
                preview_caption(ui, p, "View Layout (hover me)");
                egui::Frame::NONE
                    .fill(p.toolbar_bg_color)
                    .corner_radius(egui::CornerRadius::same(p.small_radius))
                    .inner_margin(egui::Margin::symmetric(6, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            preview_toolbar_icon(ui, p, regular::ROWS, true);
                            ui.add_space(4.0);
                            preview_toolbar_icon(ui, p, regular::IMAGES_SQUARE, false);
                            ui.add_space(4.0);
                            preview_toolbar_icon(ui, p, regular::COLUMNS, false);
                            ui.add_space(4.0);
                            preview_toolbar_icon(ui, p, regular::COLUMNS_PLUS_RIGHT, false);
                            ui.add_space(4.0);
                            preview_toolbar_icon(ui, p, regular::EYE, false);
                            ui.add_space(4.0);
                            preview_toolbar_icon(ui, p, regular::SIDEBAR, false);
                        });
                    });
            });

            ui.add_space(12.0);

            // --- Row 2: Buttons (own row - Notifications & Status used to
            // share this row side-by-side, but that squeezed both clusters
            // together; it's now its own row below instead) ---
            ui.vertical(|ui| {
                preview_caption(ui, p, "Buttons");
                ui.horizontal(|ui| {
                    preview_swatch(
                        ui,
                        egui::vec2(64.0, ROW2_HEIGHT),
                        p.button_background,
                        egui::Stroke::new(1.0, p.button_stroke),
                        p.medium_radius,
                        "Button",
                        p.button_text_color,
                    );
                    ui.add_space(6.0);
                    preview_swatch(
                        ui,
                        egui::vec2(64.0, ROW2_HEIGHT),
                        p.primary,
                        egui::Stroke::NONE,
                        p.medium_radius,
                        "Primary",
                        p.primary_button_text_color,
                    );
                    ui.add_space(6.0);
                    preview_swatch(
                        ui,
                        egui::vec2(64.0, ROW2_HEIGHT),
                        p.button_disabled_bg,
                        egui::Stroke::NONE,
                        p.medium_radius,
                        "Disabled",
                        p.button_disabled_text,
                    );
                });
            });

            ui.add_space(12.0);

            // --- Row 2b: Notifications & Status (full width, own row) ---
            ui.vertical(|ui| {
                preview_caption(ui, p, "Notifications & Status");
                ui.horizontal(|ui| {
                    preview_swatch(
                        ui,
                        egui::vec2(96.0, ROW2_HEIGHT),
                        p.notification_bg_color,
                        egui::Stroke::new(1.0, p.notification_border_color),
                        p.medium_radius,
                        "Notification",
                        p.notification_header_text_color,
                    );
                    ui.add_space(6.0);
                    preview_swatch(
                        ui,
                        egui::vec2(80.0, ROW2_HEIGHT),
                        p.navigation_toast_bg_color,
                        egui::Stroke::new(1.0, p.navigation_toast_border_color),
                        p.medium_radius,
                        "Toast",
                        p.text_normal,
                    );
                    ui.add_space(10.0);
                    for (color, label) in [
                        (p.notification_status_success, "OK"),
                        (p.notification_status_warning, "..."),
                        (p.notification_status_error, "!"),
                        (p.notification_status_info, "i"),
                    ] {
                        ui.vertical(|ui| {
                            ui.add_space((ROW2_HEIGHT - 20.0) / 2.0);
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(20.0, 20.0),
                                egui::Sense::hover(),
                            );
                            ui.painter().circle_filled(rect.center(), 8.0, color);
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                label,
                                egui::FontId::proportional(8.0),
                                p.icon_color,
                            );
                        });
                    }

                    ui.add_space(10.0);
                    // Mirrors the real notification bell + its count
                    // badge exactly (`notifications.rs`) so a Badge
                    // Color edit shows up here identically to how it'll
                    // look on the real bell.
                    ui.vertical(|ui| {
                        ui.add_space((ROW2_HEIGHT - 24.0) / 2.0);
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            regular::BELL,
                            egui::FontId::proportional(16.0),
                            p.icon_color,
                        );
                        let badge_center = rect.right_top() + egui::vec2(-2.0, 2.0);
                        let fill_luminance = 0.299 * p.badge_color.r() as f32
                            + 0.587 * p.badge_color.g() as f32
                            + 0.114 * p.badge_color.b() as f32;
                        let badge_text_color = if fill_luminance > 140.0 {
                            egui::Color32::BLACK
                        } else {
                            egui::Color32::WHITE
                        };
                        ui.painter().circle_filled(badge_center, 8.0, p.badge_color);
                        ui.painter().text(
                            badge_center,
                            egui::Align2::CENTER_CENTER,
                            "3",
                            egui::FontId::proportional(9.0),
                            badge_text_color,
                        );
                    });
                });
            });

            ui.add_space(12.0);

            // --- Row 3: Tabs (full width) - reflects Tab Corner Radius, Tab
            // Gap, and Minimum Tab Width live, since none of these
            // previously had any visual feedback short of actually opening
            // several real tabs.
            ui.vertical(|ui| {
                preview_caption(ui, p, "Tabs");
                ui.horizontal(|ui| {
                    // Matches the real tab strip's own technique exactly
                    // (`tabs.rs`: `ui.spacing_mut().item_spacing = vec2(spacing,
                    // spacing)`) rather than layering an explicit `add_space`
                    // on top of whatever this row's ambient item spacing
                    // already was - doing both double-counted the gap, so the
                    // mockup never actually matched the real tab strip's
                    // spacing (most visible at the extremes: a Tab Gap of 0
                    // still showed a visible gap here).
                    ui.spacing_mut().item_spacing.x = tab_gap;
                    for (title, active) in
                        [("Downloads", true), ("Documents", false), ("Pictures", false)]
                    {
                        preview_tab(ui, p, title, active, min_tab_width);
                    }
                });
            });

            ui.add_space(12.0);

            // --- Row 4: Status bar (full width) ---
            ui.vertical(|ui| {
                preview_caption(ui, p, "Status Bar");
                egui::Frame::NONE
                    .fill(p.status_bar_bg_color)
                    .corner_radius(egui::CornerRadius::same(p.small_radius))
                    .inner_margin(egui::Margin::symmetric(8, 5))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(regular::HARD_DRIVES)
                                    .color(p.status_bar_icon_color)
                                    .size(12.0),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new("12 items, 3.4 GB")
                                    .color(p.status_bar_text_color)
                                    .size(11.0),
                            );
                        });
                    });
            });
        });
}

/// One label + right-aligned color-picker row inside an `egui::Grid`,
/// followed by `ui.end_row()` - the exact shape every "Core Colors" row
/// already hand-wrote, pulled out into a helper since the granular-override
/// sections below repeat it several dozen times.
fn color_row(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    i18n: &I18n,
    label_key: &str,
    color: &mut egui::Color32,
) -> bool {
    eden_text_label(ui, palette, &i18n.tr(label_key));
    let mut changed = false;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        apply_eden_visual_overrides(ui, palette);
        changed = color_picker_control(ui, color);
    });
    ui.end_row();
    changed
}

fn font_selector(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    label: &str,
    current_font: &str,
) -> Option<String> {
    let fonts = get_font_list();

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let mut selected_text = current_font.to_string();
        let mut changed = false;

        draw_dropdown(
            ui,
            palette,
            egui::Id::new(label),
            200.0,
            selected_text.clone(),
            |ui| {
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        for font in fonts.iter() {
                            let rich_text = egui::RichText::new(font.as_str());

                            if ui
                                .selectable_label(font == current_font, rich_text)
                                .clicked()
                            {
                                selected_text = font.clone();
                                changed = true;
                                ui.close();
                            }
                        }
                    });
            },
        );

        if changed { Some(selected_text) } else { None }
    })
    .inner
}
