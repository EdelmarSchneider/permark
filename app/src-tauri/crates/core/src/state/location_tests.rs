use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::*;
use crate::state::StateLocation::{Profile, Project};
use crate::state::{init_project, ProjectId, PROFILE_ONLY};

struct Fx {
    _base: tempfile::TempDir,
    dirs: AppDirs,
    project: tempfile::TempDir,
}

fn fx() -> Fx {
    let base = tempfile::tempdir().unwrap();
    let dirs = AppDirs::under(base.path());
    Fx {
        _base: base,
        dirs,
        project: tempfile::tempdir().unwrap(),
    }
}

/// Where a store puts the data, and whether the folder has a team folder.
type Answer = (StateLocation, PathBuf, PathBuf, bool);

fn answer(store: &StateStore) -> Answer {
    (
        store.location(),
        store.dir().to_path_buf(),
        store.cache_dir().to_path_buf(),
        store.has_team_folder(),
    )
}

/// Every entry below `dir`, relative, `/`-separated, folders ending in `/`, sorted.
fn listing(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let rel = path
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if entry.file_type().unwrap().is_dir() {
                out.push(format!("{rel}/"));
                walk(base, &path, out);
            } else {
                out.push(rel);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

impl Fx {
    fn root(&self) -> Root {
        Root::open(self.project.path()).unwrap()
    }
    fn profile(&self) -> PathBuf {
        self.dirs
            .profile
            .join("projects")
            .join(ProjectId::of(&self.root()).as_str())
    }
    fn row_file(&self) -> PathBuf {
        self.profile().join(FILE)
    }
    fn row_bytes(&self) -> Option<Vec<u8>> {
        fs::read(self.row_file()).ok()
    }
    fn rows(&self) -> Vec<Row> {
        read(&self.row_file())
            .map(|rows| rows.rows)
            .unwrap_or_default()
    }
    fn permark(&self) -> PathBuf {
        self.project.path().join(".permark")
    }
    fn local(&self) -> PathBuf {
        self.permark().join("local")
    }
    /// The person's data in the profile.
    fn profile_state(&self) {
        fs::create_dir_all(self.profile().join("plugins").join("tags")).unwrap();
        fs::write(self.profile().join("session.json"), b"profile's").unwrap();
        fs::write(
            self.profile().join("plugins").join("tags").join("t.json"),
            b"profile's",
        )
        .unwrap();
    }
    fn team_folder(&self) {
        fs::create_dir_all(self.permark()).unwrap();
        fs::write(self.permark().join("config.toml"), b"# team\n").unwrap();
    }
    fn local_state(&self) {
        fs::create_dir_all(self.local()).unwrap();
        fs::write(self.local().join("session.json"), b"folder's").unwrap();
        fs::write(self.local().join("comments.json"), b"folder's").unwrap();
    }
    fn store(&self) -> StateStore {
        StateStore::for_project(&self.dirs, &self.root())
    }
    fn settle(&self) -> StateStore {
        settle(&self.dirs, &self.root())
    }
    /// A row written by hand: `{ place, folder?, decided }`.
    fn write_row(&self, place: &str, folder: Option<FolderIdentity>, decided: bool) {
        let mut row =
            json!({ "place": place, "decided": decided, "at": 1, "cache": "app", "moved": false });
        if let Some(folder) = folder {
            row["folder"] = serde_json::to_value(folder).unwrap();
        }
        fs::create_dir_all(self.profile()).unwrap();
        fs::write(
            self.row_file(),
            serde_json::to_vec(&json!({ "version": 1, "data": { "rows": [row] } })).unwrap(),
        )
        .unwrap();
    }
    /// Rows written as this version writes them.
    fn write_rows(&self, rows: &Rows) {
        fs::create_dir_all(self.profile()).unwrap();
        let data = serde_json::to_value(rows).unwrap();
        fs::write(
            self.row_file(),
            serde_json::to_vec(&json!({ "version": 1, "data": data })).unwrap(),
        )
        .unwrap();
    }
}

/// Resolving a store writes nothing (no row, no folder, nothing in the project), and a folder without a row keeps
/// the place earlier versions gave it.
#[test]
fn resolving_writes_nothing_and_a_folder_without_a_row_keeps_the_earlier_place() {
    type Make = fn(&Fx);
    let layouts: [(&str, Make, StateLocation); 3] = [
        ("no .permark", |_| {}, Profile),
        (".permark", Fx::team_folder, Project),
        (
            ".permark with local state",
            |f| {
                f.team_folder();
                f.local_state();
            },
            Project,
        ),
    ];
    for (name, make, place) in layouts {
        let f = fx();
        make(&f);
        let before = listing(f.project.path());
        assert_eq!(f.store().location(), place, "{name}");
        assert_eq!(listing(f.project.path()), before, "{name}");
        assert!(!f.dirs.profile.exists(), "{name}: nothing in the profile");
    }
}

/// A row decides the place: the profile stays the place while a `.permark/` arrives (with data in `local/` or not),
/// turns into a file or goes; the project's `local/` is the place only while it can hold the data (else the profile
/// for now, the row untouched). Nothing private lands in a `.permark/` that arrived.
#[test]
fn a_row_decides_the_place_and_a_permark_arriving_or_going_never_changes_it() {
    let f = fx();
    f.write_row("profile", None, false);
    let row = f.row_bytes();
    let first = answer(&f.store());
    assert_eq!(first.0, Profile);
    f.team_folder();
    f.local_state();
    let listed = listing(&f.permark());
    let store = f.store();
    assert_eq!(
        answer(&store),
        (Profile, first.1.clone(), first.2.clone(), true)
    );
    store.file::<Rows>("x.json").save(&Rows::default()).unwrap();
    store
        .cache_file::<Rows>("c.json")
        .save(&Rows::default())
        .unwrap();
    assert_eq!(listing(&f.permark()), listed, "nothing private in .permark");
    fs::remove_dir_all(f.permark()).unwrap();
    fs::write(f.permark(), "x").unwrap();
    assert_eq!(answer(&f.store()), first);
    fs::remove_file(f.permark()).unwrap();
    assert_eq!(answer(&f.store()), first);
    assert_eq!(f.row_bytes(), row);

    let f = fx();
    f.write_row("project", None, true);
    let row = f.row_bytes();
    f.team_folder();
    let project = answer(&f.store());
    assert_eq!(project.0, Project);
    assert_eq!(project.1, f.local());
    fs::remove_dir_all(f.permark()).unwrap();
    let missing = answer(&f.store());
    assert_eq!((missing.0, missing.1), (Profile, f.profile()));
    fs::write(f.permark(), "x").unwrap();
    assert_eq!(f.store().location(), Profile);
    assert_eq!(f.row_bytes(), row, "the row stays as it was");
}

/// A team folder; the profile holds only files that always stay there, this computer's folder and a plugin folder
/// kept in the profile on purpose.
fn only_what_always_stays_in_the_profile(f: &Fx) {
    f.team_folder();
    fs::create_dir_all(f.profile().join("machine").join("abc")).unwrap();
    fs::write(f.profile().join("machine").join("abc").join("j.json"), b"x").unwrap();
    for name in PROFILE_ONLY {
        fs::write(f.profile().join(name), b"x").unwrap();
    }
    fs::remove_file(f.row_file()).unwrap();
    let acp = f.profile().join("plugins").join("acp").join("work");
    fs::create_dir_all(&acp).unwrap();
    fs::write(acp.join("doc.md"), b"# copy").unwrap();
}

/// Data in the profile; `local/` holds only PerMark's own files (its ignore file, this computer's folder, the agent
/// context, profile-only files an earlier version copied).
fn only_permarks_own_files_in_local(f: &Fx) {
    f.profile_state();
    f.team_folder();
    fs::create_dir_all(f.local().join("machine").join("abc")).unwrap();
    fs::write(
        f.local().join(".gitignore"),
        b"*
",
    )
    .unwrap();
    fs::write(f.local().join("machine").join("abc").join("j.json"), b"x").unwrap();
    fs::write(f.local().join(crate::state::context::FILE), b"{}").unwrap();
    for name in PROFILE_ONLY {
        fs::write(f.local().join(name), b"x").unwrap();
    }
}

/// The first open writes the row from what the folder and the profile hold, the team files and the folder left as
/// they are; the second open writes nothing.
#[test]
fn the_first_open_settles_where_the_data_already_is() {
    type Make = fn(&Fx);
    // (case, layout, earlier versions' link test, place)
    let cases: Vec<(&str, Make, Link, StateLocation)> = vec![
        (
            "initialized by an earlier version, older profile copy",
            |f| {
                f.profile_state();
                f.team_folder();
                f.local_state();
            },
            crate::tree::is_link_meta,
            Project,
        ),
        (
            "no .permark",
            Fx::profile_state,
            crate::tree::is_link_meta,
            Profile,
        ),
        (
            ".permark arrived, nothing in local, data in the profile",
            |f| {
                f.profile_state();
                f.team_folder();
            },
            crate::tree::is_link_meta,
            Profile,
        ),
        (
            "cloud folder, data in the profile",
            |f| {
                f.profile_state();
                f.team_folder();
                fs::create_dir_all(f.local()).unwrap();
            },
            |_| true,
            Profile,
        ),
        (
            "cloud folder, nothing in the profile",
            Fx::team_folder,
            |_| true,
            Project,
        ),
        (
            "fresh clone with .permark",
            Fx::team_folder,
            crate::tree::is_link_meta,
            Project,
        ),
        (
            "the profile holds only what always stays there",
            only_what_always_stays_in_the_profile,
            crate::tree::is_link_meta,
            Project,
        ),
        (
            "local holds only PerMark's own files",
            only_permarks_own_files_in_local,
            crate::tree::is_link_meta,
            Profile,
        ),
    ];
    for (name, make, legacy, place) in cases {
        let f = fx();
        fs::create_dir_all(f.profile()).unwrap();
        make(&f);
        let folder = listing(f.project.path());
        let store = settle_as(&f.dirs, &f.root(), legacy);
        assert_eq!(store.location(), place, "{name}");
        let rows = f.rows();
        assert_eq!(rows.len(), 1, "{name}");
        assert_eq!(rows[0].place, place, "{name}");
        assert!(!rows[0].decided, "{name}");
        let identity = FolderIdentity::of(f.root().path());
        assert!(
            rows[0].folder.zip(identity).is_some_and(|(a, b)| a.same(b)),
            "{name}"
        );
        assert_eq!(
            listing(f.project.path()),
            folder,
            "{name}: folder untouched"
        );
        let row = f.row_bytes();
        let again = settle_as(&f.dirs, &f.root(), legacy);
        assert_eq!(f.row_bytes(), row, "{name}: second open writes nothing");
        assert_eq!(answer(&again), answer(&store), "{name}");
        assert_eq!(
            answer(&StateStore::for_project_as(&f.dirs, &f.root(), legacy)),
            answer(&store),
            "{name}: every reader agrees"
        );
    }
}

/// A `.permark/` that arrives after the first open (a pull, a sync tool) leaves the data in the profile: opened again,
/// the store still answers the profile, and the team files stay the only files in `.permark/`.
#[test]
fn a_team_folder_arriving_after_the_first_open_leaves_the_data_where_it_was() {
    let f = fx();
    let first = f.settle();
    assert_eq!(first.location(), Profile);
    first.file::<Rows>("x.json").save(&Rows::default()).unwrap();
    let row = f.row_bytes();
    f.team_folder();
    let store = f.settle();
    assert_eq!(
        answer(&store),
        (
            Profile,
            first.dir().to_path_buf(),
            first.cache_dir().to_path_buf(),
            true
        )
    );
    store.file::<Rows>("y.json").save(&Rows::default()).unwrap();
    assert_eq!(listing(&f.permark()), ["config.toml"]);
    assert_eq!(f.row_bytes(), row);
}

/// Another folder now at the path (a clone made again): settled again; the newest row is taken over when it names
/// the same place on the same volume (no row added: a volume whose ids change at each open writes once).
#[test]
fn another_folder_at_the_path_takes_the_newest_row_over() {
    let f = fx();
    f.profile_state();
    let first = f.settle();
    assert_eq!(first.location(), Profile);
    let before = FolderIdentity::of(f.root().path());
    fs::remove_dir_all(f.project.path()).unwrap();
    fs::create_dir(f.project.path()).unwrap();
    let after = FolderIdentity::of(f.root().path());
    if before.zip(after).is_none_or(|(a, b)| a.same(b)) {
        eprintln!("skipped: the folder made again kept its identity here");
        return;
    }
    f.team_folder();
    assert_eq!(f.settle().location(), Profile);
    let rows = f.rows();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].folder.zip(after).is_some_and(|(a, b)| a.same(b)));
    let row = f.row_bytes();
    f.settle();
    assert_eq!(f.row_bytes(), row);
}

