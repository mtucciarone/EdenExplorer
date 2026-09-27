//! Wildcard matching for Select by Pattern: `*` matches any run of
//! characters, `?` exactly one, case-insensitively. Several patterns can be
//! given at once, separated by `;` or `,` (e.g. `*.jpg; *.png`).

/// Whether `name` matches the single wildcard `pattern`.
pub fn wildcard_match(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let name: Vec<char> = name.to_lowercase().chars().collect();

    // Classic greedy matcher with backtracking to the last `*`.
    let (mut p, mut n) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut star_match = 0usize;
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            star_match = n;
            p += 1;
        } else if let Some(star_pos) = star {
            p = star_pos + 1;
            star_match += 1;
            n = star_match;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

/// The individual patterns in a `;`/`,`-separated list, empty ones dropped.
pub fn split_patterns(patterns: &str) -> Vec<&str> {
    patterns
        .split([';', ','])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect()
}

/// Whether `name` matches any pattern in the `;`/`,`-separated list.
/// A pattern without `*` or `?` matches names that contain it, so typing
/// `invoice` finds `2024 Invoice.pdf`.
pub fn matches_any(patterns: &str, name: &str) -> bool {
    split_patterns(patterns).into_iter().any(|pattern| {
        if pattern.contains(['*', '?']) {
            wildcard_match(pattern, name)
        } else {
            name.to_lowercase().contains(&pattern.to_lowercase())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        assert!(wildcard_match("*.jpg", "Photo.JPG"));
        assert!(!wildcard_match("*.jpg", "photo.jpeg"));
        assert!(wildcard_match("img_????.png", "IMG_0042.png"));
        assert!(!wildcard_match("img_????.png", "IMG_42.png"));
        assert!(wildcard_match("*report*2024*", "Q1 Report - final 2024.docx"));
        assert!(wildcard_match("*", ""));
        assert!(wildcard_match("a*b*c", "aXXbYYc"));
        assert!(!wildcard_match("a*b*c", "aXXbYY"));
    }

    #[test]
    fn lists_and_plain_text() {
        assert!(matches_any("*.jpg; *.png", "x.png"));
        assert!(matches_any("*.jpg,*.png", "x.JPG"));
        assert!(!matches_any("*.jpg; *.png", "x.gif"));
        assert!(matches_any("invoice", "2024 Invoice.pdf"));
        assert!(!matches_any(" ; ", "anything"));
    }
}
