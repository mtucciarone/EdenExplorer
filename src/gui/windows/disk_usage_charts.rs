//! The Disk Usage dashboard's Treemap and Sunburst tabs (the geometry is
//! in `core::treemap`). Both show the same folder (`chart_zoom`, below
//! the analyzed folder) and share the dashboard's selection:
//! click = select, double-click = zoom into a folder, the breadcrumbs (or
//! the sunburst's middle) = zoom back out. Picking a type in "Highlight"
//! dims everything else, using the File Types colors.

use crate::core::disk_usage::DirNode;
use crate::core::treemap::{Block, BlockKind, Rect, Segment, Treemap, layout, render, sunburst};
use crate::core::utils::files::format_size;
use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::disk_usage_ui::{DiskUsageAction, DiskUsageState, muted};
use crate::gui::windows::disk_usage_views::{ensure_types, extension_label, type_color};
use eframe::egui;
use egui::{Color32, Pos2};
use egui_phosphor::regular;
use std::f32::consts::TAU;
use std::path::PathBuf;

/// Rings the sunburst shows around the middle.
const SUNBURST_RINGS: u32 = 6;
/// Color of folders too small to split in the treemap.
const FOLDER_GREY: [u8; 3] = [0x80, 0x80, 0x80];

/// The rendered treemap and what it was made for.
pub(crate) struct TreemapCache {
    key: (u64, Vec<String>, usize, usize, Option<String>),
    map: Treemap,
    texture: egui::TextureHandle,
}

/// The sunburst layout and what it was made for.
pub(crate) struct SunburstCache {
    key: (u64, Vec<String>),
    segments: Vec<Segment>,
}

/// The folder the charts show, falling back to the analyzed folder if the
/// zoomed-in one is gone (deleted, or a rescan changed things).
fn zoomed_node(state: &mut DiskUsageState) -> Option<&DirNode> {
    let tree = state.tree.as_ref()?;
    if tree.find(&state.chart_zoom).is_none() {
        state.chart_zoom.clear();
    }
    state.tree.as_ref()?.find(&state.chart_zoom)
}

fn full_path(state: &DiskUsageState, relative: &[String]) -> PathBuf {
    let mut path = state.root.clone();
    for part in state.chart_zoom.iter().chain(relative) {
        path.push(part);
    }
    path
}

/// `state.selected` relative to the zoomed folder, if it's inside it.
fn selected_relative(state: &DiskUsageState) -> Option<Vec<String>> {
    let selected = state.selected.as_ref()?;
    let all = crate::core::disk_usage::branch_components(&state.root, selected)?;
    all.strip_prefix(state.chart_zoom.as_slice()).map(<[String]>::to_vec)
}

fn draw_breadcrumbs(ui: &mut egui::Ui, i18n: &I18n, palette: &ThemePalette, state: &mut DiskUsageState) {
    let mut go_to: Option<usize> = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let root_name = state.root.display().to_string();
        let crumbs: Vec<String> = std::iter::once(root_name).chain(state.chart_zoom.iter().cloned()).collect();
        let last = crumbs.len() - 1;
        for (i, crumb) in crumbs.iter().enumerate() {
            if i > 0 {
                ui.label(muted(palette, regular::CARET_RIGHT));
            }
            let text = egui::RichText::new(crumb).size(palette.text_size).color(if i == last {
                palette.text_normal
            } else {
                palette.primary
            });
            if i == last {
                ui.label(text.strong());
            } else if ui.add(egui::Button::new(text).frame(false)).on_hover_text(i18n.tr("disk_usage_zoom_out_here")).clicked() {
                go_to = Some(i);
            }
        }
        ui.add_space(12.0);
        if ui
            .add_enabled(!state.chart_zoom.is_empty(), egui::Button::new(format!("{} {}", regular::MAGNIFYING_GLASS_MINUS, i18n.tr("disk_usage_zoom_out"))))
            .clicked()
        {
            go_to = Some(state.chart_zoom.len().saturating_sub(1));
        }
        let zoom_target = selected_relative(state).filter(|rel| {
            !rel.is_empty() && state.tree.as_ref().and_then(|t| t.find(&[state.chart_zoom.clone(), rel.clone()].concat())).is_some()
        });
        if ui
            .add_enabled(zoom_target.is_some(), egui::Button::new(format!("{} {}", regular::MAGNIFYING_GLASS_PLUS, i18n.tr("disk_usage_zoom_in"))))
            .on_hover_text(i18n.tr("tooltip_disk_usage_zoom_in"))
            .clicked()
            && let Some(rel) = zoom_target
        {
            state.chart_zoom.extend(rel);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ensure_types(state);
            let current = state
                .type_selected
                .as_deref()
                .map(|ext| extension_label(i18n, ext))
                .unwrap_or_else(|| i18n.tr("disk_usage_highlight_none"));
            egui::ComboBox::from_id_salt("disk_usage_chart_highlight")
                .selected_text(current)
                .width(150.0)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(state.type_selected.is_none(), i18n.tr("disk_usage_highlight_none")).clicked() {
                        state.type_selected = None;
                    }
                    for stat in state.types.iter().take(40) {
                        let on = state.type_selected.as_deref() == Some(stat.extension.as_str());
                        let label = egui::RichText::new(format!("■ {}  ({})", extension_label(i18n, &stat.extension), format_size(stat.size)))
                            .color(type_color(state, &stat.extension));
                        if ui.selectable_label(on, label).clicked() {
                            state.type_selected = Some(stat.extension.clone());
                        }
                    }
                });
            ui.label(muted(palette, i18n.tr("disk_usage_highlight")));
        });
    });
    if let Some(i) = go_to {
        state.chart_zoom.truncate(i);
    }
}