fn id(volume: u64, file: u64) -> FolderIdentity {
    FolderIdentity {
        volume,
        file,
        born: None,
    }
}

fn row(place: StateLocation, folder: Option<FolderIdentity>, decided: bool, at: u64) -> Row {
    Row {
        place,
        folder,
        decided,
        at,
        cache: CachePlace::App,
        moved: false,
        rest: Map::new(),
    }
}

fn rows(list: Vec<Row>) -> Rows {
    Rows {
        rows: list,
        rest: Map::new(),
    }
}

const PROFILE: New = New {
    place: Profile,
    cache: CachePlace::App,
};
const PROJECT: New = New {
    place: Project,
    cache: CachePlace::Project,
};

/// Which row a first open writes: a row per folder identity (a roaming profile alternating two folders writes
/// nothing more), the newest row taken over by an identity that changed on its volume (the choice carried), the
/// oldest row dropped past the cap; an unknown identity is answered by the newest row.
#[test]
fn rows_follow_each_folder_and_carry_the_choice() {
    let a = Some(id(1, 10));
    let b = Some(id(2, 10));
    let first = Rows::settled(None, a, PROFILE, 5).unwrap();
    assert_eq!(first.rows, [row(Profile, a, false, 5)]);
    assert_eq!(Rows::settled(Some(&first), a, PROJECT, 6), None);

    // the same folder, its id changed on the same volume: taken over, the choice kept
    let kept = rows(vec![row(Profile, a, true, 5)]);
    let churned = Rows::settled(Some(&kept), Some(id(1, 11)), PROFILE, 9).unwrap();
    assert_eq!(churned.rows, [row(Profile, Some(id(1, 11)), true, 9)]);
    // another place: a row of its own, nothing chosen yet
    let moved = Rows::settled(Some(&kept), Some(id(1, 11)), PROJECT, 9).unwrap();
    assert_eq!(moved.rows.len(), 2);
    assert_eq!(moved.rows[1].place, Project);
    assert!(!moved.rows[1].decided);
    assert_eq!(moved.rows[1].cache, CachePlace::Project);

    // a roaming profile: one row per computer's folder, the choice carried to the new one
    let roaming = Rows::settled(Some(&kept), b, PROFILE, 9).unwrap();
    assert_eq!(
        roaming.rows,
        [row(Profile, a, true, 5), row(Profile, b, true, 9)]
    );
    assert_eq!(Rows::settled(Some(&roaming), a, PROFILE, 10), None);
    assert_eq!(Rows::settled(Some(&roaming), b, PROFILE, 11), None);

    // the volume gives no id: the newest row answers; a row written so answers any folder without its own
    assert_eq!(roaming.matching(None), Some(&roaming.rows[1]));
    let blind = rows(vec![row(Project, None, false, 3)]);
    assert_eq!(blind.matching(a), Some(&blind.rows[0]));
    assert_eq!(
        Rows::settled(None, None, PROFILE, 4).unwrap().rows,
        [row(Profile, None, false, 4)]
    );

    // past the cap: the oldest goes, never the one just written
    let full = rows(
        (0..8u64)
            .map(|n| row(Project, Some(id(10 + n, 1)), false, 100 + n))
            .collect(),
    );
    let capped = Rows::settled(Some(&full), Some(id(50, 1)), PROFILE, 1).unwrap();
    assert_eq!(capped.rows.len(), 8);
    assert!(capped.rows.iter().all(|r| r.at != 100));
    assert!(capped.matching(Some(id(50, 1))).is_some());
}

