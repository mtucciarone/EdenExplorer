//! The terminal pane docked under the file view: one or more shells per
//! tab (Command Prompt, PowerShell, PowerShell 7, Git Bash, WSL distros,
//! ...), each in its own session tab, drawn from `core::terminal`'s screen
//! grid. While it has keyboard focus every key goes to the shell (the main
//! window hands it the frame's key events before anything else sees them).

use crate::core::terminal::{Mods, TermKey, TerminalSession, default_color, key_bytes};
use crate::core::terminal_shells::{self, ShellProfile};
use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};
use eframe::egui;
use egui_phosphor::regular;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MIN_HEIGHT: f32 = 120.0;
const SPLITTER: f32 = 6.0;
const HEADER: f32 = 30.0;
const PAD: f32 = 6.0;

/// The shells running in one tab's pane.
#[derive(Default)]
struct Panel {
    sessions: Vec<TerminalSession>,
    active: usize,
    error: Option<String>,
    /// Wheel movement not yet turned into whole lines.
    scroll: f32,
}

/// What the pane asks the main window to do.
#[derive(Default)]
pub struct PanelAction {
    /// The pane is being resized to this height.
    pub new_height: Option<f32>,
    /// Save the settings (a resize ended or the default shell changed).
    pub persist: bool,
    /// Make this shell the one new terminals start with.
    pub set_default_shell: Option<String>,
    pub open_external: bool,
}

#[derive(Default)]
pub struct TerminalPanels {
    panels: HashMap<u64, Panel>,
    /// Tabs whose pane is showing (shells keep running while hidden).
    open: HashSet<u64>,
    /// The terminal had keyboard focus at the end of the last frame.
    focused: bool,
    /// Key events taken for the terminal at the start of this frame.
    input: Vec<egui::Event>,
    /// The toggle shortcut was pressed while the terminal had focus.
    pub toggle_requested: bool,
    /// Focus the terminal once it's drawn (just opened).
    focus_next: bool,
}

impl TerminalPanels {
    pub fn is_open(&self, tab: u64) -> bool {
        self.open.contains(&tab)
    }

    pub fn toggle(&mut self, tab: u64) {
        if !self.open.remove(&tab) {
            self.open.insert(tab);
            self.focus_next = true;
        } else {
            self.focused = false;
        }
    }

    /// Ends the shells of tabs that were closed.
    pub fn retain_tabs(&mut self, tabs: impl Iterator<Item = u64>) {
        if self.panels.is_empty() && self.open.is_empty() {
            return;
        }
        let alive: HashSet<u64> = tabs.collect();
        self.panels.retain(|id, _| alive.contains(id));
        self.open.retain(|id| alive.contains(id));
    }

    /// Called first thing each frame: while the terminal has focus, takes
    /// the frame's keyboard events so no app shortcut or the file list
    /// reacts to them.
    pub fn capture_input(&mut self, ctx: &egui::Context) {
        if !self.focused {
            return;
        }
        use crate::core::keymap::{ShortcutAction, pressed};
        if ctx.input(|i| pressed(i, ShortcutAction::TerminalPane)) {
            self.toggle_requested = true;
        }
        ctx.input_mut(|i| {
            let mut kept = Vec::with_capacity(i.events.len());
            for event in i.events.drain(..) {
                match event {
                    egui::Event::Text(_)
                    | egui::Event::Key { .. }
                    | egui::Event::Copy
                    | egui::Event::Cut
                    | egui::Event::Paste(_)
                    | egui::Event::Ime(_) => self.input.push(event),
                    other => kept.push(other),
                }
            }
            i.events = kept;
            i.keys_down.clear();
        });
    }

    /// Height to keep for `tab`'s pane out of `available`, if it's open.
    pub fn reserved_height(&self, tab: u64, height: f32, available: f32) -> Option<f32> {
        self.is_open(tab).then(|| height.clamp(MIN_HEIGHT, (available - 160.0).max(MIN_HEIGHT)))
    }

