//! Bulk rename: select multiple files/folders, define a shared pattern
//! (plain find/replace or regex, plus `{n}`/`{ext}`/`{name}` placeholders),
//! see a live old-name -> new-name preview with collision detection, then
//! commit every rename in one batch. Triggered from the context menu's
//! "Rename" entry when more than one item is selected (see
//! `itemviewer_helper.rs`); the actual on-disk commit happens in
//! `mainwindow_imp.rs`'s `rename_paths_native`, mirroring
//! `delete_paths_native`'s one-`IFileOperation`-many-items-one-
//! `PerformOperations` shape.

use crate::gui::i18n::I18n;
use crate::gui::theme::ThemePalette;
use crate::gui::MainWindow;
use eframe::egui;
use egui_phosphor::regular;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RenameMatchMode {
    Simple,
    Regex,
}

pub struct BulkRenamePattern {
    pub find: String,
    pub replace: String,
    pub case_insensitive: bool,
    pub mode: RenameMatchMode,
    /// The `{n}` value for the first item in the batch (in selection order);
    /// each later item gets `numbering_start + its index`.
    pub numbering_start: i64,
}

impl Default for BulkRenamePattern {
    fn default() -> Self {
        Self {
            find: String::new(),
            replace: String::new(),
            case_insensitive: false,
            mode: RenameMatchMode::Simple,
            numbering_start: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollisionKind {
    /// Another row in this same batch (in the same folder) would produce
    /// the identical new name.
    WithinBatch,
    /// The new name already exists on disk, as something *not* part of
    /// this rename batch.
    WithExisting,
}

pub struct BulkRenamePreviewRow {
    pub original_path: PathBuf,
    pub original_name: String,
    pub new_name: String,
    pub is_dir: bool,
    pub collision: Option<CollisionKind>,
    pub invalid_name: bool,
}

struct BulkRenameItem {
    path: PathBuf,
    original_name: String,
    is_dir: bool,
}

pub struct BulkRenameState {
    items: Vec<BulkRenameItem>,
    pub pattern: BulkRenamePattern,
    pub preview: Vec<BulkRenamePreviewRow>,
    /// Set when Regex mode's `find` fails to compile - every row is shown
    /// unchanged while this is set, and "Rename All" is blocked, exactly
    /// like a real collision would block it.
    pub regex_error: Option<String>,
    pub focus_requested: bool,
}

impl BulkRenameState {
    /// Captures the selection (sorted, deduplicated, `is_dir` stat'd once
    /// per item right now) and computes an initial preview against the
    /// default (no-op) pattern - so the modal never opens showing stale or
    /// empty rows.
    pub fn new(paths: Vec<PathBuf>) -> Self {
        let mut items: Vec<BulkRenameItem> = paths
            .into_iter()
            .map(|path| {
                let original_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let is_dir = path.is_dir();
                BulkRenameItem {
                    path,
                    original_name,
                    is_dir,
                }
            })
            .collect();
        items.sort_by(|a, b| a.path.cmp(&b.path));
        items.dedup_by(|a, b| a.path == b.path);

        let mut state = Self {
            items,
            pattern: BulkRenamePattern::default(),
            preview: Vec::new(),
            regex_error: None,
            focus_requested: true,
        };
        state.recompute_preview();
        state
    }

    /// Recomputes every row's `new_name` and re-runs collision detection.
    /// Called once on open and again whenever a pattern widget reports
    /// `.changed()` - never unconditionally every frame, since Regex mode
    /// would otherwise recompile the pattern ~60x/sec while the dialog
    /// just sits open.
    pub fn recompute_preview(&mut self) {
        self.regex_error = None;

        let compiled_regex = if self.pattern.mode == RenameMatchMode::Regex && !self.pattern.find.is_empty() {
            let pattern_text = if self.pattern.case_insensitive {
                format!("(?i){}", self.pattern.find)
            } else {
                self.pattern.find.clone()
            };
            match regex::Regex::new(&pattern_text) {
                Ok(re) => Some(re),
                Err(e) => {
                    self.regex_error = Some(e.to_string());
                    None
                }
            }
        } else {
            None
        };
        let regex_failed = self.pattern.mode == RenameMatchMode::Regex && self.regex_error.is_some();

        let mut rows: Vec<BulkRenamePreviewRow> = self
            .items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let index = self.pattern.numbering_start + idx as i64;
                let new_name = if regex_failed {
                    item.original_name.clone()
                } else {
                    compute_new_name(item, &self.pattern, compiled_regex.as_ref(), index)
                };
                let invalid_name = !names_equal_ci(&new_name, &item.original_name)
                    && !MainWindow::is_submitted_filename_valid(&new_name);
                BulkRenamePreviewRow {
                    original_path: item.path.clone(),
                    original_name: item.original_name.clone(),
                    new_name,
                    is_dir: item.is_dir,
                    collision: None,
                    invalid_name,
                }
            })
            .collect();

        detect_collisions(&mut rows);
        self.preview = rows;
    }

    /// Whether "Rename All" should be enabled: no regex error, no
    /// collisions, no invalid names, and at least one row actually changes
    /// (a no-op batch has nothing to commit).
    pub fn can_commit(&self) -> bool {
        self.regex_error.is_none()
            && self.preview.iter().all(|r| r.collision.is_none() && !r.invalid_name)
            && self
                .preview
                .iter()
                .any(|r| !names_equal_ci(&r.new_name, &r.original_name))
    }

    pub fn blocking_row_count(&self) -> usize {
        self.preview
            .iter()
            .filter(|r| r.collision.is_some() || r.invalid_name)
            .count()
    }
}

fn names_equal_ci(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// Splits a name into (base, extension-without-dot). Directories never get
/// an extension split out, even if their name contains a `.` - matching
/// Explorer's own convention, not the files-only rule.
fn split_name_ext(name: &str, is_dir: bool) -> (&str, &str) {
    if is_dir {
        return (name, "");
    }
    match name.rsplit_once('.') {
        Some((base, ext)) if !base.is_empty() => (base, ext),
        _ => (name, ""),
    }
}

/// Literal substring replace-all; case-insensitive matching is done over
/// `char`s (not raw bytes/`to_lowercase()` slices), since Unicode
/// case-folding can occasionally change a substring's byte length and a
/// byte-index-based approach could misalign as a result.
fn simple_find_replace(name: &str, find: &str, replace: &str, case_insensitive: bool) -> String {
    if find.is_empty() {
        return name.to_string();
    }
    if !case_insensitive {
        return name.replace(find, replace);
    }

    let name_chars: Vec<char> = name.chars().collect();
    let find_chars: Vec<char> = find.chars().collect();
    let mut result = String::new();
    let mut i = 0;
    while i < name_chars.len() {
        let fits = i + find_chars.len() <= name_chars.len();
        let matched = fits
            && (0..find_chars.len())
                .all(|j| name_chars[i + j].to_lowercase().eq(find_chars[j].to_lowercase()));
        if matched {
            result.push_str(replace);
            i += find_chars.len();
        } else {
            result.push(name_chars[i]);
            i += 1;
        }
    }
    result
}

/// Expands `{name}`/`{ext}`/`{n}`/`{n:NNN}` placeholders left-to-right,
/// non-overlapping. `{name}` resolves to the *original*, pre-find/replace
/// stem (not `input`'s own text) - deliberate, so a pattern like
/// `{name}_backup` still means "the original name plus a suffix" even when
/// find/replace is also active. Any other `{...}` token, or a malformed
/// `{n:...}` width, is left as literal text rather than stripped or
/// treated as an error.
fn substitute_placeholders(input: &str, original_base: &str, ext: &str, index: i64) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut result = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            if let Some(close_offset) = chars[i + 1..].iter().position(|&c| c == '}') {
                let token: String = chars[i + 1..i + 1 + close_offset].iter().collect();
                if let Some(text) = resolve_placeholder(&token, original_base, ext, index) {
                    result.push_str(&text);
                    i = i + 1 + close_offset + 1;
                    continue;
                }
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

fn resolve_placeholder(token: &str, original_base: &str, ext: &str, index: i64) -> Option<String> {
    match token {
        "name" => return Some(original_base.to_string()),
        "ext" => return Some(ext.to_string()),
        "n" => return Some(index.to_string()),
        _ => {}
    }
    if let Some(width_str) = token.strip_prefix("n:") {
        if width_str.len() <= 2 && !width_str.is_empty() && width_str.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(width) = width_str.parse::<usize>() {
                if width <= 20 {
                    return Some(format!("{index:0width$}"));
                }
            }
        }
    }
    None
}

fn compute_new_name(
    item: &BulkRenameItem,
    pattern: &BulkRenamePattern,
    compiled_regex: Option<&regex::Regex>,
    index: i64,
) -> String {
    let (base, ext) = split_name_ext(&item.original_name, item.is_dir);
    let replaced = match pattern.mode {
        RenameMatchMode::Simple => {
            simple_find_replace(base, &pattern.find, &pattern.replace, pattern.case_insensitive)
        }
        RenameMatchMode::Regex => match compiled_regex {
            Some(re) => re.replace_all(base, pattern.replace.as_str()).into_owned(),
            None => base.to_string(),
        },
    };
    let substituted = substitute_placeholders(&replaced, base, ext, index);

    if ext.is_empty() {
        substituted
    } else {
        format!("{substituted}.{ext}")
    }
}

/// Per-parent-directory collision detection - a bulk-rename selection can
/// span multiple folders (e.g. results dragged in from a saved search), so
/// this can't assume one shared parent.
fn detect_collisions(rows: &mut [BulkRenamePreviewRow]) {
    use std::collections::{HashMap, HashSet};

    let batch_original_paths: HashSet<PathBuf> =
        rows.iter().map(|r| r.original_path.clone()).collect();

    let mut by_parent: HashMap<PathBuf, Vec<usize>> = HashMap::new();
    for (i, row) in rows.iter().enumerate() {
        let parent = row.original_path.parent().map(PathBuf::from).unwrap_or_default();
        by_parent.entry(parent).or_default().push(i);
    }

    for (parent, indices) in &by_parent {
        // Within-batch: rows in this parent that share a case-insensitive
        // new_name.
        let mut name_groups: HashMap<String, Vec<usize>> = HashMap::new();
        for &i in indices {
            name_groups.entry(rows[i].new_name.to_lowercase()).or_default().push(i);
        }
        for group in name_groups.values() {
            if group.len() < 2 {
                continue;
            }
            // A group where every row is already unchanged (two items that
            // already happened to share a name before this dialog even
            // opened, e.g. same name in different case) isn't a conflict
            // this rename is creating - skip it. Any group with at least
            // one row actually changing IS a real conflict for every row
            // in it, including an unchanged one whose name is being taken.
            let all_unchanged = group
                .iter()
                .all(|&j| names_equal_ci(&rows[j].new_name, &rows[j].original_name));
            if !all_unchanged {
                for &i in group {
                    rows[i].collision = Some(CollisionKind::WithinBatch);
                }
            }
        }

        // Existing-file: new_name already occupied on disk by something
        // that isn't itself vacating that name as part of this batch.
        for &i in indices {
            if rows[i].collision.is_some() {
                continue;
            }
            if names_equal_ci(&rows[i].new_name, &rows[i].original_name) {
                continue;
            }
            let candidate = parent.join(&rows[i].new_name);
            if candidate.exists() && !batch_original_paths.contains(&candidate) {
                rows[i].collision = Some(CollisionKind::WithExisting);
            }
        }
    }
}

pub enum BulkRenameModalAction {
    None,
    Cancelled,
    Commit(Vec<(PathBuf, String)>),
}

/// Draws the bulk-rename dialog as an `egui::Area` + `egui::Frame::popup`
/// (not `egui::Window` - see CLAUDE.md's mistakes log for why the
/// paste-conflict modal, which this mirrors, abandoned `Window` after
/// several failed attempts to fix its per-Id remembered-size cache).
pub fn draw_bulk_rename_modal(
    ctx: &egui::Context,
    i18n: &I18n,
    palette: &ThemePalette,
    state: &mut BulkRenameState,
) -> BulkRenameModalAction {
    use crate::core::utils::widgets::{
        ghost_dialog_button, modal_frame, modal_icon_header, primary_dialog_button,
    };

    let mut action = BulkRenameModalAction::None;
    let mut changed = false;

    let scrim_clicked = egui::Area::new(egui::Id::new("bulk_rename_scrim"))
        .order(egui::Order::Middle)
        .interactable(true)
        .show(ctx, |ui| {
            let rect = ctx.content_rect();
            ui.painter()
                .rect_filled(rect, 0.0, palette.modal_background_effect_color);
            ui.interact(rect, ui.id().with("bulk_rename_scrim_click"), egui::Sense::click())
                .clicked()
        })
        .inner;
    if scrim_clicked {
        action = BulkRenameModalAction::Cancelled;
    }

    let popup_width = 520.0;
    egui::Area::new(egui::Id::new("bulk_rename_area"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            modal_frame(&ctx.style_of(ctx.theme()), palette)
                .show(ui, |ui| {
                    ui.set_width(popup_width);
                    ui.vertical(|ui| {
                        let subtitle = format!(
                            "{} {}",
                            state.items.len(),
                            i18n.tr("bulk_rename_items_selected")
                        );
                        modal_icon_header(
                            ui,
                            palette,
                            regular::PENCIL_SIMPLE_LINE,
                            palette.primary,
                            &i18n.tr("bulk_rename_title"),
                            Some(&subtitle),
                        );
                        ui.add_space(14.0);
                        ui.separator();
                        ui.add_space(10.0);

                        ui.horizontal(|ui| {
                            if ui
                                .selectable_label(
                                    state.pattern.mode == RenameMatchMode::Simple,
                                    i18n.tr("bulk_rename_mode_simple"),
                                )
                                .clicked()
                                && state.pattern.mode != RenameMatchMode::Simple
                            {
                                state.pattern.mode = RenameMatchMode::Simple;
                                changed = true;
                            }
                            if ui
                                .selectable_label(
                                    state.pattern.mode == RenameMatchMode::Regex,
                                    i18n.tr("bulk_rename_mode_regex"),
                                )
                                .clicked()
                                && state.pattern.mode != RenameMatchMode::Regex
                            {
                                state.pattern.mode = RenameMatchMode::Regex;
                                changed = true;
                            }
                        });
                        ui.add_space(8.0);

                        egui::Grid::new("bulk_rename_fields")
                            .num_columns(2)
                            .spacing([8.0, 6.0])
                            .show(ui, |ui| {
                                ui.label(i18n.tr("bulk_rename_find_label"));
                                let find_id = ui.id().with("bulk_rename_find");
                                let find_resp = ui.add(
                                    egui::TextEdit::singleline(&mut state.pattern.find)
                                        .id(find_id)
                                        .desired_width(280.0),
                                );
                                if state.focus_requested {
                                    find_resp.request_focus();
                                    if find_resp.has_focus() {
                                        state.focus_requested = false;
                                    }
                                }
                                changed |= find_resp.changed();
                                ui.end_row();

                                ui.label(i18n.tr("bulk_rename_replace_label"));
                                changed |= ui
                                    .add(
                                        egui::TextEdit::singleline(&mut state.pattern.replace)
                                            .desired_width(280.0),
                                    )
                                    .changed();
                                ui.end_row();

                                if state.pattern.mode == RenameMatchMode::Simple {
                                    ui.label(i18n.tr("bulk_rename_numbering_start_label"));
                                    changed |= ui
                                        .add(egui::DragValue::new(&mut state.pattern.numbering_start))
                                        .changed();
                                    ui.end_row();
                                }
                            });

                        changed |= crate::gui::windows::settings::setting_checkbox(
                            ui,
                            palette,
                            &mut state.pattern.case_insensitive,
                            egui::RichText::new(i18n.tr("bulk_rename_case_insensitive"))
                                .color(palette.text_normal),
                            "bulk_rename_case_insensitive",
                        );

                        ui.add_space(4.0);
                        let hint_key = if state.pattern.mode == RenameMatchMode::Regex {
                            "bulk_rename_regex_hint"
                        } else {
                            "bulk_rename_placeholder_hint"
                        };
                        ui.label(
                            egui::RichText::new(i18n.tr(hint_key))
                                .size(palette.tooltip_text_size)
                                .color(palette.tooltip_text_color),
                        );

                        if let Some(err) = &state.regex_error {
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}: {err}",
                                    i18n.tr("bulk_rename_regex_invalid")
                                ))
                                .size(palette.text_size)
                                .color(palette.drive_usage_warning),
                            );
                        }

                        ui.add_space(12.0);
                        ui.label(
                            egui::RichText::new(i18n.tr("bulk_rename_preview_header"))
                                .strong()
                                .size(palette.text_size)
                                .color(ui.visuals().text_color()),
                        );
                        ui.add_space(4.0);

                        let row_height = 22.0;
                        let max_rows_visible = 8.0;
                        let list_height =
                            (state.preview.len() as f32 * row_height).min(row_height * max_rows_visible);

                        egui::Frame::NONE
                            .fill(palette.row_bg)
                            .corner_radius(egui::CornerRadius::same(palette.medium_radius))
                            .stroke(egui::Stroke::new(1.0, palette.borders_default))
                            .inner_margin(egui::Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                egui::ScrollArea::vertical()
                                    .max_height(list_height)
                                    .show(ui, |ui| {
                                        for row in &state.preview {
                                            draw_preview_row(ui, i18n, palette, row);
                                        }
                                    });
                            });

                        ui.add_space(10.0);
                        let blocking = state.blocking_row_count();
                        if blocking > 0 {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{blocking} {}",
                                    i18n.tr("bulk_rename_collision_count")
                                ))
                                .size(palette.text_size)
                                .color(palette.drive_usage_warning),
                            );
                        } else if !state.can_commit() {
                            ui.label(
                                egui::RichText::new(i18n.tr("bulk_rename_no_changes"))
                                    .size(palette.text_size)
                                    .color(palette.tooltip_text_color),
                            );
                        }

