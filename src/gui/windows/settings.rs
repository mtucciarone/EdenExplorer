use crate::core::{
    fs::{DateStyle, MY_PC_PATH},
    indexer::WindowSizeMode,
    utils::widgets::draw_checkbox,
};
use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use crate::gui::utils::SortColumn;
use crate::gui::windows::enums::SettingsAction;
use crate::gui::windows::structs::{AppSettings, SettingsWindow};
use eframe::egui;
use egui::RichText;
use egui_phosphor::regular;
use std::path::PathBuf;

const SETTINGS_COMBO_WIDTH: f32 = 180.0;
const SETTINGS_VALUE_WIDTH: f32 = 92.0;

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
            time_format_24h: false,
            date_style: DateStyle::default(),
            sort_column: SortColumn::Name,
            sort_ascending: true,
            language: "en-US".to_string(),
        }
    }
}

// Helper function for info icon with hover text (non-clickable)
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

fn right_aligned_cell<R>(
    ui: &mut egui::Ui,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let row_height = ui.spacing().interact_size.y;
    let size = egui::vec2(ui.available_width(), row_height);
    ui.allocate_ui_with_layout(
        size,
        egui::Layout::right_to_left(egui::Align::Center),
        add_contents,
    )
}

fn setting_checkbox(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    checked: &mut bool,
    label: RichText,
    id: impl std::hash::Hash,
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

        let label_resp = ui.add(egui::Label::new(label).sense(egui::Sense::click()));
        if label_resp.clicked() {
            *checked = !*checked;
            changed = true;
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

    egui::Window::new(format!("EdenExplorer - {}", i18n.tr("settings")))
        .collapsible(false)
        .resizable(false)
        .fixed_size([400.0, 400.0])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(egui::Frame::popup(&ctx.style()).corner_radius(egui::CornerRadius::same(8)))
        .show(ctx, |ui| {
            // 🎯 Smaller font override (fix giant UI)
            let mut style = (*ui.ctx().style()).clone();
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
            ]
            .into();
            style.override_text_valign = Some(egui::Align::Center);
            ui.set_style(style);

            // SCROLLABLE CONTENT
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .button(format!(
                                "{} {}",
                                regular::ARROW_CLOCKWISE,
                                i18n.tr("settings_reset")
                            ))
                            .clicked()
                        {
                            action = Some(SettingsAction::ResetToDefaults);
                        }
                    });

                    egui::Grid::new("settings_language_row")
                        .num_columns(2)
                        .min_col_width(120.0)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label(RichText::new(i18n.tr("language")).color(palette.text_normal));

                            right_aligned_cell(ui, |ui| {
                                let mut selected_locale =
                                    settings.current_settings.language.clone();

                                egui::ComboBox::from_id_salt("language_selector")
                                    .width(SETTINGS_COMBO_WIDTH)
                                    .selected_text(match selected_locale.as_str() {
                                        "ja-JP" => i18n.tr("japanese"),
                                        "id-ID" => i18n.tr("indonesian"),
                                        "zh-CN" => i18n.tr("chinese_simple"),
                                        "zh-TW" => i18n.tr("chinese_traditional"),
                                        "zh-HK" => i18n.tr("chinese_traditional_hk"),
                                        _ => i18n.tr("english"),
                                    })
                                    .show_ui(ui, |ui| {
                                        if ui
                                            .selectable_label(
                                                selected_locale == "en-US",
                                                i18n.tr("english"),
                                            )
                                            .clicked()
                                        {
                                            selected_locale = "en-US".to_string();
                                        }

                                        if ui
                                            .selectable_label(
                                                selected_locale == "ja-JP",
                                                i18n.tr("japanese"),
                                            )
                                            .clicked()
                                        {
                                            selected_locale = "ja-JP".to_string();
                                        }

                                        if ui
                                            .selectable_label(
                                                selected_locale == "id-ID",
                                                i18n.tr("indonesian"),
                                            )
                                            .clicked()
                                        {
                                            selected_locale = "id-ID".to_string();
                                        }

                                        if ui
                                            .selectable_label(
                                                selected_locale == "zh-CN",
                                                i18n.tr("chinese_simple"),
                                            )
                                            .clicked()
                                        {
                                            selected_locale = "zh-CN".to_string();
                                        }

                                        if ui
                                            .selectable_label(
                                                selected_locale == "zh-TW",
                                                i18n.tr("chinese_traditional"),
                                            )
                                            .clicked()
                                        {
                                            selected_locale = "zh-TW".to_string();
                                        }

                                        if ui
                                            .selectable_label(
                                                selected_locale == "zh-HK",
                                                i18n.tr("chinese_traditional_hk"),
                                            )
                                            .clicked()
                                        {
                                            selected_locale = "zh-HK".to_string();
                                        }
                                    });

                                if selected_locale != settings.current_settings.language {
                                    i18n.set_locale(&selected_locale);
                                    settings.current_settings.language = selected_locale;
                                    action = Some(SettingsAction::ApplySettings);
                                }
                            });
                            ui.end_row();
                        });

                    // Folder Scanning
                    ui.horizontal(|ui| {
                        if setting_checkbox(
                            ui,
                            palette,
                            &mut settings.current_settings.folder_scanning_enabled,
                            RichText::new(&i18n.tr("settings_folderscanning"))
                                .color(palette.text_normal),
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
                            RichText::new(&i18n.tr("settings_contextmenu_enable"))
                                .color(palette.text_normal),
                            "settings_contextmenu_enable",
                        ) {
                            action = Some(SettingsAction::ApplySettings);
                        }
                        info_icon(ui, &i18n.tr("tooltip_settings_contextmenu"), palette);
                    });
                    ui.add_space(8.0);
                    // Starting Path
                    ui.label(&i18n.tr("settings_startpath"));
                    ui.horizontal(|ui| {
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

                        ui.add_sized([200.0, 18.0], egui::Label::new(path_text))
                            .on_hover_text(
                                settings
                                    .current_settings
                                    .start_path
                                    .as_ref()
                                    .map(|p| p.to_string_lossy().to_string())
                                    .unwrap_or_default(),
                            );

                        // ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        right_aligned_cell(ui, |ui| {
                            if ui
                                .button(regular::ARROW_COUNTER_CLOCKWISE)
                                .on_hover_text(i18n.tr("settings_startpath_reset_hover"))
                                .clicked()
                            {
                                settings.current_settings.start_path =
                                    Some(PathBuf::from(MY_PC_PATH));
                                action = Some(SettingsAction::ApplySettings);
                            }

                            if ui
                                .button(regular::FOLDER_OPEN)
                                .on_hover_text(i18n.tr("settings_startpath_choose_hover"))
                                .clicked()
                            {
                                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                    settings.current_settings.start_path = Some(path);
                                    action = Some(SettingsAction::ApplySettings);
                                }
                            }
                        });
                    });
                    ui.add_space(8.0);
                    // Date Style Section
                    egui::Grid::new("settings_datestyle_row")
                        .num_columns(2)
                        .min_col_width(120.0)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::LEFT), |ui| {
                                ui.label(
                                    RichText::new(i18n.tr("settings_datestyle"))
                                        .color(palette.text_normal),
                                );
                                info_icon(ui, &i18n.tr("tooltip_settings_datestyle"), palette);
                            });
                            right_aligned_cell(ui, |ui| {
                                let mut selected_style = settings.current_settings.date_style;

                                egui::ComboBox::from_id_salt("date_style_selector")
                                    .width(SETTINGS_COMBO_WIDTH)
                                    .selected_text(match selected_style {
                                        DateStyle::Iso => i18n.tr("settings_datestyle_iso_label"),
                                        DateStyle::UsShort => {
                                            i18n.tr("settings_datestyle_us_short_label")
                                        }
                                        DateStyle::Long => i18n.tr("settings_datestyle_long_label"),
                                    })
                                    .show_ui(ui, |ui| {
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
                                    });

                                if selected_style != settings.current_settings.date_style {
                                    settings.current_settings.date_style = selected_style;
                                    action = Some(SettingsAction::ApplySettings);
                                }
                            });
                            ui.end_row();
                        });
                    ui.add_space(8.0);
                    // Time Format Section
                    ui.horizontal(|ui| {
                        if setting_checkbox(
                            ui,
                            palette,
                            &mut settings.current_settings.time_format_24h,
                            RichText::new(i18n.tr("settings_timeformat_24h"))
                                .color(palette.text_normal),
                            "settings_timeformat_24h",
                        ) {
                            action = Some(SettingsAction::ApplySettings);
                        }
                        info_icon(ui, &i18n.tr("tooltip_settings_timeformat"), palette);
                    });
                    ui.add_space(8.0);
                    // Window Size Section
                    let mut window_size_changed = false;
                    egui::Grid::new("settings_windowsize_row")
                        .num_columns(2)
                        .min_col_width(120.0)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(i18n.tr("settings_windowsize"))
                                    .color(palette.text_normal),
                            );
                            right_aligned_cell(ui, |ui| {
                                // ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
                                let is_fullscreen = matches!(
                                    settings.current_settings.window_size_mode,
                                    WindowSizeMode::FullScreen
                                );

                                if ui
                                    .button(regular::ARROW_COUNTER_CLOCKWISE)
                                    .on_hover_text(i18n.tr("settings_windowsize_reset_hover"))
                                    .clicked()
                                {
                                    settings.current_settings.window_size_mode =
                                        WindowSizeMode::default();
                                    window_size_changed = true;
                                }

                                // ui.add_space(6.0);

                                egui::ComboBox::from_id_salt("window_size_mode_selector")
                                    .width(SETTINGS_COMBO_WIDTH)
                                    .selected_text(if is_fullscreen {
                                        i18n.tr("settings_windowsize_fullscreen")
                                    } else {
                                        i18n.tr("settings_windowsize_custom")
                                    })
                                    .show_ui(ui, |ui| {
                                        if ui
                                            .selectable_label(
                                                !is_fullscreen,
                                                i18n.tr("settings_windowsize_custom"),
                                            )
                                            .clicked()
                                            && is_fullscreen
                                        {
                                            settings.current_settings.window_size_mode =
                                                WindowSizeMode::Custom {
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
                                            settings.current_settings.window_size_mode =
                                                WindowSizeMode::FullScreen;
                                            window_size_changed = true;
                                        }
                                    });
                                // });
                            });
                            ui.end_row();

                            if let WindowSizeMode::Custom { width, height } =
                                &mut settings.current_settings.window_size_mode
                            {
                                ui.label(
                                    RichText::new(i18n.tr("settings_windowsize_width"))
                                        .color(palette.text_normal),
                                );
                                right_aligned_cell(ui, |ui| {
                                    window_size_changed |= ui
                                        .add_sized(
                                            [SETTINGS_VALUE_WIDTH, ui.spacing().interact_size.y],
                                            egui::DragValue::new(width)
                                                .range(800.0..=4000.0)
                                                .speed(1.0),
                                        )
                                        .changed();
                                });
                                ui.end_row();

                                ui.label(
                                    RichText::new(i18n.tr("settings_windowsize_height"))
                                        .color(palette.text_normal),
                                );
                                right_aligned_cell(ui, |ui| {
                                    window_size_changed |= ui
                                        .add_sized(
                                            [SETTINGS_VALUE_WIDTH, ui.spacing().interact_size.y],
                                            egui::DragValue::new(height)
                                                .range(600.0..=3000.0)
                                                .speed(1.0),
                                        )
                                        .changed();
                                });
                                ui.end_row();
                            }
                        });

                    if window_size_changed {
                        action = Some(SettingsAction::ApplySettings);

                        let target_size = match &settings.current_settings.window_size_mode {
                            WindowSizeMode::FullScreen => ctx.input(|i| i.viewport().monitor_size),
                            WindowSizeMode::Custom { width, height } => {
                                Some(egui::vec2(*width, *height))
                            }
                        };
                        if let Some(size) = target_size {
                            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                        }
                    }
                    ui.add_space(8.0);
                    // Favorites Reset
                    ui.horizontal(|ui| {
                        if ui
                            .button(format!(
                                "{} {}",
                                regular::TRASH,
                                i18n.tr("settings_favorites_reset")
                            ))
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
            // Reset Favorites Confirmation Dialog
            if settings.show_reset_favorites_confirmation {
                let mut should_close = false;
                egui::Window::new(i18n.tr("settings_favorites_reset_confirm"))
                    .collapsible(false)
                    .resizable(false)
                    .fixed_size([400.0, 150.0])
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .frame(
                        egui::Frame::popup(&ctx.style()).corner_radius(egui::CornerRadius::same(8)),
                    )
                    .show(ctx, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new(
                                    &i18n.tr("settings_favorite_reset_confirm_label1"),
                                )
                                .size(palette.text_size),
                            );
                            ui.label(
                                egui::RichText::new(
                                    &i18n.tr("settings_favorite_reset_confirm_label2"),
                                )
                                .size(palette.text_size),
                            );
                            ui.add_space(20.0);
                            ui.horizontal(|ui| {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button(i18n.tr("close")).clicked() {
                                            should_close = true;
                                        }
                                        if ui.button(i18n.tr("reset")).clicked() {
                                            action = Some(SettingsAction::ResetFavourites);
                                            should_close = true;
                                        }
                                    },
                                );
                            });
                        });
                    });
                if should_close {
                    settings.show_reset_favorites_confirmation = false;
                }
            }
            ui.separator();
            // Footer
            egui::Grid::new("settings_footer")
                .num_columns(2)
                .min_row_height(ui.spacing().interact_size.y)
                .spacing([12.0, 0.0])
                .show(ui, |ui| {
                    ui.label(i18n.tr("settings_changes_auto_saved"));
                    right_aligned_cell(ui, |ui| {
                        if ui
                            .button(format!("{} {}", regular::X, i18n.tr("close")))
                            .clicked()
                        {
                            should_close = true;
                        }
                    });
                    ui.end_row();
                });
        });
    // Update the open state based on should_close
    if should_close {
        settings.open = false;
    }
    action
}
