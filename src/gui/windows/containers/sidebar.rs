use crate::core::drives::{
    DriveInfo, consume_drive_list_dirty, get_drive_infos, is_raw_physical_drive_path,
};
use crate::core::fs::{MY_PC_PATH, MY_RECYCLE_BIN_PATH};
use crate::core::network;
use crate::core::utils::text::apply_eden_text_overrides;
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::utils::{draw_object_drag_ghost, drive_usage_color, eden_button, truncate_item_text};
use crate::gui::windows::containers::structs::{
    RecentLocationsState, SavedSearchRenameState, SavedSearchesState, SidebarAction, TagsState,
};
use crate::gui::windows::structs::SidebarState;
use eframe::egui;
use egui::containers::{Popup, PopupCloseBehavior};
use egui::{FontFamily, FontId, ScrollArea};
use egui_phosphor::regular;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Draws a clickable section header (label + expand/collapse chevron) and
/// toggles `expanded` when clicked. Callers wrap the section's own content in
/// `if *expanded { ... }`.
/// Returns `true` if the section was toggled this frame (so callers can
/// persist the new state).
fn draw_section_header(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    label: &str,
    expanded: &mut bool,
) -> (bool, egui::Response) {
    let chevron = if *expanded {
        regular::CARET_DOWN
    } else {
        regular::CARET_RIGHT
    };
    let resp = ui.add(
        egui::Button::new(
            egui::RichText::new(format!("{chevron}  {label}"))
                .size(palette.text_size)
                .color(palette.text_header_section)
                .strong(),
        )
        .fill(egui::Color32::TRANSPARENT)
        .stroke(egui::Stroke::NONE)
        .frame(false),
    );
    let toggled = if resp.clicked() {
        *expanded = !*expanded;
        true
    } else {
        false
    };
    (toggled, resp)
}

