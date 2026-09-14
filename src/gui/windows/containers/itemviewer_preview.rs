use crate::core::audio::AudioPreviewService;
use crate::core::fs::FileItem;
use crate::core::preview::{PreviewPayload, PreviewService};
use crate::core::video::VideoPreviewService;
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::utils::truncate_item_text;
use crate::gui::windows::containers::enums::ItemViewerAction;
use crate::gui::windows::containers::itemviewer_helper::handle_editing_file_name;
use crate::gui::windows::containers::structs::{ExplorerState, FindInPreviewState, RenameState};
use chrono::{DateTime, Local};
use eframe::egui;
use egui::TextBuffer;
use egui_extras::{Size, StripBuilder};
use egui_phosphor::regular;
use std::path::{Path, PathBuf};

pub const PREVIEW_PANE_WIDTH: f32 = 560.0;
const ROW_HEIGHT: f32 = 24.0;

/// A layout with a simple file list on the left and a preview pane on the right
/// (showing the content of the selected file - text, image, PDF, or Word document).
#[allow(clippy::too_many_arguments)]
pub fn draw_preview_view(
    ui: &mut egui::Ui,
    i18n: &I18n,
    files: &[FileItem],
    filtered_indices: &[usize],
    explorer_state: &mut ExplorerState,
    preview_service: &mut PreviewService,
    video_service: &mut VideoPreviewService,
    audio_service: &mut AudioPreviewService,
    find_in_preview: &mut FindInPreviewState,
    preview_selection: &mut Option<PathBuf>,
    icon_cache: &IconCache,
    palette: &ThemePalette,
    rename_state: &mut Option<RenameState>,
) -> Option<ItemViewerAction> {
    preview_service.pump(ui.ctx());

    let mut action = None;
    let now = ui.input(|i| i.time);

    StripBuilder::new(ui)
        .size(Size::remainder())
        .size(Size::exact(PREVIEW_PANE_WIDTH))
        .horizontal(|mut strip| {
            strip.cell(|ui| {
                egui::ScrollArea::vertical()
                    .id_salt("preview_view_list")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Left padding to match every other layout's file/folder
                        // list, which otherwise sits flush against the pane's edge.
                        ui.horizontal_top(|ui| {
                            ui.add_space(8.0);
                            ui.vertical(|ui| {
                                for &idx in filtered_indices {
                                    let file = &files[idx];
                                    let is_selected =
                                        explorer_state.selected_paths.contains(&file.path);

                                    let (rect, resp) = ui.allocate_exact_size(
                                        egui::vec2(ui.available_width(), ROW_HEIGHT),
                                        egui::Sense::click(),
                                    );

                                    if is_selected {
                                        ui.painter().rect_filled(
                                            rect,
                                            egui::CornerRadius::same(palette.small_radius),
                                            palette.primary_active,
                                        );
                                    } else if resp.hovered() {
                                        ui.painter().rect_filled(
                                            rect,
                                            egui::CornerRadius::same(palette.small_radius),
                                            palette.primary_hover,
                                        );
                                    }

                                    const ICON_SIZE: f32 = 16.0;
                                    let icon_area = egui::Rect::from_min_size(
                                        rect.left_center() + egui::vec2(4.0, -ICON_SIZE * 0.5),
                                        egui::vec2(ICON_SIZE, ICON_SIZE),
                                    );
                                    let text_x = if let Some(glyph) =
                                        icon_cache.get_custom_folder_icon(&file.path, file.is_dir)
                                    {
                                        ui.painter().text(
                                            icon_area.center(),
                                            egui::Align2::CENTER_CENTER,
                                            glyph,
                                            egui::FontId::proportional(ICON_SIZE),
                                            palette.icon_colored_hover,
                                        );
                                        icon_area.right() + 6.0
                                    } else if let Some(texture) =
                                        icon_cache.get(&file.path, file.is_dir)
                                    {
                                        ui.painter().image(
                                            (&texture).into(),
                                            icon_area,
                                            egui::Rect::from_min_size(
                                                egui::pos2(0.0, 0.0),
                                                egui::vec2(1.0, 1.0),
                                            ),
                                            palette.icon_colored_hover,
                                        );
                                        icon_area.right() + 6.0
                                    } else {
                                        rect.left() + 8.0
                                    };
                                    let is_renaming = rename_state
                                        .as_ref()
                                        .map(|rs| rs.path == file.path)
                                        .unwrap_or(false);

                                    if is_renaming {
                                        let text_rect = egui::Rect::from_min_max(
                                            egui::pos2(text_x, rect.top()),
                                            rect.right_bottom(),
                                        );
                                        if let Some(rename_action) = handle_editing_file_name(
                                            ui,
                                            i18n,
                                            file,
                                            is_selected,
                                            palette,
                                            text_rect,
                                            rename_state,
                                        ) {
                                            action = Some(rename_action);
                                        }
                                        continue;
                                    }

                                    let max_text_width = (rect.right() - text_x - 4.0).max(0.0);
                                    let name_font_id =
                                        egui::FontId::proportional(palette.text_size);
                                    let text_color = ui.visuals().text_color();
                                    let (display_name, _) = truncate_item_text(
                                        ui,
                                        &file.name,
                                        max_text_width,
                                        &name_font_id,
                                        text_color,
                                    );

                                    ui.painter().text(
                                        egui::pos2(text_x, rect.center().y),
                                        egui::Align2::LEFT_CENTER,
                                        display_name,
                                        name_font_id,
                                        text_color,
                                    );

                                    if resp.hovered() {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    }

                                    if resp.clicked() {
                                        let was_sole_selected = explorer_state
                                            .selected_paths
                                            .len()
                                            == 1
                                            && explorer_state.selected_paths.contains(&file.path);

                                        explorer_state.click_to_rename_arm = None;

                                        if was_sole_selected && !resp.double_clicked() {
                                            // Slow second click on an already-selected item -
                                            // arm the rename gesture (see `poll_click_to_rename`)
                                            // instead of re-selecting.
                                            explorer_state.click_to_rename_arm =
                                                Some((file.path.clone(), now));
                                        } else {
                                            explorer_state.selected_paths.clear();
                                            explorer_state.selected_paths.insert(file.path.clone());
                                            explorer_state.selection_focus = Some(idx);
                                            explorer_state.selection_anchor = Some(idx);
                                            *preview_selection = Some(file.path.clone());
                                            if !file.is_dir {
                                                preview_service.request(&file.path);
                                            }
                                        }
                                    }

                                    if resp.double_clicked() {
                                        explorer_state.click_to_rename_arm = None;
                                        action = Some(if file.is_dir {
                                            ItemViewerAction::Open(file.path.clone())
                                        } else {
                                            ItemViewerAction::OpenWithDefault(vec![
                                                file.path.clone(),
                                            ])
                                        });
                                    }
                                }

                                if filtered_indices.is_empty() {
                                    ui.add_space(8.0);
                                    ui.weak(i18n.tr("folder_is_empty"));
                                }

                                // Bottom padding so the last item isn't
                                // flush against the pane border.
                                ui.add_space(12.0);
                            });
                        });
                    });
            });

            strip.cell(|ui| {
                let divider_rect = ui.max_rect();
                ui.painter().vline(
                    divider_rect.left(),
                    divider_rect.y_range(),
                    egui::Stroke::new(1.5, palette.borders_default),
                );
                draw_preview_pane(
                    ui,
                    i18n,
                    preview_service,
                    video_service,
                    audio_service,
                    find_in_preview,
                    preview_selection.as_deref(),
                    palette,
                );
            });
        });

    action
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PreviewPaneTab {
    Preview,
    Details,
}

