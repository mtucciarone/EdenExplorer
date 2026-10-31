use crate::core::{
    fs::{DateStyle, MY_PC_PATH},
    indexer::{
        WindowSizeMode, default_item_viewer_drive_column_size,
        default_item_viewer_file_column_size, default_recycle_bin_column_size,
    },
    utils::text::apply_eden_text_overrides,
    utils::widgets::{
        apply_eden_visual_overrides, draw_checkbox, draw_dropdown, eden_button, eden_text_label,
        eden_toggle_button,
    },
};
use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use crate::gui::utils::SortColumn;
use crate::gui::windows::containers::enums::ItemViewerHeaderColumn;
use crate::gui::windows::enums::SettingsAction;
use crate::gui::windows::structs::{AppSettings, SettingsTab, SettingsWindow};
use eframe::egui;
use egui::RichText;
use egui_phosphor::regular;
use std::path::PathBuf;

const SETTINGS_COMBO_WIDTH: f32 = 180.0;
const SETTINGS_VALUE_WIDTH: f32 = 92.0;

const HOTKEYS: &[(&str, &str, bool)] = &[
    ("F1", "hotkey_toggle_fullscreen", true),
    ("Ctrl+Shift+C", "hotkey_copy_selected_path", true),
    ("Ctrl+W", "hotkey_close_current_tab", true),
    ("Ctrl+T", "hotkey_new_tab", true),
    ("Ctrl+Tab", "hotkey_next_tab", true),
    ("Ctrl+Shift+Tab", "hotkey_previous_tab", true),
    ("Ctrl+Shift+N", "hotkey_new_folder", true),
    ("Ctrl+R (F5)", "hotkey_refresh_directory", true),
    ("Alt+D", "hotkey_select_address_bar", true),
    ("F2", "hotkey_rename_file", true),
    ("Alt+Enter", "hotkey_open_properties", true),
    ("Home / End", "hotkey_scroll_and_select_edges", true),
    ("Alt+Left Arrow (Backspace)", "hotkey_navigate_back", true),
    ("Alt+Right Arrow", "hotkey_navigate_forward", true),
    ("Alt+Up Arrow", "hotkey_navigate_up", true),
    ("Enter", "hotkey_run_default_program", true),
    ("Ctrl+Delete", "hotkey_permanent_delete", false),
];

const HOTKEY_GROUPS: &[(&str, &[usize])] = &[
    ("hotkey_group_application", &[0]),
    ("hotkey_group_tabs", &[2, 3, 4, 5]),
    ("hotkey_group_files_folders", &[1, 6, 9, 10, 15, 16]),
    ("hotkey_group_navigation", &[7, 8, 11, 12, 13, 14]),
];

fn format_recorded_hotkey(key: egui::Key, modifiers: egui::Modifiers) -> Option<String> {
    let mut parts = Vec::new();
    if modifiers.ctrl {
        parts.push("Ctrl");
    }
    if modifiers.alt {
        parts.push("Alt");
    }
    if modifiers.shift {
        parts.push("Shift");
    }
    if modifiers.command && !modifiers.ctrl {
        parts.push("Cmd");
    }
    let key_name = match key {
        egui::Key::ArrowDown => "Arrow Down",
        egui::Key::ArrowLeft => "Arrow Left",
        egui::Key::ArrowRight => "Arrow Right",
        egui::Key::ArrowUp => "Arrow Up",
        egui::Key::Backspace => "Backspace",
        egui::Key::Delete => "Delete",
        egui::Key::End => "End",
        egui::Key::Enter => "Enter",
        egui::Key::Escape => "Escape",
        egui::Key::F1 => "F1",
        egui::Key::F2 => "F2",
        egui::Key::F3 => "F3",
        egui::Key::F4 => "F4",
        egui::Key::F5 => "F5",
        egui::Key::F6 => "F6",
        egui::Key::F7 => "F7",
        egui::Key::F8 => "F8",
        egui::Key::F9 => "F9",
        egui::Key::F10 => "F10",
        egui::Key::F11 => "F11",
        egui::Key::F12 => "F12",
        egui::Key::Home => "Home",
        egui::Key::Tab => "Tab",
        egui::Key::A => "A",
        egui::Key::B => "B",
        egui::Key::C => "C",
        egui::Key::D => "D",
        egui::Key::E => "E",
        egui::Key::F => "F",
        egui::Key::G => "G",
        egui::Key::H => "H",
        egui::Key::I => "I",
        egui::Key::J => "J",
        egui::Key::K => "K",
        egui::Key::L => "L",
        egui::Key::M => "M",
        egui::Key::N => "N",
        egui::Key::O => "O",
        egui::Key::P => "P",
        egui::Key::Q => "Q",
        egui::Key::R => "R",
        egui::Key::S => "S",
        egui::Key::T => "T",
        egui::Key::U => "U",
        egui::Key::V => "V",
        egui::Key::W => "W",
        egui::Key::X => "X",
        egui::Key::Y => "Y",
        egui::Key::Z => "Z",
        _ => return None,
    };
    parts.push(key_name);
    Some(parts.join("+"))
}

