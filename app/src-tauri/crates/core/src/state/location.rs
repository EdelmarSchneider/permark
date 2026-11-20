//! Where a project's private data sits (the person's profile | the project's `.permark/local/`), chosen once per
//! folder and kept in the profile (`location.json`, a row per folder that stood at the path, bound to its
//! [`FolderIdentity`]): a `.permark/` that arrives, goes or turns into a link later never moves the data by itself.
//! Reading a row never writes; a row is written when a folder is opened and none answers for it yet ([`settle`]), and
//! when the person initializes PerMark in it or keeps its data on this computer ([`keep`]). What the folder's window
//! offers about the place ([`offer`]) is read from the row and what each place holds.

use std::fs::Metadata;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use super::{
    private, AppDirs, Parsed, RawFile, StateData, StateFile, StateLocation, StateStore,
    PROFILE_ONLY, PROFILE_PLUGINS, SHARED_FILES,
};
use crate::fsx::FolderIdentity;
use crate::paths::Root;

/// Name of the row file in the project's profile folder.
pub const FILE: &str = "location.json";
/// `detail` of the `conflict` refusal of an Initialize while the profile and the project's `local/` both hold
/// PerMark data: nothing is carried, so neither copy hides the other.
pub const BOTH_HOLD_DATA: &str = "bothHoldData";
/// This computer's own folder in a state folder.
const MACHINE: &str = super::Machine::DIR;
/// Most rows kept: one per folder that stood at the path (a roaming profile, a dual boot, a clone made again), the
/// oldest dropped first.
const MAX_ROWS: usize = 8;

/// What counts as a link for a store's place: std's test, or the one of earlier versions (any reparse point).
pub(super) type Link = fn(&Metadata) -> bool;

/// Where the folder's rebuildable caches go, decided with its row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum CachePlace {
    /// The project's `.permark/cache/`.
    Project,
    /// The app's cache folder on this computer.
    #[default]
    App,
}

/// One folder that stood at the path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct Row {
    pub(super) place: StateLocation,
    /// `None`: the volume gave no identity when the row was written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) folder: Option<FolderIdentity>,
    /// The person chose the place (not only the first open).
    #[serde(default)]
    pub(super) decided: bool,
    /// When the row was written, ms since the UNIX epoch.
    #[serde(default)]
    pub(super) at: u64,
    #[serde(default)]
    pub(super) cache: CachePlace,
    /// The data was moved into the project: the profile's copy is the older one.
    #[serde(default)]
    pub(super) moved: bool,
    /// Keys a later version added: kept as they are when this version writes the file.
    #[serde(flatten)]
    pub(super) rest: Map<String, Value>,
}

/// `location.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Rows {
    #[serde(default)]
    pub(super) rows: Vec<Row>,
    #[serde(flatten)]
    pub(super) rest: Map<String, Value>,
}

// fields a later version adds are optional with a default: files keep loading, so the format version stays
impl StateData for Rows {
    const VERSION: u32 = 1;
    const MAX_BYTES: u64 = 16 * 1024;
}

/// What the window of an opened folder offers about the place of the person's data ([`offer`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DataOffer {
    /// Nothing to answer or know.
    #[default]
    None,
    /// The data is in the profile, not chosen yet, and a team folder whose `local/` holds nothing arrived: Move | Keep.
    Ask,
    /// As `Ask`, the profile chosen by the person: Move stays possible, nothing is asked.
    Movable,
    /// The row names the project, whose `local/` cannot be used now: the profile's copy is in use (it may be older).
    Missing,
    /// The profile and the project's `local/` both hold PerMark data: Move would be refused, so neither copy hides the
    /// other.
    Both,
}

/// Where a row puts the folder's private data and its caches (a row's answer, or the one a new row takes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Placement {
    pub(super) place: StateLocation,
    pub(super) cache: CachePlace,
}

impl Row {
    fn placement(&self) -> Placement {
        Placement {
            place: self.place,
            cache: self.cache,
        }
    }
}

impl Rows {
    /// The row that answers for the folder whose identity is `current`: the one bound to it, else one written while
    /// the volume gave none; `current` unknown -> the newest row.
    pub(super) fn matching(&self, current: Option<FolderIdentity>) -> Option<&Row> {
        self.matching_index(current).map(|i| &self.rows[i])
    }

