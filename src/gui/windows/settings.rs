use crate::core::{
    fs::{DateStyle, MY_PC_PATH},
    indexer::WindowSizeMode,
    utils::widgets::{
        apply_eden_visual_overrides, draw_checkbox, draw_dropdown, eden_button, eden_text_label,
    },
};
use crate::gui::i18n::I18n;
use crate::gui::icons::IconCache;
use crate::gui::theme::ThemePalette;
use crate::gui::utils::SortColumn;
use crate::gui::windows::containers::enums::ItemViewerHeaderColumn;
use crate::gui::windows::containers::structs::ItemViewerDisplayMode;
use crate::gui::windows::enums::SettingsAction;
use crate::gui::windows::structs::{AppSettings, SettingsWindow};
use eframe::egui;
use egui::RichText;
use egui_phosphor::regular;
use std::path::PathBuf;

const SETTINGS_COMBO_WIDTH: f32 = 180.0;
const SETTINGS_VALUE_WIDTH: f32 = 92.0;
/// Content column width, capped so rows never stretch across a wide window -
/// otherwise `setting_row`'s label-left/control-right layout ends up with the
/// control floating far away from its label.
const SETTINGS_CONTENT_MAX_WIDTH: f32 = 720.0;
const SETTINGS_SIDEBAR_WIDTH: f32 = 190.0;

/// The Settings page's left-hand category sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SettingsCategory {
    #[default]
    General,
    Behavior,
    Startup,
    Appearance,
    Favorites,
    ContextMenu,
    TabGroups,
    Tags,
    Advanced,
}

impl SettingsCategory {
    pub const ALL: [SettingsCategory; 9] = [
        SettingsCategory::General,
        SettingsCategory::Behavior,
        SettingsCategory::Startup,
        SettingsCategory::Appearance,
        SettingsCategory::Favorites,
        SettingsCategory::ContextMenu,
        SettingsCategory::TabGroups,
        SettingsCategory::Tags,
        SettingsCategory::Advanced,
    ];

    fn icon(self) -> &'static str {
        match self {
            SettingsCategory::General => regular::GEAR,
            SettingsCategory::Behavior => regular::SLIDERS,
            SettingsCategory::Startup => regular::APP_WINDOW,
            SettingsCategory::Appearance => regular::PALETTE,
            SettingsCategory::Favorites => regular::STAR,
            SettingsCategory::ContextMenu => regular::LIST,
            SettingsCategory::TabGroups => regular::FOLDERS,
            SettingsCategory::Tags => regular::TAG,
            SettingsCategory::Advanced => regular::WRENCH,
        }
    }

    fn label(self, i18n: &I18n) -> String {
        match self {
            SettingsCategory::General => i18n.tr("settings_category_general"),
            SettingsCategory::Behavior => i18n.tr("settings_category_behavior"),
            SettingsCategory::Startup => i18n.tr("settings_category_startup"),
            SettingsCategory::Appearance => i18n.tr("settings_category_appearance"),
            SettingsCategory::Favorites => i18n.tr("settings_category_favorites"),
            SettingsCategory::ContextMenu => i18n.tr("settings_category_context_menu"),
            SettingsCategory::TabGroups => i18n.tr("settings_category_tab_groups"),
            SettingsCategory::Tags => i18n.tr("settings_category_tags"),
            SettingsCategory::Advanced => i18n.tr("settings_category_advanced"),
        }
    }
}

