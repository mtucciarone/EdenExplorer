//! Settings > Shortcuts: every keyboard and mouse shortcut the app handles.
//! Rows backed by a `core::keymap::ShortcutAction` are editable - add a
//! key combination (press it), remove one, or reset the row - and the
//! change applies immediately everywhere. The rest (Ctrl+C/X/V, Delete,
//! Enter, arrows, mouse gestures, ...) are handled by Windows or text
//! editing and are listed as fixed.

use crate::core::keymap::{self, KeyCombo, ShortcutAction};
use crate::core::utils::widgets::{eden_button, eden_text_label};
use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::enums::SettingsAction;
use crate::gui::windows::settings::{setting_label, settings_section};
use crate::gui::windows::structs::SettingsWindow;
use eframe::egui;
use egui_phosphor::regular;

/// One shortcut row: either an editable action, or a fixed description
/// with its key combinations (each a list of keys pressed together).
enum Shortcut {
    Editable(ShortcutAction),
    Fixed {
        action_key: &'static str,
        combos: &'static [&'static [&'static str]],
    },
}

/// A titled group of related shortcuts, drawn as its own card.
struct ShortcutGroup {
    title_key: &'static str,
    shortcuts: &'static [Shortcut],
}

const SHORTCUT_GROUPS: &[ShortcutGroup] = &[
    ShortcutGroup {
        title_key: "shortcuts_group_tabs",
        shortcuts: &[
            Shortcut::Editable(ShortcutAction::NewTab),
            Shortcut::Editable(ShortcutAction::CloseTab),
            Shortcut::Editable(ShortcutAction::NextTab),
            Shortcut::Editable(ShortcutAction::PreviousTab),
            Shortcut::Fixed {
                action_key: "shortcut_open_in_new_tab",
                combos: &[&["Middle-Click"]],
            },
        ],
    },
    ShortcutGroup {
        title_key: "shortcuts_group_navigation",
        shortcuts: &[
            Shortcut::Editable(ShortcutAction::Back),
            Shortcut::Fixed {
                action_key: "shortcut_back_mouse",
                combos: &[&["Mouse 4"]],
            },
            Shortcut::Editable(ShortcutAction::Forward),
            Shortcut::Fixed {
                action_key: "shortcut_forward_mouse",
                combos: &[&["Mouse 5"]],
            },
            Shortcut::Editable(ShortcutAction::Up),
            Shortcut::Editable(ShortcutAction::Refresh),
            Shortcut::Editable(ShortcutAction::AddressBar),
            Shortcut::Editable(ShortcutAction::Search),
        ],
    },
    ShortcutGroup {
        title_key: "shortcuts_group_files",
        shortcuts: &[
            Shortcut::Fixed {
                action_key: "shortcut_open",
                combos: &[&["Enter"]],
            },
            Shortcut::Editable(ShortcutAction::SelectAll),
            Shortcut::Editable(ShortcutAction::InvertSelection),
            Shortcut::Editable(ShortcutAction::SelectByPattern),
            Shortcut::Fixed {
                action_key: "shortcut_select_first_last",
                combos: &[&["Home"], &["End"]],
            },
            Shortcut::Fixed {
                action_key: "shortcut_move_selection",
                combos: &[&["↑"], &["↓"]],
            },
            Shortcut::Fixed {
                action_key: "shortcut_extend_selection",
                combos: &[&["Shift", "↑"], &["Shift", "↓"]],
            },
            Shortcut::Fixed {
                action_key: "shortcut_copy",
                combos: &[&["Ctrl", "C"]],
            },
            Shortcut::Fixed {
                action_key: "shortcut_cut",
                combos: &[&["Ctrl", "X"]],
            },
            Shortcut::Fixed {
                action_key: "shortcut_paste",
                combos: &[&["Ctrl", "V"]],
            },
            Shortcut::Editable(ShortcutAction::CopyPath),
            Shortcut::Editable(ShortcutAction::Rename),
            Shortcut::Fixed {
                action_key: "shortcut_delete",
                combos: &[&["Del"]],
            },
            Shortcut::Fixed {
                action_key: "shortcut_delete_permanently",
                combos: &[&["Shift", "Del"]],
            },
            Shortcut::Editable(ShortcutAction::NewFolder),
            Shortcut::Editable(ShortcutAction::Properties),
            Shortcut::Editable(ShortcutAction::Undo),
            Shortcut::Editable(ShortcutAction::Redo),
            Shortcut::Fixed {
                action_key: "shortcut_cancel",
                combos: &[&["Esc"]],
            },
        ],
    },
    ShortcutGroup {
        title_key: "shortcuts_group_mouse",
        shortcuts: &[
            Shortcut::Fixed {
                action_key: "shortcut_multi_select",
                combos: &[&["Ctrl", "Click"], &["Shift", "Click"]],
            },
            Shortcut::Fixed {
                action_key: "shortcut_sort_add_column",
                combos: &[&["Shift", "Click"]],
            },
            Shortcut::Fixed {
                action_key: "shortcut_sort_remove_column",
                combos: &[&["Ctrl", "Click"]],
            },
        ],
    },
    ShortcutGroup {
        title_key: "shortcuts_group_window",
        shortcuts: &[
            Shortcut::Editable(ShortcutAction::Fullscreen),
            Shortcut::Editable(ShortcutAction::PerformancePanel),
        ],
    },
];

pub fn draw_shortcuts_settings(
    ui: &mut egui::Ui,
    i18n: &I18n,
    settings: &mut SettingsWindow,
    palette: &ThemePalette,
) -> Option<SettingsAction> {
    let pass = ui.ctx().cumulative_pass_nr();
    // Coming back to the page after leaving it mid-recording: start fresh.
    if settings.shortcuts_page_last_pass + 1 < pass {
        settings.recording_shortcut = None;
        settings.shortcut_message = None;
    }
    settings.shortcuts_page_last_pass = pass;

    let mut changed = false;

    if let Some(action) = settings.recording_shortcut {
        if let Some(outcome) = capture_combo(ui.ctx()) {
            settings.recording_shortcut = None;
            if let Some(combo) = outcome {
                changed |= assign_combo(i18n, settings, action, combo);
            }
        }
    }
    keymap::set_recording(settings.recording_shortcut.is_some(), pass);

    setting_label(
        ui,
        &i18n.tr("settings_category_shortcuts"),
        Some((&i18n.tr("tooltip_settings_shortcuts"), palette)),
        palette,
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(i18n.tr("shortcuts_edit_hint"))
                .size(palette.text_size - 1.0)
                .color(palette.text_normal.gamma_multiply(0.75)),
        );
        let prefs = &mut settings.current_settings.ui_prefs;
        if !prefs.shortcuts.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if eden_button(ui, palette, &i18n.tr("shortcuts_reset_all")).clicked() {
                    prefs.shortcuts.clear();
                    settings.shortcut_message = None;
                    changed = true;
                }
            });
        }
    });
    if let Some((ok, message)) = &settings.shortcut_message {
        ui.add_space(4.0);
        let color = if *ok {
            palette.notification_status_success
        } else {
            palette.notification_status_warning
        };
        ui.label(egui::RichText::new(message).size(palette.text_size - 1.0).color(color));
    }
    ui.add_space(8.0);

    for group in SHORTCUT_GROUPS {
        settings_section(ui, palette, |ui| {
            ui.label(
                egui::RichText::new(i18n.tr(group.title_key))
                    .strong()
                    .size(palette.text_size)
                    .color(palette.text_header_section),
            );
            ui.add_space(6.0);

            for (index, shortcut) in group.shortcuts.iter().enumerate() {
                if index > 0 {
                    ui.separator();
                }
                match shortcut {
                    Shortcut::Fixed { action_key, combos } => {
                        draw_fixed_row(ui, i18n, palette, action_key, combos);
                    }
                    Shortcut::Editable(action) => {
                        changed |= draw_editable_row(ui, i18n, settings, palette, *action);
                    }
                }
            }
        });
    }

    if changed {
        keymap::set_overrides(&settings.current_settings.ui_prefs.shortcuts);
        return Some(SettingsAction::ApplySettings);
    }
    None
}

