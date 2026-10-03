//! The sidebar's Folders section: an expandable tree of every drive's
//! folders, like Windows Explorer's navigation pane. Subfolders are read in
//! the background when a folder is expanded, the tree follows the folder
//! being viewed (expanding down to it and highlighting it), and folders
//! accept dropped files and spring open while dragging. Only rows on screen
//! are painted, so large expanded trees stay cheap.

use crate::core::spring_load::SpringLoad;
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use eframe::egui;
use egui_phosphor::regular;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Folders = Arc<Vec<(String, PathBuf)>>;
/// Finished background reads: (folder, its subfolders or an error).
type Inbox = Arc<Mutex<Vec<(PathBuf, Result<Vec<(String, PathBuf)>, String>)>>>;

enum Children {
    Loading,
    Loaded(Folders),
    Failed,
}

#[derive(Default)]
pub struct FolderTreeState {
    expanded: HashSet<PathBuf>,
    children: HashMap<PathBuf, Children>,
    /// Background reads that finished, picked up next frame.
    inbox: Inbox,
    /// The folder last revealed (expanded down to), so it's done once per visit.
    revealed: Option<PathBuf>,
    /// Scroll the current folder's row into view when it's next drawn.
    scroll_to_current: bool,
    show_hidden: bool,
    spring: SpringLoad,
    /// The visible tree, flattened; rebuilt only when it changes.
    rows: Option<Arc<Vec<Row>>>,
    /// The roots `rows` was built from.
    rows_roots: Vec<(PathBuf, String)>,
}

/// One visible line of the tree: (depth, path, label).
type Row = (usize, PathBuf, String);

/// What the tree asks for.
#[derive(Default)]
pub struct TreeAction {
    pub nav_to: Option<PathBuf>,
    pub open_new_tab: Option<PathBuf>,
    pub analyze_disk_usage: Option<PathBuf>,
    /// Files were dropped on this folder.
    pub drop_on: Option<PathBuf>,
}

pub struct TreeOptions<'a> {
    pub current: &'a Path,
    pub show_hidden: bool,
    pub drag_active: bool,
    pub drag_hover_target: Option<&'a PathBuf>,
    pub pointer_released: bool,
    /// `None` when spring-loaded folders are off.
    pub spring_delay: Option<Duration>,
    pub middle_click_new_tab: bool,
}

const ROW_HEIGHT: f32 = 22.0;
const INDENT: f32 = 14.0;

impl FolderTreeState {
    fn request(&mut self, dir: &Path, ctx: &egui::Context) {
        if matches!(self.children.get(dir), Some(Children::Loading)) {
            return;
        }
        self.children.insert(dir.to_path_buf(), Children::Loading);
        let (inbox, dir, ctx, show_hidden) = (
            self.inbox.clone(),
            dir.to_path_buf(),
            ctx.clone(),
            self.show_hidden,
        );
        std::thread::spawn(move || {
            let result = crate::core::fs::list_subfolders(&dir, show_hidden);
            if let Ok(mut inbox) = inbox.lock() {
                inbox.push((dir, result));
            }
            ctx.request_repaint();
        });
    }

    fn toggle(&mut self, dir: &Path, ctx: &egui::Context) {
        self.rows = None;
        if !self.expanded.remove(dir) {
            self.expanded.insert(dir.to_path_buf());
            // Re-read on every expand, so new folders show up.
            if !matches!(self.children.get(dir), Some(Children::Loading)) {
                self.children.remove(dir);
            }
            self.request(dir, ctx);
        }
    }

    /// Expands the tree down to `current` (once per visit) and refreshes the
    /// current folder's own subfolders.
    fn reveal(&mut self, current: &Path, roots: &[PathBuf], ctx: &egui::Context) {
        if self.revealed.as_deref() == Some(current) {
            return;
        }
        self.revealed = Some(current.to_path_buf());
        self.rows = None;
        let Some(root) = roots.iter().find(|r| current.starts_with(r)) else {
            return;
        };
        let mut ancestors: Vec<&Path> = current
            .ancestors()
            .take_while(|a| a.starts_with(root))
            .collect();
        ancestors.reverse();
        // Every ancestor (not the folder itself) is opened.
        for dir in ancestors.iter().take(ancestors.len().saturating_sub(1)) {
            if self.expanded.insert(dir.to_path_buf()) || !self.children.contains_key(*dir) {
                self.request(dir, ctx);
            }
        }
        // The folder being viewed may have gained folders; re-read if open.
        if self.expanded.contains(current) {
            self.children.remove(current);
            self.request(current, ctx);
        }
        self.scroll_to_current = true;
    }
}

