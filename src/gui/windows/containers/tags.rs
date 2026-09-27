use crate::core::fs::{DateStyle, FileItem, filetime_to_string};
use crate::core::indexer::TagIconStyle;
use crate::core::utils::widgets::{eden_text_label, eden_toggle_button};
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::utils::{
    clickable_icon, draw_object_drag_ghost, format_size, get_file_type_name,
    ghost_dialog_button, primary_dialog_button, rgba_color_edit_button,
};
use crate::gui::windows::containers::enums::{ItemViewerAction, ItemViewerContextAction};
use crate::gui::windows::containers::sidebar::draw_sidebar_item;
use crate::gui::windows::containers::structs::{
    TagColumn, TagColumnFitRequest, TagColumnState, TagsState,
};
use crate::gui::windows::settings::{
    master_detail_column_size, no_selection_hint, reorder_buttons, setting_label, setting_row,
    settings_section,
};
use crate::gui::windows::structs::SettingsWindow;
use eframe::egui;
use egui::FontId;
use egui::ScrollArea;
use egui::containers::{Popup, PopupCloseBehavior};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use std::collections::HashMap;
use std::path::PathBuf;

pub fn draw_tags(
    ui: &mut egui::Ui,
    i18n: &I18n,
    icon_cache: &IconCache,
    palette: &ThemePalette,
    tags_state: &mut TagsState,
    settings: &mut SettingsWindow,
) -> bool {
    let mut changed = false;
    let mut drag_state = tags_state.drag_state.take();
    let mut delete_confirmation = tags_state.delete_confirmation.take();
    let pointer_pos = ui.ctx().input(|input| input.pointer.hover_pos());
    let pointer_released = ui.ctx().input(|input| input.pointer.primary_released());
    let groups_len = tags_state.groups.len();

    setting_label(
        ui,
        &i18n.tr("settings_tags"),
        Some((&i18n.tr("tooltip_settings_tags"), palette)),
        palette,
    );
    ui.add_space(8.0);

    eden_text_label(ui, palette, &i18n.tr("tag_icon_style"));
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if eden_toggle_button(
            ui,
            palette,
            settings.current_settings.tag_icon_style == TagIconStyle::Filled,
            &i18n.tr("tag_icon_style_filled"),
        )
        .clicked()
        {
            settings.current_settings.tag_icon_style = TagIconStyle::Filled;
            crate::core::indexer::save_tag_icon_style(settings.current_settings.tag_icon_style);
            changed = true;
        }
        ui.add_space(6.0);
        if eden_toggle_button(
            ui,
            palette,
            settings.current_settings.tag_icon_style == TagIconStyle::Outline,
            &i18n.tr("tag_icon_style_outline"),
        )
        .clicked()
        {
            settings.current_settings.tag_icon_style = TagIconStyle::Outline;
            crate::core::indexer::save_tag_icon_style(settings.current_settings.tag_icon_style);
            changed = true;
        }
    });
    ui.add_space(10.0);

    if tags_state.groups.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(i18n.tr("tag_empty_state"));
        });
        tags_state.drag_state = drag_state;
        tags_state.delete_confirmation = delete_confirmation;
        return changed;
    }

    // Keep a valid selection - default to the first group once there's
    // anything to select, and drop a selection pointing at a group deleted
    // below (or from another session's import).
    if settings.selected_tag_group_id.is_none()
        || !tags_state
            .groups
            .iter()
            .any(|g| Some(g.id) == settings.selected_tag_group_id)
    {
        settings.selected_tag_group_id = tags_state.groups.first().map(|g| g.id);
    }

    let mut group_reorder: Option<(usize, usize)> = None;

    let (col_w, col_h) = master_detail_column_size(ui);
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(col_w, col_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
            ScrollArea::vertical()
                .id_salt("tags_list_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for group_index in 0..groups_len {
                        let group = &tags_state.groups[group_index];
                        let group_id = group.id;
                        let group_name = if group.name.is_empty() {
                            i18n.tr("tag_group_untitled")
                        } else {
                            group.name.clone()
                        };
                        let group_color = group.color;
                        let item_count = group.items.len();
                        let is_selected = Some(group_id) == settings.selected_tag_group_id;

                        let (tag_glyph, tag_family) = crate::core::utils::widgets::tag_glyph(
                            settings.current_settings.tag_icon_style,
                        );
                        let row = list_row(ui, palette, is_selected, |ui| {
                            // `.selectable(false)` on both - a plain `ui.label`
                            // is selectable text by default in this app's
                            // style, which registers its own click-and-drag
                            // sense *after* the row's own background sense
                            // (`list_row`'s `allocate_exact_size` call) - per
                            // egui's later-registered-sense-wins rule (already
                            // relied on deliberately for this row's own
                            // reorder/delete buttons), an unselectable label
                            // would otherwise silently steal a click meant to
                            // select the row instead of the text.
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(tag_glyph)
                                        .family(tag_family.clone())
                                        .size(palette.text_size + 3.0)
                                        .color(group_color),
                                )
                                .selectable(false),
                            );
                            ui.add(
                                egui::Label::new(egui::RichText::new(&group_name).strong())
                                    .selectable(false),
                            );

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if let Some(swap) =
                                    reorder_buttons(ui, palette, group_index, groups_len)
                                {
                                    group_reorder = Some(swap);
                                }
                                if clickable_icon(ui, regular::TRASH, palette)
                                    .on_hover_text(i18n.tr("tag_delete_group"))
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    delete_confirmation = Some(group_id);
                                }
                                egui::Frame::NONE
                                    .fill(group_color.linear_multiply(0.18))
                                    // A corner radius past the badge's own
                                    // half-height (egui clamps it down to the
                                    // actual max it can draw) reads as a
                                    // circle for a single digit and a pill
                                    // for two-plus - matches the circular
                                    // badge used for Custom Context Menu/Tab
                                    // Groups' own counts (`count_badge` in
                                    // `settings.rs`), just keeping this
                                    // badge's own per-group tag color instead
                                    // of the shared `badge_color` field.
                                    .corner_radius(egui::CornerRadius::same(255))
                                    .inner_margin(egui::Margin::symmetric(7, 2))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(item_count.to_string())
                                                .size(palette.text_size - 1.0)
                                                .color(group_color),
                                        );
                                    });
                            });
                        });

                        if row.clicked() {
                            settings.selected_tag_group_id = Some(group_id);
                        }
                    }
                });
            },
        );
        ui.allocate_ui_with_layout(
            egui::vec2(col_w, col_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
            ScrollArea::vertical()
                .id_salt("tags_detail_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let Some(selected_id) = settings.selected_tag_group_id else {
                        no_selection_hint(ui, palette, regular::TAG, &i18n.tr("tag_select_hint"));
                        return;
                    };
                    let Some(group_index) =
                        tags_state.groups.iter().position(|g| g.id == selected_id)
                    else {
                        return;
                    };

                    let group_items = tags_state.groups[group_index].items.clone();
                    let drag_source_index = drag_state
                        .as_ref()
                        .filter(|drag| drag.group_id == selected_id && drag.active)
                        .map(|drag| drag.source_index);
                    let mut drop_index: Option<usize> = None;
                    let mut should_clear_drag = false;

                    // Pre-compute is_tagged status for all items to avoid borrow conflict
                    let item_tagged_status: std::collections::HashMap<std::path::PathBuf, bool> =
                        group_items
                            .iter()
                            .map(|path| (path.clone(), tags_state.is_tagged(path)))
                            .collect();

                    settings_section(ui, palette, |ui| {
                        let group = &mut tags_state.groups[group_index];
                        setting_row(
                            ui,
                            |ui| {
                                setting_label(ui, &i18n.tr("tag_group_name"), None, palette);
                            },
                            |ui| {
                                crate::core::utils::widgets::apply_eden_visual_overrides(ui, palette);
                                changed |= ui
                                    .add_sized(
                                        [260.0, ui.spacing().interact_size.y],
                                        egui::TextEdit::singleline(&mut group.name),
                                    )
                                    .changed();
                            },
                        );
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            setting_label(ui, &i18n.tr("tag_group_color"), None, palette);
                            if rgba_color_edit_button(ui, &mut group.color).changed() {
                                changed = true;
                            }
                        });
                    });

                    ui.add_space(6.0);

                    settings_section(ui, palette, |ui| {
                        if group_items.is_empty() {
                            ui.label(
                                egui::RichText::new(i18n.tr("tag_empty_group"))
                                    .size(palette.text_size)
                                    .color(palette.text_normal),
                            );
                            return;
                        }

                        // Applied once, before either loop below - both the
                        // drag-rect precomputation pass and the real draw
                        // pass allocate items sequentially via the same `ui`,
                        // so setting this here keeps the *visible* row gap
                        // and the *rects `compute_drop_index` reasons about*
                        // in agreement (both come from the same ambient
                        // `item_spacing.y` egui adds between allocations).
                        ui.spacing_mut().item_spacing.y = 6.0;

                        let drag_is_active = drag_source_index.is_some();
                        let mut item_rects = Vec::with_capacity(group_items.len());
                        let mut item_responses = Vec::with_capacity(group_items.len());

                        if drag_is_active {
                            for _ in &group_items {
                                let (rect, resp) = tag_item_layout(ui);
                                item_rects.push(rect);
                                item_responses.push(resp);
                            }

                            if let Some(drag_source_index) = drag_source_index {
                                if let Some(pointer) = pointer_pos {
                                    drop_index = compute_drop_index(
                                        &item_rects,
                                        pointer.y,
                                        drag_source_index,
                                    );
                                }
                            }
                        }

                        for (item_index, path) in group_items.iter().enumerate() {
                            let label = tag_item_label(path);
                            let is_dir = path.is_dir();
                            let resp = if drag_is_active {
                                let rect = item_rects[item_index];
                                let item_resp = item_responses[item_index].clone();
                                draw_sidebar_item(
                                    ui,
                                    icon_cache,
                                    path,
                                    &label,
                                    is_dir,
                                    false,
                                    palette,
                                    true,
                                    Some((rect, item_resp)),
                                )
                            } else {
                                draw_sidebar_item(
                                    ui,
                                    icon_cache,
                                    path,
                                    &label,
                                    is_dir,
                                    false,
                                    palette,
                                    true,
                                    None,
                                )
                            };

                            if drag_state.is_none() && resp.drag_started() {
                                drag_state = Some(crate::gui::windows::containers::structs::TagDragState {
                                    group_id: selected_id,
                                    source_index: item_index,
                                    active: true,
                                });
                            }

                            if resp.clicked() && drag_state.is_none() {
                                let file_item = FileItem {
                                    name: label.clone(),
                                    path: path.clone(),
                                    is_dir,
                                    is_hidden: false,
                                    recycle_bin_pidl: None,
                                    file_size: None,
                                    modified_time: None,
                                    created_time: None,
                                    deleted_time: None,
                                    modified_time_raw: None,
                                    created_time_raw: None,
                                    deleted_time_raw: None,
                                    original_directory: None,
                                    total_space: None,
                                    free_space: None,
                                };
                                if let Some(action) = handle_row_click(&file_item) {
                                    tags_state.pending_action = Some(action);
                                }
                            }

                            let is_tagged = item_tagged_status.get(path).copied().unwrap_or(false);
                            Popup::context_menu(&resp)
                                .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                                .show(|ui| {
                                    let file_item = FileItem {
                                        name: label.clone(),
                                        path: path.clone(),
                                        is_dir,
                                        is_hidden: false,
                                        recycle_bin_pidl: None,
                                        file_size: None,
                                        modified_time: None,
                                        created_time: None,
                                        deleted_time: None,
                                        modified_time_raw: None,
                                        created_time_raw: None,
                                        deleted_time_raw: None,
                                        original_directory: None,
                                        total_space: None,
                                        free_space: None,
                                    };
                                    let mut action = None;
                                    handle_context_menu_actions_tags(
                                        ui,
                                        i18n,
                                        &file_item,
                                        &mut action,
                                        palette,
                                        is_tagged,
                                        Some(selected_id),
                                    );
                                    if let Some(a) = action {
                                        tags_state.pending_action = Some(a);
                                    }
                                });
                        }

                        if let Some(drag_source_index) = drag_source_index {
                            if let Some(drop) = drop_index {
                                if drop < item_rects.len() {
                                    let rect = item_rects[drop];
                                    draw_insert_line(
                                        ui,
                                        palette,
                                        rect.top(),
                                        rect.left(),
                                        rect.right(),
                                    );
                                } else if let Some(last) = item_rects.last().copied() {
                                    draw_insert_line(
                                        ui,
                                        palette,
                                        last.bottom(),
                                        last.left(),
                                        last.right(),
                                    );
                                }
                            }

                            if pointer_released {
                                if let Some(drop) = drop_index {
                                    let group = &mut tags_state.groups[group_index];
                                    if drag_source_index < group.items.len()
                                        && drag_source_index != drop
                                    {
                                        let item = group.items.remove(drag_source_index);
                                        let mut target = drop;

                                        if drop > drag_source_index {
                                            target -= 1;
                                        }

                                        target = target.min(group.items.len());
                                        group.items.insert(target, item);
                                        changed = true;
                                    }
                                }

                                should_clear_drag = true;
                            }

                            if let Some(label_path) = group_items.get(drag_source_index) {
                                draw_object_drag_ghost(
                                    ui,
                                    palette,
                                    &tag_item_label(label_path),
                                    true,
                                );
                            }
                        }
                    });

                    if should_clear_drag {
                        drag_state = None;
                    }
                });
            },
        );
    });

    if let Some((from, to)) = group_reorder {
        tags_state.groups.swap(from, to);
        changed = true;
    }

    if pointer_released && drag_state.is_some() {
        drag_state = None;
    }

    tags_state.drag_state = drag_state;
    tags_state.delete_confirmation = delete_confirmation;

    if changed {
        crate::core::indexer::save_tags(&tags_state.to_snapshot());
    }

    changed
}