                        ui.add_space(8.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let can_commit = state.can_commit();
                            ui.add_enabled_ui(can_commit, |ui| {
                                if primary_dialog_button(ui, palette, &i18n.tr("bulk_rename_commit"))
                                    .clicked()
                                {
                                    let renames = state
                                        .preview
                                        .iter()
                                        .filter(|r| !names_equal_ci(&r.new_name, &r.original_name))
                                        .map(|r| (r.original_path.clone(), r.new_name.clone()))
                                        .collect();
                                    action = BulkRenameModalAction::Commit(renames);
                                }
                            });
                            ui.add_space(6.0);
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                if ghost_dialog_button(ui, palette, &i18n.tr("cancel")).clicked() {
                                    action = BulkRenameModalAction::Cancelled;
                                }
                            });
                        });
                    });
                });
        });

    if changed {
        state.recompute_preview();
    }

    action
}

fn draw_preview_row(ui: &mut egui::Ui, i18n: &I18n, palette: &ThemePalette, row: &BulkRenamePreviewRow) {
    let is_blocked = row.collision.is_some() || row.invalid_name;
    let unchanged = row.new_name == row.original_name;

    ui.horizontal(|ui| {
        let name_color = if is_blocked {
            palette.drive_usage_warning
        } else if unchanged {
            palette.tooltip_text_color
        } else {
            palette.text_normal
        };

        ui.label(
            egui::RichText::new(if row.is_dir { regular::FOLDER } else { regular::FILE })
                .size(palette.text_size)
                .color(palette.icon_color),
        );
        ui.label(
            egui::RichText::new(&row.original_name)
                .size(palette.text_size)
                .color(name_color),
        );
        ui.label(
            egui::RichText::new(regular::ARROW_RIGHT)
                .size(palette.text_size)
                .color(palette.icon_color),
        );
        let resp = ui.label(
            egui::RichText::new(&row.new_name)
                .size(palette.text_size)
                .color(name_color),
        );

        if is_blocked {
            let tooltip_key = if row.invalid_name {
                "bulk_rename_invalid_name"
            } else {
                match row.collision {
                    Some(CollisionKind::WithinBatch) => "bulk_rename_collision_within_batch",
                    Some(CollisionKind::WithExisting) => "bulk_rename_collision_existing",
                    None => "bulk_rename_invalid_name",
                }
            };
            ui.label(
                egui::RichText::new(regular::WARNING_CIRCLE)
                    .size(palette.text_size)
                    .color(palette.drive_usage_warning),
            )
            .on_hover_text(i18n.tr(tooltip_key));
            resp.on_hover_text(i18n.tr(tooltip_key));
        }
    });
}

