use crate::core::fs::{MY_PC_PATH, MY_RECYCLE_BIN_PATH, SETTINGS_PATH, parse_tag_view_path};
use crate::core::tab_groups::TabGroup;
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::utils::truncate_item_text;
use crate::gui::windows::containers::structs::{FavoriteItem, TabInfo, TabsAction, TagGroup};
use crate::gui::windows::windowsoverrides::toggle_window_fullscreen;
use eframe::egui;
use egui::containers::{Popup, PopupCloseBehavior};
use egui::{FontFamily, FontId};
use egui_phosphor::{fill, regular};
use std::path::PathBuf;
use windows::Win32::Foundation::HWND;

const PREFERRED_TAB_WIDTH: f32 = 180.0;
const MIN_TAB_WIDTH: f32 = 110.0;
pub const TAB_HEIGHT: f32 = 32.0;
/// Guaranteed-empty strip reserved at the end of every tab row (after the last
/// tab, or after the add-tab button) purely so there's always somewhere to
/// click-drag the window by - otherwise a tab strip that happens to pack
/// tightly enough to fill a row exactly leaves no empty background at all to
/// grab.
const DRAG_HANDLE_WIDTH: f32 = 72.0;

/// Computes the tab width and the number of rows needed to lay out `tab_count` tabs
/// wrapped within `full_width`, reserving room on every row for the window controls
/// so tabs never render underneath them, plus a `DRAG_HANDLE_WIDTH` strip so a
/// packed row always has somewhere empty to drag the window by. The rest of the
/// row - including the gap between the last tab/add-tab button and the window
/// controls - also stays part of the draggable tab strip background.
pub fn compute_tab_layout(tab_count: usize, full_width: f32, spacing: f32) -> (f32, usize) {
    let windows_buttons_width = 45.0 * 3.0;
    // The add-tab button rides along in the same wrapped row as the tabs (it's the
    // last item), so its width doesn't need reserving separately here.
    let reserved = windows_buttons_width + spacing + DRAG_HANDLE_WIDTH;
    let tab_region_width = (full_width - reserved).max(MIN_TAB_WIDTH);

    if tab_count == 0 {
        return (PREFERRED_TAB_WIDTH, 1);
    }

    let tab_count_f = tab_count as f32;
    let gaps = (tab_count_f - 1.0).max(0.0);
    let natural_tab_width = ((tab_region_width - gaps * spacing) / tab_count_f).max(0.0);

    if natural_tab_width >= MIN_TAB_WIDTH {
        return (natural_tab_width.min(PREFERRED_TAB_WIDTH), 1);
    }

    let per_row = ((tab_region_width + spacing) / (MIN_TAB_WIDTH + spacing))
        .floor()
        .max(1.0) as usize;
    let rows = tab_count.div_ceil(per_row).max(1);

    (MIN_TAB_WIDTH, rows)
}

/// Number of rows `draw_tabs` will use for `tab_count` tabs in `full_width` - callers
/// that need to reserve vertical space for the tab strip should use this before
/// drawing it (drawing itself recomputes the same layout for consistency).
pub fn tab_row_count(tab_count: usize, full_width: f32, spacing: f32) -> usize {
    compute_tab_layout(tab_count, full_width, spacing).1
}

