//! More Disk Usage dashboard views (the window itself is
//! `disk_usage_ui`): the filter bar the lists share, the Overview tab
//! (drive summary, space by category, space by age), and the File Types
//! tab. The numbers come from `core::disk_usage_stats`.

use crate::core::disk_usage_stats::{
    AgeBucket, Category, ViewFilter, age_breakdown, category_of, category_totals, filetime_now,
    slack_space, type_breakdown,
};
use crate::core::utils::files::format_size;
use crate::core::utils::widgets::eden_button;
use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::disk_usage_ui::{
    DiskUsageState, Tab, copy_paths, format_count, muted, share, share_bar,
};
use eframe::egui;
use egui::Color32;
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use std::collections::HashMap;

/// Distinct colors for the biggest file types (in size order), the way
/// WinDirStat colors its extension list; smaller types get a muted shade
/// of their category's color.
const TYPE_PALETTE: [Color32; 12] = [
    Color32::from_rgb(0x4e, 0x79, 0xa7),
    Color32::from_rgb(0xe1, 0x57, 0x59),
    Color32::from_rgb(0x59, 0xa1, 0x4f),
    Color32::from_rgb(0xed, 0xc9, 0x48),
    Color32::from_rgb(0x76, 0xb7, 0xb2),
    Color32::from_rgb(0xb0, 0x7a, 0xa1),
    Color32::from_rgb(0xf2, 0x8e, 0x2b),
    Color32::from_rgb(0x9c, 0x75, 0x5f),
    Color32::from_rgb(0xff, 0x9d, 0xa7),
    Color32::from_rgb(0x86, 0xbc, 0x5a),
    Color32::from_rgb(0x5f, 0xa2, 0xdd),
    Color32::from_rgb(0xba, 0xb0, 0xac),
];

pub(crate) fn category_color(category: Category) -> Color32 {
    match category {
        Category::Video => Color32::from_rgb(0x4e, 0x79, 0xa7),
        Category::Images => Color32::from_rgb(0x59, 0xa1, 0x4f),
        Category::Audio => Color32::from_rgb(0xb0, 0x7a, 0xa1),
        Category::Documents => Color32::from_rgb(0xed, 0xc9, 0x48),
        Category::Archives => Color32::from_rgb(0xf2, 0x8e, 0x2b),
        Category::Programs => Color32::from_rgb(0xe1, 0x57, 0x59),
        Category::Code => Color32::from_rgb(0x76, 0xb7, 0xb2),
        Category::System => Color32::from_rgb(0x9c, 0x75, 0x5f),
        Category::Other => Color32::from_rgb(0x8a, 0x8a, 0x8a),
    }
}

/// The color used for files with extension `ext` everywhere in the
/// dashboard (File Types, treemap, sunburst).
pub(crate) fn type_color(state: &DiskUsageState, ext: &str) -> Color32 {
    state
        .type_colors
        .get(ext)
        .copied()
        .unwrap_or_else(|| category_color(category_of(ext)).gamma_multiply(0.7))
}

/// Rebuilds the File Types list (and the type colors) after the results or
/// the filters changed.
pub(crate) fn ensure_types(state: &mut DiskUsageState) {
    if state.types_revision == Some(state.revision) {
        return;
    }
    state.types_revision = Some(state.revision);
    let Some(tree) = state.tree.as_ref() else {
        state.types.clear();
        state.type_colors.clear();
        return;
    };
    let all = type_breakdown(tree, &state.root, &ViewFilter::default());
    state.type_colors = assign_colors(&all);
    state.types = if state.filter.is_active() {
        type_breakdown(tree, &state.root, &state.filter)
    } else {
        all
    };
}

fn assign_colors(types: &[crate::core::disk_usage_stats::TypeStat]) -> HashMap<String, Color32> {
    types
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let color = TYPE_PALETTE
                .get(i)
                .copied()
                .unwrap_or_else(|| category_color(t.category).gamma_multiply(0.7));
            (t.extension.clone(), color)
        })
        .collect()
}

fn ensure_ages(state: &mut DiskUsageState) {
    if state.ages_revision == Some(state.revision) {
        return;
    }
    state.ages_revision = Some(state.revision);
    state.ages = state
        .tree
        .as_ref()
        .map(|tree| age_breakdown(tree, &state.root, &state.filter, filetime_now()))
        .unwrap_or_default();
}

pub(crate) fn extension_label(i18n: &I18n, ext: &str) -> String {
    if ext.is_empty() {
        i18n.tr("disk_usage_no_extension")
    } else {
        format!(".{ext}")
    }
}

