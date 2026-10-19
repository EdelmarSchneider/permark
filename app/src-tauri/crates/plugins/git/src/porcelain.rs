//! Parsers of the machine-readable output of git (`status --porcelain=v2 -z`, `diff --name-status -z`, `ls-files -z`).
//! Pure: bytes in, entries out. Paths are text (a name that is not UTF-8 is skipped, as the Explorer skips it).

use crate::ops::GitError;
use crate::types::{HeadInfo, Letter};

/// One entry of `status`: path relative to the working tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub letter: Letter,
    pub staged: bool,
    pub from: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub head: HeadInfo,
    pub entries: Vec<Entry>,
}

fn text(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes).ok()
}

/// The letter of an `XY` pair of a changed (not unmerged, not renamed) entry. `None`: nothing to show.
fn letter_of(x: u8, y: u8) -> Option<Letter> {
    if x == b'D' || y == b'D' {
        Some(Letter::Deleted)
    } else if x == b'A' || y == b'A' {
        Some(Letter::Added)
    } else if matches!(x, b'M' | b'T') || matches!(y, b'M' | b'T') {
        Some(Letter::Modified)
    } else {
        None
    }
}

/// Parses `git status --porcelain=v2 -z --branch` output.
///
/// # Errors
/// A line that is not part of the format.
pub fn parse_status(output: &[u8]) -> Result<Status, GitError> {
    let mut status = Status::default();
    let mut fields = output.split(|&b| b == 0).peekable();
    while let Some(field) = fields.next() {
        if field.is_empty() {
            continue;
        }
        match field[0] {
            b'#' => header(&mut status.head, field),
            b'?' => {
                if let Some(path) = field.get(2..).and_then(text) {
                    status.entries.push(Entry {
                        path: path.to_owned(),
                        letter: Letter::Untracked,
                        staged: false,
                        from: None,
                    });
                }
            }
            b'!' => {}
            b'1' => {
                // 1 XY sub mH mI mW hH hI path   (path may hold spaces: it is the 9th field)
                let parts = split_fields(field, 9)?;
                let xy = parts[1].as_bytes();
                let (x, y) = (*xy.first().unwrap_or(&b'.'), *xy.get(1).unwrap_or(&b'.'));
                if let Some(letter) = letter_of(x, y) {
                    status.entries.push(Entry {
                        path: parts[8].to_owned(),
                        letter,
                        staged: x != b'.',
                        from: None,
                    });
                }
            }
            b'2' => {
                // 2 XY sub mH mI mW hH hI Xscore path NUL origPath
                let parts = split_fields(field, 10)?;
                let orig = fields.next().and_then(text).unwrap_or("");
                let xy = parts[1].as_bytes();
                let x = *xy.first().unwrap_or(&b'.');
                status.entries.push(Entry {
                    path: parts[9].to_owned(),
                    letter: Letter::Renamed,
                    staged: x != b'.',
                    from: (!orig.is_empty()).then(|| orig.to_owned()),
                });
            }
            b'u' => {
                // u XY sub m1 m2 m3 mW h1 h2 h3 path
                let parts = split_fields(field, 11)?;
                status.entries.push(Entry {
                    path: parts[10].to_owned(),
                    letter: Letter::Conflict,
                    staged: false,
                    from: None,
                });
            }
            other => {
                return Err(GitError::Parse(format!(
                    "status line starting with {:?}",
                    char::from(other)
                )))
            }
        }
    }
    Ok(status)
}

/// A header line: `# branch.oid <id> | (initial)`, `# branch.head <name> | (detached)`.
fn header(head: &mut HeadInfo, field: &[u8]) {
    let Some(line) = text(field) else { return };
    if let Some(oid) = line.strip_prefix("# branch.oid ") {
        if oid == "(initial)" {
            head.unborn = true;
            head.oid = None;
        } else {
            head.oid = Some(oid.to_owned());
        }
    } else if let Some(name) = line.strip_prefix("# branch.head ") {
        if name == "(detached)" {
            head.detached = true;
            head.branch = None;
        } else {
            head.branch = Some(name.to_owned());
        }
    }
}