pub fn draw_tabs(
    ui: &mut egui::Ui,
    i18n: &I18n,
    tabs: &[TabInfo],
    active_id: u64,
    palette: &ThemePalette,
    hwnd: Option<HWND>,
    scroll_to_id: Option<u64>,
    drag_active: bool,
    drag_hover_target: Option<PathBuf>,
    icon_cache: &IconCache,
    dragging_tab_index: &mut Option<usize>,
    tab_groups: &[TabGroup],
    tags: &[TagGroup],
    favorites: &[FavoriteItem],
    saved_search_count: usize,
) -> TabsAction {
    let mut action: TabsAction = TabsAction::default();
    let pointer_pos = ui.input(|i| i.pointer.interact_pos().or_else(|| i.pointer.hover_pos()));
    let pointer_released =
        ui.input(|i| i.pointer.any_released() && i.pointer.interact_pos().is_some());
    let hovered_target_ref = drag_hover_target.as_ref();
    let mut tab_drop_target: Option<PathBuf> = None;
    let full_width = ui.available_width();
    let spacing = ui.spacing().item_spacing.x;

    let (tab_width, _rows) = compute_tab_layout(tabs.len(), full_width, spacing);
    let windows_buttons_width = 45.0 * 3.0;
    let reserved = windows_buttons_width + spacing;
    let tab_region_width = (full_width - reserved).max(MIN_TAB_WIDTH);

    ui.allocate_ui_with_layout(
        egui::vec2(tab_region_width, ui.available_height()),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            let full_rect = ui.available_rect_before_wrap();

            // Interact with the background *before* drawing any tabs: when two
            // interactive regions overlap, egui gives click/drag priority to
            // whichever one was registered later, so the tabs (and their close
            // buttons) - registered after this - correctly take priority over the
            // background drag-catcher wherever they overlap it.
            let bg_resp = ui.interact(
                full_rect,
                ui.id().with("tabs_background_drag"),
                egui::Sense::click_and_drag(),
            );

            ui.add_space(-2.0);
            ui.spacing_mut().item_spacing = egui::vec2(spacing, spacing);

            let mut occupied_rects: Vec<egui::Rect> = Vec::with_capacity(tabs.len() + 1);
            let mut tab_rects: Vec<(usize, egui::Rect)> = Vec::with_capacity(tabs.len());

            // Tabs/add-button only ever wrap within a strip narrower than the
            // full row (by `DRAG_HANDLE_WIDTH`), so there's always a
            // guaranteed-empty, always-draggable gap at the end of every row -
            // `bg_resp` above already covers the *full* row including this gap.
            let packing_rect = {
                let mut r = ui.available_rect_before_wrap();
                r.set_width((r.width() - DRAG_HANDLE_WIDTH).max(MIN_TAB_WIDTH));
                r
            };

            ui.scope_builder(egui::UiBuilder::new().max_rect(packing_rect), |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (index, tab) in tabs.iter().enumerate() {
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(tab_width, TAB_HEIGHT),
                            egui::Sense::click_and_drag(),
                        );
                        occupied_rects.push(rect);
                        tab_rects.push((index, rect));

                        if Some(tab.id) == scroll_to_id {
                            resp.scroll_to_me(Some(egui::Align::Center));
                        }

                        if resp.drag_started() {
                            *dragging_tab_index = Some(index);
                        }

                        if drag_active && tab.id != active_id {
                            let hovered = hovered_target_ref
                                .map(|target| target == &tab.full_path)
                                .unwrap_or_else(|| {
                                    pointer_pos
                                        .map(|pointer| rect.contains(pointer))
                                        .unwrap_or(false)
                                });

                            if hovered {
                                let painter = ui
                                    .ctx()
                                    .layer_painter(egui::LayerId::new(
                                        egui::Order::Background,
                                        ui.id().with("tab_drop_bg").with(tab.id),
                                    ))
                                    .with_clip_rect(ui.clip_rect());

                                painter.rect_filled(
                                    rect,
                                    egui::CornerRadius::same(palette.medium_radius),
                                    palette.primary_hover,
                                );

                                if pointer_released {
                                    tab_drop_target = Some(tab.full_path.clone());
                                }
                            }
                        }

                        handle_draw_tab_new_allocated(
                            ui,
                            i18n,
                            tab,
                            rect,
                            resp.clone(),
                            active_id,
                            palette,
                            icon_cache,
                            tags,
                            &mut action,
                        );

                        Popup::context_menu(&resp)
                            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                let pin_label = if tab.is_pinned {
                                    i18n.tr("tooltip_tab_unpin")
                                } else {
                                    i18n.tr("tooltip_tab_pin")
                                };
                                let pin_icon = if tab.is_pinned {
                                    fill::PUSH_PIN
                                } else {
                                    regular::PUSH_PIN
                                };
                                if ui.button(format!("{pin_icon}  {pin_label}")).clicked() {
                                    action.toggle_pin = Some(tab.full_path.clone());
                                    ui.close();
                                }

                                // Favoriting only makes sense for a real
                                // folder - not the virtual This PC/Recycle
                                // Bin/Settings/tag-view tabs.
                                let is_real_folder = tab.full_path.to_string_lossy() != MY_PC_PATH
                                    && tab.full_path.to_string_lossy() != MY_RECYCLE_BIN_PATH
                                    && tab.full_path.to_string_lossy() != SETTINGS_PATH
                                    && parse_tag_view_path(&tab.full_path).is_none();

                                if is_real_folder {
                                    let is_favorite = favorites
                                        .iter()
                                        .any(|fav| fav.path == tab.full_path);
                                    let favorite_label = if is_favorite {
                                        i18n.tr("tooltip_tab_remove_favorite")
                                    } else {
                                        i18n.tr("tooltip_tab_add_favorite")
                                    };
                                    let favorite_icon = if is_favorite {
                                        fill::STAR
                                    } else {
                                        regular::STAR
                                    };
                                    if ui
                                        .button(format!("{favorite_icon}  {favorite_label}"))
                                        .clicked()
                                    {
                                        action.toggle_favorite = Some(tab.full_path.clone());
                                        ui.close();
                                    }
                                }

                                // Only a search-results tab (a search-view
                                // sentinel path) can be saved as a search -
                                // reuses the exact same `(query, scope)`
                                // shape the navbar's own "save this search"
                                // bookmark button already produces, so both
                                // paths funnel into the same handler.
                                if let Some((query, scope_folder)) =
                                    crate::core::fs::parse_search_view_path(&tab.full_path)
                                {
                                    let at_limit = saved_search_count
                                        >= crate::gui::windows::containers::structs::MAX_SAVED_SEARCHES;
                                    let label = if at_limit {
                                        i18n.tr("save_search_limit_reached")
                                    } else {
                                        i18n.tr("tab_save_search")
                                    };
                                    if ui
                                        .add_enabled(
                                            !at_limit,
                                            egui::Button::new(format!(
                                                "{}  {}",
                                                regular::BOOKMARK_SIMPLE,
                                                label
                                            )),
                                        )
                                        .clicked()
                                    {
                                        let scope = scope_folder
                                            .map(crate::core::everything::SearchScope::CurrentFolder)
                                            .unwrap_or(crate::core::everything::SearchScope::Everywhere);
                                        action.save_search = Some((query, scope));
                                        ui.close();
                                    }
                                }
                                ui.separator();

                                if ui.button(i18n.tr("tab_close")).clicked() {
                                    action.close = Some(tab.id);
                                    ui.close();
                                }
                                if ui
                                    .add_enabled(
                                        tabs.len() > 1,
                                        egui::Button::new(i18n.tr("tab_close_others")),
                                    )
                                    .clicked()
                                {
                                    action.close_others = Some(tab.id);
                                    ui.close();
                                }
                                if ui
                                    .add_enabled(
                                        index + 1 < tabs.len(),
                                        egui::Button::new(i18n.tr("tab_close_to_right")),
                                    )
                                    .clicked()
                                {
                                    action.close_to_right = Some(tab.id);
                                    ui.close();
                                }
                                if ui
                                    .add_enabled(
                                        index > 0,
                                        egui::Button::new(i18n.tr("tab_close_to_left")),
                                    )
                                    .clicked()
                                {
                                    action.close_to_left = Some(tab.id);
                                    ui.close();
                                }
                                ui.separator();

                                draw_tab_groups_menu(ui, i18n, icon_cache, tab_groups, &mut action);
                                ui.separator();
                                draw_add_tab_to_group_menu(
                                    ui,
                                    i18n,
                                    icon_cache,
                                    tab_groups,
                                    &tab.full_path,
                                    &mut action,
                                );
                            });
                    }

                    // ---------- ADD NEW TAB BUTTON (wraps alongside the tabs) ----------
                    let add_button_rect = handle_draw_add_new_tab_button(ui, palette, &mut action);
                    occupied_rects.push(add_button_rect);
                });
            });

            // ---------- DRAG-TO-REORDER: drop indicator + commit on release ----------
            if let Some(from) = *dragging_tab_index {
                if let Some(pointer) = pointer_pos {
                    let drop_index = compute_drop_index(&tab_rects, pointer, tabs.len());

                    if let Some(indicator_rect) = drop_indicator_rect(&tab_rects, drop_index) {
                        ui.painter().rect_filled(
                            indicator_rect,
                            egui::CornerRadius::ZERO,
                            palette.primary_active,
                        );
                    }

                    if pointer_released {
                        if drop_index != from && drop_index != from + 1 {
                            action.reorder = Some((from, drop_index));
                        }
                        *dragging_tab_index = None;
                    }
                } else if pointer_released {
                    *dragging_tab_index = None;
                }
            }

            // ---------- EMPTY SPACE: drag to move the window, double-click to (un)maximize ----------
            let press_over_tab = pointer_pos
                .map(|pos| occupied_rects.iter().any(|r| r.contains(pos)))
                .unwrap_or(false);

            if !press_over_tab {
                if bg_resp.double_clicked() {
                    if let Some(hwnd) = hwnd {
                        toggle_window_fullscreen(hwnd);
                    }
                }

                if bg_resp.drag_started() || bg_resp.dragged() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                if bg_resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
            }

            Popup::context_menu(&bg_resp)
                .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    draw_tab_groups_menu(ui, i18n, icon_cache, tab_groups, &mut action);
                });
        },
    );

    action.move_files_to_tab_dir = tab_drop_target;
    action
}