/// The "Filters" button at the end of the tab row.
pub(crate) fn draw_filter_toggle(ui: &mut egui::Ui, i18n: &I18n, palette: &ThemePalette, state: &mut DiskUsageState) {
    if state.tab == Tab::Tree {
        return;
    }
    let active = state.filter.is_active();
    let label = if active {
        format!("{} {} •", regular::FUNNEL, i18n.tr("disk_usage_filters"))
    } else {
        format!("{} {}", regular::FUNNEL, i18n.tr("disk_usage_filters"))
    };
    let text = egui::RichText::new(label).size(palette.text_size).color(if state.filter_open {
        palette.item_viewer_row_text_selected
    } else if active {
        palette.primary
    } else {
        palette.text_normal
    });
    if ui
        .add(egui::Button::selectable(state.filter_open, text))
        .on_hover_text(i18n.tr("tooltip_disk_usage_filters"))
        .clicked()
    {
        state.filter_open = !state.filter_open;
    }
}

/// The filter row (minimum size, type, folders to skip), shown when the
/// Filters button is on.
pub(crate) fn draw_filter_bar(ui: &mut egui::Ui, i18n: &I18n, palette: &ThemePalette, state: &mut DiskUsageState) {
    if !state.filter_open {
        if state.filter.is_active() {
            ui.label(
                egui::RichText::new(format!("{} {}", regular::FUNNEL, i18n.tr("disk_usage_filter_active")))
                    .size(palette.text_size - 1.0)
                    .color(palette.primary),
            );
            ui.add_space(4.0);
        }
        return;
    }
    let before = state.filter.clone();
    egui::Frame::NONE
        .fill(palette.row_bg)
        .corner_radius(egui::CornerRadius::same(palette.medium_radius))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(muted(palette, i18n.tr("disk_usage_filter_min_size")));
                let mut mb = state.filter.min_size as f64 / (1024.0 * 1024.0);
                if ui
                    .add(egui::DragValue::new(&mut mb).range(0.0..=1_000_000.0).speed(1.0).suffix(" MB"))
                    .changed()
                {
                    state.filter.min_size = (mb * 1024.0 * 1024.0) as u64;
                }
                ui.add_space(12.0);

                ui.label(muted(palette, i18n.tr("disk_usage_filter_type")));
                let current = match (&state.filter.extension, state.filter.category) {
                    (Some(ext), _) => extension_label(i18n, ext),
                    (None, Some(cat)) => i18n.tr(cat.i18n_key()),
                    (None, None) => i18n.tr("disk_usage_filter_all_types"),
                };
                egui::ComboBox::from_id_salt("disk_usage_filter_type")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(state.filter.category.is_none() && state.filter.extension.is_none(), i18n.tr("disk_usage_filter_all_types"))
                            .clicked()
                        {
                            state.filter.category = None;
                            state.filter.extension = None;
                        }
                        for cat in Category::ALL {
                            if ui
                                .selectable_label(state.filter.category == Some(cat) && state.filter.extension.is_none(), i18n.tr(cat.i18n_key()))
                                .clicked()
                            {
                                state.filter.category = Some(cat);
                                state.filter.extension = None;
                            }
                        }
                    });
                ui.add_space(12.0);

                ui.label(muted(palette, i18n.tr("disk_usage_filter_skip_folders")))
                    .on_hover_text(i18n.tr("tooltip_disk_usage_filter_skip_folders"));
                ui.add(
                    egui::TextEdit::singleline(&mut state.filter.excluded_folders)
                        .hint_text("node_modules; .git; *cache*")
                        .desired_width(220.0),
                );
                ui.add_space(12.0);
                if ui
                    .add_enabled(state.filter.is_active(), egui::Button::new(format!("{} {}", regular::FUNNEL_X, i18n.tr("disk_usage_filter_clear"))))
                    .clicked()
                {
                    state.filter = ViewFilter::default();
                }
            });
        });
    ui.add_space(6.0);
    if state.filter != before {
        // Every list and chart is recomputed from the new filter.
        state.revision += 1;
    }
}

pub(crate) fn draw_overview_toolbar(ui: &mut egui::Ui, i18n: &I18n, palette: &ThemePalette, state: &mut DiskUsageState) {
    let busy = state.scanning();
    ui.horizontal(|ui| {
        let rescan = ui
            .add_enabled_ui(!busy, |ui| {
                eden_button(ui, palette, &format!("{} {}", regular::ARROW_CLOCKWISE, i18n.tr("disk_usage_rescan")))
            })
            .inner;
        if rescan.on_hover_text(i18n.tr("tooltip_disk_usage_rescan")).clicked() {
            state.start_full_scan();
        }
    });
}

