//! New File templates: the built-in Text / Markdown / Word / Excel
//! templates plus every file in the user's templates folder (by default
//! `Templates` inside the data folder, or the folder chosen in Settings).
//! Choosing one creates a new, uniquely named file from it in the current
//! folder.

use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinTemplate {
    Text,
    Markdown,
    Word,
    Excel,
}

impl BuiltinTemplate {
    pub const ALL: [BuiltinTemplate; 4] = [
        BuiltinTemplate::Text,
        BuiltinTemplate::Markdown,
        BuiltinTemplate::Word,
        BuiltinTemplate::Excel,
    ];

    pub fn i18n_key(self) -> &'static str {
        match self {
            BuiltinTemplate::Text => "template_text",
            BuiltinTemplate::Markdown => "template_markdown",
            BuiltinTemplate::Word => "template_word",
            BuiltinTemplate::Excel => "template_excel",
        }
    }

    /// Default file name (without a uniqueness counter).
    fn file_stem(self) -> &'static str {
        match self {
            BuiltinTemplate::Text => "New Text Document",
            BuiltinTemplate::Markdown => "New Markdown File",
            BuiltinTemplate::Word => "New Word Document",
            BuiltinTemplate::Excel => "New Excel Workbook",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            BuiltinTemplate::Text => "txt",
            BuiltinTemplate::Markdown => "md",
            BuiltinTemplate::Word => "docx",
            BuiltinTemplate::Excel => "xlsx",
        }
    }

    fn contents(self) -> std::io::Result<Vec<u8>> {
        match self {
            BuiltinTemplate::Text => Ok(Vec::new()),
            BuiltinTemplate::Markdown => Ok(b"# Title\n\n".to_vec()),
            BuiltinTemplate::Word => minimal_docx(),
            BuiltinTemplate::Excel => minimal_xlsx(),
        }
    }
}

/// One entry in the New File menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Template {
    Builtin(BuiltinTemplate),
    /// A file in the user's templates folder, copied as-is.
    File(PathBuf),
}

impl Template {
    /// Menu label for a user template: its file name.
    pub fn file_label(path: &Path) -> String {
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    }
}

/// The templates folder in use: the one chosen in Settings, or `Templates`
/// in the data folder.
pub fn templates_dir(custom: Option<&Path>) -> Option<PathBuf> {
    match custom {
        Some(dir) => Some(dir.to_path_buf()),
        None => Some(crate::core::app_data::data_dir()?.join("Templates")),
    }
}

/// `user_templates`, re-read at most every two seconds per folder. The
/// New File menu is drawn every frame while it's open, and listing a
/// folder each time would make the open menu stutter.
pub fn user_templates_cached(dir: &Path) -> Vec<PathBuf> {
    use std::cell::RefCell;
    use std::time::{Duration, Instant};
    thread_local! {
        static CACHE: RefCell<Option<(PathBuf, Instant, Vec<PathBuf>)>> = const { RefCell::new(None) };
    }
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((cached_dir, at, files)) = cache.as_ref()
            && cached_dir == dir
            && at.elapsed() < Duration::from_secs(2)
        {
            return files.clone();
        }
        let files = user_templates(dir);
        *cache = Some((dir.to_path_buf(), Instant::now(), files.clone()));
        files
    })
}

/// Files in the templates folder, sorted by name (hidden files and
/// subfolders skipped). Empty if the folder doesn't exist yet.
pub fn user_templates(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|n| !n.to_string_lossy().starts_with('.'))
        })
        .collect();
    files.sort_by_key(|path| Template::file_label(path).to_lowercase());
    files
}

/// `dir\<stem>.<ext>`, or `dir\<stem> (2).<ext>`, ... - the first that
/// doesn't exist yet.
fn unique_path(dir: &Path, stem: &str, extension: &str) -> PathBuf {
    let name = |counter: usize| {
        let stem = if counter == 1 {
            stem.to_string()
        } else {
            format!("{stem} ({counter})")
        };
        if extension.is_empty() {
            stem
        } else {
            format!("{stem}.{extension}")
        }
    };
    let mut counter = 1;
    loop {
        let path = dir.join(name(counter));
        if !path.exists() {
            return path;
        }
        counter += 1;
    }
}