fn draw_hotkeys_tab(
    ui: &mut egui::Ui,
    settings: &mut SettingsWindow,
    palette: &ThemePalette,
    i18n: &I18n,
) {
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            if eden_button(ui, palette, &i18n.tr("hotkeys_reset")).clicked() {
                settings.show_reset_hotkeys_confirmation = true;
            }
        });
        ui.add_space(2.0);

        for (group_name, indices) in HOTKEY_GROUPS {
            ui.add_space(8.0);
            ui.label(
                RichText::new(i18n.tr(group_name))
                    .strong()
                    .color(palette.text_header_section),
            );
            ui.add_space(3.0);

            for index in *indices {
                let (default_shortcut, description_key, can_rebind) = HOTKEYS[*index];
                let description = i18n.tr(description_key);
                let is_recording = settings.recording_hotkey == Some(*index);
                let is_customized = settings
                    .hotkey_values
                    .get(*index)
                    .is_some_and(|shortcut| shortcut != default_shortcut);
                let shortcut = if is_recording {
                    i18n.tr("hotkeys_record")
                } else {
                    settings
                        .hotkey_values
                        .get(*index)
                        .map(String::as_str)
                        .unwrap_or(default_shortcut)
                        .to_string()
                };
                let shortcut_color = if is_recording {
                    egui::Color32::RED
                } else if is_customized {
                    egui::Color32::WHITE
                } else if can_rebind {
                    palette.primary
                } else {
                    palette.borders_default
                };
                let mut shortcut_double_clicked = false;

                ui.horizontal(|ui| {
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(205.0, 22.0), egui::Sense::click());
                    let fill = if is_recording {
                        shortcut_color.gamma_multiply(0.12)
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    ui.painter().rect_filled(rect, 4.0, fill);
                    ui.painter().rect_stroke(
                        rect,
                        4.0,
                        egui::Stroke::new(if is_recording { 2.0 } else { 1.0 }, shortcut_color),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        &shortcut,
                        egui::FontId::monospace(palette.text_size),
                        shortcut_color,
                    );
                    shortcut_double_clicked = can_rebind && response.double_clicked();
                    // Keep descriptions inside the settings window. Without an explicit
                    // width, a larger font makes the row's content determine the window
                    // width and can cause the shortcuts view to grow horizontally.
                    let description_width = ui.available_width();
                    ui.add_sized(
                        [description_width, 22.0],
                        egui::Label::new(RichText::new(description).color(palette.text_normal))
                            .truncate(),
                    );
                });

                if shortcut_double_clicked {
                    settings.recording_hotkey = Some(*index);
                }
                ui.add_space(4.0);
            }
        }
    });

    if let Some(index) = settings.recording_hotkey {
        let mut recorded = None;
        let mut reset_to_default = false;

        ui.ctx().input(|input| {
            for event in &input.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                {
                    if *key == egui::Key::Escape {
                        settings.recording_hotkey = None;
                        return;
                    }
                    if *key == egui::Key::Backspace {
                        reset_to_default = true;
                    } else {
                        recorded = format_recorded_hotkey(*key, *modifiers);
                    }
                }
            }
        });

        if reset_to_default {
            settings.hotkey_values[index] = HOTKEYS[index].0.to_string();
            settings.recording_hotkey = None;
        } else if let Some(hotkey) = recorded {
            settings.hotkey_values[index] = hotkey;
            settings.recording_hotkey = None;
        }
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            folder_scanning_enabled: true,
            show_hidden_files_folders: true,
            show_item_viewer_icons: true,
            windows_context_menu_enabled: false,
            start_path: Some(PathBuf::from(MY_PC_PATH)),
            window_size_mode: WindowSizeMode::default(),
            pinned_tabs: Vec::new(),
            remember_open_tabs: false,
            unremembered_tabs: Vec::new(),
            time_format_24h: false,
            date_style: DateStyle::default(),
            sort_column: SortColumn::Name,
            sort_ascending: true,
            language: "en-US".to_string(),
            item_viewer_file_column_order: vec![
                ItemViewerHeaderColumn::Type,
                ItemViewerHeaderColumn::Size,
                ItemViewerHeaderColumn::Tags,
                ItemViewerHeaderColumn::Modified,
                ItemViewerHeaderColumn::Created,
            ],
            item_viewer_drive_column_order: vec![
                ItemViewerHeaderColumn::Type,
                ItemViewerHeaderColumn::Size,
                ItemViewerHeaderColumn::Usage,
            ],
            recycle_bin_column_order: vec![
                ItemViewerHeaderColumn::OriginalDirectory,
                ItemViewerHeaderColumn::Type,
                ItemViewerHeaderColumn::Size,
                ItemViewerHeaderColumn::Deleted,
                ItemViewerHeaderColumn::Created,
            ],
            item_viewer_file_column_sizes: default_item_viewer_file_column_size(),
            item_viewer_drive_column_sizes: default_item_viewer_drive_column_size(),
            recycle_bin_column_sizes: default_recycle_bin_column_size(),
            directory_settings: vec![],
        }
    }
}