/// Draws the "Tab Groups" section of a right-click menu (on an individual
/// tab, or on empty tab-strip space) - one submenu per saved group, each
/// offering "Open Group" (add its folders as new tabs) and "Replace Current
/// Tabs" (close everything and open just this group's folders instead). Each
/// group's submenu button shows the real shell icon of its first folder
/// (once loaded) rather than a generic glyph, so groups are visually
/// distinguishable at a glance.
fn draw_tab_groups_menu(
    ui: &mut egui::Ui,
    i18n: &I18n,
    icon_cache: &IconCache,
    tab_groups: &[TabGroup],
    action: &mut TabsAction,
) {
    if tab_groups.is_empty() {
        ui.weak(i18n.tr("tab_group_empty"));
        return;
    }

    ui.label(i18n.tr("tab_group_menu"));
    ui.separator();

    for group in tab_groups {
        let label = if group.name.is_empty() {
            i18n.tr("tab_group_untitled")
        } else {
            group.name.clone()
        };

        let contents = |ui: &mut egui::Ui| {
            if ui
                .button(format!(
                    "{}  {}",
                    regular::ARROW_SQUARE_OUT,
                    i18n.tr("tab_group_open")
                ))
                .on_hover_text(i18n.tr("tab_group_open_hover"))
                .clicked()
            {
                action.open_group = Some(group.paths.clone());
                ui.close();
            }
            if ui
                .button(format!(
                    "{}  {}",
                    regular::ARROWS_CLOCKWISE,
                    i18n.tr("tab_group_replace")
                ))
                .on_hover_text(i18n.tr("tab_group_replace_hover"))
                .clicked()
            {
                action.replace_with_group = Some(group.paths.clone());
                ui.close();
            }
        };

        let first_folder_icon = group.paths.first().and_then(|p| icon_cache.get(p, true));
        if let Some(texture) = first_folder_icon {
            let image = egui::Image::new(&texture).fit_to_exact_size(TAB_GROUP_ICON_SIZE);
            ui.menu_image_text_button(image, label, contents);
        } else {
            ui.menu_button(label, contents);
        }
    }
}

