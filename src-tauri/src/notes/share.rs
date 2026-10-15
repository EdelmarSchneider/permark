//! What an agent can see of the notes: the ones the person shared, and nothing else.
//!
//! Notes are private. The agent connection is never on in the Notes window, and a program that works in the notes folder is
//! not served as a project (see `bridge::project`). A note is shared on purpose, one by one (`notes.json` keeps the names), and
//! then an agent of any project whose connection is on can list the shared notes and read one. It cannot list the folder, read
//! a note that is not shared, or change a note.
//!
//! This is plain Rust over the folder, without the notes service: the program an agent starts (`trailmark mcp`) reads the
//! shared notes from their files when no window answers, as it does for the project's documents.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::data::NotesData;
use super::folder;
use super::names::is_note_name;
use super::text::{head_of, title_of};
use crate::render::decode_text;
use crate::session::AppSettingsStore;
use crate::util::modified_ms;

/// The largest note an agent can read: more than this is not something to put in an agent's context, and a reply carries 4 MB at most.
pub(crate) const MAX_SHARED_BYTES: u64 = 2 * 1024 * 1024;

/// A shared note, as `list_notes` tells it. Matches the answer's `notes`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedNote {
    /// The note's file name: what `read_note` asks for.
    pub note: String,
    /// The first line, without Markdown marks. Empty for a note with no text.
    pub title: String,
    /// When the note was last written (milliseconds since the Unix epoch).
    pub modified_ms: u64,
    pub size_bytes: u64,
}

/// A shared note with its text, as `read_note` tells it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedText {
    pub note: String,
    pub title: String,
    pub modified_ms: u64,
    /// The text of the note, line endings as `\n`.
    pub text: String,
}

/// Why a note cannot be read through the agent connection.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShareError {
    /// There is no shared note with that name. Said the same whether the note exists unshared, is not there, or the name is
    /// not a name of a note at all: an agent learns nothing about what is not shared.
    #[error("there is no shared note with that name")]
    NotShared,
    #[error("that note is too large to read through an agent (over {} MB)", MAX_SHARED_BYTES / (1024 * 1024))]
    TooLarge,
    #[error("that note is not text")]
    NotText,
}

/// Where the notes are, as the person set it up: the notes folder of the data folder `app_data` (the default folder, or the
/// one chosen in Settings).
pub(crate) fn notes_root(app_data: &Path) -> PathBuf {
    let chosen = AppSettingsStore::new(app_data).load().notes_folder;
    folder::configured(app_data, chosen.as_deref())
}

/// The file of a shared note: a regular file directly in the notes folder (never a link, which could lead anywhere) that
/// `notes.json` lists as shared, under exactly this name.
fn shared_file(root: &Path, data: &NotesData, name: &str) -> Option<PathBuf> {
    let plain = !name.is_empty()
        && !name.starts_with('.')
        && !name.contains(['/', '\\', '\0'])
        && is_note_name(name);
    if !plain || !data.is_shared(name) {
        return None;
    }
    let path = root.join(name);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    meta.file_type().is_file().then_some(path)
}

/// The shared notes that are there, the newest first.
pub fn list_shared(root: &Path) -> Vec<SharedNote> {
    let data = NotesData::load(root);
    let mut notes: Vec<SharedNote> = data
        .shared
        .iter()
        .filter_map(|name| {
            let path = shared_file(root, &data, name)?;
            let meta = std::fs::metadata(&path).ok()?;
            Some(SharedNote {
                note: name.clone(),
                title: title_of(&head_of(&path)),
                modified_ms: modified_ms(&meta),
                size_bytes: meta.len(),
            })
        })
        .collect();
    notes.sort_by(|a, b| {
        b.modified_ms
            .cmp(&a.modified_ms)
            .then_with(|| a.note.cmp(&b.note))
    });
    notes
}

