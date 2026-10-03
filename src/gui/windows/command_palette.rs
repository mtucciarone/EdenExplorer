//! The command palette (Ctrl+Shift+P): a search box at the top of the
//! window listing every app command (the keyboard shortcut actions, view
//! modes, toggles, Settings pages, Quick Look, Analyze Disk Usage, ...) and
//! the favorite and recent folders to jump to, filtered as you type with
//! `core::fuzzy`. ↑/↓ pick, Enter runs, Esc closes; recently run commands
//! come first when the box is empty.

use crate::core::fuzzy;
use crate::core::keymap::{ShortcutAction, combos};
use crate::core::toolbar::ToolbarItem;
use crate::core::utils::widgets::modal_frame;
use crate::gui::i18n::I18n;
use crate::gui::windows::containers::enums::ItemViewerAction;
use crate::gui::windows::containers::structs::{ItemViewerDisplayMode, ItemViewerNavBarAction, TopbarAction};
use crate::gui::windows::mainwindow::MainWindow;
use crate::gui::windows::settings::SettingsCategory;
use eframe::egui;
use egui_phosphor::regular;
use std::path::PathBuf;

/// Results shown at once (the list scrolls within this).
const MAX_RESULTS: usize = 60;
/// Recently run commands remembered for the empty query.
const MAX_RECENT: usize = 6;
const RECENT_FOLDERS: usize = 15;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Shortcut(ShortcutAction),
    View(ItemViewerDisplayMode, ToolbarItem),
    Terminal,
    ToggleTheme,
    ToggleSidebar,
    ToggleSplit,
    ToggleHidden,
    Settings(SettingsCategory),
    AnalyzeDiskUsage,
    QuickLook,
    AddFavorite,
    About,
    GoTo(PathBuf),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Group {
    Command,
    View,
    Settings,
    Folder,
}

impl Group {
    fn i18n_key(self) -> &'static str {
        match self {
            Group::Command => "palette_group_command",
            Group::View => "palette_group_view",
            Group::Settings => "palette_group_settings",
            Group::Folder => "palette_group_folder",
        }
    }
}

struct Entry {
    command: Command,
    icon: &'static str,
    label: String,
    /// Shown dimmed after the label (a folder's path).
    detail: Option<String>,
    group: Group,
    shortcut: Option<String>,
}

pub struct CommandPaletteState {
    query: String,
    selected: usize,
    entries: Vec<Entry>,
    /// Indices into `entries` for the current query, best first.
    shown: Vec<usize>,
    shown_for: Option<String>,
    focus_requested: bool,
}

fn shortcut_icon(action: ShortcutAction) -> &'static str {
    match action {
        ShortcutAction::NewTab => regular::PLUS,
        ShortcutAction::CloseTab => regular::X,
        ShortcutAction::NextTab => regular::ARROW_SQUARE_RIGHT,
        ShortcutAction::PreviousTab => regular::ARROW_SQUARE_LEFT,
        ShortcutAction::Back => regular::ARROW_LEFT,
        ShortcutAction::Forward => regular::ARROW_RIGHT,
        ShortcutAction::Up => regular::ARROW_UP,
        ShortcutAction::Refresh => regular::ARROWS_CLOCKWISE,
        ShortcutAction::AddressBar => regular::TEXT_T,
        ShortcutAction::Search => regular::MAGNIFYING_GLASS,
        ShortcutAction::SelectAll => regular::CHECK_SQUARE,
        ShortcutAction::InvertSelection => regular::SWAP,
        ShortcutAction::SelectByPattern => regular::ASTERISK,
        ShortcutAction::CopyPath => regular::LINK,
        ShortcutAction::Rename => regular::PENCIL_SIMPLE,
        ShortcutAction::NewFolder => regular::FOLDER_PLUS,
        ShortcutAction::Properties => regular::INFO,
        ShortcutAction::Undo => regular::ARROW_COUNTER_CLOCKWISE,
        ShortcutAction::Redo => regular::ARROW_CLOCKWISE,
        ShortcutAction::Fullscreen => regular::ARROWS_OUT,
        ShortcutAction::PerformancePanel => regular::GAUGE,
        ShortcutAction::CommandPalette => regular::COMMAND,
        ShortcutAction::TerminalPane => regular::TERMINAL_WINDOW,
        ShortcutAction::FilterBar => regular::FUNNEL,
    }
}

fn folder_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Indices of `labels` matching `query`, best first. Each item is
/// `(label, detail)`; the detail (a path) counts, a little less.
pub fn rank(query: &str, items: &[(&str, Option<&str>)]) -> Vec<usize> {
    let mut scored: Vec<(i32, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, (label, detail))| {
            let on_label = fuzzy::score(query, label);
            let on_detail = detail.and_then(|d| fuzzy::score_strict(query, d)).map(|s| s / 2);
            on_label.max(on_detail).map(|s| (s, i))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, i)| i).collect()
}

