use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;
use windows::Win32::Graphics::DirectWrite::*;
use windows::core::*;

static SYSTEM_FONTS: LazyLock<Vec<String>> =
    LazyLock::new(|| get_system_fonts().unwrap_or_default());

static FONT_PATHS: LazyLock<HashMap<String, PathBuf>> = LazyLock::new(|| get_font_file_paths());

unsafe fn extract_localized_string(localized: &IDWriteLocalizedStrings) -> Result<String> {
    let length = localized.GetStringLength(0)?;
    let mut buffer = vec![0u16; (length + 1) as usize];
    localized.GetString(0, &mut buffer)?;

    Ok(String::from_utf16_lossy(&buffer[..length as usize]))
}

unsafe fn get_file_path_from_font_file(file: &IDWriteFontFile) -> Option<String> {
    let mut key_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
    let mut key_size: u32 = 0;

    if file.GetReferenceKey(&mut key_ptr, &mut key_size).is_err() {
        return None;
    }

    let loader = file.GetLoader().ok()?;

    let local_loader = loader.cast::<IDWriteLocalFontFileLoader>().ok()?;

    let mut path_buffer = vec![0u16; 512];

    if local_loader
        .GetFilePathFromKey(key_ptr, key_size, &mut path_buffer)
        .is_ok()
    {
        let end = path_buffer
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(path_buffer.len());
        Some(String::from_utf16_lossy(&path_buffer[..end]))
    } else {
        None
    }
}

fn get_system_fonts() -> Result<Vec<String>> {
    unsafe {
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

        let mut collection: Option<IDWriteFontCollection> = None;
        factory.GetSystemFontCollection(&mut collection, true)?;

        let collection = collection.ok_or(Error::from_win32())?;
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
}