/// Renders `selection` (if any) inside a small "Preview" / "Details" tab strip:
/// the Preview tab shows the file's content (text, image, or the extracted text
/// of a PDF/Word document); the Details tab shows file metadata (size,
/// dimensions for images, created/modified dates).
#[allow(clippy::too_many_arguments)]
pub fn draw_preview_pane(
    ui: &mut egui::Ui,
    i18n: &I18n,
    preview_service: &mut PreviewService,
    video_service: &mut VideoPreviewService,
    audio_service: &mut AudioPreviewService,
    find_in_preview: &mut FindInPreviewState,
    selection: Option<&Path>,
    palette: &ThemePalette,
) {
    let Some(path) = selection else {
        egui::Frame::NONE
            .fill(palette.preview_pane_bg_color)
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.weak(i18n.tr("preview_select_a_file"));
                });
            });
        return;
    };

    let tab_id = egui::Id::new("preview_pane_active_tab");
    let mut active_tab = ui
        .ctx()
        .data_mut(|d| *d.get_temp_mut_or(tab_id, PreviewPaneTab::Preview));

    egui::Frame::NONE
        .fill(palette.preview_pane_bg_color)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            // Its own small bordered box, bold + larger than body text, so
            // the previewed file's name reads as a real header rather than
            // just another line of the pane - outer margin only on the
            // top/left/bottom (not right, since it already reads as
            // anchored to the pane's own top-left corner).
            egui::Frame::NONE
                .stroke(egui::Stroke::new(1.0, palette.borders_default))
                .corner_radius(egui::CornerRadius::same(palette.small_radius))
                .inner_margin(egui::Margin {
                    left: 8,
                    right: 8,
                    top: 4,
                    bottom: 4,
                })
                .outer_margin(egui::Margin {
                    left: 4,
                    right: 0,
                    top: 4,
                    bottom: 6,
                })
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(file_name)
                            .strong()
                            .size(palette.text_size + 3.0),
                    );
                });

            ui.horizontal(|ui| {
                draw_preview_pane_tab_button(
                    ui,
                    &mut active_tab,
                    PreviewPaneTab::Preview,
                    &i18n.tr("preview_tab_preview"),
                    palette,
                );
                ui.add_space(12.0);
                draw_preview_pane_tab_button(
                    ui,
                    &mut active_tab,
                    PreviewPaneTab::Details,
                    &i18n.tr("preview_tab_details"),
                    palette,
                );
            });
            ui.separator();
            ui.add_space(8.0);

            // The selection highlight otherwise reuses the same subtle hover
            // tint as list rows/buttons - too low-contrast to notice, and
            // (since egui recolors selected glyphs to `selection.stroke.color`)
            // the wrong text color too. Use a solid, saturated fill with an
            // explicitly light/dark text color picked for contrast against it,
            // instead of another theme-derived color that might be close in
            // tone to the fill.
            ui.visuals_mut().selection.bg_fill = egui::Color32::from_rgb(
                palette.primary.r(),
                palette.primary.g(),
                palette.primary.b(),
            );
            let fill_luminance = 0.299 * palette.primary.r() as f32
                + 0.587 * palette.primary.g() as f32
                + 0.114 * palette.primary.b() as f32;
            ui.visuals_mut().selection.stroke.color = if fill_luminance > 140.0 {
                egui::Color32::BLACK
            } else {
                egui::Color32::WHITE
            };

            match active_tab {
                PreviewPaneTab::Preview => draw_preview_content(
                    ui,
                    i18n,
                    preview_service,
                    video_service,
                    audio_service,
                    find_in_preview,
                    path,
                    palette,
                ),
                PreviewPaneTab::Details => draw_preview_details(ui, i18n, preview_service, path),
            }
        });

    ui.ctx().data_mut(|d| d.insert_temp(tab_id, active_tab));
}

/// Draws a single "Preview"/"Details" tab label with an explicit, always-legible
/// text color and an underline when active - avoids relying on egui's default
/// selectable-label selection colors, which can end up low-contrast against
/// this app's custom theme.
fn draw_preview_pane_tab_button(
    ui: &mut egui::Ui,
    active_tab: &mut PreviewPaneTab,
    this_tab: PreviewPaneTab,
    label: &str,
    palette: &ThemePalette,
) {
    let is_active = *active_tab == this_tab;
    let color = if is_active {
        palette.primary
    } else {
        ui.visuals().text_color()
    };

    let resp = ui.add(
        egui::Label::new(
            egui::RichText::new(label)
                .color(color)
                .size(palette.text_size)
                .strong(),
        )
        .selectable(false)
        .sense(egui::Sense::click()),
    );

    if is_active {
        let underline_y = resp.rect.bottom() + 2.0;
        ui.painter().line_segment(
            [
                egui::pos2(resp.rect.left(), underline_y),
                egui::pos2(resp.rect.right(), underline_y),
            ],
            egui::Stroke::new(2.0, palette.primary),
        );
    }

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    if resp.clicked() {
        *active_tab = this_tab;
    }
}

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "avi", "wmv", "mkv", "webm", "flv", "mpg", "mpeg", "3gp", "3g2", "ts",
    "m2ts",
];

fn is_video_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| VIDEO_EXTENSIONS.iter().any(|v| v.eq_ignore_ascii_case(ext)))
}

const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "wav", "flac", "aac", "m4a", "wma", "ogg", "opus", "aiff", "alac",
];

fn is_audio_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| AUDIO_EXTENSIONS.iter().any(|a| a.eq_ignore_ascii_case(ext)))
}

