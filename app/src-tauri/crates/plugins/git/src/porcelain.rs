//! Parsers of the machine-readable output of git (`status --porcelain=v2 -z`, `diff --name-status -z`, `ls-files -z`,
//! `log -z --raw`, `ls-tree -z`). Pure: bytes in, entries out. Paths are text (a name that is not UTF-8 is skipped, as
//! the Explorer skips it).

use permark_core::plugins::{CommitId, FileVersion};

use crate::ops::{is_version, regular, version, GitError};
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

/// Parses `git diff | diff-tree --name-status -z` output: `M NUL path NUL`; with rename detection on, a rename is
/// `R<score> NUL old path NUL new path NUL`.
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
            Some(b'R') => {
                // the path read is the old one; the new one follows
                if let (Some(old), Some(new)) = (text(path), fields.next().and_then(text)) {
                    entries.push(Entry {
                        path: new.to_owned(),
                        letter: Letter::Renamed,
                        staged: false,
                        from: Some(old.to_owned()),
                    });
                }
                continue;
            }
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

/// One entry of the versions log: a commit that changed something at the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// The version of the file the commit made; `None`: it made none (the file deleted, only its mode changed,
    /// something else than a file there) | the entry cannot be read.
    pub version: Option<FileVersion>,
    /// The commit has a parent, and that parent holds nothing at the path: the file came with this commit.
    pub added: bool,
}

/// What the versions log prints of a commit, NUL between: id, parent ids, committer time, author name, whole message.
pub const LOG_FORMAT: &str = "--format=%H%x00%P%x00%ct%x00%an%x00%B";

/// Parses `git log -z --raw --no-abbrev --no-renames <LOG_FORMAT>` output for the file `path` (relative to the working
/// tree): one entry per commit printed, in order. A commit is its five fields, then its raw changes, each
/// `:<mode> <mode> <id> <id> <status> NUL <path> NUL`. Read by position only: no text of a commit (a message may look
/// like anything) is ever taken for a field of another.
#[must_use]
pub fn parse_versions_log(output: &[u8], path: &str) -> Vec<LogEntry> {
    let below = format!("{path}/");
    let mut entries = Vec::new();
    let mut fields = output.split(|&b| b == 0).peekable();
    while let (Some(id), Some(parents), Some(time), Some(author), Some(message)) = (
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
    ) {
        // the changes of the commit: the first follows a blank line
        let mut change = None;
        let mut folder_before = false;
        while let Some(raw) = fields.next_if(|field| raw_line(field).is_some()) {
            let Some(name) = fields.next() else { break };
            if name == path.as_bytes() {
                change = raw_line(raw).and_then(raw_change);
            } else if name.starts_with(below.as_bytes()) {
                folder_before = true;
            }
        }
        let row = change.and_then(|(before, now)| {
            if !is_version(before, now) {
                return None;
            }
            Some(version(
                CommitId::parse(text(id)?.trim())?,
                object_id(now?.1)?,
                text(time)?.trim().parse().ok()?,
                &String::from_utf8_lossy(author),
                &String::from_utf8_lossy(message),
            ))
        });
        entries.push(LogEntry {
            added: matches!(change, Some((None, _))) && !folder_before && !parents.is_empty(),
            version: row,
        });
    }
    entries
}

/// A raw change of a diff without the blank line `log` prints before the first one: `<mode> <mode> <id> <id>
/// <status>`; `None`: the field is no raw change (it starts the next commit).
fn raw_line(field: &[u8]) -> Option<&str> {
    text(field)?.trim_start_matches('\n').strip_prefix(':')
}

/// What a tree holds at a path: file mode + object id; `None`: nothing.
type Held<'a> = Option<(u32, &'a str)>;

