//! Remappable keyboard shortcuts (Settings > Shortcuts).
//!
//! Each `ShortcutAction` has default key combinations; the user can replace
//! them per action (`UiPrefs::shortcuts`). The effective keymap lives in a
//! process-wide cell (`set_overrides`) so every place that handles a
//! shortcut can simply ask `pressed(input, action)` without the settings
//! being threaded through. Keys that egui/Windows treat specially (Ctrl+C/
//! X/V, Delete, Enter, arrows, Esc, ...) are fixed and not listed here.

use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// One key plus the modifiers that must be held with it - exactly: Ctrl+Z
/// doesn't also fire for Ctrl+Shift+Z.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyCombo {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: egui::Key,
}

impl KeyCombo {
    pub const fn new(ctrl: bool, shift: bool, alt: bool, key: egui::Key) -> Self {
        Self { ctrl, shift, alt, key }
    }

    const fn key(key: egui::Key) -> Self {
        Self::new(false, false, false, key)
    }
    const fn ctrl(key: egui::Key) -> Self {
        Self::new(true, false, false, key)
    }
    const fn ctrl_shift(key: egui::Key) -> Self {
        Self::new(true, true, false, key)
    }
    const fn alt(key: egui::Key) -> Self {
        Self::new(false, false, true, key)
    }

    pub fn matches(&self, input: &egui::InputState) -> bool {
        let m = input.modifiers;
        m.ctrl == self.ctrl
            && m.shift == self.shift
            && m.alt == self.alt
            && input.key_pressed(self.key)
    }

    /// The keys to show as separate "keycaps", e.g. ["Ctrl", "Shift", "S"].
    pub fn parts(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        parts.push(key_label(self.key));
        parts
    }

    pub fn label(&self) -> String {
        self.parts().join("+")
    }
}

/// How a key is shown: arrows as symbols, everything else by egui's name.
pub fn key_label(key: egui::Key) -> String {
    match key {
        egui::Key::ArrowLeft => "←".into(),
        egui::Key::ArrowRight => "→".into(),
        egui::Key::ArrowUp => "↑".into(),
        egui::Key::ArrowDown => "↓".into(),
        egui::Key::Backspace => "Backspace".into(),
        egui::Key::Enter => "Enter".into(),
        egui::Key::Backtick => "`".into(),
        other => other.name().to_string(),
    }
}

/// Ctrl, Shift, Alt and Windows keys: reported as key presses of their own,
/// but only ever part of a combination.
pub fn is_modifier_key(key: egui::Key) -> bool {
    use egui::Key::*;
    matches!(
        key,
        ShiftLeft | ShiftRight | ControlLeft | ControlRight | AltLeft | AltRight | SuperLeft | SuperRight
    )
}