fn hover_text(i18n: &I18n, state: &DiskUsageState, relative: &[String], size: u64, is_dir: bool, ext: &str, parent_size: u64) -> String {
    let path = full_path(state, relative);
    let share = if parent_size > 0 { size as f64 / parent_size as f64 * 100.0 } else { 0.0 };
    let kind = if is_dir { i18n.tr("disk_usage_folder_word") } else { extension_label(i18n, ext) };
    format!("{}\n{} · {} · {:.1}%", path.display(), kind, format_size(size), share)
}

fn item_menu(
    ui: &mut egui::Ui,
    i18n: &I18n,
    state: &mut DiskUsageState,
    relative: &[String],
    is_dir: bool,
    action: &mut Option<DiskUsageAction>,
) {
    let path = full_path(state, relative);
    if is_dir {
        if ui.button(format!("{}  {}", regular::MAGNIFYING_GLASS_PLUS, i18n.tr("disk_usage_zoom_in"))).clicked() {
            state.chart_zoom.extend(relative.iter().cloned());
            ui.close();
        }
        if ui.button(format!("{}  {}", regular::ARROW_SQUARE_OUT, i18n.tr("disk_usage_open_in_tab"))).clicked() {
            *action = Some(DiskUsageAction::OpenInNewTab(path.clone()));
            ui.close();
        }
    } else if ui.button(format!("{}  {}", regular::ARROW_SQUARE_OUT, i18n.tr("disk_usage_show_in_folder"))).clicked() {
        *action = Some(DiskUsageAction::Reveal(path.clone()));
        ui.close();
    }
    if !is_dir
        && ui.button(format!("{}  {}", regular::FUNNEL, i18n.tr("disk_usage_highlight_this_type"))).clicked()
    {
        state.type_selected = Some(crate::core::disk_usage_stats::extension_of(relative.last().map(String::as_str).unwrap_or("")));
        ui.close();
    }
    if ui.button(format!("{}  {}", regular::LINK, i18n.tr("disk_usage_copy_path"))).clicked() {
        crate::core::utils::clipboard::copy_text_to_clipboard(&path.display().to_string());
        ui.close();
    }
}