/// A row file this version cannot write leaves the answer every reader gets without a row: a newer version's file
/// (kept byte for byte), a folder in its place. A corrupt one is read as none, then moved aside and replaced.
#[test]
fn a_row_file_this_version_cannot_write_leaves_the_answer_every_reader_gets() {
    let newer = br#"{"version":2,"data":{"rows":[{"place":"profile"}]}}"#;
    let f = fx();
    f.profile_state();
    f.team_folder();
    fs::write(f.row_file(), newer).unwrap();
    let store = f.settle();
    assert_eq!(store.location(), Project, "the earlier place");
    assert_eq!(answer(&store), answer(&f.store()));
    assert_eq!(fs::read(f.row_file()).unwrap(), newer);

    let f = fx();
    f.profile_state();
    f.team_folder();
    fs::create_dir_all(f.row_file()).unwrap();
    assert_eq!(answer(&f.settle()), answer(&f.store()));
    assert!(f.row_file().is_dir());

    let f = fx();
    f.profile_state();
    f.team_folder();
    fs::write(f.row_file(), b"not json").unwrap();
    assert_eq!(f.store().location(), Project, "read as no row");
    assert_eq!(f.settle().location(), Profile);
    assert_eq!(f.rows().len(), 1);
    let aside = fs::read_dir(f.profile())
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("location.json.broken-")
        })
        .count();
    assert_eq!(aside, 1);
    assert_eq!(f.store().location(), Profile);
}