    /// Draws `tab`'s pane in `rect`. `dir` is where new shells start and
    /// where "Go To Current Folder" goes.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        tab: u64,
        dir: &Path,
        height: f32,
        prefs: &crate::core::ui_prefs::TerminalPrefs,
        palette: &ThemePalette,
        i18n: &I18n,
    ) -> PanelAction {
        let mut action = PanelAction::default();
        let ctx = ui.ctx().clone();
        let shells = terminal_shells::shells({
            let ctx = ctx.clone();
            move || ctx.request_repaint()
        });
        let input = std::mem::take(&mut self.input);
        let panel = self.panels.entry(tab).or_default();
        let dark = ui.visuals().dark_mode;
        let start_dir = usable_dir(dir);

        // --- Splitter: drag to resize ---
        let splitter = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), SPLITTER));
        let resp = ui.interact(splitter, ui.id().with(("terminal_splitter", tab)), egui::Sense::drag());
        if resp.hovered() || resp.dragged() {
            ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        ui.painter().hline(
            rect.x_range(),
            splitter.center().y,
            egui::Stroke::new(
                1.5,
                if resp.hovered() || resp.dragged() { palette.borders_active } else { palette.borders_default },
            ),
        );
        if resp.dragged() {
            action.new_height = Some(height - resp.drag_delta().y);
        }
        action.persist |= resp.drag_stopped();

        // --- Header: session tabs, new shell, actions ---
        let header = egui::Rect::from_min_size(
            egui::pos2(rect.left(), splitter.bottom()),
            egui::vec2(rect.width(), HEADER),
        );
        let mut close_session = None;
        let mut start_profile: Option<ShellProfile> = None;
        let mut go_to_folder = false;
        let mut hide = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(header.shrink2(egui::vec2(6.0, 3.0))), |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(egui::RichText::new(regular::TERMINAL_WINDOW).color(palette.text_header_section));
                for (i, session) in panel.sessions.iter().enumerate() {
                    let selected = i == panel.active;
                    let label = format!("{} {}", session.profile.icon(), session.profile.name);
                    let mut text = egui::RichText::new(label).size(palette.text_size);
                    if session.exited.is_some() {
                        text = text.weak();
                    }
                    let tab_resp = ui.selectable_label(selected, text);
                    let tab_resp = match &session.title {
                        Some(title) => tab_resp.on_hover_text(title.as_str()),
                        None => tab_resp,
                    };
                    if tab_resp.clicked() {
                        panel.active = i;
                        self.focus_next = true;
                    }
                    if ui
                        .add(egui::Button::new(egui::RichText::new(regular::X).size(palette.tooltip_text_size)).frame(false))
                        .on_hover_text(i18n.tr("terminal_close_shell"))
                        .clicked()
                    {
                        close_session = Some(i);
                    }
                    ui.add_space(4.0);
                }
                let menu = egui::containers::menu::MenuButton::new(
                    egui::RichText::new(regular::PLUS).size(palette.text_size),
                )
                .ui(ui, |ui| match shells {
                    None => {
                        ui.weak(i18n.tr("terminal_finding_shells"));
                    }
                    Some([]) => {
                        ui.weak(i18n.tr("terminal_no_shells"));
                    }
                    Some(list) => {
                        let default_id = terminal_shells::default_shell(list, prefs.default_shell.as_deref()).map(|s| s.id.clone());
                        for profile in list {
                            let is_default = default_id.as_deref() == Some(profile.id.as_str());
                            let label = if is_default {
                                format!("{}  {}  ({})", profile.icon(), profile.name, i18n.tr("terminal_default"))
                            } else {
                                format!("{}  {}", profile.icon(), profile.name)
                            };
                            let item = ui.button(label);
                            if item.clicked() {
                                start_profile = Some(profile.clone());
                                ui.close();
                            }
                        }
                        ui.separator();
                        ui.menu_button(i18n.tr("terminal_set_default"), |ui| {
                            for profile in list {
                                let checked = default_id.as_deref() == Some(profile.id.as_str());
                                if ui.radio(checked, format!("{}  {}", profile.icon(), profile.name)).clicked() {
                                    action.set_default_shell = Some(profile.id.clone());
                                    action.persist = true;
                                    ui.close();
                                }
                            }
                        });
                    }
                });
                menu.0.on_hover_text(i18n.tr("terminal_new_shell"));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let icon_button = |ui: &mut egui::Ui, icon: &str, tip: String| {
                        ui.add(egui::Button::new(egui::RichText::new(icon).size(palette.text_size + 1.0)).frame(false))
                            .on_hover_text(tip)
                            .clicked()
                    };
                    if icon_button(ui, regular::CARET_DOWN, i18n.tr("terminal_hide")) {
                        hide = true;
                    }
                    if icon_button(ui, regular::ARROW_SQUARE_OUT, i18n.tr("tooltip_open_terminal")) {
                        action.open_external = true;
                    }
                    if icon_button(ui, regular::FOLDER_OPEN, i18n.tr("terminal_go_to_folder")) {
                        go_to_folder = true;
                    }
                });
            });
        });
        if hide {
            self.open.remove(&tab);
            self.focused = false;
            return action;
        }
        if let Some(i) = close_session {
            panel.sessions.remove(i);
            panel.active = panel.active.min(panel.sessions.len().saturating_sub(1));
            if panel.sessions.is_empty() {
                // The last shell closed: hide the pane too.
                self.open.remove(&tab);
                self.focused = false;
                return action;
            }
        }

        // --- Terminal body ---
        let body = egui::Rect::from_min_max(egui::pos2(rect.left(), header.bottom()), rect.max);
        let bg = term_background(dark, palette);
        ui.painter().rect_filled(body.shrink(2.0), palette.medium_radius, bg);
        let font = egui::FontId::monospace(prefs.font_size.clamp(8.0, 32.0));
        let cell_w = ui.fonts_mut(|f| f.glyph_width(&font, 'M')).max(1.0);
        let cell_h = ui.fonts_mut(|f| f.row_height(&font)).max(1.0);
        let inner = body.shrink(PAD);
        let cols = ((inner.width() / cell_w).floor() as u16).max(2);
        let rows = ((inner.height() / cell_h).floor() as u16).max(1);
        let cell = (cell_w.round() as u16, cell_h.round() as u16);
        let repaint: Arc<dyn Fn() + Send + Sync> = {
            let ctx = ctx.clone();
            Arc::new(move || ctx.request_repaint())
        };

        // Start a shell: the picked one, or the default when the pane is empty.
        if start_profile.is_none()
            && panel.sessions.is_empty()
            && panel.error.is_none()
            && let Some(list) = shells
        {
            start_profile = terminal_shells::default_shell(list, prefs.default_shell.as_deref()).cloned();
        }
        if let Some(profile) = start_profile {
            match TerminalSession::start(&profile, &start_dir, cols, rows, cell, repaint.clone()) {
                Ok(session) => {
                    panel.sessions.push(session);
                    panel.active = panel.sessions.len() - 1;
                    panel.error = None;
                    self.focus_next = true;
                }
                Err(err) => panel.error = Some(err),
            }
        }

        let view_id = ui.id().with(("terminal_view", tab));
        let Some(session) = panel.sessions.get_mut(panel.active) else {
            let message = match (&panel.error, shells) {
                (Some(err), _) => err.clone(),
                (None, None) => i18n.tr("terminal_finding_shells"),
                (None, Some(_)) => i18n.tr("terminal_no_shells"),
            };
            ui.painter().text(
                inner.left_top(),
                egui::Align2::LEFT_TOP,
                message,
                egui::FontId::proportional(palette.text_size),
                ui.visuals().weak_text_color(),
            );
            if panel.error.is_some() {
                let retry = egui::Rect::from_min_size(inner.left_top() + egui::vec2(0.0, 24.0), egui::vec2(90.0, 24.0));
                if ui.put(retry, egui::Button::new(i18n.tr("terminal_try_again"))).clicked() {
                    panel.error = None;
                }
            }
            self.focused = false;
            return action;
        };
        session.poll_events(|text| {
            crate::core::utils::clipboard::copy_text_to_clipboard(&text);
        });
        session.resize(cols, rows, cell);

        // Focus: click to focus, click elsewhere to leave.
        let resp = ui.interact(body, view_id, egui::Sense::click_and_drag());
        if self.focus_next || resp.clicked() || resp.drag_started() {
            ui.memory_mut(|m| m.request_focus(view_id));
            self.focus_next = false;
        }
        let pressed_outside = ui.input(|i| {
            i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !body.contains(p))
        });
        if pressed_outside && ui.memory(|m| m.has_focus(view_id)) {
            ui.memory_mut(|m| m.surrender_focus(view_id));
        }
        let focused = ui.memory(|m| m.has_focus(view_id));
        if focused {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    view_id,
                    egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
                )
            });
        }
        self.focused = focused;

        // Keys.
        if focused {
            if session.exited.is_some() {
                // Enter restarts an ended shell.
                let enter = input.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::Enter, pressed: true, .. }));
                if enter {
                    let profile = session.profile.clone();
                    match TerminalSession::start(&profile, &start_dir, cols, rows, cell, repaint) {
                        Ok(new) => *session = new,
                        Err(err) => panel.error = Some(err),
                    }
                    return action;
                }
            } else {
                handle_keys(session, input);
            }
        }
        if go_to_folder && session.exited.is_none() {
            session.write(session.profile.cd_command(&start_dir).into_bytes());
            ui.memory_mut(|m| m.request_focus(view_id));
        }

        // Mouse: wheel scrolls back, drag selects, right-click menu.
        handle_mouse(ui, &resp, session, &mut panel.scroll, inner, cell_w, cell_h, i18n);

        draw_grid(ui, session, inner, &font, cell_w, cell_h, dark, focused, palette, i18n);
        action
    }
}