fn card(ui: &mut egui::Ui, palette: &ThemePalette, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(palette.row_bg)
        .corner_radius(egui::CornerRadius::same(palette.medium_radius))
        .stroke(egui::Stroke::new(1.0, palette.borders_default))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new(title)
                    .strong()
                    .size(palette.text_size + 1.0)
                    .color(palette.text_header_section),
            );
            ui.add_space(8.0);
            add(ui);
        });
}

/// One labelled bar: `label`, a bar filled to `fraction`, then `value`.
fn bar_row(ui: &mut egui::Ui, palette: &ThemePalette, label: &str, fraction: f32, color: Color32, value: &str) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(150.0, palette.text_size + 6.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(150.0);
                ui.add(egui::Label::new(egui::RichText::new(label).size(palette.text_size).color(palette.text_normal)).truncate());
            },
        );
        let width = (ui.available_width() - 230.0).max(60.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 12.0), egui::Sense::hover());
        let radius = egui::CornerRadius::same(3);
        ui.painter().rect_filled(rect, radius, palette.drive_usage_background);
        let mut fill = rect;
        fill.set_width(rect.width() * fraction.clamp(0.0, 1.0));
        if fill.width() > 0.5 {
            ui.painter().rect_filled(fill, radius, color);
        }
        ui.label(
            egui::RichText::new(value)
                .family(egui::FontFamily::Monospace)
                .size(palette.text_size - 1.0)
                .color(palette.text_normal),
        );
    });
}