pub(crate) fn draw_treemap(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut DiskUsageState,
    action: &mut Option<DiskUsageAction>,
) {
    draw_breadcrumbs(ui, i18n, palette, state);
    ui.add_space(6.0);
    ensure_types(state);
    let Some(node_size) = zoomed_node(state).map(|n| n.size) else {
        return;
    };
    if node_size == 0 {
        ui.label(muted(palette, i18n.tr("disk_usage_no_files")));
        return;
    }

    let size = ui.available_size() - egui::vec2(0.0, 4.0);
    let (rect, response) = ui.allocate_exact_size(size.max(egui::vec2(50.0, 50.0)), egui::Sense::click());
    let ppp = ui.ctx().pixels_per_point();
    let (w_px, h_px) = ((rect.width() * ppp).round() as usize, (rect.height() * ppp).round() as usize);
    if w_px < 4 || h_px < 4 {
        return;
    }

    let key = (state.revision, state.chart_zoom.clone(), w_px, h_px, state.type_selected.clone());
    if state.treemap.as_ref().map(|c| &c.key) != Some(&key) {
        let map = layout(
            zoomed_node(state).expect("checked above"),
            Rect { x: 0.0, y: 0.0, w: w_px as f32, h: h_px as f32 },
        );
        let highlight = state.type_selected.clone();
        let pixels = render(&map, w_px, h_px, |block: &Block| match &block.kind {
            BlockKind::File { ext } => {
                let c = type_color(state, ext);
                ([c.r(), c.g(), c.b()], highlight.as_ref().is_some_and(|h| h != ext))
            }
            BlockKind::Folder => (FOLDER_GREY, highlight.is_some()),
        });
        let image = egui::ColorImage::from_rgba_unmultiplied([w_px, h_px], &pixels);
        let texture = match state.treemap.take() {
            Some(mut cache) => {
                cache.texture.set(image, egui::TextureOptions::NEAREST);
                cache.texture
            }
            None => ui.ctx().load_texture("disk_usage_treemap", image, egui::TextureOptions::NEAREST),
        };
        state.treemap = Some(TreemapCache { key, map, texture });
    }
    let Some(cache) = state.treemap.as_ref() else {
        return;
    };
    let painter = ui.painter_at(rect);
    painter.image(
        cache.texture.id(),
        rect,
        egui::Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    let to_screen = |r: Rect| {
        egui::Rect::from_min_size(
            rect.min + egui::vec2(r.x / ppp, r.y / ppp),
            egui::vec2(r.w / ppp, r.h / ppp),
        )
    };

    // The selection's outline.
    if let Some(rel) = selected_relative(state) {
        let outline = cache
            .map
            .blocks
            .iter()
            .find(|b| b.path == rel)
            .map(|b| b.rect)
            .or_else(|| cache.map.folder(&rel).map(|f| f.rect));
        if let Some(r) = outline {
            painter.rect_stroke(to_screen(r), 0.0, egui::Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Inside);
        }
    }

    let hovered = response
        .hover_pos()
        .and_then(|p| cache.map.block_at((p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp))
        .cloned();
    let mut select: Option<Vec<String>> = None;
    let mut zoom: Option<Vec<String>> = None;
    if let Some(block) = &hovered {
        painter.rect_stroke(to_screen(block.rect), 0.0, egui::Stroke::new(1.0, Color32::from_white_alpha(160)), egui::StrokeKind::Inside);
        let is_dir = block.kind == BlockKind::Folder;
        let ext = match &block.kind {
            BlockKind::File { ext } => ext.clone(),
            BlockKind::Folder => String::new(),
        };
        let text = hover_text(i18n, state, &block.path, block.size, is_dir, &ext, node_size);
        let response = response.clone().on_hover_text_at_pointer(text);
        if response.clicked() {
            select = Some(block.path.clone());
        }
        if response.double_clicked() {
            // Zoom one level towards what was double-clicked.
            let depth = if is_dir { block.path.len() } else { block.path.len().saturating_sub(1) };
            if depth > 0 {
                zoom = Some(block.path[..1].to_vec());
            }
        }
    }
    let menu_target = hovered.clone();
    response.context_menu(|ui| {
        if let Some(block) = &menu_target {
            let is_dir = block.kind == BlockKind::Folder;
            // A file's folder is what "zoom" means for it.
            item_menu(ui, i18n, state, &block.path, is_dir, action);
        } else {
            ui.close();
        }
    });
    if let Some(rel) = select {
        state.selected = Some(full_path(state, &rel));
    }
    if let Some(rel) = zoom {
        state.chart_zoom.extend(rel);
    }
}

/// A folder's sunburst color: each top-level branch gets its own hue,
/// fading outwards.
fn branch_color(branch: usize, ring: u32) -> Color32 {
    let hue = (branch as f32 * 0.618_034).fract();
    let value = (0.85 - ring as f32 * 0.07).max(0.4);
    egui::ecolor::Hsva::new(hue, 0.45, value, 1.0).into()
}

pub(crate) fn draw_sunburst(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut DiskUsageState,
    action: &mut Option<DiskUsageAction>,
) {
    draw_breadcrumbs(ui, i18n, palette, state);
    ui.add_space(6.0);
    ensure_types(state);
    let Some((node_name, node_size)) = zoomed_node(state).map(|n| (n.name.to_string(), n.size)) else {
        return;
    };
    let key = (state.revision, state.chart_zoom.clone());
    if state.sunburst.as_ref().map(|c| &c.key) != Some(&key) {
        let segments = sunburst(zoomed_node(state).expect("checked above"), SUNBURST_RINGS);
        state.sunburst = Some(SunburstCache { key, segments });
    }

    let size = ui.available_size() - egui::vec2(0.0, 4.0);
    let (rect, response) = ui.allocate_exact_size(size.max(egui::vec2(80.0, 80.0)), egui::Sense::click());
    let painter = ui.painter_at(rect);
    let center = rect.center();
    let radius = (rect.width().min(rect.height()) / 2.0 - 6.0).max(20.0);
    let segments = &state.sunburst.as_ref().expect("just built").segments;
    // Rings as wide as the depth actually shown allows.
    let rings = segments.iter().map(|s| s.ring).max().unwrap_or(1);
    let ring_width = radius / (rings as f32 + 1.0);
    let highlight = state.type_selected.clone();
    let selected = selected_relative(state);

    let point = |angle: f32, r: f32| center + egui::vec2(angle.sin() * r, -angle.cos() * r);
    let mut mesh = egui::Mesh::default();
    for seg in segments {
        let r0 = seg.ring as f32 * ring_width;
        let r1 = r0 + ring_width - 1.5;
        let mut color = if seg.is_dir {
            branch_color(seg.branch, seg.ring)
        } else {
            type_color(state, &seg.ext)
        };
        if highlight.as_ref().is_some_and(|h| seg.is_dir || *h != seg.ext) {
            color = color.gamma_multiply(0.3);
        }
        if selected.as_ref() == Some(&seg.path) {
            color = Color32::WHITE;
        }
        let steps = (((seg.end - seg.start) * r1 / 4.0).ceil() as usize).clamp(1, 256);
        let base = mesh.vertices.len() as u32;
        for i in 0..=steps {
            let a = seg.start + (seg.end - seg.start) * i as f32 / steps as f32;
            mesh.colored_vertex(point(a, r0), color);
            mesh.colored_vertex(point(a, r1), color);
        }
        for i in 0..steps as u32 {
            let v = base + i * 2;
            mesh.add_triangle(v, v + 1, v + 2);
            mesh.add_triangle(v + 1, v + 3, v + 2);
        }
    }
    painter.add(egui::Shape::mesh(mesh));

    // The middle: the folder shown, click to go up.
    let hub_radius = ring_width - 2.0;
    let pointer = response.hover_pos();
    let over_hub = pointer.is_some_and(|p| p.distance(center) < hub_radius);
    painter.circle_filled(center, hub_radius, if over_hub { palette.row_selected_bg } else { palette.row_bg });
    let name = if state.chart_zoom.is_empty() { state.root.display().to_string() } else { node_name };
    painter.text(
        center - egui::vec2(0.0, 7.0),
        egui::Align2::CENTER_CENTER,
        truncate(&name, 18),
        egui::FontId::proportional(palette.text_size - 1.0),
        palette.text_normal,
    );
    painter.text(
        center + egui::vec2(0.0, 9.0),
        egui::Align2::CENTER_CENTER,
        format_size(node_size),
        egui::FontId::monospace(palette.text_size - 2.0),
        palette.text_normal.gamma_multiply(0.75),
    );

    // What's under the pointer.
    let hovered = pointer.and_then(|p| {
        let d = p.distance(center);
        let ring = (d / ring_width).floor() as u32;
        if ring == 0 {
            return None;
        }
        let v = p - center;
        let angle = v.x.atan2(-v.y).rem_euclid(TAU);
        segments
            .iter()
            .find(|s| s.ring == ring && angle >= s.start && angle < s.end)
            .cloned()
    });
    let mut select: Option<Vec<String>> = None;
    let mut zoom: Option<Vec<String>> = None;
    let mut zoom_out = false;
    if let Some(seg) = &hovered {
        let text = hover_text(i18n, state, &seg.path, seg.size, seg.is_dir, &seg.ext, node_size);
        let response = response.clone().on_hover_text_at_pointer(text);
        if response.clicked() {
            select = Some(seg.path.clone());
        }
        if response.double_clicked() && seg.is_dir {
            zoom = Some(seg.path.clone());
        }
    } else if over_hub {
        let response = response.clone().on_hover_text_at_pointer(i18n.tr("disk_usage_sunburst_hub"));
        if response.clicked() && !state.chart_zoom.is_empty() {
            zoom_out = true;
        }
    }
    let menu_target = hovered.clone();
    response.context_menu(|ui| match &menu_target {
        Some(seg) => item_menu(ui, i18n, state, &seg.path, seg.is_dir, action),
        None => ui.close(),
    });
    if let Some(rel) = select {
        state.selected = Some(full_path(state, &rel));
    }
    if let Some(rel) = zoom {
        state.chart_zoom.extend(rel);
    }
    if zoom_out {
        state.chart_zoom.pop();
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max - 1).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branches_get_different_hues_and_fade_outwards() {
        assert_ne!(branch_color(0, 1), branch_color(1, 1));
        let inner = egui::Rgba::from(branch_color(2, 1));
        let outer = egui::Rgba::from(branch_color(2, 5));
        assert!(inner.intensity() > outer.intensity());
    }

    #[test]
    fn long_names_are_shortened() {
        assert_eq!(truncate("short", 18), "short");
        assert_eq!(truncate("a very long folder name here", 10).chars().count(), 10);
    }
}
