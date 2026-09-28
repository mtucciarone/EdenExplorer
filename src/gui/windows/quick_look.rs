//! Quick Look: Space opens a large preview of the selected item over the
//! window (Finder-style), drawn with the same renderer as the preview pane
//! (`itemviewer_preview::draw_preview_content`), so every file type the
//! pane handles works here too. The file list underneath keeps its keys:
//! ↑/↓ (and the Gallery's arrows) move the selection and Quick Look
//! follows it, Enter opens the file, Space closes Quick Look again. In the
//! Details views ←/→ step through the list as well, since the list itself
//! doesn't use them.

use crate::core::utils::files::format_size;
use crate::core::utils::widgets::{eden_button, modal_frame};
use crate::gui::windows::containers::enums::ItemViewerAction;
use crate::gui::windows::containers::structs::{ItemViewerDisplayMode, SplitSide};
use crate::gui::windows::mainwindow::MainWindow;
use eframe::egui;
use egui_phosphor::regular;
use std::collections::HashSet;
use std::path::PathBuf;

pub struct QuickLookState {
    /// The pane whose selection is shown.
    pub side: SplitSide,
    /// The tab it was opened in; switching tabs closes it.
    tab: usize,
    /// Item count of the folder last shown, so it's read once.
    folder_count: Option<(PathBuf, Option<usize>)>,
    /// Where the shown item was last frame, so a single selection isn't
    /// searched for through the whole list every frame.
    last_position: Option<usize>,
}

impl QuickLookState {
    pub fn new(side: SplitSide, tab: usize) -> Self {
        Self { side, tab, folder_count: None, last_position: None }
    }
}

/// Where the selection is in the visible list: `(position among the
/// visible items, index into files)` of the first selected visible item.
pub fn target_index<'a>(
    visible: &[usize],
    paths: impl Fn(usize) -> &'a std::path::Path,
    selected: &HashSet<PathBuf>,
) -> Option<(usize, usize)> {
    if selected.is_empty() {
        return None;
    }
    visible
        .iter()
        .enumerate()
        .find(|(_, idx)| selected.contains(paths(**idx)))
        .map(|(pos, idx)| (pos, *idx))
}

/// The visible position `delta` steps from `pos`, clamped to the list.
pub fn step(len: usize, pos: usize, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    (pos as i64 + delta as i64).clamp(0, len as i64 - 1) as usize
}

fn is_preview_mode(mode: ItemViewerDisplayMode) -> bool {
    matches!(
        mode,
        ItemViewerDisplayMode::Preview | ItemViewerDisplayMode::DetailPreview | ItemViewerDisplayMode::ColumnPreview
    )
}

impl MainWindow {
    pub(crate) fn toggle_quick_look(&mut self) {
        if self.quick_look.is_some() {
            self.close_quick_look();
        } else {
            self.quick_look = Some(QuickLookState::new(self.focused_split, self.active_tab));
        }
    }

    pub(crate) fn close_quick_look(&mut self) {
        let Some(state) = self.quick_look.take() else { return };
        if state.tab != self.active_tab || state.tab >= self.tabs.len() {
            return;
        }
        // Stop a video or song Quick Look started, unless the pane's own
        // preview is showing it too.
        let view = self.active_tab_mut().view_mut(state.side);
        if !is_preview_mode(view.display_mode) {
            view.video_service = Default::default();
            view.audio_service = Default::default();
        }
    }

