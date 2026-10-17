//! Earlier versions of the notes, in `.permark/history/<note>/<time>.md` in the notes folder.
//!
//! A version is a copy of what a note held before a save replaced it. One is taken at the first save after the note is
//! opened, then at most every ten minutes while it is edited, and one is taken before a restore, so a restore can be
//! undone. Versions are kept for 30 days, and never fewer than the last 20 of a note. A version is named after the
//! moment its text was written (UTC, to the millisecond), which is what the list shows.
//!
//! The cadence and the pruning take the time as an argument: nothing here reads a clock.

use std::collections::{BTreeSet, HashMap};
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDateTime};

use crate::util::write_atomic;

/// At most one version this often while a note is edited.
pub(crate) const MIN_GAP_MS: u64 = 10 * 60 * 1000;
/// Versions older than this are removed...
pub(crate) const KEEP_MS: u64 = 30 * 24 * 60 * 60 * 1000;
/// ...except that this many of the newest are always kept.
pub(crate) const KEEP_LAST: usize = 20;

const DIR: &str = "history";
const ID_FORMAT: &str = "%Y%m%d-%H%M%S";

/// When a version of each note was last taken, to decide when the next one is due.
#[derive(Debug, Default)]
pub(crate) struct Cadence {
    last_taken: HashMap<String, u64>,
}

impl Cadence {
    /// Whether a version of `note` is due at `now_ms`: the note has none taken since it was opened, or the last one is
    /// at least [`MIN_GAP_MS`] old.
    pub(crate) fn due(&self, note: &str, now_ms: u64) -> bool {
        self.last_taken
            .get(note)
            .is_none_or(|&last| now_ms.saturating_sub(last) >= MIN_GAP_MS)
    }

    /// A version of `note` was taken at `now_ms`.
    pub(crate) fn taken(&mut self, note: &str, now_ms: u64) {
        self.last_taken.insert(note.to_owned(), now_ms);
    }

    /// The note was opened: its next save takes a version.
    pub(crate) fn opened(&mut self, note: &str) {
        self.last_taken.remove(note);
    }

    pub(crate) fn renamed(&mut self, from: &str, to: &str) {
        if let Some(at) = self.last_taken.remove(from) {
            self.last_taken.insert(to.to_owned(), at);
        }
    }
}

/// One earlier version of a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Version {
    /// Names the version: the moment its text was written.
    pub id: String,
    /// That moment, in milliseconds since the epoch.
    pub written_ms: u64,
    pub size: u64,
}

/// The id of a version whose text was written at `written_ms`.
fn id_of(written_ms: u64) -> String {
    let at = i64::try_from(written_ms)
        .ok()
        .and_then(DateTime::from_timestamp_millis)
        .unwrap_or_default();
    format!("{}-{:03}", at.format(ID_FORMAT), written_ms % 1000)
}

/// The moment an id names, or `None` when it is not an id (which also keeps a path out of a name).
fn parse_id(id: &str) -> Option<u64> {
    let (date, millis) = id.rsplit_once('-')?;
    if millis.len() != 3 || !millis.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let at = NaiveDateTime::parse_from_str(date, ID_FORMAT).ok()?;
    let base = u64::try_from(at.and_utc().timestamp_millis()).ok()?;
    Some(base + millis.parse::<u64>().ok()?)
}

fn file_name(id: &str) -> String {
    format!("{id}.md")
}

fn id_of_file(name: &str) -> Option<String> {
    let id = name.strip_suffix(".md")?;
    parse_id(id).map(|_| id.to_owned())
}

/// The versions of the notes of one folder.
pub(crate) struct History {
    dir: PathBuf,
}

impl History {
    pub(crate) fn new(notes_root: &Path) -> Self {
        Self {
            dir: crate::session::state_dir(notes_root).join(DIR),
        }
    }

    fn note_dir(&self, note: &str) -> PathBuf {
        self.dir.join(note)
    }

