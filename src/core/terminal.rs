//! A running shell for the terminal pane: the shell runs in a Windows
//! pseudo console (ConPTY) and its output is parsed into a screen grid by
//! `alacritty_terminal` (the terminal emulator behind Alacritty), on its
//! own reader thread. The pane (`gui::windows::terminal_panel`) draws the
//! grid and sends keys back.

use crate::core::terminal_shells::ShellProfile;
use alacritty_terminal::event::{Event, EventListener, Notify, OnResize, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{NamedColor, Rgb};
use crossbeam_channel::{Receiver, Sender};
use std::borrow::Cow;
use std::path::Path;
use std::sync::Arc;

/// Scrollback kept per terminal.
const SCROLLBACK_LINES: usize = 10_000;

#[derive(Clone)]
pub struct Listener {
    tx: Sender<Event>,
    repaint: Arc<dyn Fn() + Send + Sync>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let _ = self.tx.send(event);
        (self.repaint)();
    }
}

pub type SharedTerm = Arc<FairMutex<Term<Listener>>>;

pub struct TerminalSession {
    pub profile: ShellProfile,
    pub term: SharedTerm,
    notifier: Notifier,
    events: Receiver<Event>,
    /// The title the shell set (PowerShell shows the folder), if any.
    pub title: Option<String>,
    /// Exit code once the shell has ended.
    pub exited: Option<i32>,
    size: WindowSize,
}

impl TerminalSession {
    /// Starts `profile` in `dir` with a `cols` × `rows` screen.
    pub fn start(
        profile: &ShellProfile,
        dir: &Path,
        cols: u16,
        rows: u16,
        cell: (u16, u16),
        repaint: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<TerminalSession, String> {
        let launch = profile.launch(dir);
        let program = format!("\"{}\"", launch.program.display());
        let options = tty::Options {
            shell: Some(tty::Shell::new(program, launch.args)),
            working_directory: dir.is_dir().then(|| dir.to_path_buf()),
            drain_on_exit: true,
            env: launch.env.into_iter().collect(),
            escape_args: true,
        };
        let size = WindowSize { num_lines: rows.max(1), num_cols: cols.max(2), cell_width: cell.0, cell_height: cell.1 };
        // The library asserts when the pseudo console can't be created
        // (Windows before 10 1809); treat that as an error, not a crash.
        let pty = std::panic::catch_unwind(|| tty::new(&options, size, 0))
            .map_err(|_| "Windows couldn't create a pseudo console for the terminal.".to_string())?
            .map_err(|e| format!("Couldn't start {}: {e}", profile.name))?;
        let (tx, events) = crossbeam_channel::unbounded();
        let listener = Listener { tx, repaint };
        let config = Config { scrolling_history: SCROLLBACK_LINES, ..Config::default() };
        let term = Term::new(config, &TermSize::new(size.num_cols as usize, size.num_lines as usize), listener.clone());
        let term = Arc::new(FairMutex::new(term));
        let event_loop = EventLoop::new(term.clone(), listener, pty, true, false).map_err(|e| e.to_string())?;
        let notifier = Notifier(event_loop.channel());
        event_loop.spawn();
        Ok(TerminalSession { profile: profile.clone(), term, notifier, events, title: None, exited: None, size })
    }

    /// Sends keyboard input to the shell.
    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        if self.exited.is_none() {
            self.notifier.notify(bytes);
        }
    }

    /// Pastes text: line breaks become Enter, wrapped in bracketed-paste
    /// markers when the program asked for them (so a pasted multi-line
    /// script isn't run line by line by shells that support it).
    pub fn paste(&self, text: &str) {
        let bracketed = self.term.lock().mode().contains(TermMode::BRACKETED_PASTE);
        self.write(paste_bytes(text, bracketed));
    }

    pub fn resize(&mut self, cols: u16, rows: u16, cell: (u16, u16)) {
        let size = WindowSize { num_lines: rows.max(1), num_cols: cols.max(2), cell_width: cell.0, cell_height: cell.1 };
        if (size.num_cols, size.num_lines) == (self.size.num_cols, self.size.num_lines) {
            return;
        }
        self.size = size;
        self.term.lock().resize(TermSize::new(size.num_cols as usize, size.num_lines as usize));
        self.notifier.on_resize(size);
    }