pub(crate) fn draw_overview(ui: &mut egui::Ui, i18n: &I18n, palette: &ThemePalette, state: &mut DiskUsageState) {
    ensure_types(state);
    ensure_ages(state);
    let Some(tree) = state.tree.as_ref() else {
        return;
    };
    let scanned = tree.size;
    let scanned_allocated = tree.allocated;
    let slack = slack_space(tree);
    let is_drive = crate::core::disk_usage::drive_root_letter(&state.root).is_some();
    let filtered = state.filter.is_active();

    egui::ScrollArea::vertical()
        .id_salt("disk_usage_overview")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if let Some(info) = state.drive_info.clone() {
                card(ui, palette, &i18n.tr("disk_usage_drive_summary"), |ui| {
                    let used = info.total.saturating_sub(info.free);
                    let drive = state.root.ancestors().last().map(|p| p.display().to_string()).unwrap_or_default();
                    let name = if info.label.is_empty() {
                        format!("{drive} · {}", info.file_system)
                    } else {
                        format!("{drive} {} · {}", info.label, info.file_system)
                    };
                    ui.label(egui::RichText::new(name).size(palette.text_size).color(palette.text_normal));
                    ui.add_space(6.0);
                    let fraction = if info.total > 0 { used as f32 / info.total as f32 } else { 0.0 };
                    let bar_color = if fraction > 0.9 {
                        palette.drive_usage_critical
                    } else if fraction > 0.75 {
                        palette.drive_usage_warning
                    } else {
                        palette.drive_usage_normal
                    };
                    bar_row(
                        ui,
                        palette,
                        &i18n.tr("disk_usage_drive_used"),
                        fraction,
                        bar_color,
                        &format!(
                            "{} / {} · {} {}",
                            format_size(used),
                            format_size(info.total),
                            format_size(info.free),
                            i18n.tr("disk_usage_free")
                        ),
                    );
                    ui.add_space(6.0);
                    egui::Grid::new("disk_usage_drive_grid").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                        let mut row = |key: &str, value: String, tip: Option<&str>| {
                            let label = ui.label(muted(palette, i18n.tr(key)));
                            if let Some(tip) = tip {
                                label.on_hover_text(i18n.tr(tip));
                            }
                            ui.label(egui::RichText::new(value).size(palette.text_size).color(palette.text_normal));
                            ui.end_row();
                        };
                        row("disk_usage_cluster_size", format_size(info.cluster_size), Some("tooltip_disk_usage_cluster_size"));
                        let scope = if is_drive { "disk_usage_scanned_drive" } else { "disk_usage_scanned_folder" };
                        row(scope, format!("{} ({} {})", format_size(scanned), format_size(scanned_allocated), i18n.tr("disk_usage_on_disk")), None);
                        row("disk_usage_slack", format_size(slack), Some("tooltip_disk_usage_slack"));
                        if is_drive {
                            let missing = used.saturating_sub(scanned_allocated);
                            row("disk_usage_unaccounted_label", format_size(missing), Some("tooltip_disk_usage_unaccounted"));
                        }
                    });
                });
                ui.add_space(10.0);
            }

            let totals = category_totals(&state.types);
            let total: u64 = totals.iter().map(|t| t.1).sum();
            let title = if filtered {
                format!("{} ({})", i18n.tr("disk_usage_by_category"), i18n.tr("disk_usage_filtered"))
            } else {
                i18n.tr("disk_usage_by_category")
            };
            let mut pick_category: Option<Category> = None;
            card(ui, palette, &title, |ui| {
                // One stacked bar, then a row per category.
                let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 16.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, egui::CornerRadius::same(4), palette.drive_usage_background);
                let mut x = rect.left();
                for (cat, size, _) in &totals {
                    if *size == 0 || total == 0 {
                        continue;
                    }
                    let w = rect.width() * (*size as f32 / total as f32);
                    let part = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(w, rect.height()));
                    ui.painter().rect_filled(part, 0.0, category_color(*cat));
                    x += w;
                }
                ui.add_space(8.0);
                for (cat, size, count) in &totals {
                    if *size == 0 {
                        continue;
                    }
                    let fraction = share(*size, total);
                    let response = ui
                        .horizontal(|ui| {
                            let (swatch, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                            ui.painter().rect_filled(swatch, egui::CornerRadius::same(2), category_color(*cat));
                            bar_row(
                                ui,
                                palette,
                                &i18n.tr(cat.i18n_key()),
                                fraction,
                                category_color(*cat),
                                &format!("{} · {:.1}% · {} {}", format_size(*size), fraction * 100.0, format_count(*count), i18n.tr("disk_usage_files")),
                            );
                        })
                        .response
                        .interact(egui::Sense::click())
                        .on_hover_text(i18n.tr("tooltip_disk_usage_category_row"))
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if response.clicked() {
                        pick_category = Some(*cat);
                    }
                }
                if total == 0 {
                    ui.label(muted(palette, i18n.tr("disk_usage_no_files")));
                }
            });
            if let Some(cat) = pick_category {
                // Show that category's types.
                state.filter.category = Some(cat);
                state.filter.extension = None;
                state.revision += 1;
                state.tab = Tab::Types;
            }
            ui.add_space(10.0);

            let age_total: u64 = state.ages.iter().map(|a| a.1).sum();
            let title = if filtered {
                format!("{} ({})", i18n.tr("disk_usage_by_age"), i18n.tr("disk_usage_filtered"))
            } else {
                i18n.tr("disk_usage_by_age")
            };
            card(ui, palette, &title, |ui| {
                ui.label(muted(palette, i18n.tr("disk_usage_by_age_hint")));
                ui.add_space(6.0);
                for (bucket, size, count) in &state.ages {
                    if *bucket == AgeBucket::Unknown && *count == 0 {
                        continue;
                    }
                    let fraction = share(*size, age_total);
                    // Older = warmer: the space worth reviewing.
                    let color = match bucket {
                        AgeBucket::UnderMonth => Color32::from_rgb(0x59, 0xa1, 0x4f),
                        AgeBucket::Months1To6 => Color32::from_rgb(0x86, 0xbc, 0x5a),
                        AgeBucket::Months6To12 => Color32::from_rgb(0xed, 0xc9, 0x48),
                        AgeBucket::Years1To3 => Color32::from_rgb(0xf2, 0x8e, 0x2b),
                        AgeBucket::Over3Years => Color32::from_rgb(0xe1, 0x57, 0x59),
                        AgeBucket::Unknown => Color32::from_rgb(0x8a, 0x8a, 0x8a),
                    };
                    bar_row(
                        ui,
                        palette,
                        &i18n.tr(bucket.i18n_key()),
                        fraction,
                        color,
                        &format!("{} · {:.1}% · {} {}", format_size(*size), fraction * 100.0, format_count(*count), i18n.tr("disk_usage_files")),
                    );
                }
            });
        });
}

