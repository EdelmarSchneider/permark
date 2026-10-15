//! The names of note files. A new note is named after the moment it was made (`2026-10-15 1432.md`), with `-2`, `-3` when
//! that name is taken; the copies TrailMark makes to keep text safe are named after the note they come from. No name
//! depends on the language of the interface, and none holds a character a file system refuses.

use std::path::Path;

use chrono::{Datelike, Local, Timelike};

use super::text::stem_from_title;

/// The extension of a note.
pub(crate) const EXTENSION: &str = "md";

/// A moment on the person's clock, to the minute: what a file name says about when it was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Moment {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

impl Moment {
    /// The time on the person's clock now.
    pub(crate) fn now() -> Self {
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
pub(crate) fn note_stem(moment: Moment) -> String {
    moment.label()
}

/// The base of the name of the copy that keeps the text of a note another program changed meanwhile.
pub(crate) fn conflict_stem(base: &str, moment: Moment) -> String {
    format!("{base} (conflict {})", moment.label())
}

/// The base of the name of the copy made from a leftover temporary file.
pub(crate) fn recovered_stem(base: &str) -> String {
    format!("{base} (recovered)")
}

/// The base of the name of the note that keeps the text typed into a loose file when the person takes the file's own version.
pub(crate) fn edits_stem(base: &str) -> String {
    format!("{base} (my edits)")
}

/// The base a copy of a note is named after: its title, else the base of its own file name.
pub(crate) fn copy_base(title: &str, file_name: &str) -> String {
    stem_from_title(title).unwrap_or_else(|| {
        Path::new(file_name).file_stem().map_or_else(
            || file_name.to_owned(),
            |s| s.to_string_lossy().into_owned(),
        )
    })
}

/// The file name for `stem` when it is the `n`th try (the first try is the name itself). `separator` goes between the
/// stem and the number: `-` for notes (`2026-10-15 1432-2.md`), a space for copies (`Title (recovered) 2.md`).
pub(crate) fn numbered(stem: &str, n: u32, separator: &str) -> String {
    if n <= 1 {
        format!("{stem}.{EXTENSION}")
    } else {
        format!("{stem}{separator}{n}.{EXTENSION}")
    }
}

/// How many names are tried before giving up: far more notes than anyone makes in one minute.
pub(crate) const MAX_TRIES: u32 = 1000;

/// The first of `stem`, `stem-2`, `stem-3`... for which `taken` says no.
pub(crate) fn first_free(stem: &str, separator: &str, taken: impl Fn(&str) -> bool) -> String {
    (1..=MAX_TRIES)
        .map(|n| numbered(stem, n, separator))
        .find(|name| !taken(name))
        .unwrap_or_else(|| numbered(stem, MAX_TRIES + 1, separator))
}

/// The number of a name made by [`numbered`], from where a search for the next free name can resume.
pub(crate) fn is_note_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION) || e.eq_ignore_ascii_case("markdown"))
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
    }

    #[test]
    fn copies_are_named_after_their_note_and_say_why() {
        assert_eq!(
            conflict_stem("Server IPs", MOMENT),
            "Server IPs (conflict 2026-10-05 0907)"
        );
        assert_eq!(recovered_stem("Server IPs"), "Server IPs (recovered)");
        assert_eq!(
            numbered("Server IPs (recovered)", 2, " "),
            "Server IPs (recovered) 2.md"
        );
        // a note with no title is named after its file
        assert_eq!(copy_base("", "2026-10-05 0907.md"), "2026-10-05 0907");
        assert_eq!(copy_base("Plan: v2?", "x.md"), "Plan v2");
    }

    #[test]
    fn every_generated_name_is_a_valid_file_name() {
        for name in [
            numbered(&note_stem(MOMENT), 1, "-"),
            numbered(&conflict_stem("Plan v2", MOMENT), 1, " "),
            numbered(&recovered_stem("Plan v2"), 3, " "),
        ] {
            assert_eq!(
                crate::edit::validate_file_name(&name).map_err(crate::edit::NameProblem::code),
                Ok(())
            );
        }
    }

    #[test]
    fn notes_are_markdown_files() {
        assert!(is_note_name("a.md"));
        assert!(is_note_name("a.MD"));
        assert!(is_note_name("a.markdown"));
        assert!(!is_note_name("a.txt"));
        assert!(!is_note_name("md"));
        assert!(!is_note_name(".a.md.1.2.part"));
    }
}