fn display_mode_label(i18n: &I18n, mode: ItemViewerDisplayMode) -> String {
    match mode {
        ItemViewerDisplayMode::Details => i18n.tr("view_details"),
        ItemViewerDisplayMode::Gallery => i18n.tr("view_gallery"),
        ItemViewerDisplayMode::Columns => i18n.tr("view_columns"),
        ItemViewerDisplayMode::ColumnPreview => i18n.tr("view_column_preview"),
        ItemViewerDisplayMode::Preview => i18n.tr("view_preview"),
        ItemViewerDisplayMode::DetailPreview => i18n.tr("view_detail_preview"),
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
            time_format_24h: false,
            date_style: DateStyle::default(),
            custom_date_format: String::new(),
            sort_column: SortColumn::Name,
            sort_ascending: true,
            language: "en-US".to_string(),
            item_viewer_file_column_order: vec![
                ItemViewerHeaderColumn::Type,
                ItemViewerHeaderColumn::Size,
                ItemViewerHeaderColumn::Modified,
                ItemViewerHeaderColumn::Created,
                ItemViewerHeaderColumn::Tags,
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
            item_viewer_file_column_sizes:
                crate::core::indexer::default_item_viewer_file_column_size(),
            item_viewer_drive_column_sizes:
                crate::core::indexer::default_item_viewer_drive_column_size(),
            recycle_bin_column_sizes: crate::core::indexer::default_recycle_bin_column_size(),
            directory_settings: vec![],
            double_click_navigates_up: true,
            show_selection_checkboxes: true,
            middle_click_opens_new_tab: true,
            restore_last_session_tabs: false,
            default_display_mode: ItemViewerDisplayMode::Details,
            default_search_scope: crate::core::everything::DefaultSearchScope::default(),
            search_engine: crate::core::everything::SearchEngine::default(),
            custom_context_menu: Vec::new(),
            tab_groups: Vec::new(),
        }
    }
}

pub(crate) fn info_icon(ui: &mut egui::Ui, hover_text: &str, palette: &ThemePalette) -> egui::Response {
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

pub(crate) fn setting_label(
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

pub(crate) fn setting_checkbox(
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

        let label_resp = ui.add(egui::Label::new(label).sense(egui::Sense::click()));
        if label_resp.clicked() {
            *checked = !*checked;
            changed = true;
        }
    });

    changed
}

const SETTINGS_LABEL_WIDTH: f32 = 190.0;
/// Standard vertical gap between one settings field/row and the next -
/// used everywhere a form stacks multiple `setting_row`s (or equivalent
/// field groups) so spacing reads the same across every settings category
/// instead of each page hand-tuning its own `add_space` calls.
pub(crate) const SETTINGS_FIELD_GAP: f32 = 10.0;

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

        // Flexible spacer - bounded by the content column's own max width
        // (see `SETTINGS_CONTENT_MAX_WIDTH`), so on a wide window this gap
        // stays reasonable instead of stretching the control to the far
        // edge of the screen.
        ui.add_space(ui.available_width());

        // Right-aligned controls
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });

    // Consistent breathing room before whatever comes next - a plain
    // `ui.horizontal` on its own doesn't add any trailing space, which is
    // what made consecutive rows (Width/Height, Program/Arguments, ...)
    // read as cramped together.
    ui.add_space(SETTINGS_FIELD_GAP);
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

/// A framed, padded card for one settings section - used for every group of
/// related fields instead of a bare `ui.group()`, so sections read as
/// distinct cards rather than blending into the page background.
pub(crate) fn settings_section<R>(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::NONE
        .fill(ui.visuals().faint_bg_color)
        .stroke(egui::Stroke::new(1.5, palette.borders_default))
        .corner_radius(egui::CornerRadius::same(palette.medium_radius))
        .inner_margin(egui::Margin::same(14))
        .outer_margin(egui::Margin {
            bottom: 12,
            ..Default::default()
        })
        .show(ui, add_contents)
        .inner
}

/// A smaller nested card for one entry within a `settings_section` - a tab
/// group, a custom context menu command, a favorite, etc. Uses `row_bg`
/// (distinct from both the section's `faint_bg_color` and a `TextEdit`'s own
/// `extreme_bg_color` interior) so a list of entries reads as distinct rows,
/// and - importantly - so text fields/checkboxes drawn inside them don't
/// blend invisibly into a same-colored card background.
pub(crate) fn entry_card<R>(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::NONE
        .fill(palette.row_bg)
        .stroke(egui::Stroke::new(1.5, palette.borders_default))
        .corner_radius(egui::CornerRadius::same(palette.small_radius))
        .inner_margin(egui::Margin::same(10))
        .outer_margin(egui::Margin {
            bottom: 8,
            ..Default::default()
        })
        .show(ui, add_contents)
        .inner
}

/// A muted placeholder row shown when a settings list (tab groups, custom
/// context menu commands, favorites, ...) has no entries yet.
pub(crate) fn empty_state_hint(ui: &mut egui::Ui, palette: &ThemePalette, icon: &str, text: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(10.0);
        ui.label(RichText::new(icon).size(28.0).color(palette.tooltip_text_color));
        ui.add_space(4.0);
        ui.label(RichText::new(text).color(palette.tooltip_text_color));
        ui.add_space(10.0);
    });
}