/// Draw the sidebar, supporting favorites reordering
pub fn draw_sidebar(
    ui: &mut egui::Ui,
    i18n: &I18n,
    icon_cache: &IconCache,
    sidebar_state: &mut SidebarState,
    palette: &ThemePalette,
    drag_active: bool,
    drag_hover_target: Option<PathBuf>,
    tags_state: &TagsState,
    saved_searches_state: &mut SavedSearchesState,
    recent_locations_state: &RecentLocationsState,
) -> SidebarAction {
    const DRIVE_CACHE_DURATION: Duration = Duration::from_secs(30);
    let mut action = SidebarAction::default();
    let mut sections_changed = false;
    let mut saved_search_rename_state = saved_searches_state.rename_state.take();
    let mut saved_search_rename_committed: Option<(u64, String)> = None;
    let mut drop_index: Option<usize> = None;
    let pointer_pos = ui.ctx().input(|i| i.pointer.hover_pos());
    let pointer_released = ui.ctx().input(|i| i.pointer.primary_released());
    let mut tab_drop_target: Option<PathBuf> = None;
    let hovered_target_ref = drag_hover_target.as_ref();

    const FOOTER_HEIGHT: f32 = 54.0;
    let scroll_height = (ui.available_height() - FOOTER_HEIGHT).max(0.0);

    ScrollArea::vertical()
        .id_salt("sidebar_scroll")
        .auto_shrink([false; 2]) // don't shrink horizontally or vertically
        .max_height(scroll_height)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.add_space(8.0);
                    ui.spacing_mut().item_spacing.y *= palette.sidebar_item_spacing_y;

                    let (changed, _resp) = draw_section_header(
                        ui,
                        palette,
                        &i18n.tr("places"),
                        &mut sidebar_state.places_expanded,
                    );
                    sections_changed |= changed;
                    if sidebar_state.places_expanded {
                        ui.add_space(8.0);

                        let pc_icon_path = PathBuf::from(MY_PC_PATH);
                        let resp = draw_sidebar_item(
                            ui,
                            icon_cache,
                            &pc_icon_path,
                            &i18n.tr("thispc"),
                            true,
                            false,
                            palette,
                            false,
                            None,
                        );
                        if resp.clicked() {
                            action.nav_to = Some(PathBuf::from(MY_PC_PATH));
                        }
                        if resp.middle_clicked() {
                            action.open_new_tab = Some(PathBuf::from(MY_PC_PATH));
                        }

                        Popup::context_menu(&resp)
                            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                apply_eden_text_overrides(ui, palette);
                                if ui.button(&i18n.tr("inputs_newtab")).clicked() {
                                    action.open_new_tab = Some(PathBuf::from(MY_PC_PATH));
                                    ui.close();
                                }
                            });

                        if let Some(home) = dirs::home_dir() {
                            let resp = draw_sidebar_item(
                                ui,
                                icon_cache,
                                &home,
                                &i18n.tr("my_user_home"),
                                true,
                                false,
                                palette,
                                false,
                                None,
                            );

                            if resp.clicked() {
                                action.nav_to = Some(home.clone());
                            }
                            if resp.middle_clicked() {
                                action.open_new_tab = Some(home.clone());
                            }
                            Popup::context_menu(&resp)
                                .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                                .show(|ui| {
                                    apply_eden_text_overrides(ui, palette);
                                    if ui.button(&i18n.tr("inputs_newtab")).clicked() {
                                        action.open_new_tab = Some(home.clone());
                                        ui.close();
                                    }
                                });
                        }

                        let recycle_bin_path = PathBuf::from("C:\\$Recycle.Bin");
                        let resp = draw_sidebar_item(
                            ui,
                            icon_cache,
                            &recycle_bin_path,
                            &i18n.tr("recycle_bin"),
                            true,
                            true,
                            palette,
                            false,
                            None,
                        );
                        if resp.clicked() {
                            action.nav_to = Some(PathBuf::from(MY_RECYCLE_BIN_PATH));
                        }
                        if resp.middle_clicked() {
                            action.open_new_tab = Some(PathBuf::from(MY_RECYCLE_BIN_PATH));
                        }

                        Popup::context_menu(&resp)
                            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                apply_eden_text_overrides(ui, palette);
                                if ui.button(&i18n.tr("inputs_newtab")).clicked() {
                                    action.open_new_tab = Some(PathBuf::from(MY_RECYCLE_BIN_PATH));
                                    ui.close();
                                }
                            });
                    }

                    ui.add_space(6.0);
                    let (changed, _resp) = draw_section_header(
                        ui,
                        palette,
                        &i18n.tr("storage"),
                        &mut sidebar_state.storage_expanded,
                    );
                    sections_changed |= changed;
                    if sidebar_state.storage_expanded {
                        ui.add_space(4.0);

                        if sidebar_state.cached_drives.is_empty()
                            || sidebar_state.last_drive_refresh.elapsed() > DRIVE_CACHE_DURATION
                            || consume_drive_list_dirty()
                        {
                            sidebar_state.cached_drives = get_drive_infos();
                            sidebar_state.last_drive_refresh = Instant::now();
                        }

                        for drive in sidebar_state.cached_drives.iter() {
                            let is_selected = sidebar_state
                                .item_clicked
                                .as_ref()
                                .map(|p| p == &drive.path)
                                .unwrap_or(false);

                            let resp =
                                sidebar_drive_item(ui, icon_cache, &drive, palette, is_selected);
                            if resp.clicked() {
                                if is_raw_physical_drive_path(&drive.path) {
                                    sidebar_state.non_ntfs_popup_path = Some(drive.path.clone());
                                } else {
                                    action.nav_to = Some(drive.path.clone());
                                }
                            }
                            if resp.middle_clicked() {
                                if is_raw_physical_drive_path(&drive.path) {
                                    sidebar_state.non_ntfs_popup_path = Some(drive.path.clone());
                                } else {
                                    action.open_new_tab = Some(drive.path.clone());
                                }
                            }
                        }
                    }

                    if let Some(_path) = sidebar_state.non_ntfs_popup_path.clone() {
                        let mut open = true;
                        egui::Window::new(&i18n.tr("non_nftsdrive"))
                            .collapsible(false)
                            .resizable(false)
                            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                            .open(&mut open)
                            .show(ui.ctx(), |ui| {
                                ui.label(&i18n.tr("non_nftsdrive_label1"));
                                ui.label(&i18n.tr("non_nftsdrive_label2"));
                                ui.label(&i18n.tr("non_nftsdrive_label3"));
                                ui.add_space(8.0);
                                if ui.button(&i18n.tr("ok")).clicked() {
                                    sidebar_state.non_ntfs_popup_path = None;
                                }
                            });
                        if !open {
                            sidebar_state.non_ntfs_popup_path = None;
                        }
                    }

                    ui.add_space(6.0);
                    let (changed, _resp) = draw_section_header(
                        ui,
                        palette,
                        &i18n.tr("favorites"),
                        &mut sidebar_state.favorites_expanded,
                    );
                    sections_changed |= changed;
                    if sidebar_state.favorites_expanded {
                    ui.add_space(4.0);

                    let is_dragging = sidebar_state.dragging_favorite.is_some();
                    let mut item_layouts = Vec::with_capacity(sidebar_state.favorites.len());

                    for (i, _favorite) in sidebar_state.favorites.iter().enumerate() {
                        let (rect, resp) = favorites_item_layout(ui, palette);

                        if !is_dragging && resp.drag_started() {
                            sidebar_state.dragging_favorite = Some(i);
                        }

                        item_layouts.push((rect, resp));
                    }

                    if is_dragging {
                        if let (Some(pos), Some(drag_idx)) =
                            (pointer_pos, sidebar_state.dragging_favorite)
                        {
                            drop_index = None;

                            for (i, (rect, _)) in item_layouts.iter().enumerate() {
                                let mid_y = rect.center().y;

                                let new_index = if pos.y < mid_y { i } else { i + 1 };

                                if new_index != drag_idx && new_index != drag_idx + 1 {
                                    drop_index = Some(new_index);
                                }

                                if pos.y < rect.bottom() {
                                    break;
                                }
                            }

                            if let Some(last) = item_layouts.last() {
                                if pos.y > last.0.bottom() {
                                    drop_index = Some(item_layouts.len());
                                }
                            }
                        }
                    }

                    for (i, favorite) in sidebar_state.favorites.iter().enumerate() {
                        let (rect, resp) = &item_layouts[i];
                        let icon_override = if let Some(file) = &favorite.custom_icon_file {
                            Some(SidebarIconOverride::File(file.as_path()))
                        } else {
                            favorite.custom_icon.as_deref().map(SidebarIconOverride::Glyph)
                        };
                        draw_sidebar_item_with_icon(
                            ui,
                            icon_cache,
                            &favorite.path,
                            &favorite.label,
                            true,
                            false,
                            palette,
                            true,
                            Some((*rect, resp.clone())),
                            icon_override,
                        );

                        if drag_active {
                            let hovered = hovered_target_ref
                                .map(|target| target == &favorite.path)
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
                                        ui.id().with("tab_drop_bg").with(favorite.path.clone()),
                                    ))
                                    .with_clip_rect(ui.clip_rect());
                                painter.rect_filled(
                                    *rect,
                                    egui::CornerRadius::same(palette.medium_radius),
                                    palette.primary_hover,
                                );

                                if pointer_released {
                                    tab_drop_target = Some(favorite.path.clone());
                                }
                            }
                        }

                        if !is_dragging && resp.drag_started() {
                            sidebar_state.dragging_favorite = Some(i);
                        }

                        if resp.clicked() {
                            action.nav_to = Some(favorite.path.clone());
                        }
                        if resp.secondary_clicked() {
                            action.select_favorite = Some(favorite.path.clone());
                        }
                        if resp.middle_clicked() {
                            action.open_new_tab = Some(favorite.path.clone());
                        }

                        Popup::context_menu(&resp)
                            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                apply_eden_text_overrides(ui, palette);
                                if ui.button(&i18n.tr("inputs_newtab")).clicked() {
                                    action.open_new_tab = Some(favorite.path.clone());
                                    ui.close();
                                }
                                if ui.button(&i18n.tr("remove_favorite")).clicked() {
                                    action.remove_favorite = Some(favorite.path.clone());
                                    ui.close();
                                }
                            });

                        if let Some(drop) = drop_index {
                            if drop == i {
                                let painter = ui.ctx().layer_painter(egui::LayerId::new(
                                    egui::Order::Background,
                                    egui::Id::new(format!("insert_line_{}", i)),
                                ));

                                let y = resp.rect.top();
                                let left = resp.rect.left() + 6.0;
                                let right = resp.rect.right() - 6.0;

                                painter.line_segment(
                                    [egui::pos2(left, y), egui::pos2(right, y)],
                                    egui::Stroke::new(2.0, palette.primary_active),
                                );
                            }
                        }
                    }

                    if is_dragging {
                        if let Some(drop) = drop_index {
                            if drop == sidebar_state.favorites.len() {
                                if let Some(rect) = item_layouts.last().map(|(r, _)| r) {
                                    let painter = ui.ctx().layer_painter(egui::LayerId::new(
                                        egui::Order::Background,
                                        egui::Id::new("insert_line_end"),
                                    ));

                                    let y = rect.bottom();
                                    let left = rect.left() + 6.0;
                                    let right = rect.right() - 6.0;

                                    painter.line_segment(
                                        [egui::pos2(left, y), egui::pos2(right, y)],
                                        egui::Stroke::new(2.0, palette.primary_active),
                                    );
                                }
                            }
                        }

                        if let Some(drag_idx) = sidebar_state.dragging_favorite {
                            draw_object_drag_ghost(
                                ui,
                                palette,
                                &sidebar_state.favorites[drag_idx].label,
                                true,
                            );
                        }

                        if let Some(from) = sidebar_state.dragging_favorite {
                            if pointer_released {
                                if let Some(to) = drop_index {
                                    action.reorder = Some((from, to));
                                }

                                sidebar_state.dragging_favorite = None;
                                drop_index = None;
                            }
                        }
                    }
                    } // favorites_expanded

                    ui.add_space(6.0);
                    let (changed, _resp) = draw_section_header(
                        ui,
                        palette,
                        &i18n.tr("tags"),
                        &mut sidebar_state.tags_expanded,
                    );
                    sections_changed |= changed;
                    if sidebar_state.tags_expanded {
                        ui.add_space(4.0);

                        if tags_state.groups.is_empty() {
                            ui.label(
                                egui::RichText::new(i18n.tr("tag_empty_state"))
                                    .size(palette.tooltip_text_size)
                                    .color(palette.tooltip_text_color),
                            );
                        } else {
                            for group in tags_state.groups.iter() {
                                // `regular::TAG` and `fill::TAG` are the exact
                                // same Unicode codepoint (egui_phosphor just
                                // exposes both weights' code as separate
                                // constants for convenience) - which glyph
                                // actually renders depends entirely on which
                                // *font family* is requested, since "Fill" is
                                // registered under its own named family
                                // (`fonts.rs`), not merged into `Proportional`.
                                // Passing `fill::TAG` alone (as this used to)
                                // silently rendered the outline glyph anyway.
                                let resp = draw_sidebar_virtual_item(
                                    ui,
                                    &group.name,
                                    egui_phosphor::fill::TAG,
                                    group.color,
                                    palette,
                                    egui::FontFamily::Name("phosphor_fill".into()),
                                    palette.sidebar_icon_size * 0.75,
                                );
                                if resp.clicked() {
                                    action.open_tag_view = Some(group.id);
                                }
                            }
                        }
                    }

                    ui.add_space(6.0);
                    let (changed, _resp) = draw_section_header(
                        ui,
                        palette,
                        &i18n.tr("saved_searches"),
                        &mut sidebar_state.saved_searches_expanded,
                    );
                    sections_changed |= changed;
                    if sidebar_state.saved_searches_expanded {
                        ui.add_space(4.0);

                        if saved_searches_state.items.is_empty() {
                            ui.label(
                                egui::RichText::new(i18n.tr("saved_search_empty_state"))
                                    .size(palette.tooltip_text_size)
                                    .color(palette.tooltip_text_color),
                            );
                        } else {
                            for item in saved_searches_state.items.iter() {
                                let editing = saved_search_rename_state
                                    .as_ref()
                                    .map(|state| state.id == item.id)
                                    .unwrap_or(false);

                                if editing {
                                    let mut clear_rename = false;
                                    let rename = saved_search_rename_state
                                        .as_mut()
                                        .expect("checked by `editing` above");
                                    ui.horizontal(|ui| {
                                        ui.add_space(4.0);
                                        ui.label(
                                            egui::RichText::new(regular::MAGNIFYING_GLASS)
                                                .size(palette.sidebar_icon_size)
                                                .color(palette.icon_color),
                                        );
                                        let edit_id =
                                            ui.id().with("saved_search_rename").with(item.id);
                                        let edit_response = ui.add(
                                            egui::TextEdit::singleline(&mut rename.buffer)
                                                .id(edit_id)
                                                .desired_width(ui.available_width() - 8.0)
                                                .font(FontId::new(
                                                    palette.text_size,
                                                    FontFamily::Proportional,
                                                )),
                                        );

                                        if rename.should_focus {
                                            ui.memory_mut(|mem| mem.request_focus(edit_id));
                                            edit_response.request_focus();
                                            if edit_response.has_focus() {
                                                rename.should_focus = false;
                                            }
                                        }

                                        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                                        let escape =
                                            ui.input(|i| i.key_pressed(egui::Key::Escape));

                                        if enter || edit_response.lost_focus() {
                                            let new_name = rename.buffer.trim().to_string();
                                            if !new_name.is_empty() {
                                                saved_search_rename_committed =
                                                    Some((item.id, new_name));
                                            }
                                            clear_rename = true;
                                        } else if escape {
                                            clear_rename = true;
                                        }
                                    });
                                    if clear_rename {
                                        saved_search_rename_state = None;
                                    }
                                } else {
                                    let resp = draw_sidebar_virtual_item(
                                        ui,
                                        &item.name,
                                        regular::MAGNIFYING_GLASS,
                                        palette.icon_color,
                                        palette,
                                        egui::FontFamily::Proportional,
                                        palette.sidebar_icon_size,
                                    );
                                    if resp.clicked() {
                                        action.open_saved_search = Some(item.id);
                                    }

                                    Popup::context_menu(&resp)
                                        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                                        .show(|ui| {
                                            apply_eden_text_overrides(ui, palette);
                                            if ui.button(&i18n.tr("inputs_newtab")).clicked() {
                                                action.open_saved_search = Some(item.id);
                                                ui.close();
                                            }
                                            if ui.button(&i18n.tr("inputs_rename")).clicked() {
                                                saved_search_rename_state =
                                                    Some(SavedSearchRenameState {
                                                        id: item.id,
                                                        buffer: item.name.clone(),
                                                        should_focus: true,
                                                    });
                                                ui.close();
                                            }
                                            if ui.button(&i18n.tr("saved_search_delete")).clicked()
                                            {
                                                action.remove_saved_search = Some(item.id);
                                                ui.close();
                                            }
                                        });
                                }
                            }
                        }
                    }

                    saved_searches_state.rename_state = saved_search_rename_state;
                    if let Some((id, new_name)) = saved_search_rename_committed {
                        if let Some(item) = saved_searches_state
                            .items
                            .iter_mut()
                            .find(|item| item.id == id)
                        {
                            item.name = new_name;
                        }
                        crate::core::indexer::save_saved_searches(
                            &saved_searches_state.to_snapshot(),
                        );
                    }

                    ui.add_space(6.0);
                    let (changed, recent_locations_header_resp) = draw_section_header(
                        ui,
                        palette,
                        &i18n.tr("recent_locations"),
                        &mut sidebar_state.recent_locations_expanded,
                    );
                    sections_changed |= changed;
                    if !recent_locations_state.items.is_empty() {
                        Popup::context_menu(&recent_locations_header_resp)
                            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                apply_eden_text_overrides(ui, palette);
                                if eden_button(ui, palette, &i18n.tr("clear_recent_locations")).clicked() {
                                    action.clear_recent_locations = true;
                                    ui.close();
                                }
                            });
                    }
                    if sidebar_state.recent_locations_expanded {
                        ui.add_space(4.0);

                        if recent_locations_state.items.is_empty() {
                            ui.label(
                                egui::RichText::new(i18n.tr("recent_locations_empty_state"))
                                    .size(palette.tooltip_text_size)
                                    .color(palette.tooltip_text_color),
                            );
                        } else {
                            for path in recent_locations_state.items.iter() {
                                let label = path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| path.to_string_lossy().to_string());

                                let resp = draw_sidebar_item(
                                    ui,
                                    icon_cache,
                                    path,
                                    &label,
                                    true,
                                    false,
                                    palette,
                                    false,
                                    None,
                                );
                                if resp.clicked() {
                                    action.nav_to = Some(path.clone());
                                }
                                if resp.middle_clicked() {
                                    action.open_new_tab = Some(path.clone());
                                }

                                Popup::context_menu(&resp)
                                    .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                                    .show(|ui| {
                                        apply_eden_text_overrides(ui, palette);
                                        if ui.button(&i18n.tr("inputs_newtab")).clicked() {
                                            action.open_new_tab = Some(path.clone());
                                            ui.close();
                                        }
                                        if ui.button(&i18n.tr("remove_recent_location")).clicked()
                                        {
                                            action.remove_recent_location = Some(path.clone());
                                            ui.close();
                                        }
                                    });
                            }
                        }
                    }

                    ui.add_space(6.0);
                    let (changed, _resp) = draw_section_header(
                        ui,
                        palette,
                        &i18n.tr("shared_network"),
                        &mut sidebar_state.shared_network_expanded,
                    );
                    sections_changed |= changed;
                    if sidebar_state.shared_network_expanded {
                        ui.add_space(4.0);

                        let network_icon_path = PathBuf::from("Network");
                        let resp = draw_sidebar_item(
                            ui,
                            icon_cache,
                            &network_icon_path,
                            &i18n.tr("network"),
                            true,
                            false,
                            palette,
                            false,
                            None,
                        );
                        if resp.clicked() {
                            action.open_network_browser = true;
                        }

                        for computer in network::get_network_computers_cached() {
                            let resp = draw_sidebar_item(
                                ui,
                                icon_cache,
                                &computer.path,
                                &computer.name,
                                true,
                                false,
                                palette,
                                false,
                                None,
                            );
                            if resp.clicked() {
                                action.nav_to = Some(computer.path.clone());
                            }
                            if resp.middle_clicked() {
                                action.open_new_tab = Some(computer.path.clone());
                            }
                        }
                    }

                    // Extra bottom padding so last items aren't clipped by scroll boundary.
                    ui.add_space(12.0);
                });
                ui.add_space(2.0);
            });
        });

    if sections_changed {
        crate::core::indexer::save_sidebar_sections(&crate::core::indexer::SidebarSectionsSnapshot {
            places: sidebar_state.places_expanded,
            storage: sidebar_state.storage_expanded,
            favorites: sidebar_state.favorites_expanded,
            tags: sidebar_state.tags_expanded,
            shared_network: sidebar_state.shared_network_expanded,
            saved_searches: sidebar_state.saved_searches_expanded,
            recent_locations: sidebar_state.recent_locations_expanded,
            sidebar_width: sidebar_state.sidebar_default_width,
        });
    }

    // Persistent footer (outside the scroll area): a Settings shortcut, pinned
    // to the bottom of the sidebar so it's always reachable, laid out the same
    // way as a regular list row (left-aligned with the same left padding,
    // vertically centered) instead of centered horizontally.
    ui.separator();
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.vertical(|ui| {
            let label = i18n.tr("settings");
            let settings_icon_path = PathBuf::from("Settings");
            let resp = draw_sidebar_item(
                ui,
                icon_cache,
                &settings_icon_path,
                &label,
                true,
                false,
                palette,
                false,
                None,
            );
            if resp.clicked() {
                action.open_settings = true;
            }
        });
    });
    ui.add_space(14.0);

    action.move_files_to_sidebar_dir = tab_drop_target;
    action
}