/// The text of a shared note. A note that is not shared is [`ShareError::NotShared`], whatever else is true of it.
pub fn read_shared(root: &Path, name: &str) -> Result<SharedText, ShareError> {
    let data = NotesData::load(root);
    let path = shared_file(root, &data, name).ok_or(ShareError::NotShared)?;
    let meta = std::fs::metadata(&path).map_err(|_| ShareError::NotShared)?;
    if meta.len() > MAX_SHARED_BYTES {
        return Err(ShareError::TooLarge);
    }
    let bytes = std::fs::read(&path).map_err(|_| ShareError::NotShared)?;
    let text = decode_text(&bytes).ok_or(ShareError::NotText)?;
    Ok(SharedText {
        note: name.to_owned(),
        title: title_of(&text),
        modified_ms: modified_ms(&meta),
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    /// A notes folder with these notes (name and text) and these names shared.
    fn notes(files: &[(&str, &str)], shared: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for (name, text) in files {
            fs::write(dir.path().join(name), text).expect("write");
        }
        let data = NotesData {
            shared: shared.iter().map(|name| (*name).to_owned()).collect(),
            ..NotesData::default()
        };
        data.save(dir.path()).expect("notes.json");
        dir
    }

    fn set_modified(dir: &Path, name: &str, ms: u64) {
        let file = fs::OpenOptions::new()
            .write(true)
            .open(dir.join(name))
            .expect("open");
        file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms))
            .expect("time");
    }

    #[test]
    fn only_the_shared_notes_are_listed_the_newest_first_with_what_an_agent_needs_to_choose() {
        let dir = notes(
            &[
                ("2026-10-15 1432.md", "# Server IPs\n\napi 10.0.0.4\n"),
                ("a private note.md", "# Private\n"),
                ("checklist.md", "Weekly checklist\n- [ ] backups\n"),
            ],
            &["2026-10-15 1432.md", "checklist.md"],
        );
        set_modified(dir.path(), "2026-10-15 1432.md", 1_000);
        set_modified(dir.path(), "checklist.md", 2_000);
        let listed = list_shared(dir.path());
        let names: Vec<&str> = listed.iter().map(|n| n.note.as_str()).collect();
        assert_eq!(names, ["checklist.md", "2026-10-15 1432.md"]);
        assert_eq!(listed[0].title, "Weekly checklist");
        assert_eq!(listed[1].title, "Server IPs");
        assert_eq!(listed[1].modified_ms, 1_000);
        assert_eq!(listed[1].size_bytes, 27);
        assert!(!names.contains(&"a private note.md"));
    }

    #[test]
    fn a_shared_note_is_read_and_a_note_that_is_not_shared_is_not_even_when_it_exists() {
        let dir = notes(
            &[
                ("shared.md", "# Shared\r\nthe text\r\n"),
                ("private.md", "# Private\n"),
            ],
            &["shared.md"],
        );
        let read = read_shared(dir.path(), "shared.md").expect("read");
        assert_eq!(read.title, "Shared");
        assert_eq!(read.text, "# Shared\nthe text\n");
        assert_eq!(
            read_shared(dir.path(), "private.md"),
            Err(ShareError::NotShared)
        );
        // the same answer for a note that is not there, so nothing is learned from it
        assert_eq!(
            read_shared(dir.path(), "nothing.md"),
            Err(ShareError::NotShared)
        );
    }

    #[test]
    fn nothing_but_the_name_of_a_shared_note_reads_a_note() {
        let dir = notes(
            &[("shared.md", "# Shared\n"), (".hidden.md", "# Hidden\n")],
            &[
                "shared.md",
                ".hidden.md",
                "../outside.md",
                "sub/x.md",
                "notes.json",
            ],
        );
        fs::create_dir(dir.path().join("sub")).expect("mkdir");
        fs::write(dir.path().join("sub").join("x.md"), "# Deeper\n").expect("write");
        for bad in [
            "",
            ".hidden.md",
            "../outside.md",
            "..\\outside.md",
            "sub/x.md",
            "sub\\x.md",
            "/etc/passwd",
            "C:\\Windows\\win.ini",
            "shared.txt",
            "shared.md ",
            "SHARED.md",
            "shared.md:stream",
            "notes.json",
            ".trailmark/notes.json",
        ] {
            assert_eq!(
                read_shared(dir.path(), bad),
                Err(ShareError::NotShared),
                "{bad:?}"
            );
        }
        // and the listing does not carry them either
        let listed: Vec<String> = list_shared(dir.path())
            .into_iter()
            .map(|n| n.note)
            .collect();
        assert_eq!(listed, ["shared.md"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_not_followed_even_when_its_name_is_shared() {
        let dir = notes(&[], &["linked.md"]);
        let outside = tempfile::tempdir().expect("tempdir");
        fs::write(outside.path().join("secret.md"), "# Secret\n").expect("write");
        std::os::unix::fs::symlink(
            outside.path().join("secret.md"),
            dir.path().join("linked.md"),
        )
        .expect("link");
        assert_eq!(
            read_shared(dir.path(), "linked.md"),
            Err(ShareError::NotShared)
        );
        assert_eq!(list_shared(dir.path()).len(), 0);
    }

    #[test]
    fn a_note_that_is_gone_is_not_listed_and_one_that_is_too_big_or_not_text_is_not_read() {
        let dir = notes(
            &[("binary.md", "")],
            &["gone.md", "big.md", "binary.md", "shared.md"],
        );
        fs::write(dir.path().join("binary.md"), [b'a', 0, 0, b'b']).expect("write");
        fs::File::create(dir.path().join("big.md"))
            .and_then(|file| file.set_len(MAX_SHARED_BYTES + 1))
            .expect("big");
        fs::write(dir.path().join("shared.md"), "# Fine\n").expect("write");
        let listed: Vec<String> = list_shared(dir.path())
            .into_iter()
            .map(|n| n.note)
            .collect();
        assert!(!listed.contains(&"gone.md".to_owned()));
        assert!(listed.contains(&"shared.md".to_owned()));
        assert_eq!(
            read_shared(dir.path(), "gone.md"),
            Err(ShareError::NotShared)
        );
        assert_eq!(read_shared(dir.path(), "big.md"), Err(ShareError::TooLarge));
        assert_eq!(
            read_shared(dir.path(), "binary.md"),
            Err(ShareError::NotText)
        );
    }

    #[test]
    fn a_folder_without_notes_json_shares_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("note.md"), "# Note\n").expect("write");
        assert_eq!(list_shared(dir.path()).len(), 0);
        assert_eq!(
            read_shared(dir.path(), "note.md"),
            Err(ShareError::NotShared)
        );
        assert_eq!(list_shared(&dir.path().join("not-there")).len(), 0);
    }

    #[test]
    fn the_notes_folder_is_the_one_the_person_set_up() {
        let app = tempfile::tempdir().expect("app data");
        // a debug build keeps the default notes in the data folder
        assert_eq!(notes_root(app.path()), app.path().join("notes"));
        let chosen = tempfile::tempdir().expect("chosen");
        let chosen_path = dunce::canonicalize(chosen.path()).expect("canonical");
        AppSettingsStore::new(app.path())
            .set_notes_folder(&chosen_path.display().to_string())
            .expect("set");
        assert_eq!(notes_root(app.path()), chosen_path);
    }
}