    fn matching_index(&self, current: Option<FolderIdentity>) -> Option<usize> {
        match current {
            Some(now) => self
                .rows
                .iter()
                .position(|row| row.folder.is_some_and(|then| then.same(now)))
                .or_else(|| self.rows.iter().position(|row| row.folder.is_none())),
            None => self.newest(),
        }
    }

    /// The data was moved into the project from the profile (the newest row names the project and says so): the
    /// profile's copy is the older one, never the person's current data.
    fn profile_copy_older(&self) -> bool {
        self.newest().is_some_and(|index| {
            self.rows[index].place == StateLocation::Project && self.rows[index].moved
        })
    }

    fn newest(&self) -> Option<usize> {
        self.rows
            .iter()
            .enumerate()
            .max_by_key(|(_, row)| row.at)
            .map(|(i, _)| i)
    }

    /// What a first open writes (`None`: a row already answers for `current`, nothing to write).
    pub(super) fn settled(
        disk: Option<&Self>,
        current: Option<FolderIdentity>,
        new: Placement,
        now: u64,
    ) -> Option<Self> {
        let mut rows = disk.cloned().unwrap_or_default();
        if rows.matching_index(current).is_some() {
            return None;
        }
        rows.answering(current, new, now);
        Some(rows)
    }

    /// The index of the row that answers for `current`, made when none does: the newest row when it names the same
    /// place on the same volume (a folder whose id changed: made again by a sync tool, restored, on a volume whose ids
    /// do not last) takes the new identity, so such a folder writes once, not a row per open; else a row of its own
    /// (a folder of another computer in a roaming profile keeps its row). The person's choice of the newest row, and
    /// its mark of an older profile copy, are carried when the place is the same.
    fn answering(&mut self, current: Option<FolderIdentity>, new: Placement, now: u64) -> usize {
        if let Some(index) = self.matching_index(current) {
            return index;
        }
        let same_place = self
            .newest()
            .filter(|&index| self.rows[index].place == new.place);
        let decided = same_place.is_some_and(|index| self.rows[index].decided);
        let moved = same_place.is_some_and(|index| self.rows[index].moved);
        let same_volume = same_place.filter(|&index| {
            self.rows[index]
                .folder
                .zip(current)
                .is_some_and(|(then, now)| then.volume == now.volume)
        });
        if let Some(index) = same_volume {
            let row = &mut self.rows[index];
            row.folder = current;
            row.at = now;
            row.cache = new.cache;
            return index;
        }
        self.rows.push(Row {
            place: new.place,
            folder: current,
            decided,
            at: now,
            cache: new.cache,
            moved,
            rest: Map::new(),
        });
        // the oldest goes first, never the row just written (a clock set back would make it the oldest)
        while self.rows.len() > MAX_ROWS {
            let last = self.rows.len() - 1;
            let oldest = self.rows[..last]
                .iter()
                .enumerate()
                .min_by_key(|(_, row)| row.at)
                .map_or(0, |(index, _)| index);
            self.rows.remove(oldest);
        }
        self.rows.len() - 1
    }
}

/// The rows on disk; `None` when there are none this version reads (no file, a newer version's, a corrupt one, one it
/// must not read): read as a plain profile file, never moved aside or stamped by a reader.
fn read(path: &Path) -> Option<Rows> {
    let bytes = RawFile::private(path.to_path_buf(), None)
        .capped(Rows::MAX_BYTES)
        .read()
        .ok()??;
    match StateFile::<Rows>::parse(&bytes) {
        Parsed::Data(rows) => Some(rows),
        Parsed::Newer | Parsed::Bad => None,
    }
}

/// Where the row of the folder at `root` puts its data and caches (`None`: no row answers for it). One small profile
/// file; the folder's identity is read only when the file holds rows.
pub(super) fn place_of(profile_dir: &Path, root: &Root) -> Option<Placement> {
    let rows = read(&profile_dir.join(FILE))?;
    rows.matching(FolderIdentity::of(root.path()))
        .map(Row::placement)
}

