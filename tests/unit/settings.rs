use super::truncate_path_tail;

#[test]
fn truncate_path_tail_keeps_short_strings() {
    assert_eq!(truncate_path_tail("C:/data", 40), "C:/data");
    let exact = "a".repeat(40);
    assert_eq!(truncate_path_tail(&exact, 40), exact);
}

#[test]
fn truncate_path_tail_keeps_last_chars_of_long_ascii() {
    let s = format!("C:/{}/end", "x".repeat(60));
    let out = truncate_path_tail(&s, 40);
    assert_eq!(out, format!("...{}", &s[s.len() - 40..]));
    assert!(out.ends_with("/end"));
}

#[test]
fn truncate_path_tail_does_not_split_multibyte_chars() {
    let s = format!("C:/{}", "日本語のフォルダ".repeat(6));
    let chars: Vec<char> = s.chars().collect();
    let tail: String = chars[chars.len() - 40..].iter().collect();
    assert_eq!(truncate_path_tail(&s, 40), format!("...{tail}"));
}