/// Draws small up/down move buttons for reordering `index` within a list of
/// `len` items - disabled at whichever end doesn't apply. Returns
/// `Some((from, to))` for the caller to `.swap(from, to)` on click.
pub(crate) fn reorder_buttons(
    ui: &mut egui::Ui,
    palette: &ThemePalette,
    index: usize,
    len: usize,
) -> Option<(usize, usize)> {
    let mut result = None;

    ui.add_enabled_ui(index > 0, |ui| {
        if eden_button(ui, palette, regular::CARET_UP).clicked() {
            result = Some((index, index - 1));
        }
    });
    ui.add_enabled_ui(index + 1 < len, |ui| {
        if eden_button(ui, palette, regular::CARET_DOWN).clicked() {
            result = Some((index, index + 1));
        }
    });

    result
}

/// Draws the Settings page's content directly into `ui` - used as the content of the
/// dedicated Settings tab, rather than a floating modal window.
pub fn draw_settings_page(
    ui: &mut egui::Ui,
    settings: &mut SettingsWindow,
    i18n: &mut I18n,
    palette: &ThemePalette,
    icon_cache: &IconCache,
    theme_customizer: &mut crate::gui::windows::structs::ThemeCustomizer,
    tags_state: &mut crate::gui::windows::containers::structs::TagsState,
    favorites: &mut Vec<crate::gui::windows::containers::structs::FavoriteItem>,
) -> (
    Option<SettingsAction>,
    Option<crate::gui::windows::containers::enums::ItemViewerAction>,
) {
    let ctx = ui.ctx().clone();
    let ctx = &ctx;
    let mut action = None;
    let mut item_action = None;

    {
        // 🎯 Smaller font override (fix giant UI)
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
        ]
        .into();
        style.override_text_valign = Some(egui::Align::Center);
        ui.set_style(style);

        ui.add_space(8.0);

        ui.horizontal_top(|ui| {
            // ---------------- Left: category sidebar ----------------
            ui.allocate_ui_with_layout(
                egui::vec2(SETTINGS_SIDEBAR_WIDTH, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    for category in SettingsCategory::ALL {
                        let is_selected = settings.selected_category == category;
                        let (icon_color, text_color) = if is_selected {
                            (
                                palette.settings_nav_selected_icon_color,
                                palette.settings_nav_selected_text_color,
                            )
                        } else {
                            (
                                palette.settings_nav_inactive_color,
                                palette.settings_nav_inactive_color,
                            )
                        };

                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 30.0),
                            egui::Sense::click(),
                        );

                        if is_selected {
                            ui.painter().rect_filled(
                                rect,
                                egui::CornerRadius::same(palette.medium_radius),
                                palette.primary_hover,
                            );
                        } else if resp.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                egui::CornerRadius::same(palette.medium_radius),
                                ui.visuals().faint_bg_color,
                            );
                        }

                        // Icon and label are drawn as two separate text runs
                        // (instead of one combined string) so their colors -
                        // `settings_nav_selected_icon_color`/
                        // `_text_color` - can be independently themed. `text`
                        // (used for the label) centers its `pos` argument
                        // vertically per its `Align2`, but `galley` always
                        // draws from a literal top-left origin - so the
                        // icon's own vertical centering has to be computed
                        // by hand from its galley's measured height.
                        let font_id = egui::FontId::proportional(palette.text_size);
                        let icon_galley = ui.painter().layout_no_wrap(
                            format!("{}  ", category.icon()),
                            font_id.clone(),
                            icon_color,
                        );
                        let icon_top_left = egui::pos2(
                            rect.left() + 10.0,
                            rect.center().y - icon_galley.size().y / 2.0,
                        );
                        ui.painter().galley(icon_top_left, icon_galley.clone(), icon_color);
                        ui.painter().text(
                            rect.left_center() + egui::vec2(10.0 + icon_galley.size().x, 0.0),
                            egui::Align2::LEFT_CENTER,
                            category.label(i18n),
                            font_id,
                            text_color,
                        );

                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if resp.clicked() {
                            settings.selected_category = category;
                        }
                    }
                },
            );

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);

            // ---------------- Right: selected category's content ----------------
            ui.vertical(|ui| {
                ui.set_max_width(SETTINGS_CONTENT_MAX_WIDTH.min(ui.available_width()));

                egui::ScrollArea::vertical()
                    .id_salt("settings_content_scroll")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_max_width(SETTINGS_CONTENT_MAX_WIDTH);

                        match settings.selected_category {
                            SettingsCategory::General => {
                                draw_general_section(ui, i18n, settings, palette, &mut action);
                            }
                            SettingsCategory::Behavior => {
                                draw_behavior_section(ui, i18n, settings, palette, &mut action);
                            }
                            SettingsCategory::Startup => {
                                draw_startup_section(ui, i18n, settings, palette, &mut action);
                            }
                            SettingsCategory::Appearance => {
                                if let Some(theme_action) =
                                    crate::gui::windows::customizetheme::draw_theme_customizer_content(
                                        ui,
                                        i18n,
                                        &ui.ctx().clone(),
                                        theme_customizer,
                                        palette,
                                    )
                                {
                                    action = Some(SettingsAction::ThemeCustomizer(theme_action));
                                }
                            }
                            SettingsCategory::ContextMenu => {
                                if let Some(a) = crate::gui::windows::context_menu_settings_ui::draw_custom_context_menu_settings(
                                    ui, i18n, settings, palette, icon_cache,
                                ) {
                                    action = Some(a);
                                }
                            }
                            SettingsCategory::TabGroups => {
                                if let Some(a) = crate::gui::windows::tab_groups_ui::draw_tab_groups_settings(
                                    ui, i18n, settings, palette, icon_cache,
                                ) {
                                    action = Some(a);
                                }
                            }
                            SettingsCategory::Favorites => {
                                if crate::gui::windows::favorites_ui::draw_favorites_settings(
                                    ui,
                                    i18n,
                                    palette,
                                    icon_cache,
                                    favorites,
                                    &mut settings.icon_picker_search,
                                ) {
                                    crate::core::indexer::save_favorites('C', favorites);
                                }
                            }
                            SettingsCategory::Tags => {
                                crate::gui::windows::containers::tags::draw_tags(
                                    ui, i18n, icon_cache, palette, tags_state,
                                );
                                if let Some(tags_action) = tags_state.pending_action.take() {
                                    item_action = Some(tags_action);
                                }
                            }
                            SettingsCategory::Advanced => {
                                draw_advanced_section(ui, i18n, settings, palette, &mut action);
                            }
                        }
                    });
            });
        });

        crate::gui::windows::containers::tags::draw_delete_confirmation_popup(
            ctx, i18n, palette, tags_state,
        );

        // Reset Favorites Confirmation Dialog
        if settings.show_reset_favorites_confirmation {
            let mut should_close = false;
            egui::Window::new(i18n.tr("settings_favorites_reset_confirm"))
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
                        eden_text_label(ui, palette, &i18n.tr("settings_favorites_reset_confirm"));
                        eden_text_label(
                            ui,
                            palette,
                            &i18n.tr("settings_favorite_reset_confirm_label1"),
                        );
                        eden_text_label(
                            ui,
                            palette,
                            &i18n.tr("settings_favorite_reset_confirm_label2"),
                        );
                        ui.add_space(20.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if eden_button(ui, palette, &i18n.tr("close")).clicked() {
                                        should_close = true;
                                    }
                                    if eden_button(ui, palette, &i18n.tr("reset")).clicked() {
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
    }

    (action, item_action)
}

fn draw_general_section(
    ui: &mut egui::Ui,
    i18n: &mut I18n,
    settings: &mut SettingsWindow,
    palette: &ThemePalette,
    action: &mut Option<SettingsAction>,
) {
    settings_section(ui, palette, |ui| {
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
                    *action = Some(SettingsAction::ApplySettings);
                }
            },
        );

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
                        DateStyle::Custom => i18n.tr("settings_datestyle_custom_label"),
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
                        ui.selectable_value(
                            &mut selected_style,
                            DateStyle::Custom,
                            i18n.tr("settings_datestyle_custom"),
                        );
                    },
                );

                if selected_style != settings.current_settings.date_style {
                    settings.current_settings.date_style = selected_style;
                    *action = Some(SettingsAction::ApplySettings);
                }
            },
        );

        if settings.current_settings.date_style == DateStyle::Custom {
            setting_row(
                ui,
                |ui| {
                    setting_label(
                        ui,
                        &i18n.tr("settings_datestyle_custom_label"),
                        Some((&i18n.tr("tooltip_settings_datestyle_custom"), palette)),
                        palette,
                    );
                },
                |ui| {
                    let response = ui.add_sized(
                        [SETTINGS_COMBO_WIDTH, ui.spacing().interact_size.y],
                        egui::TextEdit::singleline(&mut settings.current_settings.custom_date_format)
                            .hint_text(i18n.tr("settings_datestyle_custom_placeholder")),
                    );
                    if response.changed() {
                        *action = Some(SettingsAction::ApplySettings);
                    }
                },
            );

            // Live preview (or an "invalid pattern" notice) directly under
            // the field, so a typo is obvious immediately rather than only
            // showing up later in the Modified/Created columns.
            setting_row(
                ui,
                |ui| {
                    ui.add_space(0.0);
                },
                |ui| {
                    let preview =
                        crate::core::fs::preview_custom_date_format(&settings.current_settings.custom_date_format);
                    match preview {
                        Some(text) => {
                            ui.weak(text);
                        }
                        None => {
                            ui.colored_label(
                                egui::Color32::from_rgb(220, 90, 90),
                                i18n.tr("settings_datestyle_custom_invalid"),
                            );
                        }
                    }
                },
            );
        }

        ui.horizontal(|ui| {
            if setting_checkbox(
                ui,
                palette,
                &mut settings.current_settings.time_format_24h,
                RichText::new(i18n.tr("settings_timeformat_24h")).color(palette.text_normal),
                "settings_timeformat_24h",
            ) {
                *action = Some(SettingsAction::ApplySettings);
            }
            info_icon(ui, &i18n.tr("tooltip_settings_timeformat"), palette);
        });
    });

    settings_section(ui, palette, |ui| {
        ui.horizontal(|ui| {
            if setting_checkbox(
                ui,
                palette,
                &mut settings.current_settings.folder_scanning_enabled,
                RichText::new(&i18n.tr("settings_folderscanning")).color(palette.text_normal),
                "settings_folderscanning",
            ) {
                *action = Some(SettingsAction::ApplySettings);
            }
            info_icon(ui, &i18n.tr("tooltip_settings_folderscanning"), palette);
        });
        ui.add_space(SETTINGS_FIELD_GAP);
        ui.horizontal(|ui| {
            if setting_checkbox(
                ui,
                palette,
                &mut settings.current_settings.show_hidden_files_folders,
                RichText::new(&i18n.tr("settings_show_hidden_files_folders"))
                    .color(palette.text_normal),
                "settings_show_hidden_files_folders",
            ) {
                *action = Some(SettingsAction::ApplySettings);
            }
            info_icon(
                ui,
                &i18n.tr("tooltip_settings_show_hidden_files_folders"),
                palette,
            );
        });
        ui.add_space(SETTINGS_FIELD_GAP);
        ui.horizontal(|ui| {
            if setting_checkbox(
                ui,
                palette,
                &mut settings.current_settings.show_item_viewer_icons,
                RichText::new(&i18n.tr("settings_show_item_viewer_file_icons"))
                    .color(palette.text_normal),
                "settings_show_item_viewer_file_icons",
            ) {
                *action = Some(SettingsAction::ApplySettings);
            }
        });
        ui.add_space(SETTINGS_FIELD_GAP);
        ui.horizontal(|ui| {
            if setting_checkbox(
                ui,
                palette,
                &mut settings.current_settings.windows_context_menu_enabled,
                RichText::new(&i18n.tr("settings_contextmenu_enable")).color(palette.text_normal),
                "settings_contextmenu_enable",
            ) {
                *action = Some(SettingsAction::ApplySettings);
            }
            info_icon(ui, &i18n.tr("tooltip_settings_contextmenu"), palette);
        });
    });
}