/// Keys a later version added to the file stay when this version writes it.
#[test]
fn keys_a_later_version_added_survive_a_rewrite() {
    let f = fx();
    fs::create_dir_all(f.profile()).unwrap();
    let other = json!({ "volume": 7, "file": 7 });
    fs::write(
        f.row_file(),
        serde_json::to_vec(&json!({
            "version": 1,
            "data": {
                "rows": [{ "place": "project", "folder": other, "at": 1, "later": [1, 2] }],
                "note": "kept"
            }
        }))
        .unwrap(),
    )
    .unwrap();
    f.settle();
    let written: serde_json::Value =
        serde_json::from_slice(&fs::read(f.row_file()).unwrap()).unwrap();
    assert_eq!(written["version"], 1);
    assert_eq!(written["data"]["note"], "kept");
    assert_eq!(written["data"]["rows"][0]["later"], json!([1, 2]));
    assert_eq!(written["data"]["rows"].as_array().unwrap().len(), 2);
}

/// Initialize in a folder no window opened in this version: the data a first open would keep in the profile is
/// carried into `.permark/local/` (a `.permark/` already there never hides it), and the row names the project, as
/// the person chose; a second Initialize writes nothing more.
#[test]
fn initialize_carries_the_data_a_first_open_would_keep_in_the_profile() {
    let f = fx();
    f.profile_state();
    f.team_folder();
    let store = init_project(&f.dirs, &f.root()).unwrap();
    assert_eq!(store.location(), Project);
    assert_eq!(
        fs::read(f.local().join("session.json")).unwrap(),
        b"profile's"
    );
    assert_eq!(
        fs::read(f.local().join("plugins").join("tags").join("t.json")).unwrap(),
        b"profile's"
    );
    let rows = f.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].place, rows[0].decided), (Project, true));
    assert_eq!(
        fs::read(f.permark().join("config.toml")).unwrap(),
        b"# team\n"
    );
    let row = f.row_bytes();
    init_project(&f.dirs, &f.root()).unwrap();
    assert_eq!(f.row_bytes(), row);
    assert_eq!(f.settle().location(), Project);
}

