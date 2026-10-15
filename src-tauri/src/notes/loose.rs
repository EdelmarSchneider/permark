//! Loose files: files the person opened in the Notes window that are not in the notes folder (a Markdown file someone
//! sent, one in Downloads, a text file). They stay in the list under **Files** until they are removed from it, and removing
//! one never touches the file.
//!
//! A loose file is a scope of its own: the window refers to it by an opaque id, never by a path of the disk. Its UI path is
//! `@file/<id>/<file name>`; the file itself and the images next to it can be reached through that path and nothing else.
//! TrailMark never writes a loose file on its own: the text being typed is kept in `.trailmark/pending/` (see
//! [`pending`](super::pending)) until the person saves it.
//!
//! This module is the plain data and the rules that need no folder of notes: the entry that is kept in `notes.json`, the
//! paths, the ids, how many files are listed, and how a file that was renamed is found again.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::data::Stamp;
use crate::util::{hash_bytes, modified_ms};
use crate::vault::key as case_key;

/// What starts the UI path of a loose file.
pub(crate) const PREFIX: &str = "@file/";
/// How many loose files are kept, besides the pinned ones and the ones with unsaved text.
pub(crate) const MAX_LISTED: usize = 50;
/// How many hexadecimal digits an id has.
const ID_DIGITS: usize = 8;

/// A loose file as `notes.json` keeps it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct LooseFile {
    pub id: String,
    /// The file's place, as it was when last seen (canonical when it was added).
    pub path: String,
    /// When it was last opened: the list shows the files by this.
    pub opened_ms: u64,
    /// How the file looked then, to find it again if it is renamed.
    pub stamp: Stamp,
}

impl LooseFile {
    pub(crate) fn abs(&self) -> &Path {
        Path::new(&self.path)
    }

    /// The file's name.
    pub(crate) fn name(&self) -> String {
        self.abs()
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
    }

    /// The UI path of the file: it holds the id and the file's name.
    pub(crate) fn ui_path(&self) -> String {
        ui_path(&self.id, &self.name())
    }

    /// What the notes remember about the file under (its pin, its place, whether it was the open one).
    pub(crate) fn key(&self) -> String {
        key_of(&self.id)
    }
}

/// `@file/<id>/<name>`.
pub(crate) fn ui_path(id: &str, name: &str) -> String {
    format!("{PREFIX}{id}/{name}")
}

/// `@file/<id>`: what the pins, the places and the open note are kept under, so that they follow the file when it is renamed.
pub(crate) fn key_of(id: &str) -> String {
    format!("{PREFIX}{id}")
}

/// Whether a UI path is one of a loose file (the file or something next to it).
pub(crate) fn is_loose_path(path: &str) -> bool {
    path.starts_with(PREFIX)
}