fn draw_behavior_section(
    ui: &mut egui::Ui,
    i18n: &mut I18n,
    settings: &mut SettingsWindow,
    palette: &ThemePalette,
    action: &mut Option<SettingsAction>,
) {
    settings_section(ui, palette, |ui| {
        ui.horizontal(|ui| {
            if setting_checkbox(
                ui,
                palette,
                &mut settings.current_settings.double_click_navigates_up,
                RichText::new(i18n.tr("settings_double_click_navigates_up"))
                    .color(palette.text_normal),
                "settings_double_click_navigates_up",
            ) {
                *action = Some(SettingsAction::ApplySettings);
            }
            info_icon(
                ui,
                &i18n.tr("tooltip_settings_double_click_navigates_up"),
                palette,
            );
        });

        ui.add_space(SETTINGS_FIELD_GAP);
        ui.horizontal(|ui| {
            if setting_checkbox(
                ui,
                palette,
                &mut settings.current_settings.show_selection_checkboxes,
                RichText::new(i18n.tr("settings_show_selection_checkboxes"))
                    .color(palette.text_normal),
                "settings_show_selection_checkboxes",
            ) {
                *action = Some(SettingsAction::ApplySettings);
            }
            info_icon(
                ui,
                &i18n.tr("tooltip_settings_show_selection_checkboxes"),
                palette,
            );
        });

        ui.add_space(SETTINGS_FIELD_GAP);
        ui.horizontal(|ui| {
            if setting_checkbox(
                ui,
                palette,
                &mut settings.current_settings.middle_click_opens_new_tab,
                RichText::new(i18n.tr("settings_middle_click_opens_new_tab"))
                    .color(palette.text_normal),
                "settings_middle_click_opens_new_tab",
            ) {
                *action = Some(SettingsAction::ApplySettings);
            }
            info_icon(
                ui,
                &i18n.tr("tooltip_settings_middle_click_opens_new_tab"),
                palette,
            );
        });

        ui.add_space(SETTINGS_FIELD_GAP);
        ui.horizontal(|ui| {
            if setting_checkbox(
                ui,
                palette,
                &mut settings.current_settings.restore_last_session_tabs,
                RichText::new(i18n.tr("settings_restore_last_session_tabs"))
                    .color(palette.text_normal),
                "settings_restore_last_session_tabs",
            ) {
                *action = Some(SettingsAction::ApplySettings);
            }
            info_icon(
                ui,
                &i18n.tr("tooltip_settings_restore_last_session_tabs"),
                palette,
            );
        });
    });

    settings_section(ui, palette, |ui| {
        setting_row(
            ui,
            |ui| {
                setting_label(ui, &i18n.tr("settings_default_layout"), None, palette);
            },
            |ui| {
                let mut selected = settings.current_settings.default_display_mode;
                let selected_text = display_mode_label(i18n, selected);

                draw_dropdown(
                    ui,
                    palette,
                    "default_display_mode_selector",
                    SETTINGS_COMBO_WIDTH,
                    selected_text,
                    |ui| {
                        for mode in [
                            ItemViewerDisplayMode::Details,
                            ItemViewerDisplayMode::Gallery,
                            ItemViewerDisplayMode::Columns,
                            ItemViewerDisplayMode::ColumnPreview,
                            ItemViewerDisplayMode::Preview,
                            ItemViewerDisplayMode::DetailPreview,
                        ] {
                            if ui
                                .selectable_label(selected == mode, display_mode_label(i18n, mode))
                                .clicked()
                            {
                                selected = mode;
                            }
                        }
                    },
                );

                if selected != settings.current_settings.default_display_mode {
                    settings.current_settings.default_display_mode = selected;
                    *action = Some(SettingsAction::ApplySettings);
                }
            },
        );
        info_icon(ui, &i18n.tr("tooltip_settings_default_layout"), palette);
    });

    settings_section(ui, palette, |ui| {
        setting_row(
            ui,
            |ui| {
                setting_label(ui, &i18n.tr("settings_default_search_scope"), None, palette);
            },
            |ui| {
                let mut selected = settings.current_settings.default_search_scope;
                let selected_text = search_scope_setting_label(i18n, selected);

                draw_dropdown(
                    ui,
                    palette,
                    "default_search_scope_selector",
                    SETTINGS_COMBO_WIDTH,
                    selected_text,
                    |ui| {
                        for scope in [
                            crate::core::everything::DefaultSearchScope::CurrentFolder,
                            crate::core::everything::DefaultSearchScope::Everywhere,
                        ] {
                            if ui
                                .selectable_label(
                                    selected == scope,
                                    search_scope_setting_label(i18n, scope),
                                )
                                .clicked()
                            {
                                selected = scope;
                            }
                        }
                    },
                );

                if selected != settings.current_settings.default_search_scope {
                    settings.current_settings.default_search_scope = selected;
                    *action = Some(SettingsAction::ApplySettings);
                }
            },
        );
        info_icon(
            ui,
            &i18n.tr("tooltip_settings_default_search_scope"),
            palette,
        );
    });

    settings_section(ui, palette, |ui| {
        setting_row(
            ui,
            |ui| {
                setting_label(ui, &i18n.tr("settings_search_engine"), None, palette);
            },
            |ui| {
                let mut selected = settings.current_settings.search_engine;
                let selected_text = search_engine_setting_label(i18n, selected);

                draw_dropdown(
                    ui,
                    palette,
                    "search_engine_selector",
                    SETTINGS_COMBO_WIDTH,
                    selected_text,
                    |ui| {
                        for engine in [
                            crate::core::everything::SearchEngine::Everything,
                            crate::core::everything::SearchEngine::BuiltIn,
                        ] {
                            if ui
                                .selectable_label(
                                    selected == engine,
                                    search_engine_setting_label(i18n, engine),
                                )
                                .clicked()
                            {
                                selected = engine;
                            }
                        }
                    },
                );

                if selected != settings.current_settings.search_engine {
                    settings.current_settings.search_engine = selected;
                    *action = Some(SettingsAction::ApplySettings);
                }
            },
        );
        info_icon(ui, &i18n.tr("tooltip_settings_search_engine"), palette);
    });
}