fn usable_dir(dir: &Path) -> PathBuf {
    if dir.is_dir() {
        dir.to_path_buf()
    } else {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from(r"C:\"))
    }
}

fn term_background(dark: bool, palette: &ThemePalette) -> egui::Color32 {
    if dark { egui::Color32::from_rgb(12, 12, 14) } else { palette.input_field_bg }
}

fn to_color32(rgb: Rgb) -> egui::Color32 {
    egui::Color32::from_rgb(rgb.r, rgb.g, rgb.b)
}

fn resolve(color: Color, colors: &Colors, dark: bool) -> Rgb {
    match color {
        Color::Spec(rgb) => rgb,
        Color::Named(name) => colors[name].unwrap_or_else(|| default_color(name as usize, dark)),
        Color::Indexed(i) => colors[i as usize].unwrap_or_else(|| default_color(i as usize, dark)),
    }
}

fn map_key(key: egui::Key) -> Option<TermKey> {
    use egui::Key as K;
    Some(match key {
        K::Enter => TermKey::Enter,
        K::Backspace => TermKey::Backspace,
        K::Tab => TermKey::Tab,
        K::Escape => TermKey::Escape,
        K::ArrowUp => TermKey::Up,
        K::ArrowDown => TermKey::Down,
        K::ArrowRight => TermKey::Right,
        K::ArrowLeft => TermKey::Left,
        K::Home => TermKey::Home,
        K::End => TermKey::End,
        K::Insert => TermKey::Insert,
        K::Delete => TermKey::Delete,
        K::PageUp => TermKey::PageUp,
        K::PageDown => TermKey::PageDown,
        K::Space => TermKey::Space,
        K::F1 => TermKey::F(1),
        K::F2 => TermKey::F(2),
        K::F3 => TermKey::F(3),
        K::F4 => TermKey::F(4),
        K::F5 => TermKey::F(5),
        K::F6 => TermKey::F(6),
        K::F7 => TermKey::F(7),
        K::F8 => TermKey::F(8),
        K::F9 => TermKey::F(9),
        K::F10 => TermKey::F(10),
        K::F11 => TermKey::F(11),
        K::F12 => TermKey::F(12),
        other => {
            let name = other.name();
            let c = name.chars().next()?;
            if name.len() == 1 && c.is_ascii_alphabetic() {
                TermKey::Letter(c.to_ascii_lowercase())
            } else {
                return None;
            }
        }
    })
}

fn copy_selection(session: &TerminalSession) -> bool {
    match session.term.lock().selection_to_string() {
        Some(text) if !text.is_empty() => crate::core::utils::clipboard::copy_text_to_clipboard(&text),
        _ => false,
    }
}

fn handle_keys(session: &mut TerminalSession, input: Vec<egui::Event>) {
    let app_cursor = session.term.lock().mode().contains(TermMode::APP_CURSOR);
    let to_bottom = |session: &TerminalSession| session.term.lock().scroll_display(Scroll::Bottom);
    for event in input {
        match event {
            egui::Event::Text(text) => {
                to_bottom(session);
                session.write(text.into_bytes());
            }
            egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                to_bottom(session);
                session.write(text.into_bytes());
            }
            // Ctrl+C copies the selection if there is one, otherwise it
            // interrupts the program as usual.
            egui::Event::Copy => {
                if !copy_selection(session) {
                    session.write(&b"\x03"[..]);
                }
                session.term.lock().selection = None;
            }
            egui::Event::Cut => session.write(&b"\x18"[..]),
            egui::Event::Paste(text) => {
                to_bottom(session);
                session.paste(&text);
            }
            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                let mods = Mods { shift: modifiers.shift, alt: modifiers.alt, ctrl: modifiers.ctrl };
                // Ctrl+Shift+C / Ctrl+Shift+V: copy / paste, as in Windows Terminal.
                if mods.ctrl && mods.shift && key == egui::Key::C {
                    copy_selection(session);
                    continue;
                }
                if mods.ctrl && mods.shift && key == egui::Key::V {
                    if let Some(text) = crate::core::utils::clipboard::read_text_from_clipboard() {
                        session.paste(&text);
                    }
                    continue;
                }
                // Shift+PageUp/PageDown scroll the history.
                if mods.shift && matches!(key, egui::Key::PageUp | egui::Key::PageDown) {
                    let lines = session.term.lock().screen_lines() as i32 - 1;
                    let delta = if key == egui::Key::PageUp { lines } else { -lines };
                    session.term.lock().scroll_display(Scroll::Delta(delta));
                    continue;
                }
                // Already sent as Copy/Cut/Paste events.
                let clipboard_key = (mods.ctrl && !mods.shift && matches!(key, egui::Key::C | egui::Key::X | egui::Key::V))
                    || (matches!(key, egui::Key::Insert) && (mods.shift || mods.ctrl));
                if clipboard_key {
                    continue;
                }
                let Some(term_key) = map_key(key) else { continue };
                // Letters only with Ctrl (typed letters arrive as text;
                // Ctrl+Alt is AltGr, which also types text).
                if matches!(term_key, TermKey::Letter(_)) && (!mods.ctrl || mods.alt) {
                    continue;
                }
                let mods = if matches!(term_key, TermKey::Letter(_)) { Mods { alt: false, ..mods } } else { mods };
                if let Some(bytes) = key_bytes(term_key, mods, app_cursor) {
                    to_bottom(session);
                    session.write(bytes);
                }
            }
            _ => {}
        }
    }
}

