//! Settings > Shortcuts: a read-only reference of every keyboard and mouse
//! shortcut the app handles. The list is static - it mirrors the key handling
//! in `mainwindow_imp.rs` (window/tab-level shortcuts) and
//! `containers::itemviewer_helper` (file-list shortcuts), so a shortcut added
//! or changed there should be reflected here too.

use crate::core::utils::widgets::eden_text_label;
use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::settings::{setting_label, settings_section};
use eframe::egui;

/// One shortcut row: the i18n key of what it does, plus one or more
/// alternative key combinations (each a list of keys pressed together).
struct Shortcut {
    action_key: &'static str,
    combos: &'static [&'static [&'static str]],
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
            Shortcut { action_key: "shortcut_new_tab", combos: &[&["Ctrl", "T"]] },
            Shortcut { action_key: "shortcut_close_tab", combos: &[&["Ctrl", "W"]] },
            Shortcut { action_key: "shortcut_next_tab", combos: &[&["Ctrl", "Tab"]] },
            Shortcut {
                action_key: "shortcut_previous_tab",
                combos: &[&["Ctrl", "Shift", "Tab"]],
            },
            Shortcut {
                action_key: "shortcut_open_in_new_tab",
                combos: &[&["Middle-Click"]],
            },
        ],
    },
    ShortcutGroup {
        title_key: "shortcuts_group_navigation",
        shortcuts: &[
            Shortcut {
                action_key: "shortcut_back",
                combos: &[&["Alt", "←"], &["Backspace"], &["Mouse 4"]],
            },
            Shortcut {
                action_key: "shortcut_forward",
                combos: &[&["Alt", "→"], &["Mouse 5"]],
            },
            Shortcut { action_key: "shortcut_up", combos: &[&["Alt", "↑"]] },
            Shortcut { action_key: "shortcut_refresh", combos: &[&["Ctrl", "R"], &["F5"]] },
            Shortcut { action_key: "shortcut_address_bar", combos: &[&["Alt", "D"]] },
            Shortcut { action_key: "shortcut_search", combos: &[&["Ctrl", "F"]] },
        ],
    },
    ShortcutGroup {
        title_key: "shortcuts_group_files",
        shortcuts: &[
            Shortcut { action_key: "shortcut_open", combos: &[&["Enter"]] },
            Shortcut { action_key: "shortcut_select_all", combos: &[&["Ctrl", "A"]] },
            Shortcut { action_key: "shortcut_invert_selection", combos: &[&["Ctrl", "I"]] },
            Shortcut {
                action_key: "shortcut_select_by_pattern",
                combos: &[&["Ctrl", "Shift", "S"]],
            },
            Shortcut {
                action_key: "shortcut_select_first_last",
                combos: &[&["Home"], &["End"]],
            },
            Shortcut { action_key: "shortcut_move_selection", combos: &[&["↑"], &["↓"]] },
            Shortcut {
                action_key: "shortcut_extend_selection",
                combos: &[&["Shift", "↑"], &["Shift", "↓"]],
            },
            Shortcut { action_key: "shortcut_copy", combos: &[&["Ctrl", "C"]] },
            Shortcut { action_key: "shortcut_cut", combos: &[&["Ctrl", "X"]] },
            Shortcut { action_key: "shortcut_paste", combos: &[&["Ctrl", "V"]] },
            Shortcut { action_key: "shortcut_copy_path", combos: &[&["Ctrl", "Shift", "C"]] },
            Shortcut { action_key: "shortcut_rename", combos: &[&["F2"]] },
            Shortcut { action_key: "shortcut_delete", combos: &[&["Del"]] },
            Shortcut {
                action_key: "shortcut_delete_permanently",
                combos: &[&["Shift", "Del"]],
            },
            Shortcut { action_key: "shortcut_new_folder", combos: &[&["Ctrl", "Shift", "N"]] },
            Shortcut { action_key: "shortcut_properties", combos: &[&["Alt", "Enter"]] },
            Shortcut { action_key: "shortcut_undo", combos: &[&["Ctrl", "Z"]] },
            Shortcut {
                action_key: "shortcut_redo",
                combos: &[&["Ctrl", "Y"], &["Ctrl", "Shift", "Z"]],
            },
            Shortcut { action_key: "shortcut_cancel", combos: &[&["Esc"]] },
        ],
    },
    ShortcutGroup {
        title_key: "shortcuts_group_mouse",
        shortcuts: &[
            Shortcut {
                action_key: "shortcut_multi_select",
                combos: &[&["Ctrl", "Click"], &["Shift", "Click"]],
            },
            Shortcut {
                action_key: "shortcut_sort_add_column",
                combos: &[&["Shift", "Click"]],
            },
            Shortcut {
                action_key: "shortcut_sort_remove_column",
                combos: &[&["Ctrl", "Click"]],
            },
        ],
    },
    ShortcutGroup {
        title_key: "shortcuts_group_window",
        shortcuts: &[
            Shortcut { action_key: "shortcut_fullscreen", combos: &[&["F1"]] },
            Shortcut {
                action_key: "shortcut_performance_panel",
                combos: &[&["Ctrl", "K"]],
            },
        ],
    },
];

pub fn draw_shortcuts_settings(ui: &mut egui::Ui, i18n: &I18n, palette: &ThemePalette) {
    setting_label(
        ui,
        &i18n.tr("settings_category_shortcuts"),
        Some((&i18n.tr("tooltip_settings_shortcuts"), palette)),
        palette,
    );
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
                ui.horizontal(|ui| {
                    ui.set_min_height(ui.spacing().interact_size.y);
                    eden_text_label(ui, palette, &i18n.tr(shortcut.action_key));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Right-to-left, so the combos (and each combo's keys)
                        // are drawn last-to-first to read left-to-right.
                        for (combo_index, combo) in shortcut.combos.iter().enumerate().rev() {
                            for (key_index, key) in combo.iter().enumerate().rev() {
                                key_chip(ui, palette, key);
                                if key_index > 0 {
                                    ui.label(
                                        egui::RichText::new("+")
                                            .size(palette.text_size)
                                            .color(palette.text_normal),
                                    );
                                }
                            }
                            if combo_index > 0 {
                                ui.label(
                                    egui::RichText::new(i18n.tr("shortcuts_or"))
                                        .size(palette.text_size)
                                        .color(palette.text_normal)
                                        .weak(),
                                );
                            }
                        }
                    });
                });
            }
        });
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