/// Creates a new file in `dir` from `template` and returns its path.
pub fn create_from_template(dir: &Path, template: &Template) -> std::io::Result<PathBuf> {
    match template {
        Template::Builtin(builtin) => {
            let path = unique_path(dir, builtin.file_stem(), builtin.extension());
            let contents = builtin.contents()?;
            // `create_new` so a race with another program never overwrites.
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?
                .write_all(&contents)?;
            Ok(path)
        }
        Template::File(source) => {
            let stem = source
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "New File".to_string());
            let extension = source
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            let path = unique_path(dir, &stem, &extension);
            let contents = std::fs::read(source)?;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?
                .write_all(&contents)?;
            Ok(path)
        }
    }
}

/// Zips `parts` (path, XML) into an Office Open XML package.
fn office_package(parts: &[(&str, &str)]) -> std::io::Result<Vec<u8>> {
    use zip::write::SimpleFileOptions;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, xml) in parts {
        writer.start_file(*name, options).map_err(std::io::Error::other)?;
        writer.write_all(xml.as_bytes())?;
    }
    Ok(writer.finish().map_err(std::io::Error::other)?.into_inner())
}

/// The smallest .docx Word opens without complaint: one empty paragraph.
fn minimal_docx() -> std::io::Result<Vec<u8>> {
    office_package(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p/></w:body></w:document>"#,
        ),
    ])
}

/// The smallest .xlsx Excel opens without complaint: one empty "Sheet1".
fn minimal_xlsx() -> std::io::Result<Vec<u8>> {
    office_package(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        ),
        (
            "xl/workbook.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>"#,
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "eden_templates_{name}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn builtin_templates_get_unique_names() {
        let dir = temp_dir("builtin");
        let first = create_from_template(&dir, &Template::Builtin(BuiltinTemplate::Text)).unwrap();
        let second = create_from_template(&dir, &Template::Builtin(BuiltinTemplate::Text)).unwrap();
        assert_eq!(first.file_name().unwrap(), "New Text Document.txt");
        assert_eq!(second.file_name().unwrap(), "New Text Document (2).txt");
        let md = create_from_template(&dir, &Template::Builtin(BuiltinTemplate::Markdown)).unwrap();
        assert_eq!(std::fs::read_to_string(md).unwrap(), "# Title\n\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn docx_is_a_valid_word_package() {
        let bytes = minimal_docx().unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        for name in ["[Content_Types].xml", "_rels/.rels", "word/document.xml"] {
            assert!(archive.by_name(name).is_ok(), "{name} missing");
        }
    }

    #[test]
    fn xlsx_is_a_valid_excel_package() {
        let bytes = minimal_xlsx().unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        for name in [
            "[Content_Types].xml",
            "_rels/.rels",
            "xl/workbook.xml",
            "xl/_rels/workbook.xml.rels",
            "xl/worksheets/sheet1.xml",
        ] {
            assert!(archive.by_name(name).is_ok(), "{name} missing");
        }
        let dir = temp_dir("xlsx");
        let path = create_from_template(&dir, &Template::Builtin(BuiltinTemplate::Excel)).unwrap();
        assert_eq!(path.file_name().unwrap(), "New Excel Workbook.xlsx");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn user_templates_are_listed_and_copied() {
        let templates = temp_dir("user_src");
        std::fs::write(templates.join("Invoice.xlsx"), b"xlsx-bytes").unwrap();
        std::fs::write(templates.join("notes.txt"), b"hello").unwrap();
        std::fs::write(templates.join(".hidden"), b"x").unwrap();
        std::fs::create_dir_all(templates.join("sub")).unwrap();

        let listed = user_templates(&templates);
        let names: Vec<String> = listed.iter().map(|p| Template::file_label(p)).collect();
        assert_eq!(names, vec!["Invoice.xlsx", "notes.txt"]);

        let target = temp_dir("user_dst");
        let created = create_from_template(&target, &Template::File(listed[0].clone())).unwrap();
        assert_eq!(created.file_name().unwrap(), "Invoice.xlsx");
        assert_eq!(std::fs::read(&created).unwrap(), b"xlsx-bytes");
        let again = create_from_template(&target, &Template::File(listed[0].clone())).unwrap();
        assert_eq!(again.file_name().unwrap(), "Invoice (2).xlsx");

        assert!(user_templates(&templates.join("missing")).is_empty());
        let _ = std::fs::remove_dir_all(&templates);
        let _ = std::fs::remove_dir_all(&target);
    }
}