    pub(crate) fn draw_quick_look(&mut self, ctx: &egui::Context, palette: &crate::gui::theme::ThemePalette) {
        let Some(mut state) = self.quick_look.take() else { return };
        if state.tab != self.active_tab {
            return;
        }
        let side = state.side;

        // What to show: the first selected item in list order.
        let (target, position, total, mode) = {
            let view = self.active_tab().view(side);
            let visible = &view.item_viewer_filter_state.cached_indices;
            let selected = &view.explorer_state.selected_paths;
            let cached = state.last_position.filter(|&pos| {
                selected.len() == 1
                    && visible.get(pos).is_some_and(|&idx| idx < view.files.len() && selected.contains(&view.files[idx].path))
            });
            let found = match cached {
                Some(pos) => Some((pos, visible[pos])),
                None => target_index(visible, |i| view.files[i].path.as_path(), selected),
            };
            state.last_position = found.map(|(pos, _)| pos);
            match found {
                Some((pos, idx)) => (Some(view.files[idx].clone()), Some(pos), visible.len(), view.display_mode),
                None => (None, None, visible.len(), view.display_mode),
            }
        };
        let Some(file) = target else {
            // Nothing selected any more (navigated away, deleted, ...).
            self.quick_look = Some(state);
            self.close_quick_look();
            return;
        };

        let mut close = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let mut open = false;
        let mut move_by = 0i32;
        let details = matches!(mode, ItemViewerDisplayMode::Details | ItemViewerDisplayMode::DetailPreview);
        if details && !ctx.egui_wants_keyboard_input() {
            ctx.input(|i| {
                if i.modifiers.is_none() && i.key_pressed(egui::Key::ArrowLeft) {
                    move_by = -1;
                }
                if i.modifiers.is_none() && i.key_pressed(egui::Key::ArrowRight) {
                    move_by = 1;
                }
            });
        }

        // Dim the window; a click outside closes Quick Look.
        egui::Area::new(egui::Id::new("quick_look_scrim"))
            .order(egui::Order::Middle)
            .interactable(true)
            .show(ctx, |ui| {
                let rect = ctx.content_rect();
                ui.painter().rect_filled(rect, 0.0, palette.modal_background_effect_color);
                if ui.interact(rect, ui.id().with("click"), egui::Sense::click()).clicked() {
                    close = true;
                }
            });

        let screen = ctx.content_rect();
        let size = egui::vec2(
            (screen.width() * 0.86).clamp(360.0, 1500.0),
            (screen.height() * 0.86).clamp(300.0, 1100.0),
        );
        let i18n = &self.i18n;
        let view = self.tabs[self.active_tab].view_mut(side);
        if file.is_dir {
            if state.folder_count.as_ref().map(|(p, _)| p) != Some(&file.path) {
                let count = std::fs::read_dir(&file.path).ok().map(|d| d.count());
                state.folder_count = Some((file.path.clone(), count));
            }
        } else {
            view.preview_service.pump(ctx);
        }

        egui::Area::new(egui::Id::new("quick_look_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                modal_frame(&ctx.style_of(ctx.theme()), palette)
                    .inner_margin(egui::Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_min_size(size);
                        ui.set_max_size(size);

                        // Header: name, details, position, actions.
                        ui.horizontal(|ui| {
                            let icon = if file.is_dir { regular::FOLDER } else { regular::FILE };
                            ui.label(
                                egui::RichText::new(format!("{icon}  {}", file.name))
                                    .strong()
                                    .size(palette.text_size + 3.0)
                                    .color(palette.text_header_section),
                            );
                            let mut facts = Vec::new();
                            if file.is_dir {
                                if let Some((_, Some(count))) = &state.folder_count {
                                    facts.push(format!("{count} {}", i18n.tr("quick_look_items")));
                                }
                            } else if let Some(size) = file.file_size {
                                facts.push(format_size(size));
                            }
                            if let Some(modified) = &file.modified_time {
                                facts.push(modified.clone());
                            }
                            if let Some(pos) = position {
                                facts.push(format!("{} / {total}", pos + 1));
                            }
                            ui.label(
                                egui::RichText::new(facts.join("  ·  "))
                                    .size(palette.text_size - 1.0)
                                    .color(palette.text_normal.gamma_multiply(0.7)),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui
                                    .add(egui::Button::new(regular::X).frame(false))
                                    .on_hover_text(format!("{} (Esc / Space)", i18n.tr("close")))
                                    .clicked()
                                {
                                    close = true;
                                }
                                ui.add_space(6.0);
                                let open_label = if file.is_dir { i18n.tr("quick_look_open_folder") } else { i18n.tr("quick_look_open") };
                                if eden_button(ui, palette, &format!("{} {open_label}", regular::ARROW_SQUARE_OUT))
                                    .on_hover_text("Enter")
                                    .clicked()
                                {
                                    open = true;
                                }
                                let next = ui.add_enabled(
                                    position.is_some_and(|p| p + 1 < total),
                                    egui::Button::new(regular::CARET_RIGHT),
                                );
                                if next.on_hover_text(i18n.tr("quick_look_next")).clicked() {
                                    move_by = 1;
                                }
                                let prev = ui.add_enabled(position.is_some_and(|p| p > 0), egui::Button::new(regular::CARET_LEFT));
                                if prev.on_hover_text(i18n.tr("quick_look_previous")).clicked() {
                                    move_by = -1;
                                }
                            });
                        });
                        ui.separator();
                        ui.add_space(4.0);

                        if file.is_dir {
                            ui.add_space(size.y * 0.25);
                            ui.vertical_centered(|ui| {
                                ui.label(egui::RichText::new(regular::FOLDER).size(96.0).color(palette.icon_color));
                                ui.add_space(8.0);
                                let text = match &state.folder_count {
                                    Some((_, Some(0))) => i18n.tr("quick_look_empty_folder"),
                                    Some((_, Some(n))) => format!("{n} {}", i18n.tr("quick_look_items")),
                                    _ => i18n.tr("quick_look_folder"),
                                };
                                ui.label(egui::RichText::new(text).size(palette.text_size + 1.0).color(palette.text_normal));
                            });
                        } else {
                            crate::gui::windows::containers::itemviewer_preview::draw_preview_content(
                                ui,
                                i18n,
                                &mut view.preview_service,
                                &mut view.video_service,
                                &mut view.audio_service,
                                &mut view.find_in_preview,
                                &file.path,
                                palette,
                            );
                        }
                    });
            });

        self.quick_look = Some(state);
        if move_by != 0
            && let Some(pos) = position
        {
            let new_pos = step(total, pos, move_by);
            if new_pos != pos {
                let path = {
                    let view = self.active_tab().view(side);
                    let idx = view.item_viewer_filter_state.cached_indices[new_pos];
                    view.files[idx].path.clone()
                };
                let view = self.active_tab_mut().view_mut(side);
                view.explorer_state.selection_anchor = Some(new_pos);
                view.explorer_state.selection_focus = Some(new_pos);
                self.focused_split = side;
                crate::gui::windows::mainwindow_imp::handle_pending_actions(
                    Some(ItemViewerAction::ReplaceSelection(path)),
                    self,
                );
            }
        }
        if open {
            self.focused_split = side;
            let action = if file.is_dir {
                ItemViewerAction::Open(file.path.clone())
            } else {
                ItemViewerAction::OpenWithDefault(vec![file.path.clone()])
            };
            self.close_quick_look();
            crate::gui::windows::mainwindow_imp::handle_pending_actions(Some(action), self);
        } else if close {
            self.close_quick_look();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_is_the_first_selected_item_in_list_order() {
        let files = [PathBuf::from("a"), PathBuf::from("b"), PathBuf::from("c"), PathBuf::from("d")];
        // Sorted/filtered view showing d, b, c.
        let visible = [3, 1, 2];
        let selected: HashSet<PathBuf> = [PathBuf::from("c"), PathBuf::from("b")].into_iter().collect();
        assert_eq!(target_index(&visible, |i| files[i].as_path(), &selected), Some((1, 1)));
        let hidden: HashSet<PathBuf> = [PathBuf::from("a")].into_iter().collect();
        assert_eq!(target_index(&visible, |i| files[i].as_path(), &hidden), None);
        assert_eq!(target_index(&visible, |i| files[i].as_path(), &HashSet::new()), None);
    }

    #[test]
    fn steps_stay_inside_the_list() {
        assert_eq!(step(5, 2, 1), 3);
        assert_eq!(step(5, 4, 1), 4);
        assert_eq!(step(5, 0, -1), 0);
        assert_eq!(step(0, 0, 1), 0);
    }
}