/// One selectable row in a master-detail page's left-column list - a
/// smaller nested card, like `entry_card`, but click-sensitive and with a
/// distinct fill/border when selected.
///
/// Senses the row's own background click *before* drawing `add_contents`
/// (its buttons included), rather than after - egui gives click priority to
/// whichever overlapping sense was registered later, so sensing the
/// background first and drawing (and thus sensing) the buttons afterward is
/// what lets a reorder/delete button inside the row still work, instead of
/// the row-select swallowing every click in its rect including the ones
/// meant for a button on top of it.
fn list_row(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    selected: bool,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    // Pin this row's own automatic between-widget spacing so the gap between
    // rows is fully controlled by the trailing `add_space` below, regardless
    // of whatever ambient item_spacing this page's earlier buttons left the
    // `ui` in - see the identical comment in `context_menu_settings_ui.rs`'s
    // `list_row` for why that ambient state isn't reliable across pages that
    // otherwise share this exact helper.
    ui.spacing_mut().item_spacing.y = 0.0;

    let row_height = ui.spacing().interact_size.y + 20.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_height),
        egui::Sense::click(),
    );

    if ui.is_rect_visible(rect) {
        let fill = if selected {
            palette.primary_hover
        } else {
            palette.row_bg
        };
        let stroke_color = if selected {
            palette.borders_active
        } else {
            palette.borders_default
        };
        let corner = egui::CornerRadius::same(palette.small_radius);
        ui.painter().rect_filled(rect, corner, fill);
        ui.painter()
            .rect_stroke(rect, corner, egui::Stroke::new(1.5, stroke_color), egui::StrokeKind::Inside);
    }

    let content_rect = rect.shrink2(egui::vec2(10.0, 6.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(content_rect), |ui| {
        ui.horizontal_centered(|ui| add_contents(ui));
    });

    ui.add_space(8.0);

    response
}

