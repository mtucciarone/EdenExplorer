//! Fuzzy matching for the command palette: every character of the query
//! must appear in the text, in order (case-insensitive), and matches score
//! higher when they're consecutive, start a word, or start the text - so
//! "nt" ranks "New Tab" above "Invert Selection", and "setbeh" finds
//! "Settings: Behavior".

/// Score of `text` for `query` (higher is better), or `None` if it doesn't
/// match. An empty query matches everything with score 0.
pub fn score(query: &str, text: &str) -> Option<i32> {
    score_with(query, text, false)
}

/// Like `score`, but every query character must start a word or continue
/// a run of matched characters - for long texts such as paths, where a
/// loose match (`set` in `C:\users\root`) is almost always noise.
pub fn score_strict(query: &str, text: &str) -> Option<i32> {
    score_with(query, text, true)
}

fn score_with(query: &str, text: &str, strict: bool) -> Option<i32> {
    let query: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    if query.is_empty() {
        return Some(0);
    }
    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<char> = chars.iter().map(|c| c.to_lowercase().next().unwrap_or(*c)).collect();

    // Greedy from the left, but prefer a later word start over a mid-word
    // hit when one exists before the next query character is needed:
    // simple and good enough for short labels.
    let mut total = 0;
    let mut pos = 0usize;
    let mut prev: Option<usize> = None;
    for &q in &query {
        let candidates: Vec<usize> = (pos..lower.len()).filter(|&i| lower[i] == q).collect();
        let first = *candidates.first()?;
        let word_start = |i: usize| i == 0 || !chars[i - 1].is_alphanumeric() || (chars[i].is_uppercase() && chars[i - 1].is_lowercase());
        // Keep a consecutive run going; otherwise jump to a word start if
        // there is one.
        let chosen = if prev.is_some_and(|p| p + 1 == first) {
            first
        } else {
            candidates.iter().copied().find(|&i| word_start(i)).unwrap_or(first)
        };
        if strict && !prev.is_some_and(|p| p + 1 == chosen) && !word_start(chosen) {
            return None;
        }
        let mut s = 1;
        if prev.is_some_and(|p| p + 1 == chosen) {
            s += 5;
        }
        if word_start(chosen) {
            s += 8;
        }
        if chosen == 0 {
            s += 4;
        }
        if let Some(p) = prev {
            s -= ((chosen - p - 1) as i32).min(3);
        }
        total += s;
        prev = Some(chosen);
        pos = chosen + 1;
    }
    // Shorter texts win ties.
    Some(total * 4 - (chars.len() as i32).min(60) / 4)
}

#[cfg(test)]
mod tests {
    use super::score;

    #[test]
    fn needs_every_character_in_order() {
        assert!(score("nt", "New Tab").is_some());
        assert!(score("tn", "New Tab").is_none());
        assert!(score("xyz", "New Tab").is_none());
        assert_eq!(score("", "anything"), Some(0));
        assert!(score("NEW", "new tab").is_some(), "case-insensitive");
    }

    #[test]
    fn word_starts_and_runs_rank_higher() {
        let nt_new_tab = score("nt", "New Tab").unwrap();
        let nt_invert = score("nt", "Invert Selection").unwrap();
        assert!(nt_new_tab > nt_invert);
        assert!(score("setbeh", "Settings: Behavior").is_some());
        assert!(score("refresh", "Refresh").unwrap() > score("refresh", "Refresh Folder Sizes Later").unwrap());
        assert!(score("dark", "Toggle Dark Theme").unwrap() > score("dark", "Download Archives").unwrap_or(i32::MIN));
    }

    #[test]
    fn strict_matching_needs_word_starts_or_runs() {
        use super::score_strict;
        assert!(score_strict("set", r"C:\users\root").is_none());
        assert!(score("set", r"C:\users\root").is_some());
        assert!(score_strict("docpro", r"C:\Users\me\Documents\Projects").is_some());
        assert!(score_strict("eden", r"C:\Users\me\Documents\Eden Demo").is_some());
    }
}