impl MainWindow {
    fn palette_entries(&self) -> Vec<Entry> {
        let i18n = &self.i18n;
        let mut out = Vec::new();
        let mut add = |command: Command, icon: &'static str, label: String, group: Group, shortcut: Option<String>| {
            out.push(Entry { command, icon, label, detail: None, group, shortcut });
        };
        for action in ShortcutAction::ALL {
            if action == ShortcutAction::CommandPalette {
                continue;
            }
            let shortcut = combos(action).first().map(|c| c.label());
            add(Command::Shortcut(action), shortcut_icon(action), i18n.tr(action.i18n_key()), Group::Command, shortcut);
        }
        add(Command::QuickLook, regular::EYE, i18n.tr("palette_quick_look"), Group::Command, Some("Space".into()));
        add(Command::AnalyzeDiskUsage, regular::CHART_PIE_SLICE, i18n.tr("palette_analyze_disk_usage"), Group::Command, None);
        add(Command::Terminal, regular::TERMINAL, i18n.tr("tooltip_open_terminal"), Group::Command, None);
        add(Command::AddFavorite, regular::STAR, i18n.tr("palette_add_favorite"), Group::Command, None);
        for (mode, item) in [
            (ItemViewerDisplayMode::Details, ToolbarItem::ViewDetails),
            (ItemViewerDisplayMode::Gallery, ToolbarItem::ViewGallery),
            (ItemViewerDisplayMode::Columns, ToolbarItem::ViewColumns),
            (ItemViewerDisplayMode::ColumnPreview, ToolbarItem::ViewColumnPreview),
            (ItemViewerDisplayMode::Preview, ToolbarItem::ViewPreview),
            (ItemViewerDisplayMode::DetailPreview, ToolbarItem::ViewDetailPreview),
        ] {
            let (icon, key) = crate::gui::windows::containers::itemviewer_navbar::toolbar_item_icon_and_key(item);
            add(Command::View(mode, item), icon, format!("{}: {}", i18n.tr("palette_view"), i18n.tr(key)), Group::View, None);
        }
        let hidden_key = if self.settings_window.current_settings.show_hidden_files_folders {
            "palette_hide_hidden"
        } else {
            "palette_show_hidden"
        };
        add(Command::ToggleHidden, regular::EYE_SLASH, i18n.tr(hidden_key), Group::View, None);
        add(Command::ToggleTheme, regular::SUN, i18n.tr("palette_toggle_theme"), Group::View, None);
        add(Command::ToggleSidebar, regular::SIDEBAR_SIMPLE, i18n.tr("palette_toggle_sidebar"), Group::View, None);
        add(Command::ToggleSplit, regular::COLUMNS, i18n.tr("palette_toggle_split"), Group::View, None);
        for category in SettingsCategory::ALL {
            add(
                Command::Settings(category),
                category.icon(),
                format!("{}: {}", i18n.tr("palette_settings"), category.label(i18n)),
                Group::Settings,
                None,
            );
        }
        add(Command::About, regular::INFO, i18n.tr("about"), Group::Settings, None);