/// Combinations that can't be assigned: they're handled by Windows/egui
/// before the app sees them, or reserved for text editing and the file
/// list's fixed keys.
pub fn is_reserved(combo: &KeyCombo) -> bool {
    use egui::Key::*;
    let plain = !combo.ctrl && !combo.alt;
    is_modifier_key(combo.key)
        || (combo.ctrl && !combo.alt && !combo.shift && matches!(combo.key, C | X | V))
        || (plain && matches!(combo.key, Enter | Escape | Delete | ArrowUp | ArrowDown | Home | End | Space))
        || (!combo.ctrl && !combo.alt && combo.shift && matches!(combo.key, ArrowUp | ArrowDown | Delete))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ShortcutAction {
    NewTab,
    CloseTab,
    NextTab,
    PreviousTab,
    Back,
    Forward,
    Up,
    Refresh,
    AddressBar,
    Search,
    SelectAll,
    InvertSelection,
    SelectByPattern,
    CopyPath,
    Rename,
    NewFolder,
    Properties,
    Undo,
    Redo,
    Fullscreen,
    PerformancePanel,
    CommandPalette,
    TerminalPane,
}

impl ShortcutAction {
    pub const ALL: [ShortcutAction; 23] = [
        ShortcutAction::NewTab,
        ShortcutAction::CloseTab,
        ShortcutAction::NextTab,
        ShortcutAction::PreviousTab,
        ShortcutAction::Back,
        ShortcutAction::Forward,
        ShortcutAction::Up,
        ShortcutAction::Refresh,
        ShortcutAction::AddressBar,
        ShortcutAction::Search,
        ShortcutAction::SelectAll,
        ShortcutAction::InvertSelection,
        ShortcutAction::SelectByPattern,
        ShortcutAction::CopyPath,
        ShortcutAction::Rename,
        ShortcutAction::NewFolder,
        ShortcutAction::Properties,
        ShortcutAction::Undo,
        ShortcutAction::Redo,
        ShortcutAction::Fullscreen,
        ShortcutAction::PerformancePanel,
        ShortcutAction::CommandPalette,
        ShortcutAction::TerminalPane,
    ];

    pub fn defaults(self) -> Vec<KeyCombo> {
        use egui::Key;
        match self {
            ShortcutAction::NewTab => vec![KeyCombo::ctrl(Key::T)],
            ShortcutAction::CloseTab => vec![KeyCombo::ctrl(Key::W)],
            ShortcutAction::NextTab => vec![KeyCombo::ctrl(Key::Tab)],
            ShortcutAction::PreviousTab => vec![KeyCombo::ctrl_shift(Key::Tab)],
            ShortcutAction::Back => vec![KeyCombo::alt(Key::ArrowLeft), KeyCombo::key(Key::Backspace)],
            ShortcutAction::Forward => vec![KeyCombo::alt(Key::ArrowRight)],
            ShortcutAction::Up => vec![KeyCombo::alt(Key::ArrowUp)],
            ShortcutAction::Refresh => vec![KeyCombo::ctrl(Key::R), KeyCombo::key(Key::F5)],
            ShortcutAction::AddressBar => vec![KeyCombo::alt(Key::D)],
            ShortcutAction::Search => vec![KeyCombo::ctrl(Key::F)],
            ShortcutAction::SelectAll => vec![KeyCombo::ctrl(Key::A)],
            ShortcutAction::InvertSelection => vec![KeyCombo::ctrl(Key::I)],
            ShortcutAction::SelectByPattern => vec![KeyCombo::ctrl_shift(Key::S)],
            ShortcutAction::CopyPath => vec![KeyCombo::ctrl_shift(Key::C)],
            ShortcutAction::Rename => vec![KeyCombo::key(Key::F2)],
            ShortcutAction::NewFolder => vec![KeyCombo::ctrl_shift(Key::N)],
            ShortcutAction::Properties => vec![KeyCombo::alt(Key::Enter)],
            ShortcutAction::Undo => vec![KeyCombo::ctrl(Key::Z)],
            ShortcutAction::Redo => vec![KeyCombo::ctrl(Key::Y), KeyCombo::ctrl_shift(Key::Z)],
            ShortcutAction::Fullscreen => vec![KeyCombo::key(Key::F1)],
            ShortcutAction::PerformancePanel => vec![KeyCombo::ctrl(Key::K)],
            ShortcutAction::CommandPalette => vec![KeyCombo::ctrl_shift(Key::P)],
            ShortcutAction::TerminalPane => vec![KeyCombo::ctrl(Key::Backtick)],
        }
    }

    /// i18n key of the action's description (shared with the Shortcuts page).
    pub fn i18n_key(self) -> &'static str {
        match self {
            ShortcutAction::NewTab => "shortcut_new_tab",
            ShortcutAction::CloseTab => "shortcut_close_tab",
            ShortcutAction::NextTab => "shortcut_next_tab",
            ShortcutAction::PreviousTab => "shortcut_previous_tab",
            ShortcutAction::Back => "shortcut_back",
            ShortcutAction::Forward => "shortcut_forward",
            ShortcutAction::Up => "shortcut_up",
            ShortcutAction::Refresh => "shortcut_refresh",
            ShortcutAction::AddressBar => "shortcut_address_bar",
            ShortcutAction::Search => "shortcut_search",
            ShortcutAction::SelectAll => "shortcut_select_all",
            ShortcutAction::InvertSelection => "shortcut_invert_selection",
            ShortcutAction::SelectByPattern => "shortcut_select_by_pattern",
            ShortcutAction::CopyPath => "shortcut_copy_path",
            ShortcutAction::Rename => "shortcut_rename",
            ShortcutAction::NewFolder => "shortcut_new_folder",
            ShortcutAction::Properties => "shortcut_properties",
            ShortcutAction::Undo => "shortcut_undo",
            ShortcutAction::Redo => "shortcut_redo",
            ShortcutAction::Fullscreen => "shortcut_fullscreen",
            ShortcutAction::PerformancePanel => "shortcut_performance_panel",
            ShortcutAction::CommandPalette => "shortcut_command_palette",
            ShortcutAction::TerminalPane => "shortcut_terminal_pane",
        }
    }
}

/// User overrides: an action present here uses exactly these combos
/// (possibly none, which disables it) instead of its defaults.
pub type ShortcutOverrides = BTreeMap<ShortcutAction, Vec<KeyCombo>>;

static OVERRIDES: RwLock<Option<ShortcutOverrides>> = RwLock::new(None);
static RECORDING: AtomicBool = AtomicBool::new(false);
/// The frame (egui pass) in which the recording UI was last drawn.
static RECORDING_PASS: AtomicU64 = AtomicU64::new(0);

/// Installs the user's overrides (at startup and after every edit).
pub fn set_overrides(overrides: &ShortcutOverrides) {
    if OVERRIDES
        .read()
        .is_ok_and(|current| current.as_ref() == Some(overrides))
    {
        return;
    }
    if let Ok(mut current) = OVERRIDES.write() {
        *current = Some(overrides.clone());
    }
}