/// Menu-row icon size for a tab group's representative (first) folder -
/// small enough to sit inline with the label like every other menu item's
/// icon, rather than at the shell icon's native (much larger) size.
const TAB_GROUP_ICON_SIZE: egui::Vec2 = egui::vec2(16.0, 16.0);

/// Draws "Add to New Group" (an inline name field + create button) and "Add
/// to Existing Group" (one entry per saved group) for a right-clicked tab's
/// folder - the same path can already be in the target group; it's added
/// again regardless, same as everywhere else tab groups allow duplicates.
fn draw_add_tab_to_group_menu(
    ui: &mut egui::Ui,
    i18n: &I18n,
    icon_cache: &IconCache,
    tab_groups: &[TabGroup],
    tab_path: &PathBuf,
    action: &mut TabsAction,
) {
    ui.menu_button(
        format!(
            "{}  {}",
            regular::FOLDER_PLUS,
            i18n.tr("tab_group_add_to_new")
        ),
        |ui| {
            let name_id = egui::Id::new("new_tab_group_name_buffer");
            let mut name = ui
                .ctx()
                .data_mut(|d| d.get_temp::<String>(name_id))
                .unwrap_or_default();

            ui.add(
                egui::TextEdit::singleline(&mut name)
                    .hint_text(i18n.tr("tab_group_name"))
                    .desired_width(160.0),
            );
            ui.ctx().data_mut(|d| d.insert_temp(name_id, name.clone()));

            if ui.button(i18n.tr("tab_group_create")).clicked() {
                action.add_tab_to_new_group = Some((name, tab_path.clone()));
                ui.ctx().data_mut(|d| d.insert_temp(name_id, String::new()));
                ui.close();
            }
        },
    );

    if !tab_groups.is_empty() {
        ui.menu_button(
            format!(
                "{}  {}",
                regular::FOLDER_PLUS,
                i18n.tr("tab_group_add_to_existing")
            ),
            |ui| {
                for group in tab_groups {
                    let label = if group.name.is_empty() {
                        i18n.tr("tab_group_untitled")
                    } else {
                        group.name.clone()
                    };

                    let first_folder_icon =
                        group.paths.first().and_then(|p| icon_cache.get(p, true));
                    let clicked = if let Some(texture) = first_folder_icon {
                        let image =
                            egui::Image::new(&texture).fit_to_exact_size(TAB_GROUP_ICON_SIZE);
                        ui.add(egui::Button::image_and_text(image, label)).clicked()
                    } else {
                        ui.button(label).clicked()
                    };

                    if clicked {
                        action.add_tab_to_existing_group = Some((group.id, tab_path.clone()));
                        ui.close();
                    }
                }
            },
        );
    }
}

