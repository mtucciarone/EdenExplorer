//! Shared searchable icon picker, backed by the full Phosphor "regular" set
//! (`egui_phosphor::regular::ICONS`, ~1500 glyphs). Used by the Custom
//! Context Menu and Favorites settings pages so both can offer the same
//! large icon collection instead of a small hardcoded shortlist.

use crate::core::utils::widgets::apply_eden_visual_overrides;
use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use eframe::egui;
use egui::RichText;
use egui_phosphor::regular;

/// Draws the search box + scrollable glyph grid. Returns `Some(glyph)` when
/// the user clicks an icon this frame. `id_source` scopes the widget's
/// internal state (e.g. scroll position) when multiple pickers appear on
/// the same page.
pub fn draw_icon_picker(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    search: &mut String,
    id_source: impl std::hash::Hash + std::fmt::Debug,
    is_selected: impl Fn(&str) -> bool,
) -> Option<String> {
    let mut picked = None;

    ui.push_id(id_source, |ui| {
        apply_eden_visual_overrides(ui, palette);
        ui.add(
            egui::TextEdit::singleline(search)
                .hint_text(i18n.tr("icon_picker_search_hint"))
                .desired_width(240.0),
        );

        // Phosphor names are SCREAMING_SNAKE_CASE; match either form so a
        // space- or underscore-separated query both work.
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
        egui::ScrollArea::vertical()
            .id_salt("icon_picker_scroll")
            .max_height(280.0)
            // A ScrollArea otherwise shrinks to whatever vertical space is
            // left in its parent's layout budget, which shrinks the further
            // down a long settings page a picker sits (e.g. a submenu
            // item's icon field, well below its parent's). Forcing a
            // minimum keeps every picker on the page the same height.
            .min_scrolled_height(280.0)
            .show(ui, |ui| {
                if filtered.is_empty() {
                    ui.label(i18n.tr("icon_picker_no_results"));
                    return;
                }
                ui.horizontal_wrapped(|ui| {
                    for (name, glyph) in filtered {
                        let selected = is_selected(glyph);
                        let color = if selected {
                            palette.primary
                        } else {
                            ui.visuals().text_color()
                        };
                        let response = ui
                            .add(
                                egui::Button::new(RichText::new(*glyph).size(16.0).color(color))
                                    .min_size(egui::vec2(24.0, 24.0)),
                            )
                            .on_hover_text(name.to_lowercase().replace('_', " "));
                        if response.clicked() {
                            picked = Some((*glyph).to_string());
                        }
                    }
                });
            });
    });

    picked
}