/// Splits a line at spaces into `count` fields; the last one keeps its own spaces.
fn split_fields(field: &[u8], count: usize) -> Result<Vec<&str>, GitError> {
    let line = text(field).ok_or_else(|| GitError::Parse("a name that is not text".to_owned()))?;
    let parts: Vec<&str> = line.splitn(count, ' ').collect();
    if parts.len() == count {
        Ok(parts)
    } else {
        Err(GitError::Parse(format!(
            "short status line {line:?} ({} of {count} fields)",
            parts.len()
        )))
    }
}

/// Parses `git diff --name-status -z --no-renames` output: `M NUL path NUL`.
///
/// # Errors
/// A status letter without a path.
pub fn parse_name_status(output: &[u8]) -> Result<Vec<Entry>, GitError> {
    let mut entries = Vec::new();
    let mut fields = output.split(|&b| b == 0).filter(|f| !f.is_empty());
    while let Some(code) = fields.next() {
        let path = fields
            .next()
            .ok_or_else(|| GitError::Parse("a status letter without a path".to_owned()))?;
        let letter = match code.first() {
            Some(b'A') => Letter::Added,
            Some(b'D') => Letter::Deleted,
            Some(b'U') => Letter::Conflict,
            Some(b'M' | b'T') => Letter::Modified,
            _ => continue,
        };
        if let Some(path) = text(path) {
            entries.push(Entry {
                path: path.to_owned(),
                letter,
                staged: false,
                from: None,
            });
        }
    }
    Ok(entries)
}