fn flatten(
    expanded: &HashSet<PathBuf>,
    children: &HashMap<PathBuf, Children>,
    roots: &[(PathBuf, String)],
) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut stack: Vec<Row> = roots
        .iter()
        .rev()
        .map(|(p, l)| (0, p.clone(), l.clone()))
        .collect();
    while let Some((depth, path, label)) = stack.pop() {
        if expanded.contains(&path)
            && let Some(Children::Loaded(kids)) = children.get(&path)
        {
            for (name, kid) in kids.iter().rev() {
                stack.push((depth + 1, kid.clone(), name.clone()));
            }
        }
        rows.push((depth, path, label));
    }
    rows
}

/// Draws the tree under the "Folders" header. `roots` are the drives
/// (path, label).
pub fn draw_folder_tree(
    ui: &mut egui::Ui,
    i18n: &I18n,
    icon_cache: &IconCache,
    palette: &ThemePalette,
    state: &mut FolderTreeState,
    roots: &[(PathBuf, String)],
    opts: TreeOptions,
) -> TreeAction {
    let mut action = TreeAction::default();
    let ctx = ui.ctx().clone();
    if state.show_hidden != opts.show_hidden {
        state.show_hidden = opts.show_hidden;
        state.children.clear();
        state.revealed = None;
        state.rows = None;
    }
    if let Ok(mut inbox) = state.inbox.lock() {
        for (dir, result) in inbox.drain(..) {
            let children = match result {
                Ok(list) => Children::Loaded(Arc::new(list)),
                Err(_) => Children::Failed,
            };
            state.children.insert(dir, children);
            state.rows = None;
        }
    }
    let root_paths: Vec<PathBuf> = roots.iter().map(|(p, _)| p.clone()).collect();
    if opts.current.is_absolute() {
        state.reveal(opts.current, &root_paths, &ctx);
    }

    if state.rows_roots.as_slice() != roots {
        state.rows_roots = roots.to_vec();
        state.rows = None;
    }
    let rows = state
        .rows
        .get_or_insert_with(|| Arc::new(flatten(&state.expanded, &state.children, roots)))
        .clone();

    let pointer = ui.input(|i| i.pointer.hover_pos());
    // The visible width: rows never reach past the sidebar's edge.
    let width = ui
        .available_width()
        .min(ui.clip_rect().right() - ui.cursor().left())
        .max(40.0);
    let mut toggles: Vec<PathBuf> = Vec::new();
    let mut hovered_during_drag: Option<PathBuf> = None;
    let mut hovered_rect: Option<egui::Rect> = None;
    let selected_fill = palette.row_selected_bg;
    let hover_fill = ui.visuals().widgets.hovered.weak_bg_fill;
    let text_color = palette.text_normal;
    let font = egui::FontId::proportional(palette.text_size);
    // One block for the whole tree; only the rows inside the visible area
    // get widgets, so a huge expanded tree costs the same as a small one.
    let (block, _) = ui.allocate_exact_size(
        egui::vec2(width, ROW_HEIGHT * rows.len() as f32),
        egui::Sense::hover(),
    );
    let row_rect = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(block.left(), block.top() + i as f32 * ROW_HEIGHT),
            egui::vec2(width, ROW_HEIGHT),
        )
    };
    if state.scroll_to_current {
        state.scroll_to_current = false;
        if let Some(i) = rows
            .iter()
            .position(|(_, p, _)| p.as_path() == opts.current)
        {
            ui.scroll_to_rect(row_rect(i), Some(egui::Align::Center));
        }
    }
    let clip = ui.clip_rect();
    let first =
        (((clip.top() - block.top()) / ROW_HEIGHT).floor().max(0.0) as usize).min(rows.len());
    let last =
        ((((clip.bottom() - block.top()) / ROW_HEIGHT).ceil().max(0.0)) as usize).min(rows.len());
    for (i, (depth, path, label)) in rows.iter().enumerate().take(last).skip(first) {
        let rect = row_rect(i);
        let resp = ui.interact(
            rect,
            ui.id().with(("folder_tree_row", path)),
            egui::Sense::click(),
        );
        let is_current = path.as_path() == opts.current;
        let caret_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + *depth as f32 * INDENT, rect.top()),
            egui::vec2(16.0, ROW_HEIGHT),
        );
        let expanded = state.expanded.contains(path);
        let has_children = match state.children.get(path) {
            Some(Children::Loaded(kids)) => !kids.is_empty(),
            Some(Children::Failed) => false,
            // Not read yet: show the arrow until it's known to be empty.
            _ => true,
        };
        // Drag hover: drop target and spring-open.
        if opts.drag_active {
            let hovered = opts
                .drag_hover_target
                .map(|t| t == path)
                // Clip-aware: the row may extend past the sidebar's edge.
                .unwrap_or_else(|| ui.rect_contains_pointer(rect));
            if hovered {
                hovered_during_drag = Some(path.clone());
                hovered_rect = Some(rect);
                if opts.pointer_released {
                    action.drop_on = Some(path.clone());
                }
            }
        }
        if resp.clicked() {
            if pointer.is_some_and(|p| caret_rect.contains(p)) && has_children {
                toggles.push(path.clone());
            } else {
                action.nav_to = Some(path.clone());
            }
        }
        if resp.double_clicked() && has_children {
            toggles.push(path.clone());
        }
        if opts.middle_click_new_tab && resp.middle_clicked() {
            action.open_new_tab = Some(path.clone());
        }
        resp.context_menu(|ui| {
            crate::core::utils::text::apply_eden_text_overrides(ui, palette);
            if ui.button(i18n.tr("inputs_newtab")).clicked() {
                action.open_new_tab = Some(path.clone());
                ui.close();
            }
            if ui.button(i18n.tr("disk_usage_menu")).clicked() {
                action.analyze_disk_usage = Some(path.clone());
                ui.close();
            }
            ui.separator();
            if ui.button(i18n.tr("folder_tree_collapse_all")).clicked() {
                state.expanded.clear();
                state.rows = None;
                state.revealed = Some(opts.current.to_path_buf());
                ui.close();
            }
        });
        let painter = ui.painter();
        if is_current {
            painter.rect_filled(rect, palette.medium_radius, selected_fill);
        } else if resp.hovered() || hovered_rect == Some(rect) {
            painter.rect_filled(rect, palette.medium_radius, hover_fill);
        }
        if has_children {
            let caret = if expanded {
                regular::CARET_DOWN
            } else {
                regular::CARET_RIGHT
            };
            painter.text(
                caret_rect.center(),
                egui::Align2::CENTER_CENTER,
                caret,
                egui::FontId::proportional(palette.text_size - 2.0),
                ui.visuals().weak_text_color(),
            );
        }
        let icon_size = 16.0;
        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(caret_rect.right() + 2.0, rect.center().y - icon_size / 2.0),
            egui::vec2(icon_size, icon_size),
        );
        if let Some(icon) = icon_cache.get(path, true) {
            egui::Image::new(&icon).paint_at(ui, icon_rect);
        }
        let color = if is_current {
            palette.item_viewer_row_text_selected
        } else {
            text_color
        };
        let galley = ui.painter().layout(
            label.clone(),
            font.clone(),
            color,
            (rect.right() - icon_rect.right() - 8.0).max(10.0),
        );
        let row = galley
            .rows
            .first()
            .map(|r| r.rect().height())
            .unwrap_or(ROW_HEIGHT);
        ui.painter().galley(
            egui::pos2(icon_rect.right() + 6.0, rect.center().y - row / 2.0),
            galley,
            color,
        );
        if label.len() > 24 {
            resp.on_hover_text(path.display().to_string());
        }
    }

    // Spring-loaded: resting on a collapsed folder while dragging opens it.
    if let Some(delay) = opts.spring_delay {
        let target = hovered_during_drag.filter(|p| !state.expanded.contains(p));
        let step = state
            .spring
            .update(target.as_deref(), Instant::now(), delay);
        if let (Some(rect), Some(_)) = (hovered_rect, step.remaining) {
            let bar = egui::Rect::from_min_size(
                egui::pos2(rect.left(), rect.bottom() - 2.0),
                egui::vec2(rect.width() * step.progress, 2.0),
            );
            ui.painter().rect_filled(bar, 1.0, palette.primary);
        }
        if let Some(remaining) = step.remaining {
            ctx.request_repaint_after(remaining);
        }
        if let Some(open) = step.open {
            toggles.push(open);
        }
    }
    for path in toggles {
        state.toggle(&path, &ctx);
    }
    action
}