/// Hand-painted min/max peak waveform, matching this codebase's existing
/// hand-drawn-widget style (e.g. the sidebar's drive usage bar) rather than
/// pulling in a charting dependency for one simple bar-per-bucket display.
fn draw_waveform(ui: &mut egui::Ui, size: egui::Vec2, waveform: &crate::core::audio::Waveform, playhead_fraction: Option<f32>, palette: &ThemePalette) {
    let (rect, _response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);

    if waveform.peaks.is_empty() {
        return;
    }

    let mid_y = rect.center().y;
    let half_height = rect.height() / 2.0 - 2.0;
    let bucket_count = waveform.peaks.len();
    let bar_width = (rect.width() / bucket_count as f32).max(1.0);

    for (i, (min, max)) in waveform.peaks.iter().enumerate() {
        let x = rect.left() + i as f32 * bar_width;
        let y_top = mid_y - max * half_height;
        let y_bottom = mid_y - min * half_height;
        painter.line_segment(
            [egui::pos2(x, y_top), egui::pos2(x, y_bottom.max(y_top + 1.0))],
            egui::Stroke::new(bar_width.max(1.0), palette.primary),
        );
    }

    if let Some(fraction) = playhead_fraction {
        let x = rect.left() + rect.width() * fraction.clamp(0.0, 1.0);
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(2.0, ui.visuals().strong_text_color()),
        );
    }
}

/// A small find toolbar for the Text/Markdown preview payloads: a toggle
/// button (shows/hides the rest), a query field, and Next/Prev + match count
/// once there's a query. Recomputes matches against `haystack` (the plain
/// text, or the raw Markdown source) whenever the query changes.
fn draw_find_bar(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    find_in_preview: &mut FindInPreviewState,
    haystack: &str,
) {
    // The toggle itself is a plain icon button (not inside the frame below)
    // so it reads as "open the find bar" rather than as a field inside one -
    // matches the collapsed/expanded pattern the sidebar search box already
    // uses (a bare icon until activated, then a real input area appears).
    // Only shown when the bar is closed: once active, the field's own inline
    // close (X) button (inside the bordered frame below) is the only close
    // control - having both this row's own X *and* the inline one read as
    // two redundant close buttons stacked on top of each other.
    if !find_in_preview.active {
        let toggle_resp = ui
            .add(
                egui::Button::new(
                    egui::RichText::new(regular::MAGNIFYING_GLASS)
                        .size(14.0)
                        .color(palette.icon_color),
                )
                .fill(egui::Color32::TRANSPARENT)
                .stroke(egui::Stroke::NONE),
            )
            .on_hover_text(i18n.tr("preview_find_toggle"))
            .on_hover_cursor(egui::CursorIcon::PointingHand);

        if toggle_resp.clicked() {
            find_in_preview.active = true;
            find_in_preview.focus_requested = false;
            find_in_preview.recompute(haystack);
        }
    }

    if !find_in_preview.active {
        return;
    }

    ui.add_space(6.0);

    egui::Frame::NONE
        .fill(palette.input_field_bg)
        // Thicker and fully opaque (`borders_active`) rather than a
        // translucent `borders_default` blended over this box's own fill -
        // the same low-contrast-against-itself issue diagnosed for the
        // item viewer's type-to-filter box (`itemviewer_helper.rs`), and
        // the same fix.
        .stroke(egui::Stroke::new(2.0, palette.borders_active))
        .corner_radius(egui::CornerRadius::same(palette.medium_radius))
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(regular::MAGNIFYING_GLASS)
                        .size(palette.text_size + 1.0)
                        .color(palette.icon_color),
                );
                ui.add_space(4.0);
                // An explicit, stable id (rather than relying on the
                // default auto-id, which is derived from this widget's
                // position in the draw order) so `request_focus()` below
                // reliably reclaims the same widget frame over frame -
                // matches the item viewer's own type-to-filter box, which
                // uses the same `ui.id().with(...)` pattern for the same
                // reason.
                let text_edit_id = ui.id().with("preview_find_query");
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut find_in_preview.query)
                        .id(text_edit_id)
                        .desired_width(180.0)
                        .frame(egui::Frame::NONE)
                        .hint_text(i18n.tr("preview_find_hint"))
                        .font(egui::FontId::proportional(palette.text_size)),
                );
                // Auto-focus the query field the first frame the bar is
                // open, so clicking the magnifying-glass toggle drops the
                // user straight into typing instead of requiring a second
                // click into the field - matches the item viewer's
                // type-to-filter box's own `focus_requested` pattern.
                if !find_in_preview.focus_requested {
                    resp.request_focus();
                    find_in_preview.focus_requested = true;
                }
                if resp.changed() {
                    find_in_preview.recompute(haystack);
                }

                // Inline close, to the right of the field itself - matches
                // the navbar search box's own inline close button, rather
                // than only being reachable via the toggle icon back on the
                // opposite side of the bar. Placed before the query-empty/
                // no-matches early returns below so it always renders
                // regardless of match state.
                ui.add_space(6.0);
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(regular::X)
                                .size(palette.text_size)
                                .color(palette.icon_color),
                        )
                        .frame(false),
                    )
                    .on_hover_text(i18n.tr("preview_find_toggle"))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    find_in_preview.active = false;
                    find_in_preview.focus_requested = false;
                    return;
                }

                if find_in_preview.query.is_empty() {
                    return;
                }

                ui.add_space(4.0);
                ui.add(egui::Separator::default().vertical().spacing(4.0));
                ui.add_space(4.0);

                if find_in_preview.match_count() == 0 {
                    ui.label(
                        egui::RichText::new(i18n.tr("preview_find_no_matches"))
                            .size(palette.text_size)
                            .color(palette.drive_usage_critical),
                    );
                    return;
                }

                ui.label(
                    egui::RichText::new(format!(
                        "{}/{}",
                        find_in_preview.current_match_number(),
                        find_in_preview.match_count()
                    ))
                    .size(palette.text_size)
                    .color(palette.text_normal),
                );
                ui.add_space(2.0);
                if ui
                    .add(egui::Button::new(regular::CARET_UP).frame(false))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    find_in_preview.prev_match();
                }
                if ui
                    .add(egui::Button::new(regular::CARET_DOWN).frame(false))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    find_in_preview.next_match();
                }
            });
        });
}