const DEFAULT_FILE_COLUMNS: [ItemViewerHeaderColumn; 5] = [
    ItemViewerHeaderColumn::Type,
    ItemViewerHeaderColumn::Size,
    ItemViewerHeaderColumn::Tags,
    ItemViewerHeaderColumn::Modified,
    ItemViewerHeaderColumn::Created,
];

fn set_default_file_column_visible(
    order: &mut Vec<ItemViewerHeaderColumn>,
    column: ItemViewerHeaderColumn,
    visible: bool,
) {
    if visible {
        if order.contains(&column) {
            return;
        }

        let default_index = DEFAULT_FILE_COLUMNS
            .iter()
            .position(|candidate| *candidate == column)
            .unwrap_or(order.len());
        let insert_index = order
            .iter()
            .filter_map(|existing| DEFAULT_FILE_COLUMNS.iter().position(|c| c == existing))
            .filter(|index| *index < default_index)
            .count();
        order.insert(insert_index, column);
    } else {
        order.retain(|existing| *existing != column);
    }
}

fn file_column_label(i18n: &I18n, column: ItemViewerHeaderColumn) -> String {
    match column {
        ItemViewerHeaderColumn::Name => i18n.tr("explorer_cols_name"),
        ItemViewerHeaderColumn::Type => i18n.tr("explorer_cols_type"),
        ItemViewerHeaderColumn::Size => i18n.tr("explorer_cols_size"),
        ItemViewerHeaderColumn::Tags => i18n.tr("explorer_cols_tags"),
        ItemViewerHeaderColumn::Modified => i18n.tr("explorer_cols_modified"),
        ItemViewerHeaderColumn::Created => i18n.tr("explorer_cols_created"),
        _ => String::new(),
    }
}

fn draw_view_defaults_tab(
    ui: &mut egui::Ui,
    settings: &mut SettingsWindow,
    palette: &ThemePalette,
    i18n: &I18n,
) -> Option<SettingsAction> {
    let mut changed = false;
    let current_order = settings
        .current_settings
        .item_viewer_file_column_order
        .clone();

    if eden_button(ui, palette, &i18n.tr("settings_clear_directory_cache")).clicked() {
        settings.show_clear_directory_cache_confirmation = true;
    }

    ui.add_space(8.0);

    ui.group(|ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(i18n.tr("settings_view_defaults_columns"))
                .strong()
                .color(palette.text_header_section),
        );
        for column in DEFAULT_FILE_COLUMNS {
            let mut visible = current_order.contains(&column);
            ui.horizontal(|ui| {
                if setting_checkbox(
                    ui,
                    palette,
                    &mut visible,
                    RichText::new(file_column_label(i18n, column)).color(palette.text_normal),
                    format!("view_default_column_{column:?}"),
                ) {
                    set_default_file_column_visible(
                        &mut settings.current_settings.item_viewer_file_column_order,
                        column,
                        visible,
                    );
                    changed = true;
                }
                ui.add_space(8.0);
            });
        }
    });

    ui.add_space(8.0);

    ui.group(|ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(i18n.tr("settings_default_column_width"))
                .strong()
                .color(palette.text_header_section),
        );

        let defaults = default_item_viewer_file_column_size();
        if settings
            .current_settings
            .item_viewer_file_column_sizes
            .len()
            != defaults.len()
        {
            settings.current_settings.item_viewer_file_column_sizes = defaults;
        }

        for (index, column) in [
            ItemViewerHeaderColumn::Name,
            ItemViewerHeaderColumn::Type,
            ItemViewerHeaderColumn::Size,
            ItemViewerHeaderColumn::Tags,
            ItemViewerHeaderColumn::Modified,
            ItemViewerHeaderColumn::Created,
        ]
        .into_iter()
        .enumerate()
        {
            let mut width = settings.current_settings.item_viewer_file_column_sizes[index];
            setting_row(
                ui,
                |ui| setting_label(ui, &file_column_label(i18n, column), None, palette),
                |ui| {
                    apply_eden_visual_overrides(ui, palette);
                    if ui
                        .add_sized(
                            [SETTINGS_VALUE_WIDTH, ui.spacing().interact_size.y],
                            egui::DragValue::new(&mut width)
                                .range(20.0..=1000.0)
                                .speed(1.0),
                        )
                        .changed()
                    {
                        settings.current_settings.item_viewer_file_column_sizes[index] = width;
                        changed = true;
                    }
                },
            );
        }
    });

    changed.then_some(SettingsAction::ApplySettings)
}