pub(crate) fn draw_types_toolbar(ui: &mut egui::Ui, i18n: &I18n, palette: &ThemePalette, state: &mut DiskUsageState) {
    let busy = state.scanning();
    ui.horizontal(|ui| {
        let rescan = ui
            .add_enabled_ui(!busy, |ui| {
                eden_button(ui, palette, &format!("{} {}", regular::ARROW_CLOCKWISE, i18n.tr("disk_usage_rescan")))
            })
            .inner;
        if rescan.on_hover_text(i18n.tr("tooltip_disk_usage_rescan")).clicked() {
            state.start_full_scan();
        }
        let selected = state.type_selected.clone();
        let show = ui
            .add_enabled_ui(selected.is_some(), |ui| {
                eden_button(ui, palette, &format!("{} {}", regular::SORT_DESCENDING, i18n.tr("disk_usage_show_files")))
            })
            .inner;
        if show.on_hover_text(i18n.tr("tooltip_disk_usage_show_files")).clicked()
            && let Some(ext) = selected
        {
            show_files_of_type(state, ext);
        }
        if eden_button(ui, palette, &format!("{} {}", regular::COPY, i18n.tr("disk_usage_copy_table"))).clicked() {
            ensure_types(state);
            crate::core::utils::clipboard::copy_text_to_clipboard(&types_as_text(i18n, state));
        }
    });
}

fn show_files_of_type(state: &mut DiskUsageState, ext: String) {
    state.filter.extension = Some(ext);
    state.filter.category = None;
    state.revision += 1;
    state.tab = Tab::Largest;
}

fn types_as_text(i18n: &I18n, state: &DiskUsageState) -> String {
    let mut out = String::from("Type\tCategory\tSize\tBytes\tFiles\n");
    for t in &state.types {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            extension_label(i18n, &t.extension),
            i18n.tr(t.category.i18n_key()),
            format_size(t.size),
            t.size,
            t.count
        ));
    }
    out
}

/// Types listed before the rest are summed up in one row.
const MAX_TYPE_ROWS: usize = 500;