/// Renders `text` as a selectable-but-read-only block, backed by a real
/// `TextEdit` widget rather than a `Label` (its `TextBuffer` is `&str`,
/// whose `insert_text`/`delete_char_range` are no-ops - the standard egui
/// idiom for "selectable, but not editable"). A `Label`'s own text
/// selection has no public way to read back the current selection, which
/// is why this needs to be a `TextEdit`: `TextEditState::load` exposes the
/// cursor range directly.
///
/// Right-clicking while some text is selected copies that selection to the
/// clipboard. Both `Label` and `TextEdit` collapse the selection to the
/// click point on *any* pointer button press (including the right button)
/// before the widget itself gets a chance to react to the click - so by
/// the time a right-click is detected the selection would already be
/// gone. To work around this, the selection is read from the state as it
/// was at the *end of last frame* (before this frame's press event can
/// touch it) the moment the right-button press is seen, rather than
/// waiting for `secondary_clicked()`.
fn draw_selectable_text_with_copy(
    ui: &mut egui::Ui,
    text: &str,
    ext: &str,
    palette: &ThemePalette,
    highlight_range: Option<(usize, usize)>,
    jump_to: Option<(usize, usize)>,
) {
    let text_edit_id = ui.make_persistent_id("preview_pane_text_edit");
    let panel_rect = ui.available_rect_before_wrap();

    let secondary_just_pressed =
        ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary));

    if secondary_just_pressed
        && let Some(pointer_pos) = ui.input(|i| i.pointer.interact_pos())
        && panel_rect.contains(pointer_pos)
        && let Some(state) = egui::widgets::text_edit::TextEditState::load(ui.ctx(), text_edit_id)
        && let Some(range) = state.cursor.char_range()
    {
        let char_range = range.as_sorted_char_range();
        if char_range.start != char_range.end {
            let selected = text.char_range(char_range).to_owned();
            ui.ctx().copy_text(selected);
        }
    }

    let font_id = egui::FontId::monospace(palette.text_size);
    let widget_font_id = font_id.clone();
    let has_syntax = crate::core::syntax_highlight::has_syntax_for_extension(ext);
    let ext_owned = ext.to_string();

    // The current find-in-preview match (if any), converted to a char range
    // once up front - `paint_text_selection` (the same function egui's own
    // `TextEdit` uses to paint a *selected* range) works in char cursors,
    // not bytes. Baked directly into the galley below rather than relying
    // on `TextEdit`'s built-in selection painting, since that's gated on
    // the widget actually having keyboard focus - which this read-only
    // preview widget never does - so it would silently never show.
    let highlight_char_range = highlight_range.map(|(start_byte, end_byte)| {
        let start_char = text[..start_byte].chars().count();
        let end_char = text[..end_byte].chars().count();
        egui::text::CCursorRange::two(
            egui::text::CCursor::new(start_char),
            egui::text::CCursor::new(end_char),
        )
    });

    // `.layouter` runs at least once per frame per egui's own docs, so the
    // resulting `Arc<Galley>` is cached keyed by (text length, dark-mode,
    // highlighted range) - cheap enough to check every frame, and rebuilding
    // only happens when the previewed file, the theme, or the current find
    // match actually changes, not on every repaint. A length-only key is an
    // approximation (it can't tell two same-length edits apart), but this
    // buffer is read-only, so the text itself never changes without the
    // cache key (the enclosing `path`) changing too, which already
    // invalidates `PreviewService`'s own cache and re-triggers this whole
    // function with new `text`.
    type HighlightCache = std::sync::Arc<
        std::sync::Mutex<
            Option<(
                usize,
                bool,
                Option<egui::text::CCursorRange>,
                std::sync::Arc<egui::Galley>,
            )>,
        >,
    >;
    let cache: HighlightCache = ui.ctx().data_mut(|d| {
        d.get_temp_mut_or_insert_with(text_edit_id, || {
            std::sync::Arc::new(std::sync::Mutex::new(None))
        })
        .clone()
    });

    let mut layouter = move |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let dark_mode = ui.visuals().dark_mode;
        let text = buf.as_str();

        let mut guard = cache.lock().unwrap();
        let needs_rebuild = match guard.as_ref() {
            Some((len, cached_dark, cached_highlight, _)) => {
                *len != text.len()
                    || *cached_dark != dark_mode
                    || *cached_highlight != highlight_char_range
            }
            None => true,
        };
        if needs_rebuild {
            let mut job = if has_syntax {
                crate::core::syntax_highlight::highlighted_layout_job(
                    text,
                    &ext_owned,
                    font_id.clone(),
                    dark_mode,
                )
            } else {
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    text,
                    0.0,
                    egui::TextFormat::simple(font_id.clone(), palette_text_color(ui)),
                );
                job
            };
            job.wrap.max_width = wrap_width;
            let mut galley = ui.fonts_mut(|f| f.layout_job(job));
            if let Some(range) = highlight_char_range {
                egui::text_selection::visuals::paint_text_selection(
                    &mut galley,
                    ui.visuals(),
                    &range,
                    None,
                );
            }
            *guard = Some((text.len(), dark_mode, highlight_char_range, galley));
        } else if let Some((_, _, _, galley)) = guard.as_ref()
            && (galley.job.wrap.max_width - wrap_width).abs() > 0.5
        {
            let mut job = (*galley.job).clone();
            job.wrap.max_width = wrap_width;
            let mut new_galley = ui.fonts_mut(|f| f.layout_job(job));
            if let Some(range) = highlight_char_range {
                egui::text_selection::visuals::paint_text_selection(
                    &mut new_galley,
                    ui.visuals(),
                    &range,
                    None,
                );
            }
            *guard = Some((text.len(), dark_mode, highlight_char_range, new_galley));
        }
        guard.as_ref().unwrap().3.clone()
    };

    let mut text_ref: &str = text;
    let output = egui::TextEdit::multiline(&mut text_ref)
        .id(text_edit_id)
        .font(widget_font_id)
        .frame(egui::Frame::NONE)
        .desired_width(f32::INFINITY)
        .layouter(&mut layouter)
        .show(ui);

    if let Some((start_byte, _)) = jump_to {
        let start_char = text[..start_byte].chars().count();
        let rect = output
            .galley
            .pos_from_cursor(egui::text::CCursor::new(start_char))
            .translate(output.galley_pos.to_vec2());
        ui.scroll_to_rect(rect, Some(egui::Align::Center));
    }
}

fn palette_text_color(ui: &egui::Ui) -> egui::Color32 {
    ui.visuals().text_color()
}

/// Background color for a find-in-preview match, painted behind the matched
/// run in the Markdown preview (the Text preview instead reuses egui's own
/// `selection.bg_fill` via `paint_text_selection`, which is already
/// theme-aware) - a warm highlighter-pen yellow/amber rather than this app's
/// accent color, so a match doesn't get lost among links/headings that may
/// already use the accent color for their own styling.
fn find_highlight_bg_color(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode {
        egui::Color32::from_rgba_unmultiplied(255, 200, 0, 90)
    } else {
        egui::Color32::from_rgb(255, 235, 130)
    }
}

