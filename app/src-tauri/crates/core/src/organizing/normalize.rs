//! How tags, descriptions and tag looks are cleaned before they are stored or compared.

/// A tag is at most this many characters.
pub const MAX_TAG_CHARS: usize = 40;
/// A description is at most this many characters.
pub const MAX_DESCRIPTION_CHARS: usize = 160;
/// A file has at most this many tags (a hostile front matter cannot grow the catalog without bound).
pub const MAX_TAGS_PER_FILE: usize = 100;

/// The palette, in the order the UI offers it.
pub const COLORS: [&str; 8] = [
    "gray", "red", "orange", "yellow", "green", "teal", "blue", "purple",
];

/// The icons a tag can wear (lucide names).
pub const ICONS: [&str; 12] = [
    "star",
    "bookmark",
    "flag",
    "lightbulb",
    "check",
    "alert",
    "book-open",
    "code",
    "folder",
    "user",
    "calendar",
    "link",
];

/// The canonical form of a tag: trimmed, lowercased, whitespace runs turned into one `-`, cut to [`MAX_TAG_CHARS`]
/// characters. `None` when nothing is left.
#[must_use]
pub fn normalize_tag(raw: &str) -> Option<String> {
    let joined = raw.split_whitespace().collect::<Vec<_>>().join("-");
    let tag: String = joined.to_lowercase().chars().take(MAX_TAG_CHARS).collect();
    (!tag.is_empty()).then_some(tag)
}

/// Every tag normalized, empty ones and repeats dropped (the first spelling's position wins), at most
/// [`MAX_TAGS_PER_FILE`].
#[must_use]
pub fn normalize_tags<S: AsRef<str>>(raw: &[S]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in raw.iter().filter_map(|t| normalize_tag(t.as_ref())) {
        if out.len() >= MAX_TAGS_PER_FILE {
            break;
        }
        if !out.contains(&tag) {
            out.push(tag);
        }
    }
    out
}

/// A description on one line: every run of whitespace (newlines included) becomes one space, the ends are trimmed,
/// and the text is cut to [`MAX_DESCRIPTION_CHARS`] characters.
#[must_use]
pub fn normalize_description(raw: &str) -> String {
    let line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    line.chars().take(MAX_DESCRIPTION_CHARS).collect()
}

fn known(list: &'static [&'static str], raw: &str) -> Option<&'static str> {
    let raw = raw.trim();
    list.iter().copied().find(|s| s.eq_ignore_ascii_case(raw))
}

/// The palette color a name stands for (any case), `None` when it is not in the palette.
#[must_use]
pub fn known_color(raw: &str) -> Option<&'static str> {
    known(&COLORS, raw)
}

/// The icon a name stands for (any case), `None` when it is not in the set.
#[must_use]
pub fn known_icon(raw: &str) -> Option<&'static str> {
    known(&ICONS, raw)
}