/// Initialize after the folder was opened without `.permark/`: the data moves in and the row follows.
#[test]
fn initialize_after_an_open_moves_the_data_in_and_the_row_follows() {
    let f = fx();
    f.profile_state();
    assert_eq!(f.settle().location(), Profile);
    let store = init_project(&f.dirs, &f.root()).unwrap();
    assert_eq!(store.location(), Project);
    assert_eq!(answer(&f.store()), answer(&store));
    assert_eq!(
        fs::read(f.local().join("session.json")).unwrap(),
        b"profile's"
    );
    let rows = f.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].place, rows[0].decided), (Project, true));
}

/// Data on both sides (the profile and a `local/` that arrived, or a cloud folder's stale `local/` an earlier version
/// wrote): Initialize is refused before it makes anything, so the folder's files never hide the person's data, and
/// nothing is written anywhere.
#[test]
fn initialize_refuses_when_both_places_hold_data_and_writes_nothing() {
    type Make = fn(&Fx);
    // (case, how the folder got its row)
    let cases: [(&str, Make); 2] = [
        ("a local folder arrived after the first open", |f| {
            f.profile_state();
            assert_eq!(f.settle().location(), Profile);
            f.team_folder();
            f.local_state();
        }),
        ("a cloud folder with a stale local folder", |f| {
            f.profile_state();
            f.team_folder();
            f.local_state();
            let cloud: Link = |_| true;
            assert_eq!(settle_as(&f.dirs, &f.root(), cloud).location(), Profile);
        }),
    ];
    for (name, make) in cases {
        let f = fx();
        make(&f);
        let folder = listing(f.project.path());
        let profile = listing(&f.profile());
        let row = f.row_bytes();
        let Err(error) = init_project(&f.dirs, &f.root()) else {
            panic!("{name}: not refused");
        };
        let error = crate::IpcError::from(error);
        assert_eq!(error.kind, crate::ErrorKind::Conflict, "{name}");
        assert_eq!(error.detail.as_deref(), Some(BOTH_HOLD_DATA), "{name}");
        assert_eq!(
            listing(f.project.path()),
            folder,
            "{name}: folder untouched"
        );
        assert_eq!(listing(&f.profile()), profile, "{name}: profile untouched");
        assert_eq!(f.row_bytes(), row, "{name}: row untouched");
        assert_eq!(f.store().location(), Profile, "{name}");
        assert_eq!(
            fs::read(f.local().join("session.json")).unwrap(),
            b"folder's",
            "{name}"
        );
    }
}