fn row(ui: &mut egui::Ui, label: &str, palette: &ThemePalette, right: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(ui.spacing().interact_size.y + 2.0);
        eden_text_label(ui, palette, label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
}

fn draw_fixed_row(
    ui: &mut egui::Ui,
    i18n: &I18n,
    palette: &ThemePalette,
    action_key: &str,
    combos: &[&[&str]],
) {
    row(ui, &i18n.tr(action_key), palette, |ui| {
        ui.label(
            egui::RichText::new(regular::LOCK_SIMPLE)
                .size(palette.text_size - 1.0)
                .color(palette.text_normal.gamma_multiply(0.45)),
        )
        .on_hover_text(i18n.tr("shortcuts_fixed"));
        // Right-to-left, so combos (and each combo's keys) go last-to-first.
        for (combo_index, combo) in combos.iter().enumerate().rev() {
            draw_combo_keys(ui, palette, &combo.iter().map(|k| k.to_string()).collect::<Vec<_>>());
            if combo_index > 0 {
                or_label(ui, i18n, palette);
            }
        }
    });
}

/// Returns whether the overrides changed.
fn draw_editable_row(
    ui: &mut egui::Ui,
    i18n: &I18n,
    settings: &mut SettingsWindow,
    palette: &ThemePalette,
    action: ShortcutAction,
) -> bool {
    let mut changed = false;
    let overrides = &settings.current_settings.ui_prefs.shortcuts;
    let combos = keymap::combos_in(overrides, action);
    let customized = overrides.contains_key(&action);
    let recording = settings.recording_shortcut == Some(action);
    let mut remove: Option<KeyCombo> = None;
    let mut start_recording = false;
    let mut reset = false;

    let label = if customized {
        format!("{}  \u{2022}", i18n.tr(action.i18n_key()))
    } else {
        i18n.tr(action.i18n_key())
    };
    row(ui, &label, palette, |ui| {
        if customized
            && small_icon_button(ui, palette, regular::ARROW_COUNTER_CLOCKWISE)
                .on_hover_text(i18n.tr("shortcuts_reset_row"))
                .clicked()
        {
            reset = true;
        }
        if recording {
            egui::Frame::NONE
                .fill(palette.primary.gamma_multiply(0.25))
                .stroke(egui::Stroke::new(1.0, palette.primary))
                .corner_radius(egui::CornerRadius::same(palette.small_radius))
                .inner_margin(egui::Margin::symmetric(8, 2))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(i18n.tr("shortcuts_press_keys"))
                            .size(palette.text_size - 1.0)
                            .color(palette.text_normal),
                    );
                });
        } else if small_icon_button(ui, palette, regular::PLUS)
            .on_hover_text(i18n.tr("shortcuts_add"))
            .clicked()
        {
            start_recording = true;
        }

        if combos.is_empty() && !recording {
            ui.label(
                egui::RichText::new(i18n.tr("shortcuts_none"))
                    .italics()
                    .size(palette.text_size - 1.0)
                    .color(palette.text_normal.gamma_multiply(0.55)),
            );
        }
        for (index, combo) in combos.iter().enumerate().rev() {
            let removed = ui
                .scope(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let x = small_icon_button(ui, palette, regular::X)
                        .on_hover_text(i18n.tr("shortcuts_remove"))
                        .clicked();
                    draw_combo_keys(ui, palette, &combo.parts());
                    x
                })
                .inner;
            if removed {
                remove = Some(*combo);
            }
            if index > 0 {
                or_label(ui, i18n, palette);
            }
        }
    });

    let overrides = &mut settings.current_settings.ui_prefs.shortcuts;
    if reset {
        overrides.remove(&action);
        settings.shortcut_message = None;
        changed = true;
    }
    if let Some(combo) = remove {
        let mut list = keymap::combos_in(overrides, action);
        list.retain(|c| *c != combo);
        set_combos(overrides, action, list);
        settings.shortcut_message = None;
        changed = true;
    }
    if start_recording {
        settings.recording_shortcut = Some(action);
        settings.shortcut_message = None;
    }
    changed
}