fn draw_general_tab(
    ui: &mut egui::Ui,
    settings: &mut SettingsWindow,
    palette: &ThemePalette,
    i18n: &mut I18n,
) -> Option<SettingsAction> {
    let mut action = None;

    ui.horizontal(|ui| {
        if eden_button(ui, palette, &i18n.tr("settings_reset")).clicked() {
            settings.show_reset_confirmation = true;
        }
    });
    ui.group(|ui| {
        let language_label = i18n.tr("language");
        let english = i18n.tr("english");
        let japanese = i18n.tr("japanese");
        let indonesian = i18n.tr("indonesian");
        let chinese_simple = i18n.tr("chinese_simple");
        let chinese_traditional = i18n.tr("chinese_traditional");
        let chinese_hk = i18n.tr("chinese_traditional_hk");

        setting_row(
            ui,
            |ui| {
                setting_label(ui, &language_label, None, palette);
            },
            |ui| {
                let mut selected_locale = settings.current_settings.language.clone();

                if combo_box_string(
                    ui,
                    palette,
                    "language_selector",
                    SETTINGS_COMBO_WIDTH,
                    &mut selected_locale,
                    &[
                        ("en-US", english.clone()),
                        ("ja-JP", japanese.clone()),
                        ("id-ID", indonesian.clone()),
                        ("zh-CN", chinese_simple.clone()),
                        ("zh-TW", chinese_traditional.clone()),
                        ("zh-HK", chinese_hk.clone()),
                    ],
                ) {
                    i18n.set_locale(&selected_locale);
                    settings.current_settings.language = selected_locale;
                    action = Some(SettingsAction::ApplySettings);
                }
            },
        );

        ui.add_space(8.0);

        // Folder Scanning
        ui.horizontal(|ui| {
            if setting_checkbox(
                ui,
                palette,
                &mut settings.current_settings.folder_scanning_enabled,
                RichText::new(&i18n.tr("settings_folderscanning")).color(palette.text_normal),
                "settings_folderscanning",
            ) {
                // Auto-save when setting changes
                action = Some(SettingsAction::ApplySettings);
            }
            info_icon(ui, &i18n.tr("tooltip_settings_folderscanning"), palette);
        });
        ui.add_space(8.0);
        // Hidden Files/Folders
        ui.horizontal(|ui| {
            if setting_checkbox(
                ui,
                palette,
                &mut settings.current_settings.show_hidden_files_folders,
                RichText::new(&i18n.tr("settings_show_hidden_files_folders"))
                    .color(palette.text_normal),
                "settings_show_hidden_files_folders",
            ) {
                // Auto-save when setting changes
                action = Some(SettingsAction::ApplySettings);
            }
            info_icon(
                ui,
                &i18n.tr("tooltip_settings_show_hidden_files_folders"),
                palette,
            );
        });
        ui.add_space(8.0);
        // Remember Open Tabs
        ui.horizontal(|ui| {
            if setting_checkbox(
                ui,
                palette,
                &mut settings.current_settings.remember_open_tabs,
                RichText::new(&i18n.tr("settings_remember_open_tabs")).color(palette.text_normal),
                "settings_remember_open_tabs",
            ) {
                if settings.current_settings.remember_open_tabs {
                    // Enabling the global setting takes precedence over any
                    // per-tab opt-outs that were saved previously.
                    settings.current_settings.unremembered_tabs.clear();
                }
                action = Some(SettingsAction::ApplySettings);
            }
            info_icon(ui, &i18n.tr("tooltip_settings_remember_open_tabs"), palette);
        });
        ui.add_space(8.0);
        // Show/Hide Item Viewer File Icons
        ui.horizontal(|ui| {
            if setting_checkbox(
                ui,
                palette,
                &mut settings.current_settings.show_item_viewer_icons,
                RichText::new(&i18n.tr("settings_show_item_viewer_file_icons"))
                    .color(palette.text_normal),
                "settings_show_item_viewer_file_icons",
            ) {
                // Auto-save when setting changes
                action = Some(SettingsAction::ApplySettings);
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if setting_checkbox(
                ui,
                palette,
                &mut settings.current_settings.windows_context_menu_enabled,
                RichText::new(&i18n.tr("settings_contextmenu_enable")).color(palette.text_normal),
                "settings_contextmenu_enable",
            ) {
                action = Some(SettingsAction::ApplySettings);
            }
            info_icon(ui, &i18n.tr("tooltip_settings_contextmenu"), palette);
        });
        ui.add_space(8.0);
        // Date Style Section
        setting_row(
            ui,
            |ui| {
                setting_label(
                    ui,
                    &i18n.tr("settings_datestyle"),
                    Some((&i18n.tr("tooltip_settings_datestyle"), palette)),
                    palette,
                );
            },
            |ui| {
                let mut selected_style = settings.current_settings.date_style;

                draw_dropdown(
                    ui,
                    palette,
                    "date_style_selector",
                    SETTINGS_COMBO_WIDTH,
                    match selected_style {
                        DateStyle::Iso => i18n.tr("settings_datestyle_iso_label"),
                        DateStyle::UsShort => i18n.tr("settings_datestyle_us_short_label"),
                        DateStyle::Long => i18n.tr("settings_datestyle_long_label"),
                    },
                    |ui| {
                        ui.selectable_value(
                            &mut selected_style,
                            DateStyle::Iso,
                            i18n.tr("settings_datestyle_iso"),
                        );
                        ui.selectable_value(
                            &mut selected_style,
                            DateStyle::UsShort,
                            i18n.tr("settings_datestyle_us_short"),
                        );
                        ui.selectable_value(
                            &mut selected_style,
                            DateStyle::Long,
                            i18n.tr("settings_datestyle_long"),
                        );
                    },
                );

                if selected_style != settings.current_settings.date_style {
                    settings.current_settings.date_style = selected_style;
                    action = Some(SettingsAction::ApplySettings);
                }
            },
        );

        ui.add_space(8.0);
        // Time Format Section
        ui.horizontal(|ui| {
            if setting_checkbox(
                ui,
                palette,
                &mut settings.current_settings.time_format_24h,
                RichText::new(i18n.tr("settings_timeformat_24h")).color(palette.text_normal),
                "settings_timeformat_24h",
            ) {
                action = Some(SettingsAction::ApplySettings);
            }
            info_icon(ui, &i18n.tr("tooltip_settings_timeformat"), palette);
        });
    });
    ui.group(|ui| {
        // Starting Path
        setting_row(
            ui,
            |ui| {
                setting_label(ui, &i18n.tr("settings_startpath"), None, palette);
            },
            |ui| {
                if eden_button(ui, palette, regular::ARROW_COUNTER_CLOCKWISE)
                    .on_hover_text(i18n.tr("settings_startpath_reset_hover"))
                    .clicked()
                {
                    settings.current_settings.start_path = Some(PathBuf::from(MY_PC_PATH));
                    action = Some(SettingsAction::ApplySettings);
                }

                if eden_button(ui, palette, regular::FOLDER_OPEN)
                    .on_hover_text(i18n.tr("settings_startpath_choose_hover"))
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        settings.current_settings.start_path = Some(path);
                        action = Some(SettingsAction::ApplySettings);
                    }
                }
            },
        );

        let path_text = settings
            .current_settings
            .start_path
            .as_ref()
            .map(|p| {
                if p.as_os_str() == MY_PC_PATH {
                    return i18n.tr("settings_startpath_default");
                }

                let s = p.to_string_lossy();

                if s.len() > 40 {
                    format!("...{}", &s[s.len() - 40..])
                } else {
                    s.to_string()
                }
            })
            .unwrap_or_else(|| i18n.tr("settings_startpath_default"));

        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                apply_eden_visual_overrides(ui, palette);
                ui.add_sized(
                    [ui.available_width(), ui.spacing().interact_size.y],
                    egui::Label::new(path_text),
                )
                .on_hover_text(
                    settings
                        .current_settings
                        .start_path
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default(),
                );
            },
        );

        ui.add_space(8.0);
        // Window Size Section
        let mut window_size_changed = false;

        setting_row(
            ui,
            |ui| {
                setting_label(ui, &i18n.tr("settings_windowsize"), None, palette);
            },
            |ui| {
                let is_fullscreen = matches!(
                    settings.current_settings.window_size_mode,
                    WindowSizeMode::FullScreen
                );

                draw_dropdown(
                    ui,
                    palette,
                    "window_size_mode_selector",
                    SETTINGS_COMBO_WIDTH,
                    if is_fullscreen {
                        i18n.tr("settings_windowsize_fullscreen")
                    } else {
                        i18n.tr("settings_windowsize_custom")
                    },
                    |ui| {
                        if ui
                            .selectable_label(!is_fullscreen, i18n.tr("settings_windowsize_custom"))
                            .clicked()
                            && is_fullscreen
                        {
                            settings.current_settings.window_size_mode = WindowSizeMode::Custom {
                                width: 1200.0,
                                height: 800.0,
                            };
                            window_size_changed = true;
                        }

                        if ui
                            .selectable_label(
                                is_fullscreen,
                                i18n.tr("settings_windowsize_fullscreen"),
                            )
                            .clicked()
                            && !is_fullscreen
                        {
                            settings.current_settings.window_size_mode = WindowSizeMode::FullScreen;
                            window_size_changed = true;
                        }
                    },
                );

                if eden_button(ui, palette, regular::ARROW_COUNTER_CLOCKWISE)
                    .on_hover_text(i18n.tr("settings_windowsize_reset_hover"))
                    .clicked()
                {
                    settings.current_settings.window_size_mode = WindowSizeMode::default();
                    window_size_changed = true;
                }
            },
        );

        if let WindowSizeMode::Custom { width, height } =
            &mut settings.current_settings.window_size_mode
        {
            setting_row(
                ui,
                |ui| {
                    setting_label(ui, &i18n.tr("settings_windowsize_width"), None, palette);
                },
                |ui| {
                    apply_eden_visual_overrides(ui, palette);
                    window_size_changed |= ui
                        .add_sized(
                            [SETTINGS_VALUE_WIDTH, ui.spacing().interact_size.y],
                            egui::DragValue::new(width).range(800.0..=4000.0).speed(1.0),
                        )
                        .changed();
                },
            );

            setting_row(
                ui,
                |ui| {
                    setting_label(ui, &i18n.tr("settings_windowsize_height"), None, palette);
                },
                |ui| {
                    apply_eden_visual_overrides(ui, palette);
                    window_size_changed |= ui
                        .add_sized(
                            [SETTINGS_VALUE_WIDTH, ui.spacing().interact_size.y],
                            egui::DragValue::new(height)
                                .range(600.0..=3000.0)
                                .speed(1.0),
                        )
                        .changed();
                },
            );
        }

        if window_size_changed {
            action = Some(SettingsAction::ApplySettings);
        }

        ui.add_space(8.0);
        // Favorites Reset
        ui.horizontal(|ui| {
            if eden_button(
                ui,
                palette,
                &format!("{} {}", regular::TRASH, i18n.tr("settings_favorites_reset")),
            )
            .on_hover_text(
                egui::RichText::new(&i18n.tr("tooltip_settings_favorites_reset"))
                    .size(palette.tooltip_text_size)
                    .color(palette.tooltip_text_color),
            )
            .clicked()
            {
                settings.show_reset_favorites_confirmation = true;
            }
        });
    });

    action
}
fn draw_confirmation_dialog(
    ctx: &egui::Context,
    palette: &ThemePalette,
    i18n: &I18n,
    title: &str,
    message_lines: &[&str],
) -> Option<bool> {
    let mut result = None;
    let mut should_close = false;

    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .fixed_size([400.0, 150.0])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(
            egui::Frame::popup(&ctx.style_of(ctx.theme()))
                .corner_radius(egui::CornerRadius::same(8)),
        )
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                eden_text_label(ui, palette, title);
                for line in message_lines {
                    eden_text_label(ui, palette, line);
                }
                ui.add_space(20.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if eden_button(ui, palette, &i18n.tr("close")).clicked() {
                        should_close = true;
                    }
                    apply_eden_visual_overrides(ui, palette);
                    apply_eden_text_overrides(ui, palette);
                    if ui
                        .button(RichText::new(i18n.tr("reset")).color(palette.tab_close_hover))
                        .clicked()
                    {
                        result = Some(true);
                        should_close = true;
                    }
                });
            });
        });

    if should_close {
        result.or(Some(false))
    } else {
        None
    }
}