/// The grid cell under `pos`, and which half of it.
fn cell_at(pos: egui::Pos2, inner: egui::Rect, cell_w: f32, cell_h: f32, session: &TerminalSession) -> (Point, Side) {
    let term = session.term.lock();
    let cols = term.columns();
    let rows = term.screen_lines();
    let offset = term.grid().display_offset() as i32;
    drop(term);
    let x = ((pos.x - inner.left()) / cell_w).max(0.0);
    let y = ((pos.y - inner.top()) / cell_h).max(0.0);
    let col = (x.floor() as usize).min(cols.saturating_sub(1));
    let row = (y.floor() as usize).min(rows.saturating_sub(1));
    let side = if x.fract() < 0.5 { Side::Left } else { Side::Right };
    (Point::new(Line(row as i32 - offset), Column(col)), side)
}

#[allow(clippy::too_many_arguments)]
fn handle_mouse(
    ui: &mut egui::Ui,
    resp: &egui::Response,
    session: &mut TerminalSession,
    scroll: &mut f32,
    inner: egui::Rect,
    cell_w: f32,
    cell_h: f32,
    i18n: &I18n,
) {
    // Wheel: history, or arrow keys in full-screen programs that ask for it.
    if resp.hovered() {
        let delta = ui.input(|i| i.smooth_scroll_delta.y);
        if delta != 0.0 {
            *scroll += delta / cell_h;
            let lines = scroll.trunc() as i32;
            *scroll -= lines as f32;
            if lines != 0 {
                let mode = *session.term.lock().mode();
                if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
                    let key = if lines > 0 { TermKey::Up } else { TermKey::Down };
                    let app_cursor = mode.contains(TermMode::APP_CURSOR);
                    for _ in 0..lines.unsigned_abs() {
                        if let Some(bytes) = key_bytes(key, Mods::default(), app_cursor) {
                            session.write(bytes);
                        }
                    }
                } else {
                    session.term.lock().scroll_display(Scroll::Delta(lines));
                }
            }
        }
    }

    // Selection: drag for characters, double-click a word, triple-click a line.
    let pointer = ui.input(|i| i.pointer.interact_pos());
    if let Some(pos) = pointer {
        let (point, side) = cell_at(pos, inner, cell_w, cell_h, session);
        let clicks = ui.input(|i| {
            if i.pointer.button_triple_clicked(egui::PointerButton::Primary) {
                3
            } else if i.pointer.button_double_clicked(egui::PointerButton::Primary) {
                2
            } else {
                0
            }
        });
        if resp.hovered() && clicks > 0 {
            let ty = if clicks == 3 { SelectionType::Lines } else { SelectionType::Semantic };
            session.term.lock().selection = Some(Selection::new(ty, point, side));
        } else if resp.drag_started_by(egui::PointerButton::Primary) {
            // From where the button went down, not where the drag was noticed.
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
            let (start, start_side) = cell_at(origin, inner, cell_w, cell_h, session);
            let mut selection = Selection::new(SelectionType::Simple, start, start_side);
            selection.update(point, side);
            session.term.lock().selection = Some(selection);
        } else if resp.dragged_by(egui::PointerButton::Primary) {
            if let Some(selection) = session.term.lock().selection.as_mut() {
                selection.update(point, side);
            }
        } else if resp.clicked() {
            session.term.lock().selection = None;
        }
    }

    resp.context_menu(|ui| {
        let has_selection = session.term.lock().selection.is_some();
        if ui.add_enabled(has_selection, egui::Button::new(i18n.tr("terminal_copy"))).clicked() {
            copy_selection(session);
            ui.close();
        }
        if ui.button(i18n.tr("terminal_paste")).clicked() {
            if let Some(text) = crate::core::utils::clipboard::read_text_from_clipboard() {
                session.paste(&text);
            }
            ui.close();
        }
        if ui.button(i18n.tr("terminal_select_all")).clicked() {
            let mut term = session.term.lock();
            let top = Point::new(term.topmost_line(), Column(0));
            let bottom = Point::new(term.bottommost_line(), term.last_column());
            let mut selection = Selection::new(SelectionType::Simple, top, Side::Left);
            selection.update(bottom, Side::Right);
            term.selection = Some(selection);
            ui.close();
        }
        if ui.button(i18n.tr("terminal_clear")).clicked() {
            let mut term = session.term.lock();
            term.grid_mut().clear_history();
            term.selection = None;
            drop(term);
            // Ctrl+L redraws the prompt at the top (cmd ignores it).
            session.write(&b"\x0c"[..]);
            ui.close();
        }
    });
}