/// A custom icon overriding a sidebar row's real shell icon - either a
/// Phosphor glyph, or a user-browsed image file's own pixel content (which
/// takes priority when both would otherwise apply).
pub enum SidebarIconOverride<'a> {
    Glyph(&'a str),
    File(&'a std::path::Path),
}

pub fn draw_sidebar_item(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    path: &PathBuf,
    label: &str,
    is_dir: bool,
    is_recycle_bin: bool,
    palette: &ThemePalette,
    draggable: bool,
    rect_and_resp: Option<(egui::Rect, egui::Response)>,
) -> egui::Response {
    draw_sidebar_item_with_icon(
        ui,
        icon_cache,
        path,
        label,
        is_dir,
        is_recycle_bin,
        palette,
        draggable,
        rect_and_resp,
        None,
    )
}

/// Like `draw_sidebar_item`, but `icon_override` replaces the real shell icon
/// when set - used by favorites that have a custom icon assigned in
/// Settings.
#[allow(clippy::too_many_arguments)]
pub fn draw_sidebar_item_with_icon(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    path: &PathBuf,
    label: &str,
    is_dir: bool,
    is_recycle_bin: bool,
    palette: &ThemePalette,
    draggable: bool,
    rect_and_resp: Option<(egui::Rect, egui::Response)>,
    icon_override: Option<SidebarIconOverride>,
) -> egui::Response {
    let height = if palette.sidebar_item_spacing_y > 1.0 {
        18.0 * palette.sidebar_item_spacing_y
    } else {
        18.0
    };

    // --- Get rect + response ---
    let (rect, resp) = if let Some((rect, resp)) = rect_and_resp {
        (rect, resp)
    } else {
        let available_width = ui.available_width();
        ui.allocate_exact_size(
            egui::vec2(available_width, height),
            if draggable {
                egui::Sense::click_and_drag()
            } else {
                egui::Sense::click()
            },
        )
    };

    // --- Hover background + cursor ---
    if resp.hovered() {
        ui.ctx().set_cursor_icon(if draggable {
            egui::CursorIcon::Grab
        } else {
            egui::CursorIcon::Default
        });

        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(palette.medium_radius),
            palette.primary_hover,
        );

        if draggable {
            let handle_width = 12.0;
            let handle_rect = egui::Rect::from_min_size(
                egui::pos2(rect.right() - handle_width - 4.0, rect.top()),
                egui::vec2(handle_width, rect.height()),
            );

            ui.painter().text(
                handle_rect.center(),
                egui::Align2::CENTER_CENTER,
                regular::DOTS_SIX_VERTICAL,
                egui::FontId::new(14.0, egui::FontFamily::Proportional),
                palette.icon_color,
            );
        }
    }

    // --- Icon ---
    let icon_size = egui::vec2(palette.sidebar_icon_size, palette.sidebar_icon_size);
    let icon_padding = 4.0;

    let icon_pos = egui::pos2(rect.min.x + 4.0, rect.center().y - icon_size.y / 2.0);

    let text_offset_x = if let Some(SidebarIconOverride::File(icon_path)) = icon_override {
        if let Some(icon) = icon_cache.get_custom_file_icon(icon_path) {
            ui.painter().image(
                (&icon).into(),
                egui::Rect::from_min_size(icon_pos, icon_size),
                egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }

        palette.text_size + icon_size.x + icon_padding
    } else if let Some(SidebarIconOverride::Glyph(glyph)) = icon_override {
        ui.painter().text(
            egui::pos2(icon_pos.x + icon_size.x * 0.5, rect.center().y),
            egui::Align2::CENTER_CENTER,
            glyph,
            egui::FontId::new(icon_size.y, egui::FontFamily::Proportional),
            palette.icon_color,
        );

        palette.text_size + icon_size.x + icon_padding
    } else if is_recycle_bin {
        ui.painter().text(
            egui::pos2(icon_pos.x + icon_size.x * 0.5, rect.center().y),
            egui::Align2::CENTER_CENTER,
            regular::TRASH,
            egui::FontId::new(icon_size.y * 0.85, egui::FontFamily::Proportional),
            palette.icon_color,
        );

        palette.text_size + icon_size.x + icon_padding
    } else if let Some(glyph) = icon_cache.get_custom_folder_icon(path, is_dir) {
        // Custom Phosphor icon for recognized folders.
        ui.painter().text(
            egui::pos2(icon_pos.x + icon_size.x * 0.5, rect.center().y),
            egui::Align2::CENTER_CENTER,
            glyph,
            egui::FontId::new(icon_size.y, egui::FontFamily::Proportional),
            palette.icon_color,
        );

        palette.text_size + icon_size.x + icon_padding
    } else if let Some(icon) = icon_cache.get(path, is_dir) {
        // Windows shell icon fallback.
        ui.painter().image(
            (&icon).into(),
            egui::Rect::from_min_size(icon_pos, icon_size),
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1.0, 1.0)),
            egui::Color32::WHITE,
        );

        palette.text_size + icon_size.x + icon_padding
    } else {
        palette.text_size + 20.0 + icon_padding
    };

    // --- Text ---
    let text_width = rect.width() - text_offset_x;
    let font_id = egui::FontId::new(palette.text_size, egui::FontFamily::Proportional);
    let color = ui.visuals().text_color();

    let (display_name, truncated) = truncate_item_text(ui, label, text_width, &font_id, color);

    let text_pos = egui::pos2(rect.min.x + text_offset_x, rect.center().y - 2.0);

    ui.painter().text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        display_name,
        font_id,
        color,
    );

    // --- Tooltip + cursor ---
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);

    if truncated {
        resp.on_hover_text(
            egui::RichText::new(label)
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        )
    } else {
        resp.on_hover_text(
            egui::RichText::new(path.to_string_lossy())
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        )
    }
}