/// Right-click-to-copy for the *rendered* Markdown preview, which (unlike
/// the plain-text preview) is drawn as many separate selectable `Label`s -
/// one per heading/paragraph/run - so there's no single buffer to read a
/// selection range out of the way `draw_selectable_text_with_copy` does for
/// `TextEdit`. Instead this leans on egui's own built-in multi-widget label
/// selection/copy machinery (the same thing `Ctrl+C` triggers) by injecting
/// a synthetic `Copy` event the moment a right-click is seen, before any of
/// this frame's labels have rendered.
///
/// Caveat: egui collapses a label's selection to the click point on *any*
/// button press, including the right one, before that label reacts to the
/// click - so if the click lands on a label that's only *partially*
/// selected (typically the first or last one touched by the drag), that
/// label ends up contributing its *entire* text to the copy instead of
/// just the selected portion. Labels elsewhere in the selection (not under
/// the pointer) are unaffected. This mirrors the same trade-off documented
/// on `draw_selectable_text_with_copy`.
fn arm_markdown_copy_on_right_click(ui: &mut egui::Ui) {
    let panel_rect = ui.available_rect_before_wrap();
    let secondary_just_pressed =
        ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary));

    if secondary_just_pressed
        && let Some(pointer_pos) = ui.input(|i| i.pointer.interact_pos())
        && panel_rect.contains(pointer_pos)
    {
        ui.ctx().input_mut(|i| i.events.push(egui::Event::Copy));
    }
}

fn draw_preview_content(
    ui: &mut egui::Ui,
    i18n: &I18n,
    preview_service: &mut PreviewService,
    video_service: &mut VideoPreviewService,
    audio_service: &mut AudioPreviewService,
    find_in_preview: &mut FindInPreviewState,
    path: &Path,
    palette: &ThemePalette,
) {
    find_in_preview.ensure_current_path(path);

    if is_audio_extension(path) {
        audio_service.set_current(path);
        audio_service.tick(ui.ctx());

        if let Some(error) = audio_service.error() {
            ui.add_space(8.0);
            ui.colored_label(egui::Color32::from_rgb(220, 90, 90), error.to_string());
            return;
        }

        const CONTROLS_HEIGHT: f32 = 28.0;
        let avail = ui.available_size();
        let waveform_height = (avail.y - CONTROLS_HEIGHT - 6.0).max(0.0);

        let progress = audio_service.progress();
        let playhead_fraction =
            progress.map(|(current, duration)| (current / duration).clamp(0.0, 1.0) as f32);

        if let Some(waveform) = audio_service.waveform() {
            let truncated = waveform.truncated;
            draw_waveform(
                ui,
                egui::vec2(avail.x, waveform_height),
                waveform,
                playhead_fraction,
                palette,
            );
            if truncated {
                ui.weak(
                    egui::RichText::new(i18n.tr("preview_audio_truncated"))
                        .size(palette.tooltip_text_size),
                );
            }
        } else {
            ui.allocate_ui(egui::vec2(avail.x, waveform_height), |ui| {
                ui.centered_and_justified(|ui| {
                    ui.weak(i18n.tr("preview_loading"));
                });
            });
        }

        ui.add_space(6.0);

        ui.horizontal(|ui| {
            let is_paused = audio_service.is_paused();
            let icon = if is_paused {
                regular::PLAY
            } else {
                regular::PAUSE
            };
            if ui
                .add(egui::Button::new(egui::RichText::new(icon).size(16.0)))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                audio_service.toggle_play_pause();
            }

            if let Some((current, duration)) = progress {
                ui.label(
                    egui::RichText::new(format_video_time(current))
                        .monospace()
                        .size(palette.tooltip_text_size),
                );

                let mut fraction = (current / duration).clamp(0.0, 1.0) as f32;
                let slider_width = (ui.available_width() - 60.0).max(20.0);
                let slider_resp = ui.add_sized(
                    egui::vec2(slider_width, 18.0),
                    egui::Slider::new(&mut fraction, 0.0..=1.0).show_value(false),
                );
                if slider_resp.changed() {
                    audio_service.seek(fraction as f64 * duration);
                }

                ui.label(
                    egui::RichText::new(format_video_time(duration))
                        .monospace()
                        .size(palette.tooltip_text_size),
                );
            }
        });
        return;
    }

    if is_video_extension(path) {
        video_service.set_current(path);

        if let Some(error) = video_service.error() {
            ui.add_space(8.0);
            ui.colored_label(egui::Color32::from_rgb(220, 90, 90), error.to_string());
            return;
        }

        if let Some(texture) = video_service.texture_for(ui.ctx()) {
            const CONTROLS_HEIGHT: f32 = 28.0;
            let avail = ui.available_size();
            let video_area_height = (avail.y - CONTROLS_HEIGHT - 6.0).max(0.0);

            ui.allocate_ui(egui::vec2(avail.x, video_area_height), |ui| {
                let resp = ui.add(
                    egui::Image::new(&texture)
                        .max_size(ui.available_size())
                        .shrink_to_fit()
                        .sense(egui::Sense::click()),
                );
                let clicked = resp.clicked();
                resp.on_hover_text(i18n.tr("preview_video_toggle_play"))
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if clicked {
                    video_service.toggle_play_pause();
                }
            });

            ui.add_space(6.0);

            ui.horizontal(|ui| {
                let is_paused = video_service.is_paused();
                let icon = if is_paused {
                    regular::PLAY
                } else {
                    regular::PAUSE
                };
                if ui
                    .add(egui::Button::new(egui::RichText::new(icon).size(16.0)))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    video_service.toggle_play_pause();
                }

                if let Some((current, duration)) = video_service.progress() {
                    ui.label(
                        egui::RichText::new(format_video_time(current))
                            .monospace()
                            .size(palette.tooltip_text_size),
                    );

                    let mut fraction = (current / duration).clamp(0.0, 1.0) as f32;
                    let slider_width = (ui.available_width() - 60.0).max(20.0);
                    let slider_resp = ui.add_sized(
                        egui::vec2(slider_width, 18.0),
                        egui::Slider::new(&mut fraction, 0.0..=1.0).show_value(false),
                    );
                    if slider_resp.changed() {
                        video_service.seek(fraction as f64 * duration);
                    }

                    ui.label(
                        egui::RichText::new(format_video_time(duration))
                            .monospace()
                            .size(palette.tooltip_text_size),
                    );
                }
            });
        } else {
            ui.weak(i18n.tr("preview_loading"));
        }
        return;
    }

    // Image/Animated payloads can be large (an animated GIF's decoded frames
    // especially), so those are handled without cloning the payload itself -
    // `texture_for`/`animated_texture_for` read straight from the cache.
    if matches!(
        preview_service.get(path),
        Some(PreviewPayload::Image { .. })
    ) {
        if let Some(texture) = preview_service.texture_for(ui.ctx(), path) {
            let avail = ui.available_size();
            egui::ScrollArea::both()
                .id_salt("preview_pane_image")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add(egui::Image::new(&texture).max_size(avail).shrink_to_fit());
                });
        } else {
            ui.weak(i18n.tr("preview_loading"));
        }
        return;
    }

    if matches!(
        preview_service.get(path),
        Some(PreviewPayload::Animated { .. })
    ) {
        if let Some(texture) = preview_service.animated_texture_for(ui.ctx(), path) {
            let avail = ui.available_size();
            egui::ScrollArea::both()
                .id_salt("preview_pane_animated")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add(egui::Image::new(&texture).max_size(avail).shrink_to_fit());
                });
        } else {
            ui.weak(i18n.tr("preview_loading"));
        }
        return;
    }

    let payload = preview_service.get(path).cloned();

    match payload {
        Some(PreviewPayload::Text(text)) => {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            draw_find_bar(ui, i18n, palette, find_in_preview, &text);
            ui.add_space(6.0);
            let highlight_range = find_in_preview.current_match_range();
            let jump_to = find_in_preview.take_jump_target();
            egui::ScrollArea::both()
                .id_salt("preview_pane_text")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    draw_selectable_text_with_copy(
                        ui,
                        &text,
                        &ext,
                        palette,
                        highlight_range,
                        jump_to,
                    );
                });
        }
        Some(PreviewPayload::Markdown(markdown)) => {
            draw_find_bar(ui, i18n, palette, find_in_preview, &markdown);
            ui.add_space(6.0);
            let highlight_range = find_in_preview.current_match_range();
            let jump_to_byte = find_in_preview.take_jump_target().map(|(start, _)| start);
            egui::ScrollArea::both()
                .id_salt("preview_pane_markdown")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    arm_markdown_copy_on_right_click(ui);
                    draw_markdown(ui, palette, &markdown, highlight_range, jump_to_byte);
                });
        }
        Some(PreviewPayload::Image { .. }) | Some(PreviewPayload::Animated { .. }) => {
            // Handled above before this clone; unreachable in practice.
        }
        Some(PreviewPayload::Archive { entries, truncated }) => {
            egui::ScrollArea::both()
                .id_salt("preview_pane_archive")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for entry in &entries {
                        ui.horizontal(|ui| {
                            ui.add_space(entry.depth as f32 * 16.0);
                            let icon = if entry.is_dir {
                                regular::FOLDER_SIMPLE
                            } else {
                                regular::FILE
                            };
                            ui.colored_label(palette.icon_colored_hover, icon);
                            ui.label(egui::RichText::new(&entry.name).size(palette.text_size));
                            if !entry.is_dir {
                                ui.weak(
                                    egui::RichText::new(format_file_size(entry.size))
                                        .size(palette.tooltip_text_size),
                                );
                            }
                        });
                    }

                    if truncated {
                        ui.add_space(8.0);
                        ui.weak(i18n.tr("preview_archive_truncated"));
                    }
                });
        }
        Some(PreviewPayload::Unsupported(message)) => {
            ui.add_space(8.0);
            ui.weak(message);
        }
        Some(PreviewPayload::Error(message)) => {
            ui.add_space(8.0);
            ui.colored_label(egui::Color32::from_rgb(220, 90, 90), message);
        }
        None => {
            preview_service.request(path);
            ui.add_space(8.0);
            ui.weak(i18n.tr("preview_loading"));
        }
    }
}

