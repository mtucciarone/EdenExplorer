//! File and folder icons from the Material Icon Theme (the VS Code icon
//! theme): picks the icon for a name the same way VS Code does (exact file
//! name, then the longest matching extension; folders by name) and hands
//! out its SVG. The icons are bundled compressed and unpacked on first use.
//! Regenerate the data with `scripts/gen_material_icons.py`.

#[path = "material_icons_data.rs"]
mod data;

use std::sync::OnceLock;

/// An icon in the bundled set (an index into `data::ICONS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaterialIcon(u16);

static BLOB: &[u8] = include_bytes!("../assets/material-icons.bin");

fn find(table: &'static [(&'static str, u16)], key: &str) -> Option<MaterialIcon> {
    table.binary_search_by(|(k, _)| (*k).cmp(key)).ok().map(|i| MaterialIcon(table[i].1))
}

fn named(name: &str) -> MaterialIcon {
    let i = data::ICONS.binary_search_by(|(k, _, _)| (*k).cmp(name)).unwrap_or(0);
    MaterialIcon(i as u16)
}

/// The icon for a file called `name`.
pub fn for_file(name: &str) -> MaterialIcon {
    let lower = name.to_lowercase();
    if let Some(icon) = find(data::FILE_NAMES, &lower) {
        return icon;
    }
    // "app.spec.ts" tries "spec.ts", then "ts".
    let mut rest = lower.as_str();
    while let Some((_, ext)) = rest.split_once('.') {
        if let Some(icon) = find(data::EXTENSIONS, ext) {
            return icon;
        }
        rest = ext;
    }
    named("file")
}

/// The icon for a folder called `name`, open or closed.
pub fn for_folder(name: &str, open: bool) -> MaterialIcon {
    let lower = name.to_lowercase();
    let table = if open { data::FOLDERS_OPEN } else { data::FOLDERS };
    find(table, &lower).unwrap_or_else(|| named(if open { "folder-open" } else { "folder" }))
}

impl MaterialIcon {
    /// The variant to draw on a light background (most icons have none).
    pub fn themed(self, light: bool) -> MaterialIcon {
        if light && let Ok(i) = data::LIGHT.binary_search_by_key(&self.0, |(dark, _)| *dark) {
            return MaterialIcon(data::LIGHT[i].1);
        }
        self
    }

    pub fn name(self) -> &'static str {
        data::ICONS[self.0 as usize].0
    }

    /// The icon's SVG source.
    pub fn svg(self) -> &'static [u8] {
        static UNPACKED: OnceLock<Vec<u8>> = OnceLock::new();
        let blob = UNPACKED.get_or_init(|| {
            use std::io::Read;
            let mut out = Vec::with_capacity(data::UNCOMPRESSED_LEN);
            let _ = flate2::read::ZlibDecoder::new(BLOB).read_to_end(&mut out);
            out
        });
        let (_, offset, len) = data::ICONS[self.0 as usize];
        blob.get(offset as usize..(offset + len) as usize).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_icons_like_vs_code() {
        assert_eq!(for_file("main.rs").name(), "rust");
        assert_eq!(for_file("Cargo.toml").name(), "toml");
        assert_eq!(for_file("package.json").name(), "nodejs");
        assert_eq!(for_file("README.md").name(), "readme");
        assert_eq!(for_file("notes.md").name(), "markdown");
        assert_eq!(for_file("photo.JPG").name(), "image");
        assert_eq!(for_file("app.spec.ts").name(), "test-ts");
        assert_eq!(for_file("index.ts").name(), "typescript");
        assert_eq!(for_file("no-extension").name(), "file");
        assert_eq!(for_file("archive.tar.gz").name(), "zip");
        assert_eq!(for_folder("src", false).name(), "folder-src");
        assert_eq!(for_folder("src", true).name(), "folder-src-open");
        assert_eq!(for_folder("Holiday Photos", false).name(), "folder");
        assert_eq!(for_folder("whatever", true).name(), "folder-open");
    }

    #[test]
    fn every_icon_is_an_svg() {
        for i in 0..data::ICONS.len() {
            let svg = MaterialIcon(i as u16).svg();
            assert!(svg.starts_with(b"<svg"), "{}", MaterialIcon(i as u16).name());
        }
    }

    #[test]
    fn light_variants() {
        let jinja = for_file("page.jinja");
        assert_ne!(jinja.themed(true), jinja);
        assert_eq!(jinja.themed(false), jinja);
        assert_eq!(for_file("main.rs").themed(true).name(), "rust");
    }
}