/// Finds the tab whose row/position is closest to `pointer`, and returns the
/// index it should be inserted at (0..=tab_count) if dropped there.
fn compute_drop_index(
    tab_rects: &[(usize, egui::Rect)],
    pointer: egui::Pos2,
    tab_count: usize,
) -> usize {
    if tab_rects.is_empty() {
        return 0;
    }

    // Prefer a tab on the same row as the pointer; otherwise fall back to the
    // closest tab overall (e.g. the pointer is above/below every row).
    let same_row: Vec<&(usize, egui::Rect)> = tab_rects
        .iter()
        .filter(|(_, rect)| pointer.y >= rect.top() && pointer.y <= rect.bottom())
        .collect();

    let candidates: Vec<&(usize, egui::Rect)> = if same_row.is_empty() {
        tab_rects.iter().collect()
    } else {
        same_row
    };

    let nearest = candidates.into_iter().min_by(|a, b| {
        let da = a.1.center().distance_sq(pointer);
        let db = b.1.center().distance_sq(pointer);
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    });

    match nearest {
        Some((idx, rect)) => {
            if pointer.x < rect.center().x {
                *idx
            } else {
                (*idx + 1).min(tab_count)
            }
        }
        None => tab_count,
    }
}

/// A thin vertical highlight marking where the dragged tab would land.
fn drop_indicator_rect(tab_rects: &[(usize, egui::Rect)], drop_index: usize) -> Option<egui::Rect> {
    const INDICATOR_WIDTH: f32 = 3.0;

    if let Some((_, rect)) = tab_rects.iter().find(|(idx, _)| *idx == drop_index) {
        // Insert before this tab.
        return Some(egui::Rect::from_min_size(
            egui::pos2(rect.left() - INDICATOR_WIDTH, rect.top()),
            egui::vec2(INDICATOR_WIDTH, rect.height()),
        ));
    }

    // Dropping after the last tab (or the last tab of a row): anchor to the
    // previous tab's right edge if we can find it, else fall back to using the
    // final known rect's row.
    if let Some((_, rect)) = tab_rects.iter().find(|(idx, _)| *idx + 1 == drop_index) {
        return Some(egui::Rect::from_min_size(
            egui::pos2(rect.right(), rect.top()),
            egui::vec2(INDICATOR_WIDTH, rect.height()),
        ));
    }

    None
}