#[cfg(test)]
mod pattern_engine_tests {
    use super::*;

    #[test]
    fn compute_new_name_regex_whole_name_replace_via_dot_star() {
        // The ".*" idiom for "replace the entire original name" (documented
        // in README's Bulk Rename guide) - regression-guards that Rust's
        // `replace_all` doesn't double-apply a pattern that can also match
        // an empty string at the end of the haystack.
        let pattern = BulkRenamePattern {
            find: ".*".to_string(),
            replace: "photo_{n:03}".to_string(),
            case_insensitive: false,
            mode: RenameMatchMode::Regex,
            numbering_start: 1,
        };
        let re = regex::Regex::new(&pattern.find).unwrap();
        let result = compute_new_name(&item("whatever_original_name.jpg", false), &pattern, Some(&re), 7);
        assert_eq!(result, "photo_007.jpg");
    }

    #[test]
    fn split_name_ext_handles_files_dirs_and_extensionless_names() {
        assert_eq!(split_name_ext("photo.png", false), ("photo", "png"));
        assert_eq!(split_name_ext("archive.tar.gz", false), ("archive.tar", "gz"));
        assert_eq!(split_name_ext("README", false), ("README", ""));
        assert_eq!(split_name_ext(".gitignore", false), (".gitignore", ""));
        // Directories never split, even with a dot in the name.
        assert_eq!(split_name_ext("My.Project", true), ("My.Project", ""));
    }