/// A sidebar row for one virtual (non-folder) item - a tag group or a saved
/// search - using a caller-supplied glyph instead of a shell/folder icon,
/// followed by its name.
fn draw_sidebar_virtual_item(
    ui: &mut egui::Ui,
    label: &str,
    icon: &str,
    color: egui::Color32,
    palette: &ThemePalette,
    icon_font_family: egui::FontFamily,
    icon_size: f32,
) -> egui::Response {
    let height = if palette.sidebar_item_spacing_y > 1.0 {
        18.0 * palette.sidebar_item_spacing_y
    } else {
        18.0
    };

    let available_width = ui.available_width();
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(available_width, height), egui::Sense::click());

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(palette.medium_radius),
            palette.primary_hover,
        );
    }

    let icon_center = egui::pos2(rect.min.x + 4.0 + icon_size / 2.0, rect.center().y);
    ui.painter().text(
        icon_center,
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::new(icon_size, icon_font_family),
        color,
    );

    let text_offset_x = palette.text_size + icon_size + 4.0;
    let text_width = rect.width() - text_offset_x;
    let font_id = egui::FontId::new(palette.text_size, egui::FontFamily::Proportional);
    let text_color = ui.visuals().text_color();

    let (display_name, truncated) = truncate_item_text(ui, label, text_width, &font_id, text_color);

    ui.painter().text(
        egui::pos2(rect.min.x + text_offset_x, rect.center().y - 2.0),
        egui::Align2::LEFT_CENTER,
        display_name,
        font_id,
        text_color,
    );

    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);

    if truncated {
        resp.on_hover_text(
            egui::RichText::new(label)
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        )
    } else {
        resp
    }
}