    /// Keeps `bytes`, the text of `note` as it was written at `written_ms`. Nothing is kept when the newest version
    /// already holds exactly these bytes. Returns the version made.
    pub(crate) fn snapshot(
        &self,
        note: &str,
        bytes: &[u8],
        written_ms: u64,
    ) -> io::Result<Option<Version>> {
        let dir = self.note_dir(note);
        if let Some(newest) = self.versions(note).first() {
            if std::fs::read(dir.join(file_name(&newest.id))).is_ok_and(|kept| kept == bytes) {
                return Ok(None);
            }
        }
        // two different texts written in the same millisecond: the later one is a millisecond later
        let mut at = written_ms;
        while dir.join(file_name(&id_of(at))).exists() {
            at += 1;
        }
        let id = id_of(at);
        write_atomic(&dir.join(file_name(&id)), bytes)?;
        Ok(Some(Version {
            id,
            written_ms: at,
            size: bytes.len() as u64,
        }))
    }

    /// The versions of `note`, the newest first.
    pub(crate) fn versions(&self, note: &str) -> Vec<Version> {
        let Ok(entries) = std::fs::read_dir(self.note_dir(note)) else {
            return Vec::new();
        };
        let mut found: Vec<Version> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let id = id_of_file(&entry.file_name().to_string_lossy())?;
                let size = entry
                    .metadata()
                    .ok()
                    .filter(std::fs::Metadata::is_file)?
                    .len();
                Some(Version {
                    written_ms: parse_id(&id)?,
                    id,
                    size,
                })
            })
            .collect();
        found.sort_by(|a, b| {
            b.written_ms
                .cmp(&a.written_ms)
                .then_with(|| b.id.cmp(&a.id))
        });
        found
    }

    /// The text of a version. An id that is not one of a version is refused.
    pub(crate) fn read(&self, note: &str, id: &str) -> io::Result<Vec<u8>> {
        if parse_id(id).is_none() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a version"));
        }
        std::fs::read(self.note_dir(note).join(file_name(id)))
    }

    /// Removes the versions of `note` that are older than [`KEEP_MS`] at `now_ms`, keeping the newest [`KEEP_LAST`].
    pub(crate) fn prune(&self, note: &str, now_ms: u64) {
        let dir = self.note_dir(note);
        for version in self.versions(note).iter().skip(KEEP_LAST) {
            if now_ms.saturating_sub(version.written_ms) > KEEP_MS {
                let _ = std::fs::remove_file(dir.join(file_name(&version.id)));
            }
        }
    }

    /// Prunes every note's versions. The versions of a note that no longer exists go when they are all older than
    /// [`KEEP_MS`].
    pub(crate) fn prune_all(&self, existing: &BTreeSet<String>, now_ms: u64) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let note = entry.file_name().to_string_lossy().into_owned();
            if existing.contains(&note) {
                self.prune(&note, now_ms);
                continue;
            }
            let dir = entry.path();
            let mut left = 0;
            for version in self.versions(&note) {
                if now_ms.saturating_sub(version.written_ms) > KEEP_MS {
                    let _ = std::fs::remove_file(dir.join(file_name(&version.id)));
                } else {
                    left += 1;
                }
            }
            if left == 0 {
                // only an empty folder is removed; anything else in it is not ours to delete
                let _ = std::fs::remove_dir(&dir);
            }
        }
    }

    /// The versions follow a note that was renamed. When the name it takes has versions of its own (an older note of that name
    /// was deleted), the two are joined: a version at the very moment of one already there is put a millisecond later, so that
    /// none is lost and none is left behind.
    pub(crate) fn rename(&self, from: &str, to: &str) {
        let (from, to) = (self.note_dir(from), self.note_dir(to));
        if !from.is_dir() {
            return;
        }
        if !to.exists() {
            let _ = std::fs::rename(&from, &to);
            return;
        }
        let Ok(entries) = std::fs::read_dir(&from) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let Some(mut moment) = id_of_file(&entry.file_name().to_string_lossy())
                .as_deref()
                .and_then(parse_id)
            else {
                continue;
            };
            while to.join(file_name(&id_of(moment))).exists() {
                moment += 1;
            }
            let _ = std::fs::rename(entry.path(), to.join(file_name(&id_of(moment))));
        }
        // only an empty folder is removed; anything else in it is not ours to delete
        let _ = std::fs::remove_dir(&from);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60 * 1000;
    const DAY: u64 = 24 * 60 * MIN;
    /// 2026-10-15 14:32:00.123 UTC.
    const T0: u64 = 1_792_074_720_123;

    #[test]
    fn the_first_save_after_opening_takes_a_version_then_one_every_ten_minutes() {
        let mut cadence = Cadence::default();
        assert!(
            cadence.due("a.md", T0),
            "nothing taken since the note was opened"
        );
        cadence.taken("a.md", T0);
        assert!(!cadence.due("a.md", T0 + 3 * 1000));
        assert!(!cadence.due("a.md", T0 + 10 * MIN - 1));
        assert!(cadence.due("a.md", T0 + 10 * MIN));
        // another note has its own clock
        assert!(cadence.due("b.md", T0 + 1));
        // opening it again starts over
        cadence.opened("a.md");
        assert!(cadence.due("a.md", T0 + 1));
        cadence.taken("a.md", T0);
        cadence.renamed("a.md", "c.md");
        assert!(cadence.due("a.md", T0 + 1) && !cadence.due("c.md", T0 + 1));
    }

    #[test]
    fn the_versions_of_a_renamed_note_follow_it_and_join_those_of_the_name_it_takes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let history = History::new(dir.path());
        history.snapshot("a.md", b"a one", T0).expect("snapshot");
        history
            .snapshot("a.md", b"a two", T0 + MIN)
            .expect("snapshot");
        // a name that is free: the folder moves
        history.rename("a.md", "b.md");
        assert_eq!(history.versions("a.md"), Vec::<Version>::new());
        assert_eq!(history.versions("b.md").len(), 2);

        // a name that has versions of its own (an older note of that name was deleted): nothing is left behind or lost, and a
        // version at the very moment of another is a millisecond later
        history.snapshot("c.md", b"c one", T0).expect("snapshot");
        history
            .snapshot("c.md", b"c three", T0 + 2 * MIN)
            .expect("snapshot");
        history.rename("b.md", "c.md");
        assert_eq!(history.versions("b.md"), Vec::<Version>::new());
        assert!(!dir
            .path()
            .join(".permark")
            .join("history")
            .join("b.md")
            .exists());
        let kept: Vec<Vec<u8>> = history
            .versions("c.md")
            .iter()
            .map(|v| history.read("c.md", &v.id).expect("read"))
            .collect();
        assert_eq!(
            kept,
            [
                b"c three".to_vec(),
                b"a two".to_vec(),
                // the two written at the same moment: the one that was already there keeps the moment
                b"a one".to_vec(),
                b"c one".to_vec()
            ]
        );
    }

    #[test]
    fn a_version_is_named_after_the_moment_its_text_was_written_and_the_name_reads_back() {
        let id = id_of(T0);
        assert_eq!(id, "20261015-143200-123");
        assert_eq!(parse_id(&id), Some(T0));
        for bad in [
            "",
            "x",
            "20261015-143200",
            "20261015-143200-12",
            "2026-10-15-143200-123",
            "../20261015-143200-123",
            "20261015-143200-12a",
            "20261315-143200-123",
        ] {
            assert_eq!(parse_id(bad), None, "{bad}");
        }
    }

    #[test]
    fn versions_are_listed_newest_first_and_read_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        let history = History::new(dir.path());
        assert_eq!(history.versions("a.md"), Vec::<Version>::new());
        history.snapshot("a.md", b"one", T0).expect("snapshot");
        history
            .snapshot("a.md", b"three", T0 + 2 * MIN)
            .expect("snapshot");
        history
            .snapshot("a.md", b"two", T0 + MIN)
            .expect("snapshot");
        let versions = history.versions("a.md");
        assert_eq!(
            versions.iter().map(|v| v.written_ms).collect::<Vec<_>>(),
            [T0 + 2 * MIN, T0 + MIN, T0]
        );
        assert_eq!(versions[0].size, 5);
        assert_eq!(
            history.read("a.md", &versions[0].id).expect("read"),
            b"three"
        );
        assert!(history.read("a.md", "../../notes").is_err());
        assert!(history.read("a.md", "20250101-000000-000").is_err());
        assert_eq!(history.versions("other.md"), Vec::<Version>::new());
    }

    #[test]
    fn the_same_text_twice_and_the_same_millisecond_twice_are_handled() {
        let dir = tempfile::tempdir().expect("tempdir");
        let history = History::new(dir.path());
        assert!(history
            .snapshot("a.md", b"text", T0)
            .expect("one")
            .is_some());
        assert!(history
            .snapshot("a.md", b"text", T0 + MIN)
            .expect("same")
            .is_none());
        let other = history
            .snapshot("a.md", b"other", T0)
            .expect("clash")
            .expect("kept");
        assert_eq!(other.written_ms, T0 + 1);
        assert_eq!(history.versions("a.md").len(), 2);
    }

    #[test]
    fn old_versions_go_but_the_last_twenty_stay() {
        let dir = tempfile::tempdir().expect("tempdir");
        let history = History::new(dir.path());
        for n in 0..30 {
            history
                .snapshot("a.md", format!("v{n}").as_bytes(), T0 + n * MIN)
                .expect("snapshot");
        }
        // all thirty are 100 days old: only the newest twenty stay
        history.prune("a.md", T0 + 100 * DAY);
        let kept = history.versions("a.md");
        assert_eq!(kept.len(), KEEP_LAST);
        assert_eq!(kept[0].written_ms, T0 + 29 * MIN);
        assert_eq!(kept[KEEP_LAST - 1].written_ms, T0 + 10 * MIN);
    }

    #[test]
    fn a_young_version_stays_beyond_the_twenty_and_an_old_one_among_them_stays_too() {
        let dir = tempfile::tempdir().expect("tempdir");
        let history = History::new(dir.path());
        for n in 0..25 {
            history
                .snapshot("a.md", format!("v{n}").as_bytes(), T0 + n * DAY / 2)
                .expect("snapshot");
        }
        // 12 days of versions, all young: nothing goes
        history.prune("a.md", T0 + 13 * DAY);
        assert_eq!(history.versions("a.md").len(), 25);
        // 31 days in: the oldest ones are past 30 days, but the newest twenty stay anyway
        history.prune("a.md", T0 + 40 * DAY);
        let kept = history.versions("a.md");
        assert_eq!(kept.len(), KEEP_LAST);
        // exactly 30 days is still kept
        let edge = tempfile::tempdir().expect("tempdir");
        let history = History::new(edge.path());
        for n in 0..22 {
            history
                .snapshot("b.md", format!("v{n}").as_bytes(), T0 + n * MIN)
                .expect("snapshot");
        }
        history.prune("b.md", T0 + KEEP_MS);
        assert_eq!(
            history.versions("b.md").len(),
            22,
            "the two oldest are exactly 30 days old"
        );
        history.prune("b.md", T0 + KEEP_MS + 2 * MIN);
        assert_eq!(history.versions("b.md").len(), 20);
    }

    #[test]
    fn the_versions_of_a_note_that_is_gone_are_kept_for_thirty_days_and_then_removed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let history = History::new(dir.path());
        history.snapshot("kept.md", b"k", T0).expect("snapshot");
        history.snapshot("gone.md", b"g", T0).expect("snapshot");
        let existing: BTreeSet<String> = ["kept.md".to_owned()].into_iter().collect();
        history.prune_all(&existing, T0 + 10 * DAY);
        assert_eq!(history.versions("gone.md").len(), 1);
        history.prune_all(&existing, T0 + 31 * DAY);
        assert_eq!(history.versions("gone.md"), Vec::<Version>::new());
        assert!(!history.note_dir("gone.md").exists());
        assert_eq!(
            history.versions("kept.md").len(),
            1,
            "a note that exists keeps its newest versions"
        );
    }

    #[test]
    fn the_versions_follow_a_renamed_note() {
        let dir = tempfile::tempdir().expect("tempdir");
        let history = History::new(dir.path());
        history.snapshot("a.md", b"x", T0).expect("snapshot");
        history.rename("a.md", "b.md");
        assert_eq!(history.versions("a.md"), Vec::<Version>::new());
        assert_eq!(history.versions("b.md").len(), 1);
    }
}