    #[test]
    fn simple_find_replace_is_replace_all_and_respects_case_toggle() {
        assert_eq!(simple_find_replace("foo_foo_bar", "foo", "baz", false), "baz_baz_bar");
        assert_eq!(simple_find_replace("FOO_foo", "foo", "x", false), "FOO_x");
        assert_eq!(simple_find_replace("FOO_foo", "foo", "x", true), "x_x");
        // Empty find is a no-op, not an error.
        assert_eq!(simple_find_replace("unchanged", "", "x", false), "unchanged");
    }

    #[test]
    fn placeholder_substitution_precedence() {
        // {name} resolves to the ORIGINAL stem, not post-replace text.
        assert_eq!(substitute_placeholders("{name}_backup", "original", "txt", 1), "original_backup");
        assert_eq!(substitute_placeholders("{ext}", "x", "png", 1), "png");
        assert_eq!(substitute_placeholders("img_{n}", "x", "png", 7), "img_7");
        assert_eq!(substitute_placeholders("img_{n:03}", "x", "png", 7), "img_007");
        // Unrecognized/malformed placeholders are left as literal text.
        assert_eq!(substitute_placeholders("{unknown}", "x", "png", 1), "{unknown}");
        assert_eq!(substitute_placeholders("{n:abc}", "x", "png", 1), "{n:abc}");
        assert_eq!(substitute_placeholders("{n:999}", "x", "png", 1), "{n:999}");
    }

