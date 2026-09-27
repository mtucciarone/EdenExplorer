//! Shared searchable icon picker, backed by the full Phosphor "regular" set
//! (`egui_phosphor::regular::ICONS`, ~1500 glyphs). Used by the Custom
//! Context Menu and Favorites settings pages so both can offer the same
//! large icon collection instead of a small hardcoded shortlist.
//!
//! Rendered as a single button showing the currently-selected glyph (or a
//! placeholder if none), which opens a popup containing the search box and
//! scrollable grid - previously that search box + a permanently-expanded
//! 280px grid were drawn inline in the form itself, for every single
//! favorite/context-menu entry on the page at once, which is what actually
//! made the page read as cluttered.

use crate::core::utils::widgets::apply_eden_visual_overrides;
use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use eframe::egui;
use egui::RichText;
use egui_phosphor::regular;

/// A button showing the current glyph (or `placeholder` if `current` is
/// `None`) that opens a popup with the search box + scrollable icon grid on
/// click. Returns `Some(glyph)` the frame the user picks one; the popup
/// closes itself immediately after a pick, so the button's own glyph is the
/// only thing left showing on the form afterward.
pub fn draw_icon_picker_button(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    search: &mut String,
    id_source: impl std::hash::Hash + std::fmt::Debug,
    current: Option<&str>,
    placeholder: &str,
) -> Option<String> {
    let mut picked = None;

    ui.push_id(id_source, |ui| {
        let button_glyph = current.unwrap_or(placeholder);
        let response = ui.add(
            egui::Button::new(RichText::new(button_glyph).size(16.0).color(palette.icon_color))
                .min_size(egui::vec2(28.0, 28.0)),
        );
        response.clone().on_hover_text(i18n.tr("icon_picker_choose_hover"));

        let popup_id = response.id.with("icon_picker_popup");
        egui::containers::Popup::from_toggle_button_response(&response)
            .id(popup_id)
            .close_behavior(egui::containers::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                apply_eden_visual_overrides(ui, palette);
                ui.set_width(280.0);
                ui.add(
                    egui::TextEdit::singleline(search)
                        .hint_text(i18n.tr("icon_picker_search_hint"))
                        .desired_width(240.0),
                );

                // Phosphor names are SCREAMING_SNAKE_CASE; match either form
                // so a space- or underscore-separated query both work.
                let query = search.trim().to_lowercase().replace(' ', "_");
                let filtered: Vec<&(&str, &str)> = if query.is_empty() {
                    regular::ICONS.iter().collect()
                } else {
                    regular::ICONS
                        .iter()
                        .filter(|(name, _)| name.to_lowercase().contains(&query))
                        .collect()
                };

                ui.add_space(4.0);
                // `ScrollArea::max_height` is a cap, not a guarantee - it
                // computes `available_outer.size().at_most(max_size)`, so if
                // the popup happens to open with less than 280px of real
                // screen room below it (which depends entirely on where on
                // screen the toggle button that opened it sits), the grid
                // silently renders shorter there than it does for a button
                // positioned higher up - exactly why this picker's own list
                // showed a visibly different height in Send To vs Custom
                // Context Menu despite both calling this same function.
                // Reserving the height explicitly first (same fix already
                // used for the notifications bell panel, see CLAUDE.md's
                // entry on it) makes `available_outer` unable to under-report
                // space, so the grid renders at a consistent height
                // everywhere this picker is opened from.
                ui.set_min_height(280.0);
                egui::ScrollArea::vertical()
                    .id_salt("icon_picker_scroll")
                    .max_height(280.0)
                    .show(ui, |ui| {
                        if filtered.is_empty() {
                            ui.label(i18n.tr("icon_picker_no_results"));
                            return;
                        }
                        ui.horizontal_wrapped(|ui| {
                            for (name, glyph) in filtered {
                                let selected = current == Some(*glyph);
                                let color = if selected {
                                    palette.primary
                                } else {
                                    ui.visuals().text_color()
                                };
                                let glyph_response = ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(*glyph).size(16.0).color(color),
                                        )
                                        .min_size(egui::vec2(24.0, 24.0)),
                                    )
                                    .on_hover_text(name.to_lowercase().replace('_', " "));
                                if glyph_response.clicked() {
                                    picked = Some((*glyph).to_string());
                                    ui.close();
                                }
                            }
                        });
                    });
            });
    });

    picked
}