/// Renders Markdown source (headings, bold/italic/strikethrough, inline and
/// fenced code, lists, block quotes, rules, task list checkboxes, and simple
/// tables) instead of showing it as raw text. Hand-rolled on top of
/// `pulldown-cmark` (a pure parser with no UI dependency of its own) rather
/// than a ready-made egui markdown widget, since none of those are published
/// against this app's egui version. HTML blocks, footnotes, and definition
/// lists are silently skipped - rare in the kind of README/notes-style
/// Markdown this is meant for.
fn draw_markdown(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    markdown: &str,
    highlight_range: Option<(usize, usize)>,
    jump_to_byte: Option<usize>,
) {
    use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

    let base_size = palette.text_size;
    let mono_font = egui::FontId::monospace(base_size);
    let text_color = ui.visuals().text_color();
    let strong_color = ui.visuals().strong_text_color();
    let weak_color = ui.visuals().weak_text_color();
    let heading_color = palette.text_header_section;
    let code_bg = ui.visuals().extreme_bg_color;

    let mut bold_depth = 0u32;
    let mut italic_depth = 0u32;
    let mut strike_depth = 0u32;
    let mut link_depth = 0u32;
    let mut blockquote_depth = 0u32;
    let mut heading_level: Option<HeadingLevel> = None;
    let mut code_block: Option<String> = None;
    let mut code_block_lang: Option<String> = None;
    let mut list_stack: Vec<(bool, u64)> = Vec::new();
    let mut job = egui::text::LayoutJob::default();
    let mut job_has_content = false;
    // The source byte range spanned by everything folded into `job` since
    // the last flush - used to tell whether a pending find-in-preview match
    // (a byte offset into `markdown`) falls inside the block about to be
    // flushed, so we can scroll to it. Code blocks/tables are rendered
    // outside `job` (a `Frame` and a `Grid`, not a `Label`) and deliberately
    // aren't tracked here - find-in-preview only resolves to paragraph/
    // heading/list-item/block-quote granularity, not into a fenced code
    // block or table cell.
    let mut block_range: Option<(usize, usize)> = None;

    /// Renders the accumulated `job` as a `Label` (if it has any content),
    /// and scrolls to it if `jump_to_byte` falls within the source range
    /// that went into it.
    fn flush_job(
        ui: &mut egui::Ui,
        job: &mut egui::text::LayoutJob,
        has_content: &mut bool,
        block_range: &mut Option<(usize, usize)>,
        jump_to_byte: Option<usize>,
    ) {
        let range = block_range.take();
        if *has_content {
            job.wrap.max_width = ui.available_width().max(1.0);
            let response = ui.label(job.clone());
            if let (Some(target), Some((start, end))) = (jump_to_byte, range)
                && (start..end).contains(&target)
            {
                ui.scroll_to_rect(response.rect, Some(egui::Align::Center));
            }
            *job = egui::text::LayoutJob::default();
            *has_content = false;
        }
    }

    /// Appends `text` to `job` with `format`, splitting it into up to three
    /// runs (before/matched/after) and giving the matched run a highlighted
    /// background when `highlight_range` (a byte range in the *source*
    /// Markdown) overlaps `source_range` (this `Event::Text`'s own source
    /// byte range). Falls back to one plain append whenever the overlap
    /// can't be resolved cleanly (no overlap, or `text` doesn't line up
    /// byte-for-byte with `source_range` - e.g. an HTML entity that decoded
    /// to a different length than its source spelling) rather than risk
    /// slicing at a non-char boundary.
    fn append_with_highlight(
        job: &mut egui::text::LayoutJob,
        text: &str,
        source_range: &std::ops::Range<usize>,
        format: egui::text::TextFormat,
        highlight_range: Option<(usize, usize)>,
        highlight_bg: egui::Color32,
    ) {
        if let Some((highlight_start, highlight_end)) = highlight_range
            && text.len() == source_range.len()
        {
            let overlap_start = highlight_start.max(source_range.start);
            let overlap_end = highlight_end.min(source_range.end);
            if overlap_start < overlap_end {
                let local_start = overlap_start - source_range.start;
                let local_end = overlap_end - source_range.start;
                if text.is_char_boundary(local_start) && text.is_char_boundary(local_end) {
                    if local_start > 0 {
                        job.append(&text[..local_start], 0.0, format.clone());
                    }
                    let mut highlighted = format.clone();
                    highlighted.background = highlight_bg;
                    job.append(&text[local_start..local_end], 0.0, highlighted);
                    if local_end < text.len() {
                        job.append(&text[local_end..], 0.0, format);
                    }
                    return;
                }
            }
        }
        job.append(text, 0.0, format);
    }

    let mut events = Parser::new_ext(
        markdown,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    )
    .into_offset_iter();

    while let Some((event, source_range)) = events.next() {
        block_range = Some(match block_range {
            Some((start, _)) => (start, source_range.end),
            None => (source_range.start, source_range.end),
        });

        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    flush_job(ui, &mut job, &mut job_has_content, &mut block_range, jump_to_byte);
                    ui.add_space(8.0);
                    heading_level = Some(level);
                }
                Tag::BlockQuote(_) => {
                    blockquote_depth += 1;
                    flush_job(ui, &mut job, &mut job_has_content, &mut block_range, jump_to_byte);
                }
                Tag::CodeBlock(kind) => {
                    flush_job(ui, &mut job, &mut job_has_content, &mut block_range, jump_to_byte);
                    code_block = Some(String::new());
                    code_block_lang = match kind {
                        pulldown_cmark::CodeBlockKind::Fenced(lang) if !lang.is_empty() => {
                            Some(lang.to_string())
                        }
                        _ => None,
                    };
                }
                Tag::List(start) => {
                    flush_job(ui, &mut job, &mut job_has_content, &mut block_range, jump_to_byte);
                    list_stack.push((start.is_some(), start.unwrap_or(1)));
                }
                Tag::Item => {
                    flush_job(ui, &mut job, &mut job_has_content, &mut block_range, jump_to_byte);
                    let depth = list_stack.len().saturating_sub(1) as f32;
                    let prefix = match list_stack.last_mut() {
                        Some((true, n)) => {
                            let s = format!("{n}. ");
                            *n += 1;
                            s
                        }
                        _ => "•  ".to_string(),
                    };
                    job.append(
                        &prefix,
                        depth * 18.0,
                        egui::text::TextFormat {
                            font_id: egui::FontId::proportional(base_size),
                            color: text_color,
                            ..Default::default()
                        },
                    );
                    job_has_content = true;
                }
                Tag::Emphasis => italic_depth += 1,
                Tag::Strong => bold_depth += 1,
                Tag::Strikethrough => strike_depth += 1,
                Tag::Link { .. } => link_depth += 1,
                Tag::Table(_) => {
                    flush_job(ui, &mut job, &mut job_has_content, &mut block_range, jump_to_byte);
                    draw_markdown_table(ui, &mut events, base_size, text_color, strong_color);
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Paragraph => {
                    flush_job(ui, &mut job, &mut job_has_content, &mut block_range, jump_to_byte);
                    ui.add_space(6.0);
                }
                TagEnd::Heading(_) => {
                    flush_job(ui, &mut job, &mut job_has_content, &mut block_range, jump_to_byte);
                    ui.add_space(4.0);
                    heading_level = None;
                }
                TagEnd::BlockQuote(_) => {
                    blockquote_depth = blockquote_depth.saturating_sub(1);
                    flush_job(ui, &mut job, &mut job_has_content, &mut block_range, jump_to_byte);
                    ui.add_space(4.0);
                }
                TagEnd::CodeBlock => {
                    if let Some(code) = code_block.take() {
                        let code = code.trim_end_matches('\n').to_string();
                        let lang = code_block_lang.take();

                        let mermaid_diagram = lang
                            .as_deref()
                            .filter(|l| l.eq_ignore_ascii_case("mermaid"))
                            .and_then(|_| {
                                let diagram_font = egui::FontId::proportional(base_size);
                                let measure = |text: &str, font_id: &egui::FontId| {
                                    ui.painter()
                                        .layout_no_wrap(text.to_string(), font_id.clone(), text_color)
                                        .size()
                                };
                                crate::core::mermaid::parse_and_layout(&code, &diagram_font, &measure)
                            });

                        if let Some(diagram) = mermaid_diagram {
                            let diagram_font = egui::FontId::proportional(base_size);
                            let size = crate::core::mermaid::size(&diagram);
                            let (rect, _response) =
                                ui.allocate_exact_size(size + egui::vec2(4.0, 4.0), egui::Sense::hover());
                            crate::core::mermaid::paint(
                                ui.painter(),
                                rect.min + egui::vec2(2.0, 2.0),
                                &diagram,
                                &diagram_font,
                                text_color,
                                weak_color,
                                code_bg,
                            );
                        } else {
                            egui::Frame::NONE
                                .fill(code_bg)
                                .inner_margin(egui::Margin::same(8))
                                .corner_radius(4)
                                .show(ui, |ui| {
                                    let has_syntax = lang.as_deref().is_some_and(|lang| {
                                        crate::core::syntax_highlight::has_syntax_for_extension(lang)
                                    });
                                    if let Some(lang) = lang.filter(|_| has_syntax) {
                                        let job = crate::core::syntax_highlight::highlighted_layout_job(
                                            &code,
                                            &lang,
                                            egui::FontId::monospace(base_size),
                                            ui.visuals().dark_mode,
                                        );
                                        ui.add(egui::Label::new(job).wrap());
                                    } else {
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(code).monospace().size(base_size),
                                            )
                                            .wrap(),
                                        );
                                    }
                                });
                        }
                        ui.add_space(6.0);
                        // The code block's own source range was never
                        // flushed through `flush_job` (it's drawn straight
                        // into a `Frame`, not folded into `job`) - clear it
                        // here so it doesn't get misattributed to whatever
                        // paragraph flushes next.
                        block_range = None;
                    }
                }
                TagEnd::List(_) => {
                    list_stack.pop();
                    if list_stack.is_empty() {
                        ui.add_space(4.0);
                    }
                }
                TagEnd::Item => flush_job(ui, &mut job, &mut job_has_content, &mut block_range, jump_to_byte),
                TagEnd::Emphasis => italic_depth = italic_depth.saturating_sub(1),
                TagEnd::Strong => bold_depth = bold_depth.saturating_sub(1),
                TagEnd::Strikethrough => strike_depth = strike_depth.saturating_sub(1),
                TagEnd::Link => link_depth = link_depth.saturating_sub(1),
                _ => {}
            },
            Event::Text(text) => {
                if let Some(code) = code_block.as_mut() {
                    code.push_str(&text);
                } else {
                    let mut size = base_size;
                    let mut color = if blockquote_depth > 0 {
                        weak_color
                    } else {
                        text_color
                    };
                    if bold_depth > 0 {
                        color = strong_color;
                    }
                    if let Some(level) = heading_level {
                        size = match level {
                            HeadingLevel::H1 => base_size + 10.0,
                            HeadingLevel::H2 => base_size + 7.0,
                            HeadingLevel::H3 => base_size + 5.0,
                            HeadingLevel::H4 => base_size + 3.0,
                            HeadingLevel::H5 => base_size + 1.0,
                            HeadingLevel::H6 => base_size,
                        };
                        color = heading_color;
                    }
                    if link_depth > 0 {
                        color = palette.primary;
                    }
                    let strikethrough = if strike_depth > 0 {
                        egui::Stroke::new(1.0, color)
                    } else {
                        egui::Stroke::NONE
                    };
                    append_with_highlight(
                        &mut job,
                        &text,
                        &source_range,
                        egui::text::TextFormat {
                            font_id: egui::FontId::proportional(size),
                            color,
                            italics: italic_depth > 0,
                            strikethrough,
                            ..Default::default()
                        },
                        highlight_range,
                        find_highlight_bg_color(ui),
                    );
                    job_has_content = true;
                }
            }
            Event::Code(text) => {
                job.append(
                    &text,
                    0.0,
                    egui::text::TextFormat {
                        font_id: mono_font.clone(),
                        color: text_color,
                        background: code_bg,
                        ..Default::default()
                    },
                );
                job_has_content = true;
            }
            Event::SoftBreak => {
                job.append(" ", 0.0, egui::text::TextFormat::default());
                job_has_content = true;
            }
            Event::HardBreak => flush_job(ui, &mut job, &mut job_has_content, &mut block_range, jump_to_byte),
            Event::Rule => {
                flush_job(ui, &mut job, &mut job_has_content, &mut block_range, jump_to_byte);
                ui.separator();
            }
            Event::TaskListMarker(checked) => {
                job.append(
                    if checked { "\u{2611} " } else { "\u{2610} " },
                    0.0,
                    egui::text::TextFormat {
                        font_id: egui::FontId::proportional(base_size),
                        color: text_color,
                        ..Default::default()
                    },
                );
                job_has_content = true;
            }
            _ => {}
        }
    }

    flush_job(ui, &mut job, &mut job_has_content, &mut block_range, jump_to_byte);
}