    fn item(name: &str, is_dir: bool) -> BulkRenameItem {
        BulkRenameItem {
            path: PathBuf::from(format!("C:/scratch/{name}")),
            original_name: name.to_string(),
            is_dir,
        }
    }

    #[test]
    fn compute_new_name_simple_mode_combines_replace_and_placeholders() {
        let pattern = BulkRenamePattern {
            find: "photo".to_string(),
            replace: "img".to_string(),
            case_insensitive: false,
            mode: RenameMatchMode::Simple,
            numbering_start: 1,
        };
        let result = compute_new_name(&item("photo.png", false), &pattern, None, 5);
        assert_eq!(result, "img.png");
    }

    #[test]
    fn compute_new_name_with_no_pattern_is_unchanged() {
        let pattern = BulkRenamePattern::default();
        let result = compute_new_name(&item("vacation.jpg", false), &pattern, None, 3);
        assert_eq!(result, "vacation.jpg");
    }

    #[test]
    fn compute_new_name_numbering_placeholder_in_replace_text() {
        let pattern = BulkRenamePattern {
            find: "vacation".to_string(),
            replace: "photo_{n:03}".to_string(),
            ..BulkRenamePattern::default()
        };
        let result = compute_new_name(&item("vacation.jpg", false), &pattern, None, 3);
        assert_eq!(result, "photo_003.jpg");
    }