pub(crate) fn draw_types(ui: &mut egui::Ui, i18n: &I18n, palette: &ThemePalette, state: &mut DiskUsageState) {
    ensure_types(state);
    // Category chips: a quick way to narrow the list.
    let mut chip: Option<Option<Category>> = None;
    ui.horizontal_wrapped(|ui| {
        let all = state.filter.category.is_none();
        if ui.selectable_label(all, i18n.tr("disk_usage_filter_all_types")).clicked() {
            chip = Some(None);
        }
        for cat in Category::ALL {
            let on = state.filter.category == Some(cat);
            let text = egui::RichText::new(format!("● {}", i18n.tr(cat.i18n_key())))
                .color(if on { palette.item_viewer_row_text_selected } else { palette.text_normal });
            if ui.selectable_label(on, text).clicked() {
                chip = Some(if on { None } else { Some(cat) });
            }
        }
    });
    if let Some(cat) = chip {
        state.filter.category = cat;
        state.filter.extension = None;
        state.revision += 1;
        ensure_types(state);
    }
    ui.add_space(6.0);

    let total: u64 = state.types.iter().map(|t| t.size).sum();
    if state.types.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| ui.label(muted(palette, i18n.tr("disk_usage_no_files"))));
        return;
    }
    let shown = state.types.len().min(MAX_TYPE_ROWS);
    let rest: (usize, u64, u64) = state.types[shown..]
        .iter()
        .fold((0, 0, 0), |(n, s, c), t| (n + 1, s + t.size, c + t.count));
    let row_count = shown + usize::from(rest.0 > 0);

    let row_height = (palette.text_size + 10.0).max(22.0);
    let header_color = palette.text_normal.gamma_multiply(0.75);
    ui.style_mut().interaction.selectable_labels = false;
    let mut clicked: Option<String> = None;
    let mut open: Option<String> = None;
    let mut copy_largest: Option<std::path::PathBuf> = None;

    TableBuilder::new(ui)
        .id_salt("disk_usage_types_table")
        .striped(false)
        .resizable(true)
        .sense(egui::Sense::click())
        .auto_shrink([false, false])
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(150.0).at_least(100.0).clip(true))
        .column(Column::initial(110.0).at_least(80.0).clip(true))
        .column(Column::initial(96.0).at_least(70.0))
        .column(Column::initial(170.0).at_least(110.0))
        .column(Column::initial(90.0).at_least(60.0))
        .column(Column::initial(96.0).at_least(70.0))
        .column(Column::remainder().at_least(160.0).clip(true))
        .header(row_height, |mut header| {
            for key in [
                "disk_usage_col_type",
                "disk_usage_col_category",
                "disk_usage_col_size",
                "disk_usage_col_share_total",
                "disk_usage_col_files",
                "disk_usage_col_on_disk",
                "disk_usage_col_largest_file",
            ] {
                header.col(|ui| {
                    ui.label(egui::RichText::new(i18n.tr(key)).strong().size(palette.text_size - 1.0).color(header_color));
                });
            }
        })
        .body(|body| {
            body.rows(row_height, row_count, |mut row| {
                let index = row.index();
                if index >= shown {
                    row.col(|ui| {
                        ui.label(muted(palette, format!("{} {} {}", regular::DOTS_THREE, format_count(rest.0 as u64), i18n.tr("disk_usage_more_types"))));
                    });
                    row.col(|_| {});
                    row.col(|ui| {
                        ui.label(muted(palette, format_size(rest.1)));
                    });
                    row.col(|ui| share_bar(ui, palette, share(rest.1, total), palette.text_normal.gamma_multiply(0.4)));
                    row.col(|ui| {
                        ui.label(muted(palette, format_count(rest.2)));
                    });
                    row.col(|_| {});
                    row.col(|_| {});
                    return;
                }
                let stat = &state.types[index];
                let is_selected = state.type_selected.as_deref() == Some(stat.extension.as_str());
                row.set_selected(is_selected);
                let color = if is_selected { palette.item_viewer_row_text_selected } else { palette.text_normal };
                let swatch = type_color(state, &stat.extension);
                let mono = |s: String| egui::RichText::new(s).family(egui::FontFamily::Monospace).size(palette.text_size - 1.0).color(color);
                row.col(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, egui::CornerRadius::same(2), swatch);
                    ui.label(egui::RichText::new(extension_label(i18n, &stat.extension)).size(palette.text_size).color(color));
                });
                row.col(|ui| {
                    ui.label(egui::RichText::new(i18n.tr(stat.category.i18n_key())).size(palette.text_size - 1.0).color(color.gamma_multiply(0.8)));
                });
                row.col(|ui| {
                    ui.label(mono(format_size(stat.size)));
                });
                row.col(|ui| share_bar(ui, palette, share(stat.size, total), swatch));
                row.col(|ui| {
                    ui.label(mono(format_count(stat.count)));
                });
                row.col(|ui| {
                    ui.label(mono(format_size(stat.allocated)).color(color.gamma_multiply(0.85)));
                });
                row.col(|ui| {
                    if let Some((path, size)) = &stat.largest {
                        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        ui.label(egui::RichText::new(format!("{name} ({})", format_size(*size))).size(palette.text_size - 1.0).color(color.gamma_multiply(0.8)))
                            .on_hover_text(path.display().to_string());
                    }
                });
                let response = row.response();
                if response.clicked() {
                    clicked = Some(stat.extension.clone());
                }
                if response.double_clicked() {
                    open = Some(stat.extension.clone());
                }
                response.context_menu(|ui| {
                    if ui.button(format!("{}  {}", regular::SORT_DESCENDING, i18n.tr("disk_usage_show_files"))).clicked() {
                        open = Some(stat.extension.clone());
                        ui.close();
                    }
                    if ui.button(format!("{}  {}", regular::FUNNEL, i18n.tr("disk_usage_filter_to_type"))).clicked() {
                        clicked = Some(stat.extension.clone());
                        ui.close();
                    }
                    if let Some((path, _)) = &stat.largest
                        && ui.button(format!("{}  {}", regular::LINK, i18n.tr("disk_usage_copy_largest_path"))).clicked()
                    {
                        copy_largest = Some(path.clone());
                        ui.close();
                    }
                });
            });
        });

    if let Some(ext) = clicked {
        state.type_selected = if state.type_selected.as_deref() == Some(ext.as_str()) { None } else { Some(ext) };
    }
    if let Some(path) = copy_largest {
        copy_paths(&[path]);
    }
    if let Some(ext) = open {
        state.type_selected = Some(ext.clone());
        show_files_of_type(state, ext);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::disk_usage_stats::TypeStat;

    #[test]
    fn the_biggest_types_get_distinct_colors() {
        let types: Vec<TypeStat> = (0..15)
            .map(|i| TypeStat {
                extension: format!("e{i}"),
                category: Category::Other,
                size: 100 - i,
                allocated: 0,
                count: 1,
                largest: None,
            })
            .collect();
        let colors = assign_colors(&types);
        let first: std::collections::HashSet<Color32> = (0..12).map(|i| colors[&format!("e{i}")]).collect();
        assert_eq!(first.len(), 12);
        assert_eq!(colors["e13"], colors["e14"], "the rest share their category's shade");
    }
}