/// The combos currently assigned to `action`.
pub fn combos(action: ShortcutAction) -> Vec<KeyCombo> {
    OVERRIDES
        .read()
        .ok()
        .and_then(|current| current.as_ref().and_then(|o| o.get(&action).cloned()))
        .unwrap_or_else(|| action.defaults())
}

pub fn combos_in(overrides: &ShortcutOverrides, action: ShortcutAction) -> Vec<KeyCombo> {
    overrides
        .get(&action)
        .cloned()
        .unwrap_or_else(|| action.defaults())
}

/// Whether one of `action`'s combos was pressed this frame. Always false
/// while the Shortcuts page is recording a new combination.
pub fn pressed(input: &egui::InputState, action: ShortcutAction) -> bool {
    // Checked for every shortcut on every frame; most frames have no key
    // press at all, so skip the lookup (a lock and a copy) for those.
    let any_key_pressed = input
        .events
        .iter()
        .any(|e| matches!(e, egui::Event::Key { pressed: true, .. }));
    any_key_pressed
        && !is_recording()
        && combos(action).iter().any(|combo| combo.matches(input))
}

/// Which other action already uses `combo`, if any.
pub fn conflict(
    overrides: &ShortcutOverrides,
    combo: &KeyCombo,
    except: ShortcutAction,
) -> Option<ShortcutAction> {
    ShortcutAction::ALL
        .into_iter()
        .filter(|&action| action != except)
        .find(|&action| combos_in(overrides, action).contains(combo))
}

/// Set while the Shortcuts page waits for a key combination, so pressing
/// e.g. Ctrl+W to assign it doesn't also close the tab.
/// `pass` is the current egui pass number; see `expire_recording`.
pub fn set_recording(recording: bool, pass: u64) {
    RECORDING.store(recording, Ordering::Relaxed);
    RECORDING_PASS.store(pass, Ordering::Relaxed);
}

/// Ends recording if the page that started it wasn't drawn in the last
/// frame (the user navigated away mid-recording), so shortcuts never stay
/// switched off. Call once per frame.
pub fn expire_recording(current_pass: u64) {
    if is_recording() && RECORDING_PASS.load(Ordering::Relaxed) + 1 < current_pass {
        RECORDING.store(false, Ordering::Relaxed);
    }
}

pub fn is_recording() -> bool {
    RECORDING.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_have_no_conflicts_and_nothing_reserved() {
        let empty = ShortcutOverrides::new();
        for action in ShortcutAction::ALL {
            for combo in action.defaults() {
                assert!(!is_reserved(&combo), "{action:?} uses reserved {}", combo.label());
                assert_eq!(conflict(&empty, &combo, action), None, "{}", combo.label());
            }
        }
    }

    #[test]
    fn overrides_replace_defaults_and_report_conflicts() {
        let mut overrides = ShortcutOverrides::new();
        overrides.insert(ShortcutAction::NewTab, vec![KeyCombo::ctrl(egui::Key::N)]);
        assert_eq!(combos_in(&overrides, ShortcutAction::NewTab), vec![KeyCombo::ctrl(egui::Key::N)]);
        assert_eq!(
            combos_in(&overrides, ShortcutAction::CloseTab),
            ShortcutAction::CloseTab.defaults()
        );
        // Ctrl+T is free again; Ctrl+N now belongs to New Tab.
        assert_eq!(conflict(&overrides, &KeyCombo::ctrl(egui::Key::T), ShortcutAction::Search), None);
        assert_eq!(
            conflict(&overrides, &KeyCombo::ctrl(egui::Key::N), ShortcutAction::Search),
            Some(ShortcutAction::NewTab)
        );
    }

    #[test]
    fn reserved_keys() {
        assert!(is_reserved(&KeyCombo::ctrl(egui::Key::C)));
        assert!(is_reserved(&KeyCombo::key(egui::Key::Delete)));
        assert!(!is_reserved(&KeyCombo::ctrl_shift(egui::Key::C)));
        assert!(!is_reserved(&KeyCombo::alt(egui::Key::ArrowUp)));
        assert!(is_reserved(&KeyCombo::ctrl(egui::Key::ControlLeft)));
    }

    #[test]
    fn labels_and_json() {
        assert_eq!(KeyCombo::ctrl_shift(egui::Key::S).label(), "Ctrl+Shift+S");
        assert_eq!(KeyCombo::alt(egui::Key::ArrowLeft).label(), "Alt+←");
        let mut overrides = ShortcutOverrides::new();
        overrides.insert(ShortcutAction::Undo, vec![KeyCombo::ctrl(egui::Key::U)]);
        let json = serde_json::to_string(&overrides).unwrap();
        let back: ShortcutOverrides = serde_json::from_str(&json).unwrap();
        assert_eq!(back, overrides);
    }
}