/// Converts a filesystem `SystemTime` into the Windows FILETIME representation
/// `filetime_to_string` expects, so tag-view rows can reuse the same
/// date/time formatting as the rest of the app.
fn system_time_to_filetime(time: std::time::SystemTime) -> i64 {
    match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(dur) => {
            (dur.as_secs() as i64 + 11_644_473_600) * 10_000_000 + (dur.subsec_nanos() as i64 / 100)
        }
        Err(err) => {
            let dur = err.duration();
            (11_644_473_600 - dur.as_secs() as i64) * 10_000_000
        }
    }
}

struct TagViewRow {
    path: PathBuf,
    name: String,
    is_dir: bool,
    type_name: String,
    location: String,
    size: Option<u64>,
    modified: Option<String>,
}

fn measure_text_width(ui: &mut egui::Ui, text: &str, font_id: &FontId) -> f32 {
    ui.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(text.to_owned(), font_id.clone(), egui::Color32::WHITE)
            .size()
            .x
    })
}

const TAG_HEADER_TOP_PADDING: f32 = 6.0;

fn tag_column_min_width(column: TagColumn) -> f32 {
    match column {
        TagColumn::Name => 160.0,
        TagColumn::Type => 70.0,
        TagColumn::Location => 150.0,
        TagColumn::Size => 60.0,
        TagColumn::Modified => 100.0,
    }
}

