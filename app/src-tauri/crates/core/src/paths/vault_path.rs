//! `VaultPath`: validated project-relative path (UI | agent input parsed here, nowhere else).

use std::fmt;

use unicode_normalization::UnicodeNormalization;

use super::{fold, VaultError};

/// Longest path text accepted (bytes); anything longer is not a real path.
const MAX_LEN: usize = 32 * 1024;

/// Project-relative path: `/` separators, no `.` | `..` | empty segments, no leading `/`, no drive | UNC prefix, no
/// NUL; Windows: no `:` (alternate data streams), no reserved device names, no segment ending in `.` | space.
/// Empty = the root. Worktree paths keep their `@wt/<name>/` prefix (split by `Vault`).
/// Text kept as given (a Linux file name is bytes: an NFD name must stay reachable); identity uses [`VaultPath::key`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VaultPath(String);

impl VaultPath {
    /// The root itself.
    #[must_use]
    pub fn root() -> Self {
        Self(String::new())
    }

    /// Parses raw input. `\` = separator; `.` and empty segments dropped; `..` pops a segment.
    ///
    /// # Errors
    /// `Escapes` when `..` climbs above the root; `Invalid` for absolute | drive | UNC | NUL | reserved input.
    pub fn parse(raw: &str) -> Result<Self, VaultError> {
        if raw.len() > MAX_LEN {
            return Err(VaultError::Invalid("path too long".to_owned()));
        }
        if raw.contains('\0') {
            return Err(VaultError::Invalid("contains NUL".to_owned()));
        }
        let text = raw.replace('\\', "/");
        if text.starts_with('/') || is_drive(&text) {
            return Err(VaultError::Invalid(text));
        }
        let mut parts: Vec<&str> = Vec::new();
        for segment in text.split('/') {
            match segment {
                "" | "." => {}
                ".." => {
                    if parts.pop().is_none() {
                        return Err(VaultError::Escapes(text.clone()));
                    }
                }
                name => {
                    if cfg!(windows) && !windows_name_ok(name) {
                        return Err(VaultError::Invalid(text.clone()));
                    }
                    parts.push(name);
                }
            }
        }
        Ok(Self(parts.join("/")))
    }

    /// Joins already-validated segments (from disk). Caller guarantees segments came from real names.
    pub(crate) fn from_segments<'a>(segments: impl IntoIterator<Item = &'a str>) -> Self {
        Self(segments.into_iter().collect::<Vec<_>>().join("/"))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// Segments (none for the root).
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|s| !s.is_empty())
    }

    /// Identity key: NFC + case folded where the OS ignores case.
    #[must_use]
    pub fn key(&self) -> String {
        fold(&self.0.nfc().collect::<String>())
    }

    /// Last segment (file | folder name); `None` for the root.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.segments().last()
    }

    /// The folder this path is in (the root for a top-level entry); `None` for the root itself.
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        if self.is_root() {
            return None;
        }
        Some(Self(
            self.0
                .rsplit_once('/')
                .map_or("", |(parent, _)| parent)
                .to_owned(),
        ))
    }

    /// This path plus one more segment. `name` must be one valid name (see `fileops::check_name`).
    #[must_use]
    pub fn join_name(&self, name: &str) -> Self {
        if self.is_root() {
            Self(name.to_owned())
        } else {
            Self(format!("{}/{name}", self.0))
        }
    }

    /// Whether `self` is `other` or inside it (whole segments, identity keys).
    #[must_use]
    pub fn is_within(&self, other: &Self) -> bool {
        let (mine, theirs) = (self.key(), other.key());
        other.is_root()
            || mine == theirs
            || mine
                .strip_prefix(theirs.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
    }
}

impl fmt::Display for VaultPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn is_drive(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// Windows device names: `CON`, `PRN`, `AUX`, `NUL`, `COM0-9`, `LPT0-9` (also with an extension, any case, and the
/// superscript digits Windows treats the same).
fn is_reserved_device(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end()
        .to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" => true,
        _ => {
            // `get`: a multi-byte char at index 3 is no device name (never slice mid-char)
            let (Some(base), Some(digit)) = (stem.get(..3), stem.get(3..)) else {
                return false;
            };
            (base == "COM" || base == "LPT")
                && matches!(
                    digit,
                    "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
        }
    }
}

/// One file name Windows accepts: no `:`, no trailing dot | space, no `< > " | ? *`, no control chars, no device name.
pub(crate) fn windows_name_ok(name: &str) -> bool {
    !name.contains(':')
        && !name.ends_with('.')
        && !name.ends_with(' ')
        && !name
            .chars()
            .any(|c| matches!(c, '<' | '>' | '"' | '|' | '?' | '*') || c < ' ')
        && !is_reserved_device(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(raw: &str) -> String {
        VaultPath::parse(raw).unwrap().as_str().to_owned()
    }

    #[test]
    fn normalizes_separators_dots_and_empty_segments() {
        assert_eq!(ok("a/./b/../b/c.md"), "a/b/c.md");
        assert_eq!(ok("a\\b.md"), "a/b.md");
        assert_eq!(ok("a//b/"), "a/b");
        assert_eq!(ok(""), "");
        assert_eq!(ok("."), "");
        assert!(VaultPath::parse("").unwrap().is_root());
    }

    #[test]
    fn climbing_above_the_root_escapes() {
        for raw in [
            "..",
            "../x.md",
            "a/../../x",
            "a\\..\\..\\x",
            "a/b/../../../c",
        ] {
            assert!(
                matches!(VaultPath::parse(raw), Err(VaultError::Escapes(_))),
                "{raw}"
            );
        }
    }

    #[test]
    fn absolute_drive_unc_and_nul_are_invalid() {
        for raw in [
            "/etc/passwd",
            "C:/Windows",
            "C:\\Windows",
            "c:x",
            "\\\\server\\share",
            "//server/share",
            "a\0b",
        ] {
            assert!(
                matches!(VaultPath::parse(raw), Err(VaultError::Invalid(_))),
                "{raw:?}"
            );
        }
        assert!(VaultPath::parse(&"a/".repeat(20_000)).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_reserved_names_and_streams_are_invalid() {
        for raw in [
            "CON",
            "con.md",
            "docs/NUL",
            "aux.txt",
            "COM1",
            "lpt9.md",
            "COM¹",
            "a.md:evil",
            "a.md::$DATA",
            "dir./x",
            "trailing ",
            "a<b",
            "q?.md",
            "star*.md",
            "pipe|.md",
            "tab\t.md",
        ] {
            assert!(
                matches!(VaultPath::parse(raw), Err(VaultError::Invalid(_))),
                "{raw:?}"
            );
        }
        assert_eq!(ok("console.md"), "console.md");
        assert_eq!(ok("COM10.md"), "COM10.md");
        assert_eq!(ok("nul-not.md"), "nul-not.md");
    }

    #[test]
    fn unicode_cjk_and_spaces_are_kept_as_given() {
        assert_eq!(ok("文档/my notes/ação.md"), "文档/my notes/ação.md");
        // multi-byte char right at the device-name boundary: no slicing panic
        assert_eq!(ok("ÄÖx.md"), "ÄÖx.md");
        assert_eq!(ok("CO文"), "CO文");
        let nfd = "a\u{0301}.md";
        assert_eq!(ok(nfd), nfd);
        assert_eq!(VaultPath::parse(nfd).unwrap().key(), fold("\u{e1}.md"));
    }

    #[test]
    fn name_is_the_last_segment() {
        assert_eq!(VaultPath::parse("a/b/c.md").unwrap().name(), Some("c.md"));
        assert_eq!(VaultPath::root().name(), None);
    }
}