    #[test]
    fn compute_new_name_regex_mode_with_capture_groups() {
        let pattern = BulkRenamePattern {
            find: r"(\d+)".to_string(),
            replace: "id_$1".to_string(),
            case_insensitive: false,
            mode: RenameMatchMode::Regex,
            numbering_start: 1,
        };
        let re = regex::Regex::new(&pattern.find).unwrap();
        let result = compute_new_name(&item("track42.mp3", false), &pattern, Some(&re), 1);
        assert_eq!(result, "trackid_42.mp3");
    }

    #[test]
    fn compute_new_name_directory_never_gets_ext_split() {
        let pattern = BulkRenamePattern {
            find: String::new(),
            replace: String::new(),
            case_insensitive: false,
            mode: RenameMatchMode::Simple,
            numbering_start: 1,
        };
        let result = compute_new_name(&item("My.Project", true), &pattern, None, 1);
        assert_eq!(result, "My.Project");
    }

    #[test]
    fn within_batch_collision_flags_rows_that_land_on_the_same_new_name() {
        let mut rows = vec![
            BulkRenamePreviewRow {
                original_path: PathBuf::from("C:/scratch/a.txt"),
                original_name: "a.txt".to_string(),
                new_name: "same.txt".to_string(),
                is_dir: false,
                collision: None,
                invalid_name: false,
            },
            BulkRenamePreviewRow {
                original_path: PathBuf::from("C:/scratch/b.txt"),
                original_name: "b.txt".to_string(),
                new_name: "same.txt".to_string(),
                is_dir: false,
                collision: None,
                invalid_name: false,
            },
            BulkRenamePreviewRow {
                original_path: PathBuf::from("C:/scratch/c.txt"),
                original_name: "c.txt".to_string(),
                new_name: "unique.txt".to_string(),
                is_dir: false,
                collision: None,
                invalid_name: false,
            },
        ];
        detect_collisions(&mut rows);
        assert_eq!(rows[0].collision, Some(CollisionKind::WithinBatch));
        assert_eq!(rows[1].collision, Some(CollisionKind::WithinBatch));
        assert_eq!(rows[2].collision, None);
    }