/// A `.permark/` with only team files arriving after the first open leaves the data in the profile; Initialize then
/// moves it in: the data in `local/`, the files that always stay in the profile and the team files left out, the
/// profile copy kept byte for byte, the row naming the project; running it again writes nothing.
#[test]
fn a_team_folder_that_arrived_takes_the_data_only_by_initialize() {
    let f = fx();
    f.profile_state();
    // the row file is written by the first open below
    for name in PROFILE_ONLY
        .iter()
        .chain(&SHARED_FILES)
        .filter(|name| **name != FILE)
    {
        fs::write(f.profile().join(name), b"profile's own").unwrap();
    }
    assert_eq!(f.settle().location(), Profile);
    f.team_folder();
    let team = listing(&f.permark());
    assert_eq!(f.settle().location(), Profile);
    assert_eq!(listing(&f.permark()), team, "nothing private written");
    let profile: Vec<(String, Vec<u8>)> = listing(&f.profile())
        .into_iter()
        .filter(|name| !name.ends_with('/') && name != FILE)
        .map(|name| {
            let bytes = fs::read(f.profile().join(&name)).unwrap();
            (name, bytes)
        })
        .collect();
    let store = init_project(&f.dirs, &f.root()).unwrap();
    assert_eq!(store.location(), Project);
    assert_eq!(
        listing(&f.local()),
        [
            ".gitignore",
            "plugins/",
            "plugins/tags/",
            "plugins/tags/t.json",
            "session.json"
        ]
    );
    for (name, bytes) in &profile {
        assert_eq!(&fs::read(f.profile().join(name)).unwrap(), bytes, "{name}");
    }
    let rows = f.rows();
    assert_eq!((rows[0].place, rows[0].decided), (Project, true));
    let (row, local) = (f.row_bytes(), listing(&f.local()));
    init_project(&f.dirs, &f.root()).unwrap();
    assert_eq!((f.row_bytes(), listing(&f.local())), (row, local));
}

/// Makes a profile file unreadable while the returned handle lives (a copy of it fails).
#[cfg(windows)]
fn unreadable(path: &Path) -> impl Sized {
    use std::os::windows::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
        .unwrap()
}

/// Makes a profile file unreadable while the returned handle lives (a copy of it fails).
#[cfg(unix)]
fn unreadable(path: &Path) -> impl Sized {
    use std::os::unix::fs::PermissionsExt;
    struct Restore(PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o644));
        }
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
    Restore(path.to_path_buf())
}

