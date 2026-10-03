use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Graphics::DirectWrite::*;
use windows::core::*;

static SYSTEM_FONTS: LazyLock<Vec<String>> =
    LazyLock::new(|| get_system_fonts().unwrap_or_default());

static FONT_PATHS: LazyLock<HashMap<String, PathBuf>> = LazyLock::new(|| get_font_file_paths());

unsafe fn extract_localized_string(localized: &IDWriteLocalizedStrings) -> Result<String> {
    let length = unsafe { localized.GetStringLength(0)? };

    let mut buffer = vec![0u16; (length + 1) as usize];

    unsafe {
        localized.GetString(0, &mut buffer)?;
    }

    Ok(String::from_utf16_lossy(&buffer[..length as usize]))
}

unsafe fn get_file_path_from_font_file(file: &IDWriteFontFile) -> Option<String> {
    let mut key_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
    let mut key_size: u32 = 0;

    let loader = unsafe {
        file.GetReferenceKey(&mut key_ptr, &mut key_size).ok()?;
        file.GetLoader().ok()?
    };

    let local_loader = loader.cast::<IDWriteLocalFontFileLoader>().ok()?;

    let mut path_buffer = vec![0u16; 512];

    unsafe {
        local_loader
            .GetFilePathFromKey(key_ptr, key_size, &mut path_buffer)
            .ok()?;
    }

    let end = path_buffer
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(path_buffer.len());

    Some(String::from_utf16_lossy(&path_buffer[..end]))
}

fn get_system_fonts() -> Result<Vec<String>> {
    unsafe {
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

        let mut collection: Option<IDWriteFontCollection> = None;
        factory.GetSystemFontCollection(&mut collection, true)?;

        let collection = collection.ok_or(Error::from(E_FAIL))?;
        let count = collection.GetFontFamilyCount();
        let mut fonts = Vec::with_capacity(count as usize);

        for i in 0..count {
            if let Ok(family) = collection.GetFontFamily(i) {
                if let Ok(localized_names) = family.GetFamilyNames() {
                    if let Ok(name) = extract_localized_string(&localized_names) {
                        if !name.is_empty() {
                            fonts.push(name);
                        }
                    }
                }
            }
        }

        fonts.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
        Ok(fonts)
    }
}