/// Parses `git ls-files --others -z` output: untracked paths.
#[must_use]
pub fn parse_untracked(output: &[u8]) -> Vec<Entry> {
    output
        .split(|&b| b == 0)
        .filter(|f| !f.is_empty())
        .filter_map(text)
        .map(|path| Entry {
            path: path.to_owned(),
            letter: Letter::Untracked,
            staged: false,
            from: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nul(parts: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        for part in parts {
            out.extend_from_slice(part.as_bytes());
            out.push(0);
        }
        out
    }

    const H: &str = "1111111111111111111111111111111111111111";

    fn one(xy: &str, path: &str) -> String {
        format!("1 {xy} N... 100644 100644 100644 {H} {H} {path}")
    }

    #[test]
    fn headers_name_the_branch_and_the_commit() {
        let out = nul(&[
            &format!("# branch.oid {H}"),
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +1 -0",
        ]);
        let status = parse_status(&out).unwrap();
        assert_eq!(status.head.branch.as_deref(), Some("main"));
        assert_eq!(status.head.oid.as_deref(), Some(H));
        assert!(!status.head.detached && !status.head.unborn);
        assert!(status.entries.is_empty());
    }

    #[test]
    fn unborn_and_detached_heads() {
        let unborn = parse_status(&nul(&["# branch.oid (initial)", "# branch.head main"])).unwrap();
        assert!(unborn.head.unborn);
        assert_eq!(unborn.head.oid, None);
        assert_eq!(unborn.head.branch.as_deref(), Some("main"));
        let detached = parse_status(&nul(&[
            &format!("# branch.oid {H}"),
            "# branch.head (detached)",
        ]))
        .unwrap();
        assert!(detached.head.detached);
        assert_eq!(detached.head.branch, None);
    }

    #[test]
    fn changed_entries_map_to_letters_and_the_staged_flag() {
        let out = nul(&[
            &one("M.", "staged mod.md"),
            &one(".M", "unstaged mod.md"),
            &one("MM", "both.md"),
            &one("A.", "added.md"),
            &one("AM", "added then edited.md"),
            &one("AD", "added then deleted.md"),
            &one(".D", "deleted.md"),
            &one("D.", "staged delete.md"),
            &one("T.", "typechange"),
            &one(".A", "intent to add.md"),
            &one("..", "nothing.md"),
            "? new file.md",
            "! ignored.md",
        ]);
        let entries = parse_status(&out).unwrap().entries;
        let got: Vec<(&str, Letter, bool)> = entries
            .iter()
            .map(|e| (e.path.as_str(), e.letter, e.staged))
            .collect();
        assert_eq!(
            got,
            [
                ("staged mod.md", Letter::Modified, true),
                ("unstaged mod.md", Letter::Modified, false),
                ("both.md", Letter::Modified, true),
                ("added.md", Letter::Added, true),
                ("added then edited.md", Letter::Added, true),
                ("added then deleted.md", Letter::Deleted, true),
                ("deleted.md", Letter::Deleted, false),
                ("staged delete.md", Letter::Deleted, true),
                ("typechange", Letter::Modified, true),
                ("intent to add.md", Letter::Added, false),
                ("new file.md", Letter::Untracked, false),
            ]
        );
    }

    #[test]
    fn renames_carry_their_old_path_and_unmerged_entries_are_conflicts() {
        let out = nul(&[
            &format!("2 R. N... 100644 100644 100644 {H} {H} R100 new name.md"),
            "old name.md",
            &format!("u UU N... 100644 100644 100644 100644 {H} {H} {H} both changed.md"),
            &one(".M", "after.md"),
        ]);
        let entries = parse_status(&out).unwrap().entries;
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].letter, Letter::Renamed);
        assert_eq!(entries[0].path, "new name.md");
        assert_eq!(entries[0].from.as_deref(), Some("old name.md"));
        assert!(entries[0].staged);
        assert_eq!(entries[1].letter, Letter::Conflict);
        assert_eq!(entries[1].path, "both changed.md");
        // the entry after a rename is not eaten by its old path
        assert_eq!(entries[2].path, "after.md");
    }

    #[test]
    fn hard_names_survive_spaces_newlines_and_non_ascii() {
        let out = nul(&[
            &one(".M", "dir with  spaces/a\nb.md"),
            &one(".M", "日本語/файл ü.md"),
            "? tab\there.md",
        ]);
        let entries = parse_status(&out).unwrap().entries;
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "dir with  spaces/a\nb.md",
                "日本語/файл ü.md",
                "tab\there.md"
            ]
        );
    }

    #[test]
    fn malformed_output_is_an_error_not_a_panic() {
        for bad in [
            nul(&["1 .M N... 100644"]),
            nul(&["x garbage"]),
            nul(&["2 R. short"]),
            nul(&["u UU too short"]),
        ] {
            assert!(parse_status(&bad).is_err(), "{bad:?}");
        }
        // names that are not UTF-8 are skipped
        let mut raw = b"? bad ".to_vec();
        raw.push(0xff);
        raw.push(0);
        assert!(parse_status(&raw).unwrap().entries.is_empty());
        assert!(parse_status(b"").unwrap().entries.is_empty());
        assert!(parse_status(&[0, 0, 0]).unwrap().entries.is_empty());
    }

    #[test]
    fn name_status_and_untracked_listings() {
        let out = nul(&[
            "M", "a b.md", "A", "new.md", "D", "gone.md", "U", "c.md", "R100", "x",
        ]);
        let entries = parse_name_status(&out).unwrap();
        let got: Vec<(&str, Letter)> = entries
            .iter()
            .map(|e| (e.path.as_str(), e.letter))
            .collect();
        assert_eq!(
            got,
            [
                ("a b.md", Letter::Modified),
                ("new.md", Letter::Added),
                ("gone.md", Letter::Deleted),
                ("c.md", Letter::Conflict),
            ]
        );
        assert!(parse_name_status(&nul(&["M"])).is_err());
        let untracked = parse_untracked(&nul(&["u 1.md", "dir/ü.md"]));
        assert_eq!(untracked.len(), 2);
        assert!(untracked.iter().all(|e| e.letter == Letter::Untracked));
    }
}