fn search_engine_setting_label(
    i18n: &I18n,
    engine: crate::core::everything::SearchEngine,
) -> String {
    match engine {
        crate::core::everything::SearchEngine::Everything => i18n.tr("search_engine_everything"),
        crate::core::everything::SearchEngine::BuiltIn => i18n.tr("search_engine_builtin"),
    }
}

fn search_scope_setting_label(
    i18n: &I18n,
    scope: crate::core::everything::DefaultSearchScope,
) -> String {
    match scope {
        crate::core::everything::DefaultSearchScope::CurrentFolder => {
            i18n.tr("search_scope_current_folder")
        }
        crate::core::everything::DefaultSearchScope::Everywhere => {
            i18n.tr("search_scope_everywhere")
        }
    }
}

fn draw_startup_section(
    ui: &mut egui::Ui,
    i18n: &mut I18n,
    settings: &mut SettingsWindow,
    palette: &ThemePalette,
    action: &mut Option<SettingsAction>,
) {
    settings_section(ui, palette, |ui| {
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
                    *action = Some(SettingsAction::ApplySettings);
                }

                if eden_button(ui, palette, regular::FOLDER_OPEN)
                    .on_hover_text(i18n.tr("settings_startpath_choose_hover"))
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        settings.current_settings.start_path = Some(path);
                        *action = Some(SettingsAction::ApplySettings);
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
    });

    settings_section(ui, palette, |ui| {
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
            *action = Some(SettingsAction::ApplySettings);
        }
    });
}

fn draw_advanced_section(
    ui: &mut egui::Ui,
    i18n: &mut I18n,
    settings: &mut SettingsWindow,
    palette: &ThemePalette,
    action: &mut Option<SettingsAction>,
) {
    settings_section(ui, palette, |ui| {
        ui.horizontal(|ui| {
            if eden_button(ui, palette, &i18n.tr("settings_reset")).clicked() {
                *action = Some(SettingsAction::ResetToDefaults);
            }
            if eden_button(ui, palette, &i18n.tr("settings_export")).clicked() {
                *action = Some(SettingsAction::ExportSettings);
            }
            if eden_button(ui, palette, &i18n.tr("settings_import")).clicked() {
                *action = Some(SettingsAction::ImportSettings);
            }
        });
    });

    settings_section(ui, palette, |ui| {
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
}