fn info_icon(ui: &mut egui::Ui, hover_text: &str, palette: &ThemePalette) -> egui::Response {
    let resp = ui.add(egui::Label::new(regular::QUESTION).sense(egui::Sense::hover()));

    if resp.hovered() {
        ui.painter().text(
            resp.rect.center(),
            egui::Align2::CENTER_CENTER,
            regular::QUESTION,
            egui::FontId::default(),
            palette.primary,
        );
    }

    if resp.hovered() {
        ui.ctx()
            .output_mut(|o| o.cursor_icon = egui::CursorIcon::Default);
        egui::containers::Area::new(ui.next_auto_id())
            .current_pos(resp.rect.right_top())
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(hover_text)
                                .size(palette.text_size)
                                .color(ui.visuals().text_color()),
                        );
                    });
            });
    }

    resp
}

fn setting_label(
    ui: &mut egui::Ui,
    text: &str,
    info: Option<(&str, &ThemePalette)>,
    palette: &ThemePalette,
) {
    let h = ui.spacing().interact_size.y;

    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            eden_text_label(ui, palette, text);
            if let Some((hover_text, palette)) = info {
                ui.add_space(4.0);
                info_icon(ui, hover_text, palette);
            }
        },
    );
}

fn setting_checkbox(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    checked: &mut bool,
    label: RichText,
    id: impl std::hash::Hash + std::fmt::Debug,
) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        let checkbox_resp = ui
            .allocate_ui_with_layout(
                egui::vec2(16.0, ui.spacing().interact_size.y),
                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                |ui| draw_checkbox(ui, palette, checked, id),
            )
            .inner;

        if checkbox_resp.clicked() {
            changed = true;
        }

        ui.add_space(4.0);
        let label_resp = ui.add(egui::Label::new(label).sense(egui::Sense::click()));
        if label_resp.clicked() {
            *checked = !*checked;
            changed = true;
        }
    });

    changed
}