        // Folders: favorites, then recent ones not already listed.
        let mut seen = std::collections::HashSet::new();
        for favorite in &self.sidebar_state.favorites {
            if seen.insert(favorite.path.clone()) {
                out.push(Entry {
                    command: Command::GoTo(favorite.path.clone()),
                    icon: regular::STAR,
                    label: favorite.label.clone(),
                    detail: Some(favorite.path.display().to_string()),
                    group: Group::Folder,
                    shortcut: None,
                });
            }
        }
        for path in self.recent_locations_state.items.iter().take(RECENT_FOLDERS) {
            if seen.insert(path.clone()) {
                out.push(Entry {
                    command: Command::GoTo(path.clone()),
                    icon: regular::CLOCK_COUNTER_CLOCKWISE,
                    label: folder_name(path),
                    detail: Some(path.display().to_string()),
                    group: Group::Folder,
                    shortcut: None,
                });
            }
        }
        out
    }

    pub(crate) fn toggle_command_palette(&mut self) {
        if self.command_palette.is_some() {
            self.command_palette = None;
            return;
        }
        self.close_quick_look();
        let entries = self.palette_entries();
        self.command_palette = Some(CommandPaletteState {
            query: String::new(),
            selected: 0,
            entries,
            shown: Vec::new(),
            shown_for: None,
            focus_requested: false,
        });
    }

    pub(crate) fn draw_command_palette(&mut self, ctx: &egui::Context, palette: &crate::gui::theme::ThemePalette) {
        let Some(mut state) = self.command_palette.take() else { return };

        if state.shown_for.as_deref() != Some(state.query.as_str()) {
            state.shown = if state.query.trim().is_empty() {
                // Recent commands first, then everything else in order.
                let mut order: Vec<usize> = self
                    .palette_recent
                    .iter()
                    .filter_map(|c| state.entries.iter().position(|e| &e.command == c))
                    .collect();
                let rest: Vec<usize> = (0..state.entries.len()).filter(|i| !order.contains(i)).collect();
                order.extend(rest);
                order
            } else {
                let items: Vec<(&str, Option<&str>)> =
                    state.entries.iter().map(|e| (e.label.as_str(), e.detail.as_deref())).collect();
                rank(&state.query, &items)
            };
            state.shown.truncate(MAX_RESULTS);
            state.shown_for = Some(state.query.clone());
            state.selected = 0;
        }

        let (up, down, enter, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        let count = state.shown.len();
        if count > 0 {
            if down {
                state.selected = (state.selected + 1) % count;
            }
            if up {
                state.selected = (state.selected + count - 1) % count;
            }
        }
        let mut run: Option<Command> = None;
        if enter && let Some(&idx) = state.shown.get(state.selected) {
            run = Some(state.entries[idx].command.clone());
        }
        let mut close = escape;

        egui::Area::new(egui::Id::new("command_palette_scrim"))
            .order(egui::Order::Middle)
            .interactable(true)
            .show(ctx, |ui| {
                let rect = ctx.content_rect();
                ui.painter().rect_filled(rect, 0.0, palette.modal_background_effect_color.gamma_multiply(0.6));
                if ui.interact(rect, ui.id().with("click"), egui::Sense::click()).clicked() {
                    close = true;
                }
            });

        let width = (ctx.content_rect().width() - 40.0).clamp(320.0, 620.0);
        let i18n: &I18n = &self.i18n;
        let scroll_to_selected = up || down;
        egui::Area::new(egui::Id::new("command_palette_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 56.0))
            .show(ctx, |ui| {
                modal_frame(&ctx.style_of(ctx.theme()), palette)
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(regular::MAGNIFYING_GLASS).size(palette.text_size + 2.0).color(palette.icon_color));
                            let edit = ui.add(
                                egui::TextEdit::singleline(&mut state.query)
                                    .hint_text(i18n.tr("palette_hint"))
                                    .frame(egui::Frame::NONE)
                                    .desired_width(f32::INFINITY)
                                    .font(egui::FontId::proportional(palette.text_size + 2.0)),
                            );
                            if !state.focus_requested {
                                edit.request_focus();
                                state.focus_requested = true;
                            }
                            // The results were worked out before this edit;
                            // draw again so they match what was typed.
                            if edit.changed() {
                                ui.ctx().request_repaint();
                            }
                        });
                        ui.separator();

                        if state.shown.is_empty() {
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new(i18n.tr("palette_no_matches")).color(palette.text_normal.gamma_multiply(0.6)));
                            ui.add_space(6.0);
                            return;
                        }
                        let row_height = palette.text_size + 14.0;
                        egui::ScrollArea::vertical()
                            .max_height(row_height * 12.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.style_mut().interaction.selectable_labels = false;
                                let mut last_group = None;
                                let recent_count = if state.query.trim().is_empty() { self.palette_recent.len() } else { 0 };
                                for (row, &idx) in state.shown.iter().enumerate() {
                                    let entry = &state.entries[idx];
                                    // Group headings when browsing (not while searching).
                                    if state.query.trim().is_empty() {
                                        let heading = if row < recent_count { None } else { Some(entry.group) };
                                        if row == 0 && recent_count > 0 {
                                            ui.label(egui::RichText::new(i18n.tr("palette_group_recent")).size(palette.text_size - 2.0).color(palette.text_normal.gamma_multiply(0.55)));
                                        }
                                        if heading.is_some() && heading != last_group {
                                            ui.add_space(2.0);
                                            ui.label(egui::RichText::new(i18n.tr(entry.group.i18n_key())).size(palette.text_size - 2.0).color(palette.text_normal.gamma_multiply(0.55)));
                                            last_group = heading;
                                        }
                                    }
                                    let selected = row == state.selected;
                                    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_height), egui::Sense::click());
                                    if selected || response.hovered() {
                                        ui.painter().rect_filled(
                                            rect,
                                            egui::CornerRadius::same(palette.small_radius),
                                            if selected { palette.row_selected_bg } else { palette.row_bg },
                                        );
                                    }
                                    if selected && scroll_to_selected {
                                        response.scroll_to_me(None);
                                    }
                                    let text_color = if selected { palette.item_viewer_row_text_selected } else { palette.text_normal };
                                    let painter = ui.painter_at(rect);
                                    let y = rect.center().y;
                                    painter.text(
                                        egui::pos2(rect.left() + 10.0, y),
                                        egui::Align2::LEFT_CENTER,
                                        entry.icon,
                                        egui::FontId::proportional(palette.text_size + 1.0),
                                        if selected { text_color } else { palette.icon_color },
                                    );
                                    let label_rect = painter.text(
                                        egui::pos2(rect.left() + 36.0, y),
                                        egui::Align2::LEFT_CENTER,
                                        &entry.label,
                                        egui::FontId::proportional(palette.text_size),
                                        text_color,
                                    );
                                    let mut right = rect.right() - 10.0;
                                    if let Some(shortcut) = &entry.shortcut {
                                        let r = painter.text(
                                            egui::pos2(right, y),
                                            egui::Align2::RIGHT_CENTER,
                                            shortcut,
                                            egui::FontId::monospace(palette.text_size - 2.0),
                                            text_color.gamma_multiply(0.7),
                                        );
                                        right = r.left() - 12.0;
                                    }
                                    if let Some(detail) = &entry.detail
                                        && right > label_rect.right() + 40.0
                                    {
                                        let detail_painter = ui.painter_at(egui::Rect::from_min_max(
                                            egui::pos2(label_rect.right() + 12.0, rect.top()),
                                            egui::pos2(right, rect.bottom()),
                                        ));
                                        detail_painter.text(
                                            egui::pos2(label_rect.right() + 12.0, y),
                                            egui::Align2::LEFT_CENTER,
                                            detail,
                                            egui::FontId::proportional(palette.text_size - 2.0),
                                            text_color.gamma_multiply(0.55),
                                        );
                                    }
                                    if response.clicked() {
                                        run = Some(entry.command.clone());
                                    }
                                    if response.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                                        // Follow the mouse like a menu, without fighting the keys.
                                        state.selected = row;
                                    }
                                }
                            });
                    });
            });

        if let Some(command) = run {
            self.palette_recent.retain(|c| c != &command);
            if !matches!(command, Command::GoTo(_)) {
                self.palette_recent.insert(0, command.clone());
                self.palette_recent.truncate(MAX_RECENT);
            }
            self.run_palette_command(command);
        } else if !close {
            self.command_palette = Some(state);
        }
    }

    fn run_palette_command(&mut self, command: Command) {
        match command {
            Command::Shortcut(action) => self.run_shortcut(action),
            // The same as the toolbar's view buttons.
            Command::View(mode, _) => {
                let side = self.focused_split;
                self.active_tab_mut().view_mut(side).display_mode = mode;
            }
            Command::Terminal => {
                let dir = self.current_nav().current.clone();
                crate::gui::windows::containers::itemviewer_navbar::open_default_terminal(&dir);
            }
            Command::ToggleTheme => self.handle_topbar_action(Some(TopbarAction {
                toggle_theme: true,
                ..Default::default()
            })),
            Command::ToggleSidebar => self.handle_topbar_action(Some(TopbarAction {
                toggle_sidebar: true,
                ..Default::default()
            })),
            Command::ToggleSplit => self.handle_topbar_action(Some(TopbarAction {
                toggle_active_tab_split: true,
                ..Default::default()
            })),
            Command::About => self.handle_topbar_action(Some(TopbarAction {
                about: true,
                ..Default::default()
            })),
            Command::ToggleHidden => {
                let settings = &mut self.settings_window.current_settings;
                settings.show_hidden_files_folders = !settings.show_hidden_files_folders;
                self.save_app_settings_to_disk();
                self.load_path();
            }
            Command::Settings(category) => {
                self.settings_window.selected_category = category;
                self.open_or_focus_settings_tab();
            }
            Command::AnalyzeDiskUsage => {
                let dir = self.current_nav().current.clone();
                self.analyze_disk_usage(dir);
            }
            Command::QuickLook => {
                crate::gui::windows::mainwindow_imp::handle_pending_actions(Some(ItemViewerAction::ToggleQuickLook), self);
            }
            Command::AddFavorite => self.add_favorite(),
            Command::GoTo(path) => self.handle_tabbar_action(
                Some(ItemViewerNavBarAction {
                    nav_to: Some(path),
                    ..Default::default()
                }),
                None,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::rank;

    #[test]
    fn ranking_prefers_labels_then_paths() {
        let items = [
            ("Invert Selection", None),
            ("New Tab", None),
            ("Projects", Some(r"C:\Users\me\Documents\Projects")),
            ("Downloads", Some(r"C:\Users\me\Downloads")),
        ];
        assert_eq!(rank("nt", &items)[0], 1);
        // "docs" only matches the Projects path (…\Documents\…).
        assert_eq!(rank("docpro", &items), vec![2]);
        // Loose letter-by-letter hits in a path don't count.
        assert!(!rank("sme", &items).contains(&3));
        assert!(rank("zzz", &items).is_empty());
        assert_eq!(rank("", &items), vec![0, 1, 2, 3]);
    }
}