/// A copy that fails half way leaves the folder as it was: a `.permark/` the attempt made is gone, a `local/` that
/// was there holds what it held (nothing half copied, no temp left beside it), the profile and the row are untouched;
/// once the file can be read, Initialize runs again and carries everything.
#[test]
fn a_copy_that_fails_leaves_the_folder_as_it_was_and_initialize_runs_again() {
    for team in [false, true] {
        let f = fx();
        f.profile_state();
        assert_eq!(f.settle().location(), Profile);
        if team {
            f.team_folder();
            fs::create_dir_all(f.local().join("machine").join("abc")).unwrap();
            fs::write(
                f.local().join(".gitignore"),
                b"*
",
            )
            .unwrap();
            fs::write(f.local().join("machine").join("abc").join("j.json"), b"x").unwrap();
        }
        let locked = f.profile().join("zz-locked.json");
        fs::write(&locked, b"profile's").unwrap();
        let local = listing(&f.local());
        let profile = listing(&f.profile());
        let row = f.row_bytes();
        {
            let _held = unreadable(&locked);
            if fs::read(&locked).is_ok() {
                eprintln!("skipped: the file stays readable here");
                return;
            }
            assert!(init_project(&f.dirs, &f.root()).is_err(), "team {team}");
        }
        if team {
            assert_eq!(listing(&f.local()), local, "local/ as it was");
            assert_eq!(
                fs::read(f.permark().join("config.toml")).unwrap(),
                b"# team
"
            );
            let temps: Vec<String> = fs::read_dir(f.permark())
                .unwrap()
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| {
                    ![".gitignore", "cache", "config.toml", "local"].contains(&name.as_str())
                })
                .collect();
            assert!(temps.is_empty(), "nothing left beside local/: {temps:?}");
        } else {
            assert!(!f.permark().exists(), "the .permark/ it made is gone");
        }
        assert_eq!(
            listing(&f.profile()),
            profile,
            "team {team}: profile untouched"
        );
        assert_eq!(f.row_bytes(), row, "team {team}: row untouched");
        assert_eq!(f.store().location(), Profile, "team {team}");
        let store = init_project(&f.dirs, &f.root()).unwrap();
        assert_eq!(store.location(), Project, "team {team}");
        for file in ["session.json", "zz-locked.json", "plugins/tags/t.json"] {
            assert_eq!(
                fs::read(f.local().join(file)).unwrap(),
                b"profile's",
                "team {team}: {file}"
            );
        }
    }
}

/// Initialize that carries the profile's data in marks the row: the profile's copy is the older one from then on.
/// Undone (the data carried back), the profile is the place again and the mark goes.
#[test]
fn initialize_marks_the_profile_copy_as_moved_and_undo_names_the_profile_again() {
    let f = fx();
    f.profile_state();
    assert_eq!(f.settle().location(), Profile);
    init_project(&f.dirs, &f.root()).unwrap();
    let rows = f.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].place, rows[0].moved), (Project, true));
    // a second run carries nothing and keeps the mark
    let row = f.row_bytes();
    init_project(&f.dirs, &f.root()).unwrap();
    assert_eq!(f.row_bytes(), row);

    assert!(crate::state::uninit_project(&f.dirs, &f.root()).unwrap());
    let rows = f.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].place, rows[0].moved), (Profile, false));
    // a `.permark/` arriving later (a pull) never takes the data back into the folder by itself
    f.team_folder();
    assert_eq!(f.store().location(), Profile);
    assert_eq!(f.settle().location(), Profile);
}

/// Another folder at the path after the data moved into the project (a clone made again, its `local/` empty): the
/// profile's copy is the older one, so it never counts as the person's current data, and the folder's team folder
/// takes the data, as for a fresh clone. Without the mark the profile's data is current and stays the place.
#[test]
fn a_folder_made_again_after_a_move_never_takes_the_older_profile_copy_for_current() {
    for moved in [true, false] {
        let f = fx();
        f.profile_state();
        f.team_folder();
        let mut then = row(Project, Some(id(u64::MAX, 7)), true, 1);
        then.moved = moved;
        f.write_rows(&rows(vec![then]));
        let place = if moved { Project } else { Profile };
        assert_eq!(f.settle().location(), place, "moved {moved}");
        let now = f.rows();
        assert_eq!(now.len(), 2, "moved {moved}");
        assert_eq!(now[1].moved, moved && place == Project, "moved {moved}");
    }
}

/// The profile-only store reads profile files at the paths every store gives them, whatever the folder holds.
#[test]
fn the_profile_only_store_answers_profile_files_without_looking_at_the_folder() {
    let f = fx();
    f.team_folder();
    f.local_state();
    let store = StateStore::profile_only(&f.dirs, &f.root());
    assert_eq!(store.location(), Profile);
    assert_eq!(
        store.profile_file::<Rows>("agent.json").path(),
        f.store().profile_file::<Rows>("agent.json").path()
    );
    assert!(!f.dirs.profile.exists());
}

