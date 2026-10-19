//! The names of note files. A new note is named after the moment it was made (`2026-10-15 1432.md`), with `-2`, `-3`
//! when that name is taken, as 0.5 named them; the copies the app makes to keep text safe are named after the note they
//! come from. No name depends on the language of the interface, and none holds a character a file system refuses.

use std::path::Path;

use chrono::{Datelike, Local, Timelike};

use crate::text::stem_from_title;

/// The extension of a note.
pub const EXTENSION: &str = "md";

/// A moment on the person's clock, to the minute: what a file name says about when it was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moment {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

impl Moment {
    /// The time on the person's clock now.
    #[must_use]
    pub fn now() -> Self {
        let now = Local::now();
        Self {
            year: now.year(),
            month: now.month(),
            day: now.day(),
            hour: now.hour(),
            minute: now.minute(),
        }
    }

    /// `2026-10-15 1432`.
    fn label(self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}{:02}",
            self.year, self.month, self.day, self.hour, self.minute
        )
    }
}

/// The base of the name of a note made at `moment`.
#[must_use]
pub fn note_stem(moment: Moment) -> String {
    moment.label()
}

/// The base a copy of a note is named after: its title, else the base of its own file name.
#[must_use]
pub fn copy_base(title: &str, file_name: &str) -> String {
    stem_from_title(title).unwrap_or_else(|| {
        Path::new(file_name).file_stem().map_or_else(
            || file_name.to_owned(),
            |s| s.to_string_lossy().into_owned(),
        )
    })
}

/// The base of the name of the note a deleted note comes back as when its own name was taken meanwhile.
#[must_use]
pub fn restored_stem(base: &str) -> String {
    format!("{base} (restored)")
}

/// The base of the name of the note made from text a crash left (a loose file's pending text).
#[must_use]
pub fn edits_stem(base: &str) -> String {
    format!("{base} (my edits)")
}

/// The file name for `stem` when it is the `n`th try (the first try is the name itself). `separator` goes between the
/// stem and the number: `-` for notes (`2026-10-15 1432-2.md`), a space for copies (`Title (restored) 2.md`).
#[must_use]
pub fn numbered(stem: &str, n: u32, separator: &str) -> String {
    if n <= 1 {
        format!("{stem}.{EXTENSION}")
    } else {
        format!("{stem}{separator}{n}.{EXTENSION}")
    }
}

/// How many names are tried before giving up: far more notes than anyone makes in one minute.
pub const MAX_TRIES: u32 = 1000;

/// The first of `stem`, `stem-2`, `stem-3`... for which `taken` says no.
#[must_use]
pub fn first_free(stem: &str, separator: &str, taken: impl Fn(&str) -> bool) -> String {
    (1..=MAX_TRIES)
        .map(|n| numbered(stem, n, separator))
        .find(|name| !taken(name))
        .unwrap_or_else(|| numbered(stem, MAX_TRIES + 1, separator))
}

/// Whether a file name is a note's: Markdown, by its extension; not hidden.
#[must_use]
pub fn is_note_name(name: &str) -> bool {
    !name.starts_with('.')
        && Path::new(name).extension().is_some_and(|e| {
            e.eq_ignore_ascii_case(EXTENSION) || e.eq_ignore_ascii_case("markdown")
        })
}

/// Why a file name the person typed is refused (`None`: fine). `.md` is added by the caller when there is no extension.
#[must_use]
pub fn name_problem(name: &str) -> Option<&'static str> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return Some("empty");
    }
    if trimmed.chars().any(|c| {
        matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
    }) {
        return Some("chars");
    }
    if trimmed.len() > 200 {
        return Some("tooLong");
    }
    if trimmed.ends_with('.') || trimmed.starts_with('.') {
        return Some("dot");
    }
    let stem = trimmed
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    reserved.then_some("reserved")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOMENT: Moment = Moment {
        year: 2026,
        month: 10,
        day: 5,
        hour: 9,
        minute: 7,
    };

    #[test]
    fn a_new_note_is_named_after_its_moment_in_a_form_no_language_changes() {
        assert_eq!(note_stem(MOMENT), "2026-10-05 0907");
        assert_eq!(numbered(&note_stem(MOMENT), 1, "-"), "2026-10-05 0907.md");
        assert_eq!(numbered(&note_stem(MOMENT), 2, "-"), "2026-10-05 0907-2.md");
        assert_eq!(
            numbered(&note_stem(MOMENT), 12, "-"),
            "2026-10-05 0907-12.md"
        );
    }

    #[test]
    fn a_taken_name_gives_the_next_number() {
        let taken = ["2026-10-05 0907.md", "2026-10-05 0907-2.md"];
        let name = first_free("2026-10-05 0907", "-", |n| taken.contains(&n));
        assert_eq!(name, "2026-10-05 0907-3.md");
        assert_eq!(first_free("fresh", "-", |_| false), "fresh.md");
        // never an endless loop
        assert_eq!(first_free("x", "-", |_| true), "x-1001.md");
    }

    #[test]
    fn copies_are_named_after_their_note() {
        assert_eq!(restored_stem("Server IPs"), "Server IPs (restored)");
        assert_eq!(
            numbered("Server IPs (restored)", 2, " "),
            "Server IPs (restored) 2.md"
        );
        assert_eq!(edits_stem("plan"), "plan (my edits)");
        // a note with no title is named after its file
        assert_eq!(copy_base("", "2026-10-05 0907.md"), "2026-10-05 0907");
        assert_eq!(copy_base("Plan: v2?", "x.md"), "Plan v2");
    }

    #[test]
    fn note_names_are_markdown_and_not_hidden() {
        for yes in ["a.md", "A.MD", "x y.markdown", "2026-10-05 0907.md"] {
            assert!(is_note_name(yes), "{yes}");
        }
        for no in [".hidden.md", "a.txt", "a", "a.md.bak", "folder", ""] {
            assert!(!is_note_name(no), "{no}");
        }
    }

    #[test]
    fn typed_names_are_checked() {
        assert_eq!(name_problem("plan"), None);
        assert_eq!(name_problem("plan v2.md"), None);
        assert_eq!(name_problem("日本語.md"), None);
        assert_eq!(name_problem("  "), Some("empty"));
        assert_eq!(name_problem(".."), Some("empty"));
        assert_eq!(name_problem("a/b"), Some("chars"));
        assert_eq!(name_problem("a:b"), Some("chars"));
        assert_eq!(name_problem("a\u{7}b"), Some("chars"));
        assert_eq!(name_problem(".hidden"), Some("dot"));
        assert_eq!(name_problem("end."), Some("dot"));
        assert_eq!(name_problem("con.md"), Some("reserved"));
        assert_eq!(name_problem("LPT1"), Some("reserved"));
        assert_eq!(name_problem(&"a".repeat(201)), Some("tooLong"));
    }

    #[test]
    fn the_clock_gives_a_valid_moment() {
        let now = Moment::now();
        assert!((1..=12).contains(&now.month));
        assert!((1..=31).contains(&now.day));
        assert!(now.hour < 24 && now.minute < 60);
        assert_eq!(note_stem(now).len(), "2026-10-05 0907".len());
    }
}