    /// Handles what the terminal reported since the last frame. `copy` is
    /// called for text a program asked to put on the clipboard.
    pub fn poll_events(&mut self, mut copy: impl FnMut(String)) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Title(title) => self.title = Some(title),
                Event::ResetTitle => self.title = None,
                Event::PtyWrite(text) => self.write(text.into_bytes()),
                Event::ClipboardStore(_, text) => copy(text),
                Event::ColorRequest(index, format) => {
                    let color = self.term.lock().colors()[index].unwrap_or_else(|| default_color(index, true));
                    self.write(format(color).into_bytes());
                }
                Event::TextAreaSizeRequest(format) => self.write(format(self.size).into_bytes()),
                Event::ChildExit(status) => self.exited = Some(status.code().unwrap_or(-1)),
                Event::Exit => {
                    self.exited.get_or_insert(0);
                }
                // Programs may not read the clipboard (it could hold
                // anything); bells and cursor changes need nothing here.
                _ => {}
            }
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        // Closing the pseudo console ends the shell.
        let _ = self.notifier.0.send(Msg::Shutdown);
    }
}

/// Bytes for a paste (see `TerminalSession::paste`).
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        // A paste can't end the bracket early.
        let text = text.replace("\x1b[201~", "");
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.into_bytes()
    }
}

/// A key the terminal turns into an escape sequence (printable text is
/// sent as typed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TermKey {
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Right,
    Left,
    Home,
    End,
    Insert,
    Delete,
    PageUp,
    PageDown,
    F(u8),
    /// A letter with Ctrl (or Alt) held: `'a'..='z'`.
    Letter(char),
    Space,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// The bytes xterm sends for `key` (`app_cursor`: the program switched the
/// arrow keys to application mode, as full-screen programs do).
pub fn key_bytes(key: TermKey, mods: Mods, app_cursor: bool) -> Option<Vec<u8>> {
    // xterm's modifier parameter: 1 + Shift + 2·Alt + 4·Ctrl.
    let modifier = 1 + mods.shift as u8 + 2 * mods.alt as u8 + 4 * mods.ctrl as u8;
    let cursor = |letter: char| -> Vec<u8> {
        if modifier > 1 {
            format!("\x1b[1;{modifier}{letter}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{letter}").into_bytes()
        } else {
            format!("\x1b[{letter}").into_bytes()
        }
    };
    let tilde = |code: u8| -> Vec<u8> {
        if modifier > 1 { format!("\x1b[{code};{modifier}~").into_bytes() } else { format!("\x1b[{code}~").into_bytes() }
    };
    let alt_prefix = |bytes: Vec<u8>| if mods.alt { [b"\x1b".to_vec(), bytes].concat() } else { bytes };
    Some(match key {
        TermKey::Enter => alt_prefix(b"\r".to_vec()),
        TermKey::Backspace => alt_prefix(if mods.ctrl { b"\x08".to_vec() } else { b"\x7f".to_vec() }),
        TermKey::Tab => {
            if mods.shift {
                b"\x1b[Z".to_vec()
            } else {
                b"\t".to_vec()
            }
        }
        TermKey::Escape => b"\x1b".to_vec(),
        TermKey::Up => cursor('A'),
        TermKey::Down => cursor('B'),
        TermKey::Right => cursor('C'),
        TermKey::Left => cursor('D'),
        TermKey::Home => cursor('H'),
        TermKey::End => cursor('F'),
        TermKey::Insert => tilde(2),
        TermKey::Delete => tilde(3),
        TermKey::PageUp => tilde(5),
        TermKey::PageDown => tilde(6),
        TermKey::F(n @ 1..=4) => {
            let letter = (b'P' + n - 1) as char;
            if modifier > 1 { format!("\x1b[1;{modifier}{letter}").into_bytes() } else { format!("\x1bO{letter}").into_bytes() }
        }
        TermKey::F(n @ 5..=12) => tilde([15, 17, 18, 19, 20, 21, 23, 24][(n - 5) as usize]),
        TermKey::F(_) => return None,
        TermKey::Letter(c) if mods.ctrl => {
            let byte = (c.to_ascii_lowercase() as u8).checked_sub(b'a')? + 1;
            alt_prefix(vec![byte])
        }
        TermKey::Letter(c) if mods.alt => vec![0x1b, c as u8],
        TermKey::Letter(_) => return None,
        TermKey::Space if mods.ctrl => vec![0],
        TermKey::Space => return None,
    })
}