/// What the folder's window offers about the place of the data, from the row and what each place holds: Move | Keep
/// while a team folder arrived after the first open and its `local/` holds nothing; Move alone once the person kept
/// the data on this computer; a neutral word when both places hold data (Move would be refused); the profile copy
/// in use while the project's `local/` cannot be used; nothing otherwise.
#[test]
fn the_offer_follows_the_row_and_what_each_place_holds() {
    type Make = fn(&Fx);
    let cases: [(&str, Make, DataOffer); 9] = [
        (
            "no team folder",
            |f| {
                f.profile_state();
                let _ = f.settle();
            },
            DataOffer::None,
        ),
        (
            "a team folder arrived after the first open",
            |f| {
                f.profile_state();
                let _ = f.settle();
                f.team_folder();
            },
            DataOffer::Ask,
        ),
        (
            "the person kept the data on this computer",
            |f| {
                f.profile_state();
                let _ = f.settle();
                f.team_folder();
                f.write_row("profile", FolderIdentity::of(f.project.path()), true);
            },
            DataOffer::Movable,
        ),
        (
            "both places hold data",
            |f| {
                f.profile_state();
                let _ = f.settle();
                f.team_folder();
                f.local_state();
            },
            DataOffer::Both,
        ),
        (
            "only the folder's local holds data",
            |f| {
                let _ = f.settle();
                f.team_folder();
                f.local_state();
            },
            DataOffer::None,
        ),
        (
            "local is a file",
            |f| {
                f.profile_state();
                let _ = f.settle();
                f.team_folder();
                fs::write(f.local(), b"x").unwrap();
            },
            DataOffer::None,
        ),
        (
            "the data is in the project",
            |f| {
                f.team_folder();
                f.local_state();
                assert_eq!(f.settle().location(), Project);
            },
            DataOffer::None,
        ),
        (
            "the project's .permark is gone",
            |f| {
                f.team_folder();
                f.local_state();
                assert_eq!(f.settle().location(), Project);
                fs::remove_dir_all(f.permark()).unwrap();
            },
            DataOffer::Missing,
        ),
        (
            "the project's local is a file",
            |f| {
                f.team_folder();
                assert_eq!(f.settle().location(), Project);
                fs::write(f.local(), b"x").unwrap();
            },
            DataOffer::Missing,
        ),
    ];
    for (name, make, want) in cases {
        let f = fx();
        make(&f);
        let folder = listing(f.project.path());
        let row = f.row_bytes();
        assert_eq!(offer(&f.store()), want, "{name}");
        assert_eq!(listing(f.project.path()), folder, "{name}: reads only");
        assert_eq!(f.row_bytes(), row, "{name}: reads only");
    }
    // no row this version can read: nothing to offer
    let f = fx();
    f.profile_state();
    f.team_folder();
    assert_eq!(offer(&f.store()), DataOffer::None);
}

/// Keep: the row names the profile as the person's choice, the place and everything in the folder unchanged; Move is
/// still offered; a second Keep writes nothing. Keeping the copy in use while the project's `.permark/` is gone does
/// the same, so a `.permark/` coming back never takes the data by itself.
#[test]
fn keep_records_the_profile_as_chosen_and_writes_nothing_into_the_folder() {
    let f = fx();
    f.profile_state();
    let first = answer(&f.settle());
    f.team_folder();
    let folder = listing(f.project.path());
    assert_eq!(offer(&f.store()), DataOffer::Ask);
    keep(&f.dirs, &f.root()).unwrap();
    let rows = f.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].place, rows[0].decided), (Profile, true));
    assert_eq!(
        answer(&f.store()),
        (first.0, first.1.clone(), first.2.clone(), true)
    );
    assert_eq!(offer(&f.store()), DataOffer::Movable);
    assert_eq!(listing(f.project.path()), folder);
    let row = f.row_bytes();
    keep(&f.dirs, &f.root()).unwrap();
    assert_eq!(f.row_bytes(), row);

    let f = fx();
    f.team_folder();
    f.local_state();
    assert_eq!(f.settle().location(), Project);
    fs::remove_dir_all(f.permark()).unwrap();
    let fallback = answer(&f.store());
    assert_eq!(offer(&f.store()), DataOffer::Missing);
    keep(&f.dirs, &f.root()).unwrap();
    assert_eq!(answer(&f.store()), fallback);
    assert_eq!(offer(&f.store()), DataOffer::None);
    let rows = f.rows();
    assert_eq!((rows[0].place, rows[0].decided), (Profile, true));
    f.team_folder();
    f.local_state();
    assert_eq!(f.store().location(), Profile);
    assert_eq!(
        offer(&f.store()),
        DataOffer::None,
        "local holds data, the profile none"
    );
}