/// Paints the visible screen: backgrounds, text in runs of the same style,
/// the selection, and the cursor.
#[allow(clippy::too_many_arguments)]
fn draw_grid(
    ui: &mut egui::Ui,
    session: &TerminalSession,
    inner: egui::Rect,
    font: &egui::FontId,
    cell_w: f32,
    cell_h: f32,
    dark: bool,
    focused: bool,
    palette: &ThemePalette,
    i18n: &I18n,
) {
    let painter = ui.painter().with_clip_rect(inner.expand(1.0));
    let term = session.term.lock();
    let content = term.renderable_content();
    let colors = content.colors;
    let offset = content.display_offset as i32;
    let selection = content.selection;
    let default_fg = to_color32(resolve(Color::Named(NamedColor::Foreground), colors, dark));
    let selection_bg = palette.row_selected_bg;
    let selection_fg = palette.item_viewer_row_text_selected;

    struct Run {
        row: i32,
        col: usize,
        text: String,
        fg: egui::Color32,
        italic: bool,
        underline: bool,
        strike: bool,
    }
    let flush = |run: &mut Option<Run>| {
        let Some(run) = run.take() else { return };
        if run.text.trim().is_empty() && !run.underline && !run.strike {
            return;
        }
        let stroke = |on: bool| if on { egui::Stroke::new(1.0, run.fg) } else { egui::Stroke::NONE };
        let mut job = egui::text::LayoutJob::default();
        job.append(
            &run.text,
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color: run.fg,
                italics: run.italic,
                underline: stroke(run.underline),
                strikethrough: stroke(run.strike),
                ..Default::default()
            },
        );
        let galley = painter.layout_job(job);
        let pos = egui::pos2(inner.left() + run.col as f32 * cell_w, inner.top() + run.row as f32 * cell_h);
        painter.galley(pos, galley, run.fg);
    };
    let mut run: Option<Run> = None;

    for indexed in content.display_iter {
        let cell = indexed.cell;
        let point = indexed.point;
        let row = point.line.0 + offset;
        let col = point.column.0;
        if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            continue;
        }
        let width = if cell.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
        let mut fg_color = cell.fg;
        // Bold text uses the bright variant of the 8 basic colors.
        if cell.flags.contains(Flags::BOLD)
            && let Color::Named(name) = fg_color
            && (name as usize) < 8
        {
            fg_color = Color::Indexed(name as u8 + 8);
        }
        let mut fg = to_color32(resolve(fg_color, colors, dark));
        let default_bg = matches!(cell.bg, Color::Named(NamedColor::Background)) && colors[NamedColor::Background].is_none();
        let mut bg = (!default_bg).then(|| to_color32(resolve(cell.bg, colors, dark)));
        if cell.flags.contains(Flags::DIM) {
            fg = fg.gamma_multiply(0.66);
        }
        if cell.flags.contains(Flags::INVERSE) {
            let old_bg = bg.unwrap_or_else(|| term_background(dark, palette));
            bg = Some(fg);
            fg = old_bg;
        }
        if selection.is_some_and(|s| s.contains(point)) {
            bg = Some(selection_bg);
            fg = selection_fg;
        }
        if cell.flags.contains(Flags::HIDDEN) {
            fg = egui::Color32::TRANSPARENT;
        }
        let cell_rect = egui::Rect::from_min_size(
            egui::pos2(inner.left() + col as f32 * cell_w, inner.top() + row as f32 * cell_h),
            egui::vec2(cell_w * width as f32, cell_h),
        );
        if let Some(bg) = bg {
            painter.rect_filled(cell_rect, 0.0, bg);
        }
        let italic = cell.flags.contains(Flags::ITALIC);
        let underline = cell.flags.intersects(Flags::ALL_UNDERLINES);
        let strike = cell.flags.contains(Flags::STRIKEOUT);
        let c = if cell.c == '\t' { ' ' } else { cell.c };
        let zerowidth = cell.zerowidth();
        // ASCII runs keep their columns in a monospace font; anything
        // wider (CJK, emoji, combining marks) is placed cell by cell.
        if c.is_ascii() && width == 1 && zerowidth.is_none() {
            let joins = run.as_ref().is_some_and(|r| {
                r.row == row
                    && r.col + r.text.len() == col
                    && r.fg == fg
                    && (r.italic, r.underline, r.strike) == (italic, underline, strike)
            });
            if joins {
                if let Some(r) = run.as_mut() {
                    r.text.push(c);
                }
            } else {
                flush(&mut run);
                run = Some(Run { row, col, text: c.to_string(), fg, italic, underline, strike });
            }
        } else {
            flush(&mut run);
            let mut text = c.to_string();
            if let Some(marks) = zerowidth {
                text.extend(marks.iter());
            }
            run = Some(Run { row, col, text, fg, italic, underline, strike });
            flush(&mut run);
        }
    }
    flush(&mut run);

    // Cursor.
    let cursor = content.cursor;
    let cursor_row = cursor.point.line.0 + offset;
    if cursor.shape != CursorShape::Hidden && session.exited.is_none() && cursor_row >= 0 {
        let rect = egui::Rect::from_min_size(
            egui::pos2(
                inner.left() + cursor.point.column.0 as f32 * cell_w,
                inner.top() + cursor_row as f32 * cell_h,
            ),
            egui::vec2(cell_w, cell_h),
        );
        let color = colors[NamedColor::Cursor].map(to_color32).unwrap_or(default_fg);
        match (cursor.shape, focused) {
            (CursorShape::Beam, _) => {
                painter.rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(2.0, cell_h)), 0.0, color);
            }
            (CursorShape::Underline, _) => {
                painter.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(rect.left(), rect.bottom() - 2.0), egui::vec2(cell_w, 2.0)),
                    0.0,
                    color,
                );
            }
            (_, true) => {
                painter.rect_filled(rect, 0.0, color.gamma_multiply(0.75));
            }
            _ => {
                painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, color), egui::StrokeKind::Inside);
            }
        }
    }
    drop(term);

    if let Some(code) = session.exited {
        let text = format!("{} {code}. {}", i18n.tr("terminal_exited"), i18n.tr("terminal_restart_hint"));
        let pos = egui::pos2(inner.left(), inner.bottom() - cell_h);
        painter.rect_filled(egui::Rect::from_min_size(pos, egui::vec2(inner.width(), cell_h)), 0.0, palette.sidebar_bg_color);
        painter.text(pos, egui::Align2::LEFT_TOP, text, font.clone(), ui.visuals().weak_text_color());
    }
}