/// Renders a Markdown table (already-consumed `Tag::Table` start event) as an
/// `egui::Grid`, consuming events directly from `events` up through its
/// matching `TagEnd::Table`. Cell content is flattened to plain text (inline
/// formatting within cells, e.g. bold headers, is dropped) to keep this
/// simple.
fn draw_markdown_table(
    ui: &mut egui::Ui,
    events: &mut pulldown_cmark::OffsetIter<'_>,
    base_size: f32,
    text_color: egui::Color32,
    strong_color: egui::Color32,
) {
    use pulldown_cmark::{Event, Tag, TagEnd};

    egui::Grid::new(ui.id().with("markdown_table"))
        .striped(true)
        .spacing(egui::vec2(16.0, 4.0))
        .show(ui, |ui| {
            let mut in_head = false;
            let mut cell_text = String::new();

            for (event, _range) in events.by_ref() {
                match event {
                    Event::Start(Tag::TableHead) => in_head = true,
                    Event::End(TagEnd::TableHead) => {
                        in_head = false;
                        ui.end_row();
                    }
                    Event::Start(Tag::TableCell) => cell_text.clear(),
                    Event::End(TagEnd::TableCell) => {
                        let color = if in_head { strong_color } else { text_color };
                        ui.label(
                            egui::RichText::new(cell_text.trim().to_string())
                                .size(base_size)
                                .color(color),
                        );
                    }
                    Event::End(TagEnd::TableRow) => ui.end_row(),
                    Event::Text(t) | Event::Code(t) => cell_text.push_str(&t),
                    Event::End(TagEnd::Table) => break,
                    _ => {}
                }
            }
        });

    ui.add_space(6.0);
}

