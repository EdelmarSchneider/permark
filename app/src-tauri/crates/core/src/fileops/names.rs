//! Names of files and folders the Explorer creates or renames to: the same rules on every system (a project is
//! synced between them).

use crate::paths::windows_name_ok;
use crate::tree::extension_of;

/// Longest name (bytes) accepted: what the common file systems allow.
const MAX_NAME: usize = 255;

/// Why a name cannot be used. `as_str` is the `detail` of the `invalidName` error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameProblem {
    Empty,
    /// `\ / : * ? " < > |` or a control character.
    Chars,
    /// Starts or ends with a space, or ends with a dot.
    Edge,
    /// `.` | `..`
    Dot,
    /// A name Windows keeps for devices (`CON`, `NUL`, `COM1`, ...).
    Reserved,
    TooLong,
    /// New files are Markdown (`.md`) or plain text (`.txt`).
    NotMarkdown,
}

impl NameProblem {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Chars => "chars",
            Self::Edge => "edge",
            Self::Dot => "dot",
            Self::Reserved => "reserved",
            Self::TooLong => "tooLong",
            Self::NotMarkdown => "notMarkdown",
        }
    }
}

/// Checks a name for a file or folder.
///
/// # Errors
/// The first problem found.
pub fn check_name(name: &str) -> Result<(), NameProblem> {
    if name.trim().is_empty() {
        return Err(NameProblem::Empty);
    }
    if name == "." || name == ".." {
        return Err(NameProblem::Dot);
    }
    if name.chars().any(|c| {
        matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
    }) {
        return Err(NameProblem::Chars);
    }
    if name.starts_with(' ') || name.ends_with(' ') || name.ends_with('.') {
        return Err(NameProblem::Edge);
    }
    if name.len() > MAX_NAME {
        return Err(NameProblem::TooLong);
    }
    if !windows_name_ok(name) {
        return Err(NameProblem::Reserved);
    }
    Ok(())
}

/// The name of a new file: no extension gets `.md`; only Markdown and plain text are made here.
///
/// # Errors
/// A problem of the name (`NotMarkdown` for another extension).
pub fn new_file_name(name: &str) -> Result<String, NameProblem> {
    check_name(name)?;
    let full = match extension_of(name) {
        None if !name.starts_with('.') => format!("{name}.md"),
        Some(ext) if matches!(ext.as_str(), "md" | "markdown" | "txt") => name.to_owned(),
        _ => return Err(NameProblem::NotMarkdown),
    };
    check_name(&full)?;
    Ok(full)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_names_pass_including_unicode_and_spaces() {
        for name in [
            "a.md",
            "My notes.md",
            "ação 日本語.md",
            ".hidden",
            "a b c",
            "x.tar.gz",
            "ü",
        ] {
            assert_eq!(check_name(name), Ok(()), "{name}");
        }
    }

    #[test]
    fn each_problem_is_named() {
        let cases = [
            ("", NameProblem::Empty),
            ("   ", NameProblem::Empty),
            (".", NameProblem::Dot),
            ("..", NameProblem::Dot),
            ("a/b", NameProblem::Chars),
            ("a\\b", NameProblem::Chars),
            ("a:b", NameProblem::Chars),
            ("a*b", NameProblem::Chars),
            ("a?b", NameProblem::Chars),
            ("a\"b", NameProblem::Chars),
            ("a<b", NameProblem::Chars),
            ("a|b", NameProblem::Chars),
            ("a\u{7}b", NameProblem::Chars),
            ("a\0b", NameProblem::Chars),
            (" a", NameProblem::Edge),
            ("a ", NameProblem::Edge),
            ("a.", NameProblem::Edge),
            ("CON", NameProblem::Reserved),
            ("nul.txt", NameProblem::Reserved),
            ("com1.md", NameProblem::Reserved),
        ];
        for (name, problem) in cases {
            assert_eq!(check_name(name), Err(problem), "{name:?}");
        }
        assert_eq!(check_name(&"a".repeat(256)), Err(NameProblem::TooLong));
        assert_eq!(check_name(&"a".repeat(255)), Ok(()));
        // multi-byte names are counted in bytes
        assert_eq!(check_name(&"日".repeat(86)), Err(NameProblem::TooLong));
    }

    #[test]
    fn new_files_are_markdown_or_text() {
        assert_eq!(new_file_name("notes").unwrap(), "notes.md");
        assert_eq!(new_file_name("notes.md").unwrap(), "notes.md");
        assert_eq!(new_file_name("Notes.MD").unwrap(), "Notes.MD");
        assert_eq!(new_file_name("todo.txt").unwrap(), "todo.txt");
        assert_eq!(new_file_name("a.markdown").unwrap(), "a.markdown");
        assert_eq!(new_file_name("run.sh"), Err(NameProblem::NotMarkdown));
        assert_eq!(new_file_name("a.json"), Err(NameProblem::NotMarkdown));
        assert_eq!(new_file_name(".env"), Err(NameProblem::NotMarkdown));
        assert_eq!(new_file_name("a/b"), Err(NameProblem::Chars));
        assert_eq!(new_file_name("con"), Err(NameProblem::Reserved));
        assert_eq!(
            new_file_name(&"a".repeat(253)),
            Err(NameProblem::TooLong),
            "the added .md counts"
        );
    }
}