/// The store of a folder being opened (a project window, a project included in one): when no row answers for it
/// yet (its first open in this version, or another folder now stands at the path), one is written from what the
/// folder and the profile hold ([`derive`]); then the store every reader of the folder gets
/// ([`StateStore::for_project`]). A row that cannot be written (a read-only profile, a newer version's file) leaves
/// the answer every reader gets without one. Never writes into the folder.
#[must_use]
pub fn settle(dirs: &AppDirs, root: &Root) -> StateStore {
    settle_as(dirs, root, crate::tree::is_link_meta)
}

/// [`settle`], `legacy_link` the earlier versions' link test (tests stand a cloud folder in with it).
pub(crate) fn settle_as(dirs: &AppDirs, root: &Root, legacy_link: Link) -> StateStore {
    let file = row_file(dirs, root);
    let current = FolderIdentity::of(root.path());
    let rows = read(file.path());
    let answered = rows
        .as_ref()
        .is_some_and(|rows| rows.matching(current).is_some());
    if !answered {
        let new = derive(dirs, root, legacy_link, rows.as_ref());
        // best effort: a row that cannot be written leaves the answer of a folder without one, as every reader sees
        let _ = file.update(|disk| Rows::settled(disk, current, new, now_ms()));
    }
    StateStore::for_project_as(dirs, root, legacy_link)
}

/// The store [`settle`] would give the folder now, without writing a row: the place of the person's data before an
/// Initialize, also in a folder no window opened in this version (the earlier rule alone would take a `.permark/`
/// that arrived for the place and leave the profile's data unseen).
pub(super) fn answer(dirs: &AppDirs, root: &Root, legacy_link: Link) -> StateStore {
    let rows = read(row_file(dirs, root).path());
    let placed = rows
        .as_ref()
        .and_then(|rows| rows.matching(FolderIdentity::of(root.path())))
        .map_or_else(
            || derive(dirs, root, legacy_link, rows.as_ref()),
            Row::placement,
        );
    StateStore::at_place(dirs, root, legacy_link, Some(placed))
}

/// Writes the folder's row: its data at `place`, `decided` by the person (a row for another folder at the path is
/// taken over or added as [`settle`] does). `moved`: the profile's data was just carried into the project, so the
/// profile's copy is the older one from now on (kept while the data stays in the project; the profile as the place
/// is the current copy again). Writes nothing when the row already says so.
///
/// # Errors
/// The row file cannot be written (IO, a newer version's file).
pub(super) fn record(
    dirs: &AppDirs,
    root: &Root,
    place: StateLocation,
    decided: bool,
    moved: bool,
) -> Result<(), super::StateError> {
    let current = FolderIdentity::of(root.path());
    let new = Placement {
        place,
        cache: cache_place(root, place, crate::tree::is_link_meta),
    };
    let now = now_ms();
    row_file(dirs, root)
        .update(|disk| {
            let mut rows = disk.cloned().unwrap_or_default();
            let index = rows.answering(current, new, now);
            let row = &mut rows.rows[index];
            row.place = place;
            row.decided = decided;
            row.cache = new.cache;
            row.moved = place == StateLocation::Project && (row.moved || moved);
            Some(rows)
        })
        .map(|_| ())
}

/// Records the profile as the place the person chose for the folder's data (Keep, also of the copy in use while the
/// project's `local/` cannot be used): nothing is written into the folder, a `.permark/` arriving later never takes
/// the data by itself, Move stays possible. Writes nothing when the row already says so.
///
/// # Errors
/// The row file cannot be written (IO, a newer version's file).
pub fn keep(dirs: &AppDirs, root: &Root) -> Result<(), super::StateError> {
    record(dirs, root, StateLocation::Profile, true, false)
}

/// What the window of the folder `store` was made for offers about the place of the person's data, from the folder's
/// row and what each place holds: one small profile file and at most two folder listings, so it is worked out when
/// the folder opens or its state is read again, never per view. No row this version reads (the app-wide store, a
/// profile it cannot write): nothing.
#[must_use]
pub fn offer(store: &StateStore) -> DataOffer {
    let Some(root) = store.root.as_ref() else {
        return DataOffer::None;
    };
    let row = read(&store.profile_dir.join(FILE)).and_then(|rows| {
        rows.matching(FolderIdentity::of(root.path()))
            .map(|row| (row.place, row.decided))
    });
    match row {
        Some((StateLocation::Project, _)) if store.location == StateLocation::Profile => {
            DataOffer::Missing
        }
        // no row this version reads, or the data in the project as its row says
        None | Some((StateLocation::Project, _)) => DataOffer::None,
        Some((StateLocation::Profile, decided)) => {
            let local = root.path().join(super::PERMARK_DIR).join("local");
            // as Initialize takes them: a team folder whose `local/` is real or missing, else Move cannot run
            if !store.has_team_folder() || matches!(private::entry(&local), private::Entry::Other) {
                DataOffer::None
            } else if !local_holds_data(&local) {
                if decided {
                    DataOffer::Movable
                } else {
                    DataOffer::Ask
                }
            } else if profile_holds_data(&store.profile_dir) {
                DataOffer::Both
            } else {
                DataOffer::None
            }
        }
    }
}