fn draw_preview_details(
    ui: &mut egui::Ui,
    i18n: &I18n,
    preview_service: &mut PreviewService,
    path: &Path,
) {
    let metadata = std::fs::metadata(path).ok();

    egui::Grid::new("preview_pane_details_grid")
        .num_columns(2)
        .spacing(egui::vec2(12.0, 6.0))
        .show(ui, |ui| {
            if let Some(metadata) = &metadata {
                ui.weak(i18n.tr("preview_details_size"));
                ui.label(format_file_size(metadata.len()));
                ui.end_row();
            }

            if let Some(PreviewPayload::Image { size, .. }) = preview_service.get(path) {
                ui.weak(i18n.tr("preview_details_dimensions"));
                ui.label(format!("{} x {}", size[0], size[1]));
                ui.end_row();
            }

            if let Some(created) = metadata.as_ref().and_then(|m| m.created().ok()) {
                ui.weak(i18n.tr("preview_details_created"));
                ui.label(format_system_time(created));
                ui.end_row();
            }

            if let Some(modified) = metadata.as_ref().and_then(|m| m.modified().ok()) {
                ui.weak(i18n.tr("preview_details_modified"));
                ui.label(format_system_time(modified));
                ui.end_row();
            }
        });

    if metadata.is_none() {
        ui.add_space(8.0);
        ui.weak(i18n.tr("preview_details_unavailable"));
    }
}

fn format_file_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;
    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }
    if unit_idx == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{size:.1} {}", UNITS[unit_idx])
    }
}

fn format_system_time(time: std::time::SystemTime) -> String {
    let dt: DateTime<Local> = time.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}

fn format_video_time(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let secs = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}
