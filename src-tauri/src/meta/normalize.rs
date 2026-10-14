//! How tags and descriptions are cleaned up before they are stored or compared.

/// A tag is at most this many characters.
pub const MAX_TAG_CHARS: usize = 40;
/// A description is at most this many characters.
pub const MAX_DESCRIPTION_CHARS: usize = 120;

/// The canonical form of a tag: trimmed, lowercased, runs of whitespace turned into one `-`, cut to
/// [`MAX_TAG_CHARS`]. `None` when nothing is left.
pub fn normalize_tag(raw: &str) -> Option<String> {
    let words: Vec<&str> = raw.split_whitespace().collect();
    let joined = words.join("-").to_lowercase();
    let tag: String = joined.chars().take(MAX_TAG_CHARS).collect();
    (!tag.is_empty()).then_some(tag)
}

/// Normalizes every tag, dropping empty ones and repeats (the first spelling's position wins).
pub fn normalize_tags<S: AsRef<str>>(raw: &[S]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in raw.iter().filter_map(|t| normalize_tag(t.as_ref())) {
        if !out.contains(&tag) {
            out.push(tag);
        }
    }
    out
}

/// A description on one line: every run of whitespace (newlines included) becomes one space, the
/// ends are trimmed, and the text is cut to [`MAX_DESCRIPTION_CHARS`].
pub fn normalize_description(raw: &str) -> String {
    let line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    line.chars().take(MAX_DESCRIPTION_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_trimmed_lowercased_and_hyphenated() {
        assert_eq!(
            normalize_tag("  Needs Review "),
            Some("needs-review".to_owned())
        );
        assert_eq!(normalize_tag("a \t\n b"), Some("a-b".to_owned()));
        assert_eq!(normalize_tag("SPEC"), Some("spec".to_owned()));
        assert_eq!(normalize_tag("area/api"), Some("area/api".to_owned()));
        assert_eq!(normalize_tag("Ünï"), Some("ünï".to_owned()));
    }

    #[test]
    fn empty_tags_are_dropped() {
        assert_eq!(normalize_tag(""), None);
        assert_eq!(normalize_tag("   \n\t"), None);
        assert_eq!(normalize_tags(&["", " ", "x"]), ["x"]);
    }

    #[test]
    fn tags_are_cut_to_forty_characters_not_bytes() {
        let long = "é".repeat(60);
        assert_eq!(normalize_tag(&long).map(|t| t.chars().count()), Some(40));
        let exact = "a".repeat(40);
        assert_eq!(normalize_tag(&exact), Some(exact.clone()));
        assert_eq!(
            normalize_tag(&format!("{exact}b")).map(|t| t.chars().count()),
            Some(40)
        );
    }

    #[test]
    fn repeated_tags_collapse_in_order() {
        assert_eq!(
            normalize_tags(&["Spec", "feed", "spec", "FEED", "new idea"]),
            ["spec", "feed", "new-idea"]
        );
    }

    #[test]
    fn descriptions_are_one_line_and_cut_to_120_characters() {
        assert_eq!(normalize_description("  A\nB\r\n\r\nC\tD  "), "A B C D");
        assert_eq!(normalize_description(""), "");
        assert_eq!(normalize_description(" \n "), "");
        let long = "x".repeat(300);
        assert_eq!(normalize_description(&long).chars().count(), 120);
        let wide = "日".repeat(200);
        assert_eq!(normalize_description(&wide).chars().count(), 120);
    }
}