/// Whether `id` is an id: eight lowercase hexadecimal digits.
pub(crate) fn is_id(id: &str) -> bool {
    id.len() == ID_DIGITS
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A UI path of a loose file taken apart: the id, and what comes after it (the file's name, or an image next to it).
pub(crate) fn parse(path: &str) -> Option<(&str, &str)> {
    let (id, rest) = path.strip_prefix(PREFIX)?.split_once('/')?;
    (is_id(id) && !rest.is_empty()).then_some((id, rest))
}

/// The id out of a key or a UI path of a loose file.
pub(crate) fn id_of(key_or_path: &str) -> Option<&str> {
    let id = key_or_path.strip_prefix(PREFIX)?.split('/').next()?;
    is_id(id).then_some(id)
}

/// A new id: eight hexadecimal digits made from `seed` (the path and the time), different from every id `taken` knows.
pub(crate) fn new_id(seed: &str, now_ms: u64, taken: impl Fn(&str) -> bool) -> String {
    let mut bytes = seed.as_bytes().to_vec();
    bytes.extend_from_slice(&now_ms.to_le_bytes());
    for round in 0u32.. {
        let mut salted = bytes.clone();
        salted.extend_from_slice(&round.to_le_bytes());
        let id = format!("{:08x}", hash_bytes(&salted) & 0xffff_ffff);
        if !taken(&id) {
            return id;
        }
    }
    unreachable!("a free id is found within a few rounds")
}

/// Whether two paths are the same file as far as their names go (case does not matter where the system ignores it).
pub(crate) fn same_path(a: &str, b: &str) -> bool {
    case_key(a) == case_key(b)
}

/// The folder of a file as the list shows it: with `~` for the home folder, and the system's separators.
pub(crate) fn folder_label(file: &Path, home: Option<&Path>) -> String {
    let Some(folder) = file.parent() else {
        return String::new();
    };
    if let Some(home) = home {
        if let Ok(rest) = folder.strip_prefix(home) {
            let sep = std::path::MAIN_SEPARATOR;
            return if rest.as_os_str().is_empty() {
                "~".to_owned()
            } else {
                format!("~{sep}{}", rest.display())
            };
        }
    }
    folder.display().to_string()
}

/// The size and modification time of a file, or `None` when it is not there.
pub(crate) fn stamp_of(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path)
        .ok()
        .filter(std::fs::Metadata::is_file)?;
    Some(Stamp {
        size: meta.len(),
        modified_ms: modified_ms(&meta),
    })
}

/// How a stamp is told to the UI, which only compares it with the one it had.
pub(crate) fn stamp_label(stamp: Stamp) -> String {
    format!("{}-{}", stamp.size, stamp.modified_ms)
}

/// The file that `entry` became when it was renamed in its folder: the one file there with the size and modification time the
/// entry had, that `taken` does not claim (another entry's). `None` when there is no such file or more than one.
pub(crate) fn find_renamed(entry: &LooseFile, taken: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    if entry.stamp == Stamp::default() {
        return None;
    }
    let folder = entry.abs().parent()?;
    let mut found: Option<PathBuf> = None;
    for item in std::fs::read_dir(folder).ok()?.filter_map(Result::ok) {
        let path = item.path();
        if taken(&path) || stamp_of(&path) != Some(entry.stamp) {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(path);
    }
    found
}

/// Keeps the list to [`MAX_LISTED`] files: the oldest are dropped, except the ones `keep` says must stay (pinned, unsaved text).
pub(crate) fn prune(files: &mut Vec<LooseFile>, keep: impl Fn(&LooseFile) -> bool) {
    let mut loose: Vec<(u64, String)> = files
        .iter()
        .filter(|file| !keep(file))
        .map(|file| (file.opened_ms, file.id.clone()))
        .collect();
    if loose.len() <= MAX_LISTED {
        return;
    }
    // the newest first; the ones past the limit go
    loose.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let dropped: Vec<String> = loose
        .into_iter()
        .skip(MAX_LISTED)
        .map(|(_, id)| id)
        .collect();
    files.retain(|file| !dropped.contains(&file.id));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: &str, path: &str, opened_ms: u64) -> LooseFile {
        LooseFile {
            id: id.to_owned(),
            path: path.to_owned(),
            opened_ms,
            stamp: Stamp::default(),
        }
    }

    #[test]
    fn a_ui_path_holds_the_id_and_the_name_and_is_taken_apart_again() {
        let loose = file("0a1b2c3d", "/home/ana/Downloads/proposal.md", 1);
        assert_eq!(loose.ui_path(), "@file/0a1b2c3d/proposal.md");
        assert_eq!(loose.key(), "@file/0a1b2c3d");
        assert_eq!(
            parse("@file/0a1b2c3d/proposal.md"),
            Some(("0a1b2c3d", "proposal.md"))
        );
        assert_eq!(id_of("@file/0a1b2c3d"), Some("0a1b2c3d"));
        assert_eq!(id_of("@file/0a1b2c3d/proposal.md"), Some("0a1b2c3d"));
        assert!(is_loose_path("@file/anything"));
        assert!(!is_loose_path("2026-10-15 1432.md"));
    }

    #[test]
    fn what_is_not_a_path_of_a_loose_file_does_not_parse() {
        for bad in [
            "proposal.md",
            "@file/",
            "@file/0a1b2c3d",
            "@file/0a1b2c3d/",
            "@file/0A1B2C3D/x.md",
            "@file/0A1B2C3/x.md",
            "@file/not-hex!/x.md",
            "@file/0a1b2c3d4/x.md",
            "@wt/main/x.md",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
        assert_eq!(id_of("@file/xyz"), None);
        assert_eq!(id_of("note.md"), None);
    }

    #[test]
    fn an_id_is_eight_hex_digits_and_differs_from_the_ones_in_use() {
        let first = new_id("/a/b.md", 5, |_| false);
        assert_eq!(first.len(), 8);
        assert!(first.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(first, new_id("/a/b.md", 5, |_| false), "the same seed");
        let second = new_id("/a/b.md", 5, |id| id == first);
        assert_ne!(first, second);
        assert_eq!(second.len(), 8);
    }

    #[test]
    fn the_folder_is_shown_from_the_home_folder_when_it_is_inside() {
        let home = Path::new("/home/ana");
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(
            folder_label(Path::new("/home/ana/Downloads/x.md"), Some(home)),
            format!("~{sep}Downloads")
        );
        assert_eq!(folder_label(Path::new("/home/ana/x.md"), Some(home)), "~");
        assert_eq!(
            folder_label(Path::new("/srv/share/x.md"), Some(home)),
            Path::new("/srv/share").display().to_string()
        );
        assert_eq!(
            folder_label(Path::new("/srv/share/x.md"), None),
            Path::new("/srv/share").display().to_string()
        );
    }

    #[test]
    fn only_the_oldest_files_that_nothing_keeps_are_dropped() {
        let mut files: Vec<LooseFile> = (0..(MAX_LISTED as u64 + 5))
            .map(|n| file(&format!("{n:08x}"), &format!("/f/{n}.md"), 100 + n))
            .collect();
        // the oldest one is pinned, the second oldest has unsaved text
        let keep = |f: &LooseFile| f.id == "00000000" || f.id == "00000001";
        prune(&mut files, keep);
        let ids: Vec<&str> = files.iter().map(|f| f.id.as_str()).collect();
        assert!(ids.contains(&"00000000") && ids.contains(&"00000001"));
        assert_eq!(files.len(), MAX_LISTED + 2, "the limit applies to the rest");
        // the ones dropped are the oldest of the rest: 53 of them, three too many
        assert!(!ids.contains(&"00000002") && !ids.contains(&"00000004"));
        assert!(ids.contains(&"00000005"));
        // nothing happens below the limit
        let mut few = vec![file("00000010", "/f/a.md", 1)];
        prune(&mut few, |_| false);
        assert_eq!(few.len(), 1);
    }

    #[test]
    fn a_renamed_file_is_found_by_its_size_and_time_and_only_when_it_is_the_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let old = dir.path().join("proposal.md");
        std::fs::write(&old, "# Proposal\n").expect("write");
        let stamp = stamp_of(&old).expect("stamp");
        let entry = LooseFile {
            id: "0a1b2c3d".to_owned(),
            path: old.display().to_string(),
            opened_ms: 1,
            stamp,
        };
        let renamed = dir.path().join("proposal v2.md");
        std::fs::rename(&old, &renamed).expect("rename");
        assert_eq!(find_renamed(&entry, |_| false), Some(renamed.clone()));
        // a file that another entry has is not this one
        assert_eq!(find_renamed(&entry, |path| path == renamed), None);
        // a second file that looks the same is no longer certain
        let twin = dir.path().join("twin.md");
        std::fs::copy(&renamed, &twin).expect("copy");
        let same_time = std::fs::metadata(&renamed)
            .and_then(|meta| meta.modified())
            .expect("mtime");
        std::fs::File::options()
            .write(true)
            .open(&twin)
            .and_then(|file| file.set_modified(same_time))
            .expect("same time");
        assert_eq!(find_renamed(&entry, |_| false), None);
        // an entry that never had a stamp finds nothing
        let unknown = LooseFile {
            stamp: Stamp::default(),
            ..entry
        };
        assert_eq!(find_renamed(&unknown, |_| false), None);
    }
}