/// Content-based width for every tag-view column, computed fresh from
/// `rows` whenever a fit is requested (mirrors
/// `compute_item_viewer_column_widths` for the regular folder/recycle-bin
/// table).
fn compute_tag_column_widths(
    ui: &mut egui::Ui,
    i18n: &I18n,
    rows: &[TagViewRow],
    font_id: &FontId,
) -> [f32; 5] {
    let icon_padding = 22.0;
    let mut widths = [
        measure_text_width(ui, &i18n.tr(TagColumn::Name.i18n_key()), font_id),
        measure_text_width(ui, &i18n.tr(TagColumn::Type.i18n_key()), font_id),
        measure_text_width(ui, &i18n.tr(TagColumn::Location.i18n_key()), font_id),
        measure_text_width(ui, &i18n.tr(TagColumn::Size.i18n_key()), font_id),
        measure_text_width(ui, &i18n.tr(TagColumn::Modified.i18n_key()), font_id),
    ];

    for row in rows {
        widths[TagColumn::Name.index()] = widths[TagColumn::Name.index()]
            .max(measure_text_width(ui, &row.name, font_id) + icon_padding);
        widths[TagColumn::Type.index()] =
            widths[TagColumn::Type.index()].max(measure_text_width(ui, &row.type_name, font_id));
        widths[TagColumn::Location.index()] = widths[TagColumn::Location.index()]
            .max(measure_text_width(ui, &row.location, font_id));
        let size_text = row.size.map(format_size).unwrap_or_default();
        widths[TagColumn::Size.index()] =
            widths[TagColumn::Size.index()].max(measure_text_width(ui, &size_text, font_id));
        widths[TagColumn::Modified.index()] = widths[TagColumn::Modified.index()].max(
            measure_text_width(ui, row.modified.as_deref().unwrap_or(""), font_id),
        );
    }

    for column in [
        TagColumn::Name,
        TagColumn::Type,
        TagColumn::Location,
        TagColumn::Size,
        TagColumn::Modified,
    ] {
        widths[column.index()] = widths[column.index()].max(tag_column_min_width(column));
    }

    widths
}

fn draw_tag_header_context_menu(
    ui: &mut egui::Ui,
    i18n: &I18n,
    clicked_column: TagColumn,
    column_state: &mut TagColumnState,
    order_index: Option<usize>,
    order_len: usize,
) {
    if ui
        .button(i18n.tr("itemviewer_size_column_to_fit"))
        .clicked()
    {
        column_state.pending_fit_request = Some(TagColumnFitRequest::Column(clicked_column));
        column_state.layout_generation = column_state.layout_generation.wrapping_add(1);
        ui.close();
    }

    if ui
        .button(i18n.tr("itemviewer_size_all_columns_to_fit"))
        .clicked()
    {
        column_state.pending_fit_request = Some(TagColumnFitRequest::All);
        column_state.layout_generation = column_state.layout_generation.wrapping_add(1);
        ui.close();
    }

    if clicked_column != TagColumn::Name {
        ui.separator();

        let can_move_left = order_index.is_some_and(|idx| idx > 0);
        let can_move_right = order_index.is_some_and(|idx| idx + 1 < order_len);

        if ui
            .add_enabled(can_move_left, egui::Button::new("Move left"))
            .clicked()
        {
            column_state.move_left(clicked_column);
            column_state.layout_generation = column_state.layout_generation.wrapping_add(1);
            ui.close();
        }

        if ui
            .add_enabled(can_move_right, egui::Button::new("Move right"))
            .clicked()
        {
            column_state.move_right(clicked_column);
            column_state.layout_generation = column_state.layout_generation.wrapping_add(1);
            ui.close();
        }

        if ui
            .add_enabled(can_move_left, egui::Button::new("Move to start"))
            .clicked()
        {
            column_state.move_to_start(clicked_column);
            column_state.layout_generation = column_state.layout_generation.wrapping_add(1);
            ui.close();
        }

        if ui
            .add_enabled(can_move_right, egui::Button::new("Move to end"))
            .clicked()
        {
            column_state.move_to_end(clicked_column);
            column_state.layout_generation = column_state.layout_generation.wrapping_add(1);
            ui.close();
        }
    }
}