fn get_font_file_paths() -> HashMap<String, PathBuf> {
    let mut paths = HashMap::new();

    let user_font_dir = dirs::font_dir().map(|p| p.to_string_lossy().to_string());

    let font_dirs = [r"C:\Windows\Fonts"];

    unsafe {
        if let Ok(factory) = DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED) {
            let mut collection: Option<IDWriteFontCollection> = None;
            if factory
                .GetSystemFontCollection(&mut collection, true)
                .is_ok()
            {
                if let Some(collection) = collection {
                    let count = collection.GetFontFamilyCount();

                    for i in 0..count {
                        if let Ok(family) = collection.GetFontFamily(i) {
                            if let Ok(font) = family.GetFirstMatchingFont(
                                DWRITE_FONT_WEIGHT_NORMAL,
                                DWRITE_FONT_STRETCH_NORMAL,
                                DWRITE_FONT_STYLE_NORMAL,
                            ) {
                                if let Ok(face) = font.CreateFontFace() {
                                    let mut num_files: u32 = 0;
                                    if face.GetFiles(&mut num_files, None).is_ok() && num_files > 0
                                    {
                                        let mut files: Vec<Option<IDWriteFontFile>> =
                                            vec![None; num_files as usize];
                                        if face
                                            .GetFiles(
                                                &mut num_files,
                                                Some(files.as_mut_ptr()
                                                    as *mut Option<IDWriteFontFile>),
                                            )
                                            .is_ok()
                                        {
                                            if let Some(Some(file)) = files.into_iter().next() {
                                                if let Some(path) =
                                                    get_file_path_from_font_file(&file)
                                                {
                                                    if let Ok(localized_names) =
                                                        family.GetFamilyNames()
                                                    {
                                                        if let Ok(name) = extract_localized_string(
                                                            &localized_names,
                                                        ) {
                                                            paths
                                                                .insert(name, PathBuf::from(&path));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    for dir in &font_dirs {
        let dir_path = std::path::Path::new(dir);
        if !dir_path.exists() {
            continue;
        }

        if let Ok(entries) = std::fs::read_dir(dir_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(ext) = path.extension() {
                    if ext == "ttf" || ext == "otf" || ext == "ttc" {
                        if let Some(stem) = path.file_stem() {
                            let name = stem.to_string_lossy().to_string();
                            paths.entry(name).or_insert(path);
                        }
                    }
                }
            }
        }
    }

    if let Some(user_dir) = user_font_dir {
        let dir_path = std::path::Path::new(&user_dir);
        if dir_path.exists() {
            if let Ok(entries) = std::fs::read_dir(dir_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(ext) = path.extension() {
                        if ext == "ttf" || ext == "otf" || ext == "ttc" {
                            if let Some(stem) = path.file_stem() {
                                let name = stem.to_string_lossy().to_string();
                                paths.entry(name).or_insert(path);
                            }
                        }
                    }
                }
            }
        }
    }

    paths
}

pub fn get_font_list() -> &'static [String] {
    &SYSTEM_FONTS
}

pub fn get_font_path(font_name: &str) -> Option<PathBuf> {
    if let Some(path) = FONT_PATHS.get(font_name) {
        return Some(path.clone());
    }

    for (name, path) in FONT_PATHS.iter() {
        if name.eq_ignore_ascii_case(font_name) {
            return Some(path.clone());
        }
    }

    let candidates = [
        format!(r"C:\Windows\Fonts\{}.ttf", font_name),
        format!(r"C:\Windows\Fonts\{}.otf", font_name),
        format!(r"C:\Windows\Fonts\{} Regular.ttf", font_name),
        format!(r"C:\Windows\Fonts\{}-Regular.ttf", font_name),
        format!(r"C:\Windows\Fonts\{}_Regular.ttf", font_name),
    ];

    for candidate in candidates {
        let path = PathBuf::from(&candidate);
        if path.exists() {
            return Some(path);
        }
    }

    None
}

/// The egui font family the terminal pane draws with.
pub const TERMINAL_FAMILY: &str = "terminal";

/// The terminal font chosen in Settings (`None` = automatic).
static TERMINAL_FONT: std::sync::RwLock<Option<Option<String>>> = std::sync::RwLock::new(None);

/// Records the terminal font setting; true when it changed (the fonts
/// then need rebuilding with `apply_font_to_context`).
pub fn set_terminal_font(choice: Option<&str>) -> bool {
    let choice = choice.map(str::to_owned);
    let Ok(mut current) = TERMINAL_FONT.write() else { return false };
    if current.as_ref() == Some(&choice) {
        return false;
    }
    *current = Some(choice);
    true
}

/// The font the terminal uses: the chosen one, or automatically an
/// installed Nerd Font (preferring the "Mono" variants, whose icons fit
/// one cell), then Cascadia Mono, Cascadia Code, or Consolas.
pub fn terminal_font_name() -> Option<String> {
    let chosen = TERMINAL_FONT.read().ok().and_then(|c| c.clone()).flatten();
    if chosen.is_some() {
        return chosen;
    }
    pick_terminal_font(get_font_list())
}

/// What "Automatic" resolves to on this PC.
pub fn pick_terminal_font_public() -> Option<String> {
    pick_terminal_font(get_font_list())
}

fn pick_terminal_font(installed: &[String]) -> Option<String> {
    let is_nerd = |name: &str| {
        let lower = name.to_lowercase();
        lower.contains("nerd font") || lower.ends_with(" nf") || lower.ends_with(" nfm") || lower.contains(" nf ")
    };
    let is_mono = |name: &str| {
        let lower = name.to_lowercase();
        lower.ends_with("mono") || lower.ends_with(" nfm")
    };
    // Oh My Posh recommends Meslo; the others are popular coding fonts.
    let preferred = ["meslo", "caskaydia", "jetbrains", "fira", "hack"];
    let rank = |name: &str| {
        let lower = name.to_lowercase();
        let family = preferred.iter().position(|p| lower.contains(p)).unwrap_or(preferred.len());
        (!is_mono(name), family, lower)
    };
    if let Some(best) = installed.iter().filter(|n| is_nerd(n)).min_by_key(|n| rank(n)) {
        return Some(best.clone());
    }
    ["Cascadia Mono", "Cascadia Code", "Consolas"]
        .iter()
        .find_map(|want| installed.iter().find(|n| n.eq_ignore_ascii_case(want)).cloned())
}

pub fn load_font_data(font_name: &str) -> Option<Vec<u8>> {
    let path = get_font_path(font_name)?;
    std::fs::read(&path).ok()
}

pub fn apply_custom_font_definitions(fonts: &mut egui::FontDefinitions) {
    // 1. Phosphor Regular (adds to Proportional/Monospace)
    egui_phosphor::add_to_fonts(fonts, egui_phosphor::Variant::Regular);

    // 2. Phosphor Fill (Custom named family)
    fonts.font_data.insert(
        "phosphor_fill".to_owned(),
        egui_phosphor::Variant::Fill.font_data().into(),
    );
    fonts.families.insert(
        egui::FontFamily::Name("phosphor_fill".into()),
        vec!["phosphor_fill".to_owned()],
    );

    // 2b. Phosphor priority layer - on a system whose "Segoe UI" (or
    // whichever font a user picked) has been replaced by a large merged/
    // localized font build (e.g. the common "SyrianSegoe"-style registry
    // redirect seen on some Arabic-locale Windows installs), that font can
    // happen to define its own glyphs at the exact private-use codepoints
    // (U+E000+) Phosphor's icon glyphs live at - and since `apply_font_to_
    // context` (theme.rs) inserts the user's chosen font at index 0 of the
    // Proportional/Monospace family *before* this function runs, egui's
    // first-font-that-has-the-glyph resolution would pick that font's
    // (wrong, unrelated CJK) glyph over Phosphor's for every codepoint the
    // replacement font happens to cover - explaining reports of some icons
    // rendering as random CJK characters while others (codepoints the
    // replacement font doesn't define) render correctly.
    //
    // Fixed by inserting a font at index 0 of both families, ahead of
    // whatever font the user's own selection put there - but this MUST be
    // a private-use-only *subset*, not the full `egui_phosphor::Variant::
    // Regular` font reused as-is: that was the first cut here, on the
    // assumption a dedicated icon font could only ever define glyphs in
    // its own reserved codepoint range. Verified false for this exact font
    // by inspecting its cmap/glyf tables directly - `Phosphor.ttf` also
    // maps real (non-empty, 1-contour) glyphs onto plain lowercase ASCII
    // (e.g. codepoints for 'h'/'i'/'s'), and giving it blanket priority
    // broke ordinary lowercase text app-wide ("This PC" rendering as
    // "T PC", etc.) the instant it shipped - confirmed by live-testing,
    // not just reasoning about it. `assets/PhosphorIconsOnly.ttf` is that
    // same font run through `fonttools`' `pyftsubset` restricted to
    // `U+E000-F8FF` (the private-use area Phosphor's ~1500 icons actually
    // live in), so it structurally cannot contain a stray Latin glyph
    // regardless of what upstream `egui_phosphor` ships. Needs manual
    // regeneration (the same `pyftsubset` invocation, or equivalent) if a
    // future `egui-phosphor` version changes Phosphor.ttf's own codepoint
    // mapping - not automatic like reusing the crate's own accessor would
    // have been, but reusing it demonstrably isn't safe for this font.
    fonts.font_data.insert(
        "phosphor_priority".to_owned(),
        egui::FontData::from_static(include_bytes!("../../assets/PhosphorIconsOnly.ttf")).into(),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "phosphor_priority".to_owned());
    }

    // 3. Japanese Font (adds to ALL families as a fallback)
    let japanese_font = "japanese_font".to_owned();
    fonts.font_data.insert(
        japanese_font.clone(),
        egui::FontData::from_static(include_bytes!("../../assets/NotoSansJP-Regular.ttf")).into(),
    );

    for family in fonts.families.values_mut() {
        if !family.contains(&japanese_font) {
            family.push(japanese_font.clone());
        }
    }

    // 4. The terminal's family: its own font first (so a Nerd Font's
    // private-use icons win), then the monospace fallbacks - without the
    // Phosphor icon layer, whose glyphs share those codepoints.
    let mut terminal = Vec::new();
    if let Some(name) = terminal_font_name()
        && let Some(data) = load_font_data(&name)
    {
        fonts.font_data.insert("terminal_font".to_owned(), egui::FontData::from_owned(data).into());
        terminal.push("terminal_font".to_owned());
    }
    if let Some(mono) = fonts.families.get(&egui::FontFamily::Monospace) {
        terminal.extend(mono.iter().filter(|key| !key.starts_with("phosphor")).cloned());
    }
    fonts.families.insert(egui::FontFamily::Name(TERMINAL_FAMILY.into()), terminal);
}

#[cfg(test)]
mod terminal_font_tests {
    use super::pick_terminal_font;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn prefers_nerd_font_mono_variants() {
        let installed = names(&[
            "Arial",
            "Consolas",
            "CaskaydiaCove Nerd Font",
            "CaskaydiaCove Nerd Font Mono",
            "MesloLGM Nerd Font",
            "MesloLGM Nerd Font Mono",
        ]);
        assert_eq!(pick_terminal_font(&installed).as_deref(), Some("MesloLGM Nerd Font Mono"));
        let installed = names(&["Consolas", "Hack NF"]);
        assert_eq!(pick_terminal_font(&installed).as_deref(), Some("Hack NF"));
        let installed = names(&["Arial", "Consolas", "Cascadia Mono"]);
        assert_eq!(pick_terminal_font(&installed).as_deref(), Some("Cascadia Mono"));
        assert_eq!(pick_terminal_font(&names(&["Arial"])), None);
    }
}