fn handle_draw_tab_new_allocated(
    ui: &mut egui::Ui,
    i18n: &I18n,
    tab: &TabInfo,
    rect: egui::Rect,
    resp: egui::Response,
    active_id: u64,
    palette: &ThemePalette,
    icon_cache: &IconCache,
    tags: &[TagGroup],
    action: &mut TabsAction,
) {
    let is_active = tab.id == active_id;
    let corner = if is_active {
        palette.tab_active_radius
    } else {
        palette.tab_inactive_radius
    };
    let tab_fill = if is_active {
        ui.visuals().widgets.active.bg_fill
    } else {
        palette.tab_inactive_bg_color
    };

    // --- Font and colors ---
    let font_id = FontId::new(palette.text_size, FontFamily::Proportional);
    let icon_font_id = FontId::new(palette.tab_icon_size, FontFamily::Proportional);
    let label_color = if is_active {
        palette.tab_text_selected
    } else {
        palette.text_normal
    };
    let icon_color = if tab.is_pinned {
        palette.pinned_tab_color
    } else {
        label_color
    };

    // --- Layout parameters ---
    let icon_size = palette.tab_icon_size;
    let spacing = 6.0;
    let padding = 8.0;
    let close_button_width = 20.0;
    let icon_top_left = egui::pos2(rect.left() + padding, rect.center().y - icon_size * 0.5);
    let icon_rect = egui::Rect::from_min_size(icon_top_left, egui::vec2(icon_size, icon_size));

    // Pin/unpin is now a right-click menu item rather than a click on this
    // icon, so it's purely a status indicator now (a filled pin glyph when
    // pinned, the real folder/shell icon otherwise).
    enum TabIconPaint {
        Glyph(&'static str),
        ColoredGlyph(&'static str, egui::Color32),
        Texture(egui::TextureHandle),
    }

    let icon_paint = if tab.is_pinned {
        TabIconPaint::Glyph(fill::PUSH_PIN)
    } else if tab.full_path.to_string_lossy() == MY_RECYCLE_BIN_PATH {
        TabIconPaint::Glyph(regular::TRASH)
    } else if tab.full_path.to_string_lossy() == SETTINGS_PATH {
        TabIconPaint::Glyph(regular::GEAR)
    } else if let Some(group_id) = parse_tag_view_path(&tab.full_path) {
        let color = tags
            .iter()
            .find(|g| g.id == group_id)
            .map(|g| g.color)
            .unwrap_or(icon_color);
        TabIconPaint::ColoredGlyph(fill::TAG, color)
    } else if let Some(glyph) = icon_cache.get_custom_folder_icon(&tab.full_path, true) {
        TabIconPaint::Glyph(glyph)
    } else if let Some(texture) = icon_cache.get(&tab.full_path, true) {
        TabIconPaint::Texture(texture)
    } else {
        // Real icon hasn't finished loading yet (or failed) - fall back to a
        // generic folder glyph rather than leaving the slot blank.
        TabIconPaint::Glyph(regular::FOLDER_SIMPLE)
    };

    let icon_draw_width = match &icon_paint {
        TabIconPaint::Glyph(glyph) | TabIconPaint::ColoredGlyph(glyph, _) => {
            let glyph_width = ui
                .painter()
                .layout_no_wrap(glyph.to_string(), icon_font_id.clone(), icon_color)
                .size()
                .x;
            icon_size.max(glyph_width)
        }
        TabIconPaint::Texture(_) => icon_size,
    };
    let text_pos = egui::pos2(
        rect.left() + padding + icon_draw_width + spacing,
        rect.center().y,
    );
    let text_width = rect.width() - icon_draw_width - spacing - 2.0 * padding - close_button_width;

    let (display_title, truncated) =
        truncate_item_text(ui, &tab.title, text_width, &font_id, label_color);

    // --- NOW safe to use painter ---
    let painter = ui.painter();

    // --- Paint background ---
    let rect = egui::Rect::from_min_max(rect.min.round(), rect.max.round());

    let rounding = egui::CornerRadius {
        nw: corner.nw,
        ne: corner.ne,
        sw: 0,
        se: 0,
    };

    painter.rect_filled(rect, rounding, tab_fill);

    match icon_paint {
        TabIconPaint::Glyph(glyph) => {
            painter.text(
                icon_rect.left_center(),
                egui::Align2::LEFT_CENTER,
                glyph,
                icon_font_id,
                icon_color,
            );
        }
        TabIconPaint::ColoredGlyph(glyph, color) => {
            painter.text(
                icon_rect.left_center(),
                egui::Align2::LEFT_CENTER,
                glyph,
                icon_font_id,
                color,
            );
        }
        TabIconPaint::Texture(texture) => {
            let image_rect = egui::Rect::from_center_size(
                egui::pos2(icon_rect.left() + icon_size * 0.5, icon_rect.center().y),
                egui::vec2(icon_size, icon_size),
            );
            painter.image(
                (&texture).into(),
                image_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }

    painter.text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        display_title,
        font_id.clone(),
        label_color,
    );

    // --- Draw border ---
    let stroke = if is_active {
        egui::Stroke::new(1.0, palette.borders_active)
    } else {
        egui::Stroke::new(1.5, palette.borders_default)
    };

    painter.rect_stroke(rect, rounding, stroke, egui::StrokeKind::Inside);

    if is_active {
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            egui::Stroke::new(2.0, ui.visuals().panel_fill),
        );
    }

    let close_resp = tab_close_button(ui, rect, tab.id, is_active, palette);
    if close_resp.clicked() {
        action.close = Some(tab.id);
    } else if resp.clicked() {
        action.activate = Some(tab.id);
    } else if resp.middle_clicked() {
        action.duplicate = Some(tab.full_path.clone());
    }

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);

        let tooltip_text = if truncated {
            tab.title.clone()
        } else {
            if tab.full_path.to_string_lossy().to_string() == MY_PC_PATH {
                i18n.tr("thispc")
            } else if tab.full_path.to_string_lossy().to_string() == MY_RECYCLE_BIN_PATH {
                i18n.tr("recycle_bin")
            } else if tab.full_path.to_string_lossy().to_string() == SETTINGS_PATH {
                i18n.tr("settings")
            } else {
                tab.full_path.to_string_lossy().to_string()
            }
        };

        resp.on_hover_text(
            egui::RichText::new(tooltip_text)
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        );
    }
}