/// Renders the virtual "tagged items" list for one tag group: every file/
/// folder tagged with it, shown as its own table (name, type, location,
/// size, modified) rather than a real filesystem directory listing.
#[allow(clippy::too_many_arguments)]
pub fn draw_tag_view(
    ui: &mut egui::Ui,
    i18n: &I18n,
    icon_cache: &IconCache,
    palette: &ThemePalette,
    tags_state: &mut TagsState,
    group_id: u64,
    file_type_cache: &mut HashMap<String, String>,
    date_style: DateStyle,
    time_format_24h: bool,
    custom_date_format: &str,
) -> Option<ItemViewerAction> {
    let mut action = None;

    let Some(group) = tags_state.groups.iter().find(|g| g.id == group_id) else {
        ui.centered_and_justified(|ui| {
            ui.label(i18n.tr("tag_empty_state"));
        });
        return None;
    };

    if group.items.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(i18n.tr("tag_empty_group"));
        });
        return None;
    }

    let rows: Vec<TagViewRow> = group
        .items
        .iter()
        .map(|path| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string());
            let location = path
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            let metadata = std::fs::metadata(path).ok();
            let is_dir = metadata
                .as_ref()
                .map(|m| m.is_dir())
                .unwrap_or_else(|| path.is_dir());
            let type_name = if is_dir {
                "Folder".to_string()
            } else {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                get_file_type_name(ext, file_type_cache).to_string()
            };
            let size = metadata.as_ref().filter(|_| !is_dir).map(|m| m.len());
            let modified = metadata.as_ref().and_then(|m| m.modified().ok()).and_then(|t| {
                filetime_to_string(
                    system_time_to_filetime(t),
                    date_style,
                    time_format_24h,
                    custom_date_format,
                )
            });

            TagViewRow {
                path: path.clone(),
                name,
                is_dir,
                type_name,
                location,
                size,
                modified,
            }
        })
        .collect();

    let row_height = 22.0;
    let header_height = row_height + TAG_HEADER_TOP_PADDING * 2.0;
    let font_id = egui::FontId::proportional(palette.text_size);

    let column_state = &mut tags_state.column_state;

    let mut fit_request = column_state.pending_fit_request.take();

    // Auto-fit on the very first render of this (freshly opened) group's
    // table, same "fit once, then leave the user's manual resizes alone"
    // behavior as the regular folder/recycle-bin table.
    if fit_request.is_none() && !column_state.auto_fit_checked {
        column_state.auto_fit_checked = true;
        let sizes_are_default = TagColumnState::default().order == column_state.order
            && [
                TagColumn::Name,
                TagColumn::Type,
                TagColumn::Location,
                TagColumn::Size,
                TagColumn::Modified,
            ]
            .iter()
            .all(|c| column_state.width(*c) == TagColumnState::default().width(*c));

        if sizes_are_default {
            fit_request = Some(TagColumnFitRequest::All);
        }
    }

    if let Some(fit_request) = fit_request {
        let widths = compute_tag_column_widths(ui, i18n, &rows, &font_id);

        match fit_request {
            TagColumnFitRequest::All => {
                for column in [
                    TagColumn::Name,
                    TagColumn::Type,
                    TagColumn::Location,
                    TagColumn::Size,
                    TagColumn::Modified,
                ] {
                    column_state.set_width(column, widths[column.index()]);
                }
            }
            TagColumnFitRequest::Column(column) => {
                column_state.set_width(column, widths[column.index()]);
            }
        }

        column_state.layout_generation = column_state.layout_generation.wrapping_add(1);
    }

    let order = column_state.order.clone();
    let table_id_salt = egui::Id::new(("tag_view_table", group_id, column_state.layout_generation));

    // Same left inset as the regular folder/recycle-bin table (see
    // `left_margin` in itemviewer.rs), so the header/row content lines up
    // consistently across every view.
    const LEFT_MARGIN: f32 = 8.0;
    let table_rect = ui.available_rect_before_wrap();
    let table_rect = egui::Rect::from_min_max(
        egui::pos2(table_rect.left() + LEFT_MARGIN, table_rect.top()),
        table_rect.right_bottom(),
    );

    ui.scope_builder(egui::UiBuilder::new().max_rect(table_rect), |ui| {
    ui.push_id(table_id_salt, |ui| {
        let mut table = TableBuilder::new(ui)
            .striped(false)
            .resizable(true)
            .vscroll(true)
            .sense(egui::Sense::click());

        for column in &order {
            table = table.column(
                Column::initial(tags_state.column_state.width(*column))
                    .at_least(tag_column_min_width(*column))
                    .resizable(true),
            );
        }

        table
            .header(header_height, |mut header| {
                for column in order.clone() {
                    header.col(|ui| {
                        ui.add_space(TAG_HEADER_TOP_PADDING);
                        let cell_id = ui.id().with(("tag_header_cell", column));
                        let cell_resp = ui.interact(ui.max_rect(), cell_id, egui::Sense::click());

                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(i18n.tr(column.i18n_key()))
                                    .size(palette.text_size)
                                    .color(palette.text_header_section),
                            )
                            .selectable(false),
                        );

                        if cell_resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
                        }

                        let order_index = order.iter().position(|c| *c == column);

                        Popup::context_menu(&cell_resp)
                            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                draw_tag_header_context_menu(
                                    ui,
                                    i18n,
                                    column,
                                    &mut tags_state.column_state,
                                    order_index,
                                    order.len(),
                                );
                            });
                    });
                }
            })
            .body(|body| {
                body.rows(row_height, rows.len(), |mut row| {
                    let item = &rows[row.index()];

                    for column in &order {
                        row.col(|ui| match column {
                            TagColumn::Name => {
                                ui.horizontal(|ui| {
                                    if let Some(icon) = icon_cache.get(&item.path, item.is_dir) {
                                        ui.add(
                                            egui::Image::new(&icon)
                                                .fit_to_exact_size(egui::vec2(16.0, 16.0)),
                                        );
                                    }
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&item.name).size(palette.text_size),
                                        )
                                        .selectable(false),
                                    );
                                });
                            }
                            TagColumn::Type => {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&item.type_name).size(palette.text_size),
                                    )
                                    .selectable(false),
                                );
                            }
                            TagColumn::Location => {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&item.location).size(palette.text_size),
                                    )
                                    .selectable(false),
                                )
                                .on_hover_text(&item.location);
                            }
                            TagColumn::Size => {
                                let text = item.size.map(format_size).unwrap_or_default();
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(text).size(palette.text_size),
                                    )
                                    .selectable(false),
                                );
                            }
                            TagColumn::Modified => {
                                let text = item.modified.clone().unwrap_or_default();
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(text).size(palette.text_size),
                                    )
                                    .selectable(false),
                                );
                            }
                        });
                    }

                    let resp = row.response();
                    if resp.double_clicked() {
                        action = Some(if item.is_dir {
                            ItemViewerAction::Open(item.path.clone())
                        } else {
                            ItemViewerAction::OpenWithDefault(vec![item.path.clone()])
                        });
                    }

                    let is_tagged = tags_state.is_tagged(&item.path);
                    Popup::context_menu(&resp)
                        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            let file_item = FileItem {
                                name: item.name.clone(),
                                path: item.path.clone(),
                                is_dir: item.is_dir,
                                is_hidden: false,
                                recycle_bin_pidl: None,
                                file_size: item.size,
                                modified_time: item.modified.clone(),
                                created_time: None,
                                deleted_time: None,
                                modified_time_raw: None,
                                created_time_raw: None,
                                deleted_time_raw: None,
                                original_directory: Some(item.location.clone()),
                                total_space: None,
                                free_space: None,
                            };
                            handle_context_menu_actions_tags(
                                ui,
                                i18n,
                                &file_item,
                                &mut action,
                                palette,
                                is_tagged,
                                Some(group_id),
                            );
                        });
                });
            });
    });
    });

    action
}