/// Default colors (Windows Terminal's "Campbell" scheme on dark themes,
/// "One Half Light" on light ones) for the 16 ANSI colors, the 6×6×6 cube
/// and gray ramp (16-255), and the named foreground/background/cursor.
pub fn default_color(index: usize, dark: bool) -> Rgb {
    const CAMPBELL: [(u8, u8, u8); 16] = [
        (12, 12, 12),
        (197, 15, 31),
        (19, 161, 14),
        (193, 156, 0),
        (0, 55, 218),
        (136, 23, 152),
        (58, 150, 221),
        (204, 204, 204),
        (118, 118, 118),
        (231, 72, 86),
        (22, 198, 12),
        (249, 241, 165),
        (59, 120, 255),
        (180, 0, 158),
        (97, 214, 214),
        (242, 242, 242),
    ];
    const ONE_HALF_LIGHT: [(u8, u8, u8); 16] = [
        (56, 58, 66),
        (228, 86, 73),
        (80, 161, 79),
        (193, 132, 1),
        (1, 132, 188),
        (166, 38, 164),
        (9, 151, 179),
        (250, 250, 250),
        (79, 82, 93),
        (223, 108, 117),
        (152, 195, 121),
        (228, 192, 122),
        (97, 175, 239),
        (197, 119, 221),
        (86, 181, 193),
        (255, 255, 255),
    ];
    let rgb = |(r, g, b): (u8, u8, u8)| Rgb { r, g, b };
    match index {
        0..=15 => rgb(if dark { CAMPBELL[index] } else { ONE_HALF_LIGHT[index] }),
        16..=231 => {
            let i = index - 16;
            let level = |v: usize| if v == 0 { 0 } else { (55 + v * 40) as u8 };
            rgb((level(i / 36), level((i / 6) % 6), level(i % 6)))
        }
        232..=255 => {
            let v = (8 + (index - 232) * 10) as u8;
            rgb((v, v, v))
        }
        i if i == NamedColor::Foreground as usize || i == NamedColor::BrightForeground as usize || i == NamedColor::Cursor as usize => {
            if dark { rgb((204, 204, 204)) } else { rgb((56, 58, 66)) }
        }
        i if i == NamedColor::DimForeground as usize => {
            if dark { rgb((140, 140, 140)) } else { rgb((110, 112, 120)) }
        }
        // Background (the pane paints its own) and the dim variants.
        i if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize).contains(&i) => {
            let base = default_color(i - NamedColor::DimBlack as usize, dark);
            Rgb { r: (base.r as u16 * 2 / 3) as u8, g: (base.g as u16 * 2 / 3) as u8, b: (base.b as u16 * 2 / 3) as u8 }
        }
        _ => {
            if dark { rgb((12, 12, 12)) } else { rgb((250, 250, 250)) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_become_xterm_sequences() {
        let none = Mods::default();
        let ctrl = Mods { ctrl: true, ..none };
        assert_eq!(key_bytes(TermKey::Enter, none, false).unwrap(), b"\r");
        assert_eq!(key_bytes(TermKey::Backspace, none, false).unwrap(), b"\x7f");
        assert_eq!(key_bytes(TermKey::Up, none, false).unwrap(), b"\x1b[A");
        assert_eq!(key_bytes(TermKey::Up, none, true).unwrap(), b"\x1bOA");
        assert_eq!(key_bytes(TermKey::Right, ctrl, false).unwrap(), b"\x1b[1;5C");
        assert_eq!(key_bytes(TermKey::Delete, none, false).unwrap(), b"\x1b[3~");
        assert_eq!(key_bytes(TermKey::F(1), none, false).unwrap(), b"\x1bOP");
        assert_eq!(key_bytes(TermKey::F(12), none, false).unwrap(), b"\x1b[24~");
        assert_eq!(key_bytes(TermKey::Letter('c'), ctrl, false).unwrap(), b"\x03");
        assert_eq!(key_bytes(TermKey::Letter('D'), ctrl, false).unwrap(), b"\x04");
        assert_eq!(key_bytes(TermKey::Letter('b'), Mods { alt: true, ..none }, false).unwrap(), b"\x1bb");
        assert_eq!(key_bytes(TermKey::Tab, Mods { shift: true, ..none }, false).unwrap(), b"\x1b[Z");
        assert!(key_bytes(TermKey::Letter('a'), none, false).is_none());
    }

    #[test]
    fn pastes_use_enter_and_brackets() {
        assert_eq!(paste_bytes("a\r\nb\nc", false), b"a\rb\rc");
        assert_eq!(paste_bytes("ls", true), b"\x1b[200~ls\x1b[201~");
        assert_eq!(paste_bytes("x\x1b[201~y", true), b"\x1b[200~xy\x1b[201~");
    }

    #[test]
    fn palette() {
        assert_eq!(default_color(1, true), Rgb { r: 197, g: 15, b: 31 });
        assert_eq!(default_color(16, true), Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(default_color(231, true), Rgb { r: 255, g: 255, b: 255 });
        assert_eq!(default_color(232, true), Rgb { r: 8, g: 8, b: 8 });
    }
}