fn handle_draw_add_new_tab_button(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    action: &mut TabsAction,
) -> egui::Rect {
    let size = egui::vec2(32.0, 32.0);

    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());

    let rect = egui::Rect::from_min_max(rect.min.round(), rect.max.round());

    let hovered = resp.hovered();

    let fill = if hovered {
        ui.visuals().widgets.hovered.bg_fill
    } else {
        egui::Color32::TRANSPARENT
    };

    let stroke = if hovered {
        egui::Stroke::new(1.0, palette.borders_active)
    } else {
        egui::Stroke::new(1.5, palette.borders_default)
    };

    let rounding = egui::CornerRadius {
        nw: palette.tab_inactive_radius.nw,
        ne: palette.tab_inactive_radius.ne,
        sw: 0,
        se: 0,
    };

    let painter = ui.painter();

    painter.rect_filled(rect, rounding, fill);
    painter.rect_stroke(rect, rounding, stroke, egui::StrokeKind::Inside);

    let _ = tab_add_button(ui, rect.shrink(3.0), resp.clone(), palette);

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    if resp.clicked() {
        action.open_new = true;
    }

    rect
}

fn tab_close_button(
    ui: &mut egui::Ui,
    tab_rect: egui::Rect,
    tab_id: u64,
    is_active: bool,
    palette: &ThemePalette,
) -> egui::Response {
    let size = egui::vec2(18.0, 18.0);
    let rect = egui::Rect::from_min_size(
        egui::pos2(
            tab_rect.right() - size.x - 6.0,
            tab_rect.center().y - size.y * 0.5,
        ),
        size,
    );
    let resp = ui.interact(
        rect,
        ui.id().with(("tab_close", tab_id)),
        egui::Sense::click(),
    );
    let hovered = resp.hovered();
    let bg = if hovered {
        palette.tab_close_hover
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, palette.tab_button_radius, bg);

    let color = if hovered {
        palette.icon_colored_hover
    } else if is_active {
        palette.tab_close_active
    } else {
        palette.tab_close_normal
    };

    let font_id = FontId::new(palette.tab_icon_size, FontFamily::Proportional);

    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        regular::X,
        font_id,
        color,
    );

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    resp
}

fn tab_add_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    resp: egui::Response,
    palette: &ThemePalette,
) -> egui::Response {
    let hovered = resp.hovered();

    let bg = if hovered {
        palette.tab_add_hover
    } else {
        egui::Color32::TRANSPARENT
    };

    let visual_rect = rect.shrink(4.0);

    ui.painter()
        .rect_filled(visual_rect, palette.tab_button_radius, bg);

    let color = if hovered {
        palette.icon_colored_hover
    } else {
        palette.icon_color
    };

    let font_id = FontId::new(palette.tab_icon_size, FontFamily::Proportional);

    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        regular::PLUS,
        font_id,
        color,
    );

    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    resp
}