const SETTINGS_LABEL_WIDTH: f32 = 150.0;
pub fn setting_row<L, R>(ui: &mut egui::Ui, left: L, right: R)
where
    L: FnOnce(&mut egui::Ui),
    R: FnOnce(&mut egui::Ui),
{
    let row_height = ui.spacing().interact_size.y;

    ui.horizontal(|ui| {
        // Fixed-width label column
        ui.allocate_ui_with_layout(
            egui::vec2(SETTINGS_LABEL_WIDTH, row_height),
            egui::Layout::left_to_right(egui::Align::Center),
            left,
        );

        // Flexible spacer
        ui.add_space(ui.available_width());

        // Right-aligned controls
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
}

pub fn combo_box_string(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    id: impl std::hash::Hash + std::fmt::Debug,
    width: f32,
    value: &mut String,
    options: &[(&str, String)],
) -> bool {
    let mut changed = false;

    let selected_text = options
        .iter()
        .find(|(key, _)| *key == value)
        .map(|(_, label)| label.as_str())
        .unwrap_or("");

    draw_dropdown(ui, palette, id, width, selected_text, |ui| {
        for (key, label) in options {
            if ui.selectable_label(value == key, label).clicked() {
                *value = (*key).to_string();
                changed = true;
            }
        }
    });

    changed
}

pub fn draw_settings_window(
    ctx: &egui::Context,
    settings: &mut SettingsWindow,
    i18n: &mut I18n,
    palette: &ThemePalette,
) -> Option<SettingsAction> {
    let mut action = None;

    if !settings.open {
        return None;
    }

    let mut should_close = false;

    // 🌑 Dark background overlay (modal effect); clicking it dismisses the window
    let modal_bg_clicked = egui::Area::new(egui::Id::new("settings_modal_bg"))
        .order(egui::Order::Middle)
        .interactable(true)
        .show(ctx, |ui| {
            let rect = ctx.content_rect();
            ui.painter()
                .rect_filled(rect, 0.0, palette.modal_background_effect_color);
            ui.interact(
                rect,
                ui.id().with("settings_modal_bg_click"),
                egui::Sense::click(),
            )
            .clicked()
        })
        .inner;

    if modal_bg_clicked {
        should_close = true;
    }

    let mut settings_open = settings.open;

    egui::Window::new(i18n.tr("settings"))
        .collapsible(false)
        .resizable(false)
        .fixed_size([600.0, 550.0])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut settings_open)
        .frame(
            egui::Frame::popup(&ctx.style_of(ctx.theme()))
                .corner_radius(egui::CornerRadius::same(8)),
        )
        .show(ctx, |ui| {
            let mut style = (**ui.style()).clone();
            style.text_styles = [
                (egui::TextStyle::Heading, egui::FontId::proportional(14.0)),
                (
                    egui::TextStyle::Body,
                    egui::FontId::proportional(palette.text_size),
                ),
                (
                    egui::TextStyle::Button,
                    egui::FontId::proportional(palette.text_size),
                ),
                (
                    egui::TextStyle::Small,
                    egui::FontId::proportional(palette.text_size),
                ),
                (
                    egui::TextStyle::Monospace,
                    egui::FontId::monospace(palette.text_size),
                ),
            ]
            .into();
            style.override_text_valign = Some(egui::Align::Center);
            ui.set_style(style);

            ui.horizontal(|ui| {
                if eden_toggle_button(
                    ui,
                    palette,
                    settings.selected_tab == SettingsTab::General,
                    &i18n.tr("settings_tab_general"),
                )
                .clicked()
                {
                    settings.selected_tab = SettingsTab::General;
                }
                if eden_toggle_button(
                    ui,
                    palette,
                    settings.selected_tab == SettingsTab::ViewDefaults,
                    &i18n.tr("settings_tab_view_defaults"),
                )
                .clicked()
                {
                    settings.selected_tab = SettingsTab::ViewDefaults;
                }
                if eden_toggle_button(
                    ui,
                    palette,
                    settings.selected_tab == SettingsTab::Hotkeys,
                    &i18n.tr("settings_tab_hotkeys"),
                )
                .clicked()
                {
                    settings.selected_tab = SettingsTab::Hotkeys;
                }
            });
            ui.separator();

            // SCROLLABLE CONTENT
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    if settings.selected_tab == SettingsTab::Hotkeys {
                        if settings.hotkey_values.len() != HOTKEYS.len() {
                            settings.hotkey_values = HOTKEYS
                                .iter()
                                .map(|(shortcut, _, _)| (*shortcut).to_string())
                                .collect();
                        }
                        draw_hotkeys_tab(ui, settings, palette, i18n);
                        return;
                    }

                    if settings.selected_tab == SettingsTab::ViewDefaults {
                        action = draw_view_defaults_tab(ui, settings, palette, i18n);
                        return;
                    }

                    action = draw_general_tab(ui, settings, palette, i18n);
                });

            // These confirmation dialogs live outside the selected-tab content so they
            // remain visible if the user changes tabs while a destructive action is pending.
            if settings.show_reset_favorites_confirmation {
                if let Some(confirmed) = draw_confirmation_dialog(
                    ctx,
                    palette,
                    i18n,
                    &i18n.tr("settings_favorites_reset_confirm"),
                    &[
                        &i18n.tr("settings_favorite_reset_confirm_label1"),
                        &i18n.tr("settings_favorite_reset_confirm_label2"),
                    ],
                ) {
                    settings.show_reset_favorites_confirmation = false;
                    if confirmed {
                        action = Some(SettingsAction::ResetFavourites);
                    }
                }
            }

            if settings.show_reset_confirmation {
                if let Some(confirmed) = draw_confirmation_dialog(
                    ctx,
                    palette,
                    i18n,
                    &i18n.tr("settings_reset_confirm"),
                    &[
                        &i18n.tr("settings_reset_confirm_label1"),
                        &i18n.tr("settings_reset_confirm_label2"),
                    ],
                ) {
                    settings.show_reset_confirmation = false;
                    if confirmed {
                        action = Some(SettingsAction::ResetToDefaults);
                    }
                }
            }

            if settings.show_reset_hotkeys_confirmation {
                if let Some(confirmed) = draw_confirmation_dialog(
                    ctx,
                    palette,
                    i18n,
                    &i18n.tr("hotkeys_reset_confirm"),
                    &[
                        &i18n.tr("hotkeys_reset_confirm_label1"),
                        &i18n.tr("hotkeys_reset_confirm_label2"),
                    ],
                ) {
                    settings.show_reset_hotkeys_confirmation = false;
                    if confirmed {
                        settings.hotkey_values = HOTKEYS
                            .iter()
                            .map(|(shortcut, _, _)| (*shortcut).to_string())
                            .collect();
                        settings.recording_hotkey = None;
                    }
                }
            }

            if settings.show_clear_directory_cache_confirmation {
                if let Some(confirmed) = draw_confirmation_dialog(
                    ctx,
                    palette,
                    i18n,
                    &i18n.tr("settings_clear_directory_cache_confirm"),
                    &[
                        &i18n.tr("settings_clear_directory_cache_confirm_label1"),
                        &i18n.tr("settings_clear_directory_cache_confirm_label2"),
                    ],
                ) {
                    settings.show_clear_directory_cache_confirmation = false;
                    if confirmed {
                        action = Some(SettingsAction::ClearDirectoryCache);
                    }
                }
            }
        });
    if should_close || !settings_open {
        settings.open = false;
    }
    action
}