/// Stores `list` for `action`, dropping the override again when it equals
/// the defaults.
fn set_combos(overrides: &mut keymap::ShortcutOverrides, action: ShortcutAction, list: Vec<KeyCombo>) {
    if list == action.defaults() {
        overrides.remove(&action);
    } else {
        overrides.insert(action, list);
    }
}

/// Adds `combo` to `action`, taking it away from any other action that had
/// it. Returns whether anything changed.
fn assign_combo(
    i18n: &I18n,
    settings: &mut SettingsWindow,
    action: ShortcutAction,
    combo: KeyCombo,
) -> bool {
    if keymap::is_reserved(&combo) {
        settings.shortcut_message = Some((
            false,
            format!("{} {}", combo.label(), i18n.tr("shortcuts_reserved")),
        ));
        return false;
    }
    let overrides = &mut settings.current_settings.ui_prefs.shortcuts;
    let mut list = keymap::combos_in(overrides, action);
    if list.contains(&combo) {
        return false;
    }
    let mut message = (true, format!("{} {}", combo.label(), i18n.tr("shortcuts_assigned")));
    if let Some(other) = keymap::conflict(overrides, &combo, action) {
        let mut other_list = keymap::combos_in(overrides, other);
        other_list.retain(|c| *c != combo);
        set_combos(overrides, other, other_list);
        message = (
            false,
            format!(
                "{} {} \"{}\"",
                combo.label(),
                i18n.tr("shortcuts_moved_from"),
                i18n.tr(other.i18n_key())
            ),
        );
    }
    list.push(combo);
    set_combos(overrides, action, list);
    settings.shortcut_message = Some(message);
    true
}