pub fn draw_delete_confirmation_popup(
    ctx: &egui::Context,
    i18n: &I18n,
    palette: &ThemePalette,
    tags_state: &mut TagsState,
) -> bool {
    let Some(group_id) = tags_state.delete_confirmation else {
        return false;
    };

    let mut changed = false;
    let mut close_requested = false;
    let mut confirmed = false;

    let group_name = tags_state
        .groups
        .iter()
        .find(|g| g.id == group_id)
        .map(|g| g.name.clone())
        .unwrap_or_else(|| "Unknown".to_string());

    egui::Area::new(egui::Id::new("tag_delete_modal_bg"))
        .order(egui::Order::Middle)
        .interactable(true)
        .show(ctx, |ui| {
            let rect = ctx.content_rect();
            ui.painter()
                .rect_filled(rect, 0.0, palette.modal_background_effect_color);
        });

    egui::Window::new(i18n.tr("tag_delete_group"))
        .collapsible(false)
        .resizable(false)
        .default_width(360.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(
            egui::Frame::popup(&ctx.style_of(ctx.theme()))
                .corner_radius(egui::CornerRadius::same(8)),
        )
        .show(ctx, |ui| {
            let mut style = (**ui.style()).clone();
            style.text_styles = [
                (egui::TextStyle::Heading, egui::FontId::proportional(14.0)),
                (
                    egui::TextStyle::Body,
                    egui::FontId::proportional(palette.text_size),
                ),
                (
                    egui::TextStyle::Button,
                    egui::FontId::proportional(palette.text_size),
                ),
                (
                    egui::TextStyle::Small,
                    egui::FontId::proportional(palette.text_size),
                ),
            ]
            .into();
            ui.set_style(style);

            ui.label(egui::RichText::new(i18n.tr("tag_delete_confirm")).strong());
            ui.add_space(8.0);

            ui.label(
                egui::RichText::new(format!("{}: \"{}\"", i18n.tr("tag_group_name"), group_name))
                    .color(ui.visuals().text_color().linear_multiply(0.8)),
            );

            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if primary_dialog_button(ui, palette, &i18n.tr("ok")).clicked() {
                    confirmed = true;
                    close_requested = true;
                }
                ui.add_space(6.0);
                if ghost_dialog_button(ui, palette, &i18n.tr("close")).clicked() {
                    close_requested = true;
                }
            });
        });

    if close_requested {
        if confirmed {
            tags_state.groups.retain(|g| g.id != group_id);
            changed = true;
        }
        tags_state.delete_confirmation = None;
    } else {
        tags_state.delete_confirmation = Some(group_id);
    }

    if changed {
        crate::core::indexer::save_tags(&tags_state.to_snapshot());
    }

    changed
}