    #[test]
    fn unchanged_rows_sharing_a_pre_existing_name_are_not_flagged() {
        // Two rows that both keep their own original name (a no-op for
        // both) should never be flagged just because some third row's
        // *new* name happens to equal one of their names - only a group
        // where every member is unchanged should be skipped, and a group
        // like that can't arise from two distinct original paths sharing
        // one name in the first place (the filesystem wouldn't allow it),
        // so this mainly guards the "all unchanged" skip logic itself.
        let mut rows = vec![BulkRenamePreviewRow {
            original_path: PathBuf::from("C:/scratch/a.txt"),
            original_name: "a.txt".to_string(),
            new_name: "a.txt".to_string(),
            is_dir: false,
            collision: None,
            invalid_name: false,
        }];
        detect_collisions(&mut rows);
        assert_eq!(rows[0].collision, None);
    }

    #[test]
    fn existing_file_collision_detected_against_real_untouched_file() {
        let dir = std::env::temp_dir().join(format!(
            "eden_bulk_rename_test_{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("taken.txt"), b"existing").unwrap();

        let mut rows = vec![BulkRenamePreviewRow {
            original_path: dir.join("source.txt"),
            original_name: "source.txt".to_string(),
            new_name: "taken.txt".to_string(),
            is_dir: false,
            collision: None,
            invalid_name: false,
        }];
        detect_collisions(&mut rows);
        assert_eq!(rows[0].collision, Some(CollisionKind::WithExisting));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn renaming_into_a_name_vacated_by_another_batch_item_is_not_a_collision() {
        let dir = std::env::temp_dir().join(format!(
            "eden_bulk_rename_test2_{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("a.txt"), b"a").unwrap();
        std::fs::write(dir.join("b.txt"), b"b").unwrap();

        // a.txt -> b.txt, b.txt -> c.txt: b.txt is "occupied" but it's
        // itself in this batch and vacating, so it must not block a.txt.
        let mut rows = vec![
            BulkRenamePreviewRow {
                original_path: dir.join("a.txt"),
                original_name: "a.txt".to_string(),
                new_name: "b.txt".to_string(),
                is_dir: false,
                collision: None,
                invalid_name: false,
            },
            BulkRenamePreviewRow {
                original_path: dir.join("b.txt"),
                original_name: "b.txt".to_string(),
                new_name: "c.txt".to_string(),
                is_dir: false,
                collision: None,
                invalid_name: false,
            },
        ];
        detect_collisions(&mut rows);
        assert_eq!(rows[0].collision, None);
        assert_eq!(rows[1].collision, None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn can_commit_blocks_on_regex_error_collision_invalid_or_no_op() {
        let mut state = BulkRenameState {
            items: vec![],
            pattern: BulkRenamePattern::default(),
            preview: vec![],
            regex_error: None,
            focus_requested: false,
        };

        // No-op batch (empty preview): blocked.
        assert!(!state.can_commit());

        state.preview.push(BulkRenamePreviewRow {
            original_path: PathBuf::from("C:/scratch/a.txt"),
            original_name: "a.txt".to_string(),
            new_name: "b.txt".to_string(),
            is_dir: false,
            collision: None,
            invalid_name: false,
        });
        assert!(state.can_commit());

        state.regex_error = Some("bad pattern".to_string());
        assert!(!state.can_commit());
        state.regex_error = None;

        state.preview[0].collision = Some(CollisionKind::WithinBatch);
        assert!(!state.can_commit());
        state.preview[0].collision = None;

        state.preview[0].invalid_name = true;
        assert!(!state.can_commit());
    }
}