/// The row file of the folder, in its profile folder.
fn row_file(dirs: &AppDirs, root: &Root) -> StateFile<Rows> {
    StateStore::profile_only(dirs, root).profile_file(FILE)
}

/// The place for a folder no row answers for (`rows`: the rows of other folders at the path). The profile unless the
/// project's `local/` can hold the data (a real `.permark/` and `local/` real or missing) and nothing the person has
/// would drop out of view: the profile holds none of the person's current data (none, or only the older copy left
/// when the data moved into a folder at the path), or earlier versions already kept the data in `local/` and it
/// holds some.
fn derive(dirs: &AppDirs, root: &Root, legacy_link: Link, rows: Option<&Rows>) -> Placement {
    let earlier = StateStore::at_place(dirs, root, legacy_link, None);
    let local = root.path().join(super::PERMARK_DIR).join("local");
    let usable =
        earlier.has_team_folder() && !matches!(private::entry(&local), private::Entry::Other);
    let older = rows.is_some_and(Rows::profile_copy_older);
    let in_profile = || !older && profile_holds_data(&earlier.profile_dir);
    let place = if usable
        && ((earlier.location() == StateLocation::Project && local_holds_data(&local))
            || !in_profile())
    {
        StateLocation::Project
    } else {
        StateLocation::Profile
    };
    Placement {
        place,
        cache: cache_place(root, place, legacy_link),
    }
}

/// Where the caches of a folder whose data is at `place` go: the project's `.permark/cache/` only with the data in
/// the project, on a local disk, in a `.permark/` no cloud service keeps as a placeholder (earlier versions' link
/// test calls it a link, std a folder); else this computer's app cache.
fn cache_place(root: &Root, place: StateLocation, legacy_link: Link) -> CachePlace {
    let permark = root.path().join(super::PERMARK_DIR);
    if place == StateLocation::Project
        && crate::fsx::is_local_path(root.path())
        && matches!(
            private::entry_as(&permark, legacy_link),
            private::Entry::Folder
        )
    {
        CachePlace::Project
    } else {
        CachePlace::App
    }
}

/// Whether the project's profile folder holds the person's data: anything but the files that always stay there, the
/// profile copy of the team files, this computer's own folder and the plugin folders kept in the profile on purpose.
/// Unreadable: held (the profile's data is never assumed away).
pub(super) fn profile_holds_data(dir: &Path) -> bool {
    holds(dir, true, |name, path| {
        if PROFILE_ONLY.contains(&name) || SHARED_FILES.contains(&name) || name == MACHINE {
            return false;
        }
        name != "plugins" || holds(path, true, |plugin, _| !PROFILE_PLUGINS.contains(&plugin))
    })
}

/// Whether the project's `local/` holds the person's data: anything but PerMark's own files there (its ignore file,
/// this computer's folder, the agent context, profile-only files an earlier version copied). Unreadable: none (the
/// profile's data is never hidden behind it).
pub(super) fn local_holds_data(dir: &Path) -> bool {
    holds(dir, false, |name, _| {
        !(PROFILE_ONLY.contains(&name)
            || name == ".gitignore"
            || name == MACHINE
            || name == super::context::FILE)
    })
}

/// Whether `dir` lists an entry that `counts` (name, path); a missing folder holds none, an unreadable one
/// `unreadable`.
fn holds(dir: &Path, unreadable: bool, counts: impl Fn(&str, &Path) -> bool) -> bool {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .any(|entry| counts(&entry.file_name().to_string_lossy(), &entry.path())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => unreadable,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
#[path = "location_tests.rs"]
mod tests;