fn tag_item_layout(ui: &mut egui::Ui) -> (egui::Rect, egui::Response) {
    ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 18.0),
        egui::Sense::click_and_drag(),
    )
}

pub fn draw_tag_picker_popup(
    ctx: &egui::Context,
    i18n: &I18n,
    palette: &ThemePalette,
    tags_state: &mut TagsState,
) -> bool {
    let Some(mut picker) = tags_state.picker.take() else {
        return false;
    };

    let mut changed = false;
    let mut close_requested = false;

    egui::Area::new(egui::Id::new("tag_modal_bg"))
        .order(egui::Order::Middle)
        .interactable(true)
        .show(ctx, |ui| {
            let rect = ctx.content_rect();
            ui.painter()
                .rect_filled(rect, 0.0, palette.modal_background_effect_color);
        });

    egui::Window::new(i18n.tr("tag_add"))
        .collapsible(false)
        .resizable(false)
        .default_width(360.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(
            egui::Frame::popup(&ctx.style_of(ctx.theme()))
                .corner_radius(egui::CornerRadius::same(8)),
        )
        .show(ctx, |ui| {
            let mut style = (**ui.style()).clone();
            style.text_styles = [
                (egui::TextStyle::Heading, egui::FontId::proportional(14.0)),
                (
                    egui::TextStyle::Body,
                    egui::FontId::proportional(palette.text_size),
                ),
                (
                    egui::TextStyle::Button,
                    egui::FontId::proportional(palette.text_size),
                ),
                (
                    egui::TextStyle::Small,
                    egui::FontId::proportional(palette.text_size),
                ),
            ]
            .into();
            ui.set_style(style);

            ui.label(egui::RichText::new(i18n.tr("tag_add_to_existing_group")).strong());
            ui.add_space(6.0);

            let group_choices: Vec<(u64, String, egui::Color32, usize)> = tags_state
                .groups
                .iter()
                .map(|group| (group.id, group.name.clone(), group.color, group.items.len()))
                .collect();

            if group_choices.is_empty() {
                ui.label(
                    egui::RichText::new(i18n.tr("tag_no_groups"))
                        .color(ui.visuals().text_color().linear_multiply(0.8)),
                );
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);

                    for (group_id, group_name, group_color, item_count) in &group_choices {
                        let all_tagged = tags_state
                            .groups
                            .iter()
                            .find(|group| group.id == *group_id)
                            .is_some_and(|group| {
                                picker.paths.iter().all(|p| group.items.contains(p))
                            });

                        let button_label = if all_tagged {
                            format!("{} {} ({})", regular::CHECK, group_name, item_count)
                        } else {
                            format!("{} ({})", group_name, item_count)
                        };
                        let fill_amount = if all_tagged { 0.45 } else { 0.25 };
                        let button = egui::Button::new(button_label)
                            .fill(group_color.gamma_multiply(fill_amount))
                            .stroke(egui::Stroke::new(1.0, group_color.gamma_multiply(0.6)));

                        if ui
                            .add(button)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                            && tags_state.toggle_group_for_paths(*group_id, &picker.paths)
                        {
                            changed = true;
                        }
                    }
                });
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            ui.label(egui::RichText::new(i18n.tr("tag_create_new_group")).strong());
            ui.add_space(6.0);

            ui.label(i18n.tr("tag_group_name"));
            let name_id = ui.id().with("tag_group_name_input");
            let name_response = ui.add(
                egui::TextEdit::singleline(&mut picker.new_group_name)
                    .id(name_id)
                    .desired_width(240.0)
                    .font(egui::FontId::new(
                        palette.text_size,
                        egui::FontFamily::Proportional,
                    )),
            );

            if picker.focus_requested {
                ui.memory_mut(|mem| mem.request_focus(name_id));
                name_response.request_focus();
                if name_response.has_focus() {
                    picker.focus_requested = false;
                }
            }

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(i18n.tr("tag_group_color"));
                if rgba_color_edit_button(ui, &mut picker.new_group_color).changed() {
                    changed = true;
                }
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let can_create = !picker.new_group_name.trim().is_empty();
                let create_resp = ui.add_enabled_ui(can_create, |ui| {
                    primary_dialog_button(ui, palette, &i18n.tr("tag_create_group"))
                });
                if create_resp.inner.clicked() {
                    if tags_state.create_group_and_add(
                        picker.new_group_name.clone(),
                        picker.new_group_color,
                        &picker.paths,
                    ) {
                        changed = true;
                    }
                    picker.new_group_name.clear();
                }

                if ghost_dialog_button(ui, palette, &i18n.tr("close")).clicked() {
                    close_requested = true;
                }
            });
        });

    if !close_requested {
        tags_state.picker = Some(picker);
    } else {
        changed = true;
    }

    changed
}

fn tag_item_label(path: &PathBuf) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.display().to_string())
}