/// While recording: `Some(Some(combo))` once a key is pressed,
/// `Some(None)` if Esc cancels, `None` while still waiting.
fn capture_combo(ctx: &egui::Context) -> Option<Option<KeyCombo>> {
    ctx.input(|input| {
        for event in &input.events {
            // Ctrl+C/X/V arrive as clipboard events rather than keys; report
            // them so the user sees why they can't be assigned.
            let clipboard_key = match event {
                egui::Event::Copy => Some(egui::Key::C),
                egui::Event::Cut => Some(egui::Key::X),
                egui::Event::Paste(_) => Some(egui::Key::V),
                _ => None,
            };
            if let Some(key) = clipboard_key {
                return Some(Some(KeyCombo::new(true, false, false, key)));
            }
            if let egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } = event
            {
                // Holding Ctrl/Shift/Alt reports the modifier key itself
                // first - keep waiting for the actual key.
                if keymap::is_modifier_key(*key) {
                    continue;
                }
                if *key == egui::Key::Escape && !modifiers.any() {
                    return Some(None);
                }
                return Some(Some(KeyCombo::new(
                    modifiers.ctrl,
                    modifiers.shift,
                    modifiers.alt,
                    *key,
                )));
            }
        }
        None
    })
}

fn small_icon_button(ui: &mut egui::Ui, palette: &ThemePalette, icon: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(icon)
                .size(palette.text_size - 1.0)
                .color(palette.text_normal),
        )
        .frame(false),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn or_label(ui: &mut egui::Ui, i18n: &I18n, palette: &ThemePalette) {
    ui.label(
        egui::RichText::new(i18n.tr("shortcuts_or"))
            .size(palette.text_size)
            .color(palette.text_normal)
            .weak(),
    );
}

/// One combination's keys as keycaps joined by "+", drawn right-to-left.
fn draw_combo_keys(ui: &mut egui::Ui, palette: &ThemePalette, keys: &[String]) {
    for (key_index, key) in keys.iter().enumerate().rev() {
        key_chip(ui, palette, key);
        if key_index > 0 {
            ui.label(
                egui::RichText::new("+")
                    .size(palette.text_size)
                    .color(palette.text_normal),
            );
        }
    }
}

/// One key drawn as a small bordered "keycap".
fn key_chip(ui: &mut egui::Ui, palette: &ThemePalette, key: &str) {
    egui::Frame::NONE
        .fill(palette.row_bg)
        .stroke(egui::Stroke::new(1.0, palette.borders_default))
        .corner_radius(egui::CornerRadius::same(palette.small_radius))
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(key)
                    .family(egui::FontFamily::Monospace)
                    .size(palette.text_size - 1.0)
                    .color(palette.text_normal),
            );
        });
}
