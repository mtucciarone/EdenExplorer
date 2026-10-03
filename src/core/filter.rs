//! The file view's type-to-filter: plain text (contains), wildcards
//! (`*.jpg; *.png`, used automatically when the query has `*` or `?`), or
//! a regular expression, optionally inverted ("hide matches"), plus a file
//! kind chip (folders, images, documents, ...).

use crate::core::disk_usage_stats::{Category, category_of, extension_of};
use crate::core::pattern::{split_patterns, wildcard_match};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KindChip {
    Folders,
    Images,
    Documents,
    Video,
    Audio,
    Archives,
    Code,
}

impl KindChip {
    pub const ALL: [KindChip; 7] = [
        KindChip::Folders,
        KindChip::Images,
        KindChip::Documents,
        KindChip::Video,
        KindChip::Audio,
        KindChip::Archives,
        KindChip::Code,
    ];

    pub fn matches(self, name: &str, is_dir: bool) -> bool {
        if self == KindChip::Folders {
            return is_dir;
        }
        if is_dir {
            return false;
        }
        let category = category_of(&extension_of(name));
        matches!(
            (self, category),
            (KindChip::Images, Category::Images)
                | (KindChip::Documents, Category::Documents)
                | (KindChip::Video, Category::Video)
                | (KindChip::Audio, Category::Audio)
                | (KindChip::Archives, Category::Archives)
                | (KindChip::Code, Category::Code)
        )
    }
}

#[derive(Clone, Debug)]
enum Matcher {
    All,
    Contains(String),
    Wildcards(Vec<String>),
    Regex(regex::Regex),
    /// An invalid regular expression: nothing matches until it's fixed.
    Invalid,
}

/// A filter ready to test names against (built once per query change).
#[derive(Clone, Debug)]
pub struct CompiledFilter {
    matcher: Matcher,
    invert: bool,
    kind: Option<KindChip>,
}

impl CompiledFilter {
    /// Builds the filter; the error is a regex's syntax error, if any.
    pub fn new(
        query: &str,
        regex: bool,
        invert: bool,
        kind: Option<KindChip>,
    ) -> (CompiledFilter, Option<String>) {
        let mut error = None;
        let matcher = if query.is_empty() {
            Matcher::All
        } else if regex {
            match regex::RegexBuilder::new(query)
                .case_insensitive(true)
                .size_limit(1 << 20)
                .build()
            {
                Ok(re) => Matcher::Regex(re),
                Err(e) => {
                    // The last line says what's wrong ("error: unclosed group").
                    let message = e.to_string();
                    let last = message.lines().last().unwrap_or("invalid").trim();
                    error = Some(last.trim_start_matches("error:").trim().to_string());
                    Matcher::Invalid
                }
            }
        } else if query.contains(['*', '?']) {
            Matcher::Wildcards(
                split_patterns(query)
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            )
        } else {
            Matcher::Contains(query.to_lowercase())
        };
        // "Hide matches" means nothing without a query.
        let invert = invert && !query.is_empty();
        (
            CompiledFilter {
                matcher,
                invert,
                kind,
            },
            error,
        )
    }

    pub fn matches(&self, name: &str, is_dir: bool) -> bool {
        if self.kind.is_some_and(|kind| !kind.matches(name, is_dir)) {
            return false;
        }
        let hit = match &self.matcher {
            Matcher::All => true,
            Matcher::Contains(needle) => name.to_lowercase().contains(needle.as_str()),
            Matcher::Wildcards(patterns) => patterns.iter().any(|p| wildcard_match(p, name)),
            Matcher::Regex(re) => re.is_match(name),
            Matcher::Invalid => return false,
        };
        hit != self.invert
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(filter: &CompiledFilter) -> Vec<&'static str> {
        let files = [
            ("Photos", true),
            ("beach.JPG", false),
            ("sunset.png", false),
            ("notes.txt", false),
            ("report.pdf", false),
            ("main.rs", false),
            ("song.mp3", false),
        ];
        files
            .iter()
            .filter(|(n, d)| filter.matches(n, *d))
            .map(|(n, _)| *n)
            .collect()
    }

    #[test]
    fn text_wildcards_regex_invert_and_kinds() {
        let (f, _) = CompiledFilter::new("", false, false, None);
        assert_eq!(names(&f).len(), 7);
        let (f, _) = CompiledFilter::new("o", false, false, None);
        assert_eq!(names(&f), ["Photos", "notes.txt", "report.pdf", "song.mp3"]);
        let (f, _) = CompiledFilter::new("*.jpg; *.png", false, false, None);
        assert_eq!(names(&f), ["beach.JPG", "sunset.png"]);
        let (f, _) = CompiledFilter::new(r"^(main|song)\.", true, false, None);
        assert_eq!(names(&f), ["main.rs", "song.mp3"]);
        let (f, _) = CompiledFilter::new("*.jpg; *.png", false, true, None);
        assert_eq!(
            names(&f),
            ["Photos", "notes.txt", "report.pdf", "main.rs", "song.mp3"]
        );
        let (f, _) = CompiledFilter::new("", false, false, Some(KindChip::Images));
        assert_eq!(names(&f), ["beach.JPG", "sunset.png"]);
        let (f, _) = CompiledFilter::new("", false, false, Some(KindChip::Folders));
        assert_eq!(names(&f), ["Photos"]);
        let (f, _) = CompiledFilter::new("s", false, false, Some(KindChip::Documents));
        assert_eq!(names(&f), ["notes.txt"]);
        // Invert with no query hides nothing.
        let (f, _) = CompiledFilter::new("", false, true, Some(KindChip::Code));
        assert_eq!(names(&f), ["main.rs"]);
        let (f, err) = CompiledFilter::new("(unclosed", true, false, None);
        assert!(err.is_some());
        assert!(names(&f).is_empty());
    }

    #[test]
    fn plain_text_is_a_case_insensitive_contiguous_substring() {
        let (f, _) = CompiledFilter::new("te", false, false, None);
        assert!(f.matches("Eclipse Temurin", false));
        assert!(f.matches("Paste Folder", true));
        assert!(f.matches("TEST FOLDER", true));
        // Not a subsequence match (an old bug: 't' then later 'e').
        assert!(!f.matches("Make The Doc", false));
        assert!(!f.matches("Fast Meet", false));
    }
}