fn compute_drop_index(
    item_rects: &[egui::Rect],
    pointer_y: f32,
    source_index: usize,
) -> Option<usize> {
    if item_rects.is_empty() {
        return Some(0);
    }

    let mut drop_index: Option<usize> = None;

    for (index, rect) in item_rects.iter().enumerate() {
        let midpoint = rect.center().y;
        let new_index = if pointer_y < midpoint {
            index
        } else {
            index + 1
        };

        if new_index != source_index && new_index != source_index + 1 {
            drop_index = Some(new_index);
        }

        if pointer_y < rect.bottom() {
            break;
        }
    }

    if let Some(last) = item_rects.last() {
        if pointer_y > last.bottom() {
            drop_index = Some(item_rects.len());
        }
    }

    drop_index
}

fn draw_insert_line(ui: &mut egui::Ui, palette: &ThemePalette, y: f32, left: f32, right: f32) {
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("tag_insert_line"),
    ));

    painter.line_segment(
        [egui::pos2(left + 6.0, y), egui::pos2(right - 6.0, y)],
        egui::Stroke::new(2.0, palette.primary_active),
    );
}

fn handle_context_menu_actions_tags(
    ui: &mut egui::Ui,
    i18n: &I18n,
    file: &FileItem,
    action: &mut Option<ItemViewerAction>,
    _palette: &ThemePalette,
    is_tagged: bool,
    current_group_id: Option<u64>,
) {
    // Apply context-menu-specific typography
    let mut style = (**ui.style()).clone();
    style.text_styles = [
        (
            egui::TextStyle::Body,
            FontId::proportional(_palette.text_size),
        ),
        (
            egui::TextStyle::Button,
            FontId::proportional(_palette.text_size),
        ),
        (
            egui::TextStyle::Small,
            FontId::proportional(_palette.text_size),
        ),
        (
            egui::TextStyle::Heading,
            FontId::proportional(_palette.text_size + 2.0),
        ),
    ]
    .into();
    style.spacing.button_padding = egui::vec2(4.0, 2.0);
    style.spacing.item_spacing = egui::vec2(6.0, 2.0);
    style.spacing.menu_margin = egui::Margin::same(4);
    style.spacing.interact_size =
        egui::vec2(style.spacing.interact_size.x, _palette.text_size + 6.0);
    style.visuals.widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
    style.visuals.widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
    style.visuals.widgets.hovered.bg_fill = _palette.primary;
    style.visuals.widgets.hovered.weak_bg_fill = _palette.primary;
    style.visuals.widgets.active.bg_fill = _palette.primary;
    style.visuals.widgets.active.weak_bg_fill = _palette.primary;
    ui.set_style(style);

    let context_paths = vec![file.path.clone()];

    // --- NORMAL FILE VIEW ---
    // Determine button label based on selection count
    let label = i18n.tr("open_default_program");

    // Check if all selected files are not directories
    let all_files = context_paths.iter().all(|path| !path.is_dir());

    // Add the button with dynamic label
    if ui
        .add_enabled(all_files, egui::Button::new(label))
        .clicked()
    {
        *action = Some(ItemViewerAction::OpenWithDefault(context_paths.clone()));
        ui.close();
    }

    if ui.button(i18n.tr("inputs_copy_path")).clicked() {
        *action = Some(ItemViewerAction::Context(
            ItemViewerContextAction::CopyPath(context_paths.clone()),
        ));
        ui.close();
    }

    // A tagged item's own folder is (unlike a normal listing's current
    // directory) essentially never the tag view's own "current directory" -
    // tag groups gather items from anywhere, so "open where this actually
    // lives" is a meaningfully useful entry here.
    if let Some(parent) = file.path.parent() {
        if ui.button(i18n.tr("inputs_open_location")).clicked() {
            *action = Some(ItemViewerAction::OpenInNewTab(parent.to_path_buf()));
            ui.close();
        }
    }

    // Properties (multi-select aware)
    if ui.button(i18n.tr("properties")).clicked() {
        *action = Some(ItemViewerAction::Context(
            ItemViewerContextAction::Properties(context_paths.clone()),
        ));
        ui.close();
    }

    // "Add Tag" is always offered, even for an item already tagged - being
    // in one group doesn't preclude being added to another, so already
    // being tagged shouldn't hide the only way to add a second tag (see
    // the matching fix in `itemviewer_helper.rs`'s own context menu).
    if ui.button(i18n.tr("tag_add")).clicked() {
        *action = Some(ItemViewerAction::Context(ItemViewerContextAction::AddTag(
            context_paths.clone(),
        )));
        ui.close();
    }

    if is_tagged && ui.button(i18n.tr("tag_remove")).clicked() {
        *action = Some(ItemViewerAction::Context(match current_group_id {
            Some(group_id) => {
                ItemViewerContextAction::RemoveTagFromGroup(group_id, context_paths.clone())
            }
            None => ItemViewerContextAction::RemoveTag(context_paths.clone()),
        }));
        ui.close();
    }
}

fn handle_row_click(object: &FileItem) -> Option<ItemViewerAction> {
    return Some(if object.is_dir {
        ItemViewerAction::Open(object.path.clone())
    } else {
        ItemViewerAction::OpenWithDefault(vec![object.path.clone()])
    });
}