/// The two sides of a raw change ([`raw_line`]): what the parent holds at the path, what the commit holds.
fn raw_change(line: &str) -> Option<(Held<'_>, Held<'_>)> {
    let mut parts = line.split(' ');
    let (mode_before, mode_now) = (parts.next()?, parts.next()?);
    let (id_before, id_now) = (parts.next()?, parts.next()?);
    let held = |mode: &str, id| {
        let mode = u32::from_str_radix(mode, 8).ok()?;
        Some((mode != 0).then_some((mode, id)))
    };
    Some((held(mode_before, id_before)?, held(mode_now, id_now)?))
}

/// A full object id as git prints it (hex digits), lowercase; `None`: anything else.
fn object_id(text: &str) -> Option<String> {
    (text.len() >= 40 && text.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| text.to_ascii_lowercase())
}

/// Parses `git ls-tree -z` output (`<mode> <type> <id> TAB <path> NUL`): the blob id of the regular file `path`
/// (relative to the working tree); `None`: no such entry | something else than a file.
#[must_use]
pub fn parse_tree_file(output: &[u8], path: &str) -> Option<String> {
    output.split(|&b| b == 0).find_map(|entry| {
        let tab = entry.iter().position(|&b| b == b'\t')?;
        let (facts, name) = (entry.get(..tab)?, entry.get(tab + 1..)?);
        if name != path.as_bytes() {
            return None;
        }
        let mut facts = text(facts)?.split(' ');
        let (mode, _kind, id) = (facts.next()?, facts.next()?, facts.next()?);
        regular(u32::from_str_radix(mode, 8).ok()?)
            .then(|| object_id(id))
            .flatten()
    })
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

    #[test]
    fn a_detected_rename_names_its_old_and_new_path() {
        let out = nul(&[
            "R100",
            "old.md",
            "new ü.md",
            "M",
            "x.md",
            "R087",
            "a/b",
            "c/d",
        ]);
        let entries = parse_name_status(&out).unwrap();
        let got: Vec<(&str, Letter, Option<&str>)> = entries
            .iter()
            .map(|e| (e.path.as_str(), e.letter, e.from.as_deref()))
            .collect();
        assert_eq!(
            got,
            [
                ("new ü.md", Letter::Renamed, Some("old.md")),
                ("x.md", Letter::Modified, None),
                ("c/d", Letter::Renamed, Some("a/b")),
            ]
        );
    }

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const ZERO: &str = "0000000000000000000000000000000000000000";
    const FILE: &str = "docs/a ü.md";

    fn id(n: u8) -> String {
        format!("{n:02x}").repeat(20)
    }

    /// One commit as the versions log prints it: its fields, then `"\n"` + its raw changes, each with its path.
    fn logged(fields: [&str; 5], changes: &[(&str, &str)]) -> Vec<u8> {
        let mut out = nul(&fields);
        for (n, (raw, path)) in changes.iter().enumerate() {
            if n == 0 {
                out.push(b'\n');
            }
            out.extend(nul(&[raw, path]));
        }
        out
    }

    fn raw(old_mode: &str, new_mode: &str, old: &str, new: &str, status: &str) -> String {
        format!(":{old_mode} {new_mode} {old} {new} {status}")
    }

    #[test]
    #[allow(clippy::too_many_lines)] // one commit per kind of change
    fn the_versions_log_is_one_entry_per_commit() {
        let changed = raw("100644", "100644", A, B, "M");
        let deleted = raw("100755", "000000", B, ZERO, "D");
        let mode_only = raw("100644", "100755", B, B, "M");
        let added = raw("000000", "100644", ZERO, A, "A");
        let was_a_link = raw("120000", "100644", A, A, "T");
        let now_a_link = raw("100644", "120000", A, B, "T");
        let mut out = Vec::new();
        out.extend(logged(
            [
                &id(7),
                &id(6),
                "1700000007",
                " Ann Ü ",
                "Seventh\n\nBody\nCo-authored-by: Bea <b@x>\n",
            ],
            &[(&changed, FILE)],
        ));
        out.extend(logged(
            [&id(6), &id(5), "1700000006", "Ann", "gone\n"],
            &[(&deleted, FILE)],
        ));
        out.extend(logged(
            [&id(5), &id(4), "1700000005", "Ann", "mode\n"],
            &[(&mode_only, FILE)],
        ));
        // other paths of the commit are not this file
        out.extend(logged(
            [&id(4), &id(3), "1700000004", "Ann", "others\n"],
            &[(&changed, "docs/a ü.md2"), (&changed, "docs/a ü.m")],
        ));
        out.extend(logged(
            [&id(3), &id(2), "1700000003", "Ann", "a link before\n"],
            &[(&was_a_link, FILE)],
        ));
        out.extend(logged(
            [&id(2), &id(1), "1700000002", "Ann", "a link now\n"],
            &[(&now_a_link, FILE)],
        ));
        // a folder of that name went in the same commit: the path was not free before
        out.extend(logged(
            [&id(1), &id(0), "1700000001", "Ann", "file for folder\n"],
            &[(&deleted, "docs/a ü.md/inner.md"), (&added, FILE)],
        ));
        out.extend(logged(
            [&id(0), &id(9), "1700000000", "Ann", "added\n"],
            &[(&added, FILE)],
        ));
        // the root commit has no parent to have renamed the file from
        out.extend(logged(
            [&id(9), "", "1699999999", "Ann", "root"],
            &[(&added, FILE)],
        ));
        let entries = parse_versions_log(&out, FILE);
        let got: Vec<(Option<&str>, Option<&str>, bool)> = entries
            .iter()
            .map(|e| {
                (
                    e.version.as_ref().map(|v| v.subject.as_str()),
                    e.version.as_ref().map(|v| v.blob.as_str()),
                    e.added,
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                (Some("Seventh"), Some(B), false),
                (None, None, false),
                (None, None, false),
                (None, None, false),
                (Some("a link before"), Some(A), false),
                (None, None, false),
                (Some("file for folder"), Some(A), false),
                (Some("added"), Some(A), true),
                (Some("root"), Some(A), false),
            ]
        );
        let first = entries[0].version.as_ref().unwrap();
        assert_eq!(first.commit.as_str(), id(7));
        assert_eq!(first.time_ms, 1_700_000_007_000);
        assert_eq!(first.author, "Ann Ü");
        assert_eq!(first.co_authors, ["Bea"]);
    }

    #[test]
    fn the_text_of_a_commit_is_never_read_as_a_field_of_the_log() {
        let changed = raw("100644", "100644", A, B, "M");
        let hostile_message = format!("{}\n\n{changed}", id(8));
        let mut out = Vec::new();
        // a message that is a commit id, then a raw change; an author that looks like a raw change
        out.extend(logged(
            [&id(3), &id(2), "1700000003", &changed, &hostile_message],
            &[(&changed, FILE)],
        ));
        // a message that is nothing but a commit id
        out.extend(logged(
            [&id(2), &id(1), "1700000002", "Ann", &id(8)],
            &[(&changed, FILE)],
        ));
        out.extend(logged(
            [&id(1), "", "1700000001", "Ann", ""],
            &[(&raw("000000", "100644", ZERO, A, "A"), FILE)],
        ));
        let entries = parse_versions_log(&out, FILE);
        let got: Vec<(String, String)> = entries
            .iter()
            .map(|e| {
                let v = e.version.as_ref().unwrap();
                (v.commit.to_string(), v.subject.clone())
            })
            .collect();
        assert_eq!(
            got,
            [(id(3), id(8)), (id(2), id(8)), (id(1), String::new())]
        );
        assert_eq!(entries[0].version.as_ref().unwrap().author, changed);
    }

    #[test]
    fn an_unreadable_entry_of_the_log_is_counted_and_skipped() {
        let changed = raw("100644", "100644", A, B, "M");
        let good = |n: u8| {
            logged(
                [&id(n), &id(0), "1700000000", "Ann", "ok\n"],
                &[(&changed, FILE)],
            )
        };
        let mut out = good(5);
        // not a commit id | not a time | a raw change that is none | an object id that is none | no change at all
        out.extend(logged(
            ["HEAD", "", "1700000000", "Ann", "x"],
            &[(&changed, FILE)],
        ));
        out.extend(logged(
            [&id(4), "", "soon", "Ann", "x"],
            &[(&changed, FILE)],
        ));
        out.extend(logged(
            [&id(3), "", "1700000000", "Ann", "x"],
            &[(":100644 100644", FILE)],
        ));
        out.extend(logged(
            [&id(2), "", "1700000000", "Ann", "x"],
            &[(&raw("100644", "100644", A, "not-hex", "M"), FILE)],
        ));
        out.extend(logged([&id(1), "", "1700000000", "Ann", "x"], &[]));
        out.extend(good(6));
        let entries = parse_versions_log(&out, FILE);
        let got: Vec<Option<String>> = entries
            .iter()
            .map(|e| e.version.as_ref().map(|v| v.commit.to_string()))
            .collect();
        assert_eq!(
            got,
            [Some(id(5)), None, None, None, None, None, Some(id(6))]
        );
        // text that is not UTF-8 is read as far as it can be: no panic, the entry stays
        let mut odd = nul(&[&id(7), "", "1700000000"]);
        odd.extend([b'A', 0xff, b'n', 0, 0xc3, b'\n', b'x', 0xe2, 0x82, 0]);
        odd.extend(nul(&[&format!("\n{changed}"), FILE]));
        let entries = parse_versions_log(&odd, FILE);
        assert_eq!(entries.len(), 1);
        let version = entries[0].version.as_ref().unwrap();
        assert_eq!(version.author, "A\u{fffd}n");
        assert_eq!(version.subject, "\u{fffd}");
        // output that stops early gives what was whole; nothing gives nothing
        let mut cut = good(5);
        cut.extend(nul(&[&id(4), "", "17"]));
        assert_eq!(parse_versions_log(&cut, FILE).len(), 1);
        let whole = good(5);
        for end in 0..whole.len() {
            let _ = parse_versions_log(&whole[..end], FILE);
        }
        assert!(parse_versions_log(b"", FILE).is_empty());
        assert!(parse_versions_log(&[0, 0, 0], FILE).is_empty());
    }

    #[test]
    fn the_file_of_a_tree_listing_is_named_by_its_blob_id() {
        let line = |mode: &str, kind: &str, path: &str| format!("{mode} {kind} {A}\t{path}");
        let file = |out: &[&str]| parse_tree_file(&nul(out), FILE);
        assert_eq!(file(&[&line("100644", "blob", FILE)]).as_deref(), Some(A));
        assert_eq!(file(&[&line("100755", "blob", FILE)]).as_deref(), Some(A));
        assert_eq!(
            file(&[
                &line("100644", "blob", "docs/a ü.md2"),
                &line("100644", "blob", FILE)
            ])
            .as_deref(),
            Some(A)
        );
        // a link, a folder, a submodule, another path, garbage
        assert_eq!(file(&[&line("120000", "blob", FILE)]), None);
        assert_eq!(file(&[&line("040000", "tree", FILE)]), None);
        assert_eq!(file(&[&line("160000", "commit", FILE)]), None);
        assert_eq!(file(&[&line("100644", "blob", "docs/other.md")]), None);
        assert_eq!(file(&[&format!("100644 blob nope\t{FILE}")]), None);
        assert_eq!(file(&[FILE]), None);
        assert_eq!(file(&[]), None);
        // a name with a tab is cut at the first one only
        assert_eq!(
            parse_tree_file(&nul(&[&line("100644", "blob", "a\tb.md")]), "a\tb.md").as_deref(),
            Some(A)
        );
    }
}