fn favorites_item_layout(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
) -> (egui::Rect, egui::Response) {
    let available_width = ui.available_width();

    let height = if palette.sidebar_item_spacing_y > 1.0 {
        18.0 * palette.sidebar_item_spacing_y
    } else {
        18.0
    };

    ui.allocate_exact_size(
        egui::vec2(available_width, height),
        egui::Sense::click_and_drag(),
    )
}

/// Draw a drive item with usage bar and size on hover
fn sidebar_drive_item(
    ui: &mut egui::Ui,
    icon_cache: &IconCache,
    drive: &DriveInfo,
    palette: &ThemePalette,
    selected: bool,
) -> egui::Response {
    let available_width = ui.available_width();
    let height = if palette.sidebar_item_spacing_y > 1.0 {
        32.0 * palette.sidebar_item_spacing_y
    } else {
        32.0
    };

    let (rect, mut resp) =
        ui.allocate_exact_size(egui::vec2(available_width, height), egui::Sense::click());

    // Background (selected > active click > hover)
    let fill_color = if selected {
        palette.primary_active
    } else if resp.is_pointer_button_down_on() {
        palette.primary_active
    } else if resp.hovered() {
        palette.primary_hover
    } else {
        egui::Color32::TRANSPARENT
    };

    if fill_color != egui::Color32::TRANSPARENT {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(palette.medium_radius),
            fill_color,
        );
    }

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // --- Top row: icon + label ---
    let icon_size = egui::vec2(palette.sidebar_icon_size, palette.sidebar_icon_size);
    let icon_padding = 4.0;

    let text_offset_x = if let Some(icon) = icon_cache.get(&drive.path, true) {
        let icon_pos = egui::pos2(rect.min.x + 4.0, rect.min.y + 4.0);

        ui.painter().image(
            (&icon).into(),
            egui::Rect::from_min_size(icon_pos, icon_size),
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1.0, 1.0)),
            palette.icon_windows,
        );

        palette.text_size + icon_size.x + icon_padding
    } else {
        palette.text_size + 20.0 + icon_padding
    };

    // --- DISPLAY TEXT ---
    let text_width = available_width - text_offset_x;
    let max_chars = (text_width / 7.0) as usize;

    let display_name = if drive.display.len() > max_chars && max_chars > 3 {
        // Use character boundaries instead of byte indices
        let mut char_count = 0;
        let mut byte_end = 0;
        for (i, _) in drive.display.char_indices() {
            if char_count >= max_chars - 3 {
                break;
            }
            char_count += 1;
            byte_end = i;
        }
        format!("{}...", &drive.display[..byte_end])
    } else {
        drive.display.clone()
    };

    let text_y = rect.min.y + 4.0 + icon_size.y / 2.0;
    let text_pos = egui::pos2(rect.min.x + text_offset_x, text_y);
    let font_id = FontId::new(palette.text_size, FontFamily::Proportional);

    ui.painter().text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        display_name,
        font_id,
        ui.visuals().text_color(),
    );

    // --- Bottom row: progress bar ---
    if let (Some(total), Some(free)) = (drive.total_space, drive.free_space) {
        let bar_height = 6.0;
        let max_bar_width = 180.0;
        let bar_width = (available_width - 8.0).min(max_bar_width);

        let bar_rect = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + 4.0, rect.bottom() - bar_height),
            egui::vec2(bar_width, bar_height),
        );

        let bar_bg = palette.drive_usage_background;
        let bar_fill = drive_usage_color((total - free) as f32 / total as f32, palette);

        ui.painter().rect_filled(
            bar_rect,
            egui::CornerRadius::same(palette.small_radius),
            bar_bg,
        );

        let used_ratio = (total - free) as f32 / total as f32;
        let fill_width = bar_rect.width() * used_ratio;

        let fill_rect = egui::Rect::from_min_size(bar_rect.min, egui::vec2(fill_width, bar_height));

        ui.painter().rect_filled(
            fill_rect,
            egui::CornerRadius::same(palette.small_radius),
            bar_fill,
        );

        let gb = 1024.0 * 1024.0 * 1024.0;
        let used_gb = (total - free) as f64 / gb;
        let total_gb = total as f64 / gb;

        resp = resp.on_hover_text(
            egui::RichText::new(format!("{:.1}/{:.1}GB", used_gb, total_gb))
                .size(palette.tooltip_text_size)
                .color(palette.tooltip_text_color),
        );
    }

    resp
}
