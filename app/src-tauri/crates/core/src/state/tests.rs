use super::*;
use std::collections::BTreeMap;
use std::fs;

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
struct Sample {
    items: BTreeMap<String, u32>,
}

impl StateData for Sample {
    const VERSION: u32 = 3;
}

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

impl Fx {
    fn root(&self) -> Root {
        Root::open(self.project.path()).unwrap()
    }
    fn store(&self) -> StateStore {
        StateStore::for_project(&self.dirs, &self.root())
    }
}

fn sample(n: u32) -> Sample {
    Sample {
        items: BTreeMap::from([("a".to_owned(), n)]),
    }
}

#[test]
fn profile_location_keyed_by_project_id_and_project_untouched() {
    let f = fx();
    let s = f.store();
    assert_eq!(s.location(), StateLocation::Profile);
    assert_eq!(
        s.dir(),
        f.dirs
            .profile
            .join("projects")
            .join(ProjectId::of(&f.root()).as_str())
    );
    assert_eq!(ProjectId::of(&f.root()).as_str().len(), 16);
    s.file::<Sample>("x.json").save(&sample(1)).unwrap();
    assert!(fs::read_dir(f.project.path()).unwrap().next().is_none());
}

#[test]
fn save_load_round_trip_with_version_envelope() {
    let f = fx();
    let file = f.store().file::<Sample>("x.json");
    assert_eq!(file.load(), Loaded::Missing);
    file.save(&sample(7)).unwrap();
    assert_eq!(file.load(), Loaded::Found(sample(7)));
    let raw: serde_json::Value = serde_json::from_slice(&fs::read(file.path()).unwrap()).unwrap();
    assert_eq!(raw["version"], 3);
}

#[test]
fn corrupt_and_other_version_files_are_moved_aside() {
    let f = fx();
    let file = f.store().file::<Sample>("x.json");
    fs::create_dir_all(file.path().parent().unwrap()).unwrap();
    for bad in [
        &b"{ not json"[..],
        b"",
        b"{\"version\":2,\"data\":{\"items\":{}}}",
        b"{\"version\":3,\"data\":[1]}",
        b"\xFF\xFE",
    ] {
        fs::write(file.path(), bad).unwrap();
        let Loaded::Recovered {
            backup: Some(backup),
        } = file.load()
        else {
            panic!("not recovered: {bad:?}");
        };
        assert!(!file.path().exists());
        assert_eq!(fs::read(&backup).unwrap(), bad);
        assert!(backup
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("x.json.broken-"));
    }
    // five backups, none overwritten
    let backups = fs::read_dir(file.path().parent().unwrap()).unwrap().count();
    assert_eq!(backups, 5);
    assert_eq!(file.load().into_value(), Sample::default());
}

#[test]
fn project_location_needs_a_real_permark_folder() {
    let f = fx();
    fs::create_dir(f.project.path().join(".permark")).unwrap();
    let s = f.store();
    assert_eq!(s.location(), StateLocation::Project);
    s.file::<Sample>("x.json").save(&sample(1)).unwrap();
    let local = f.root().path().join(".permark").join("local");
    assert!(local.join("x.json").is_file());
    assert_eq!(fs::read_to_string(local.join(".gitignore")).unwrap(), "*\n");
}

#[test]
fn a_file_named_permark_keeps_state_in_the_profile() {
    let f = fx();
    fs::write(f.project.path().join(".permark"), "x").unwrap();
    assert_eq!(f.store().location(), StateLocation::Profile);
}

#[cfg(unix)]
#[test]
fn links_in_permark_are_never_written_through() {
    let f = fx();
    let elsewhere = tempfile::tempdir().unwrap();
    // .permark itself a link
    std::os::unix::fs::symlink(elsewhere.path(), f.project.path().join(".permark")).unwrap();
    assert_eq!(f.store().location(), StateLocation::Profile);
    fs::remove_file(f.project.path().join(".permark")).unwrap();
    // local/ a link
    fs::create_dir(f.project.path().join(".permark")).unwrap();
    std::os::unix::fs::symlink(
        elsewhere.path(),
        f.project.path().join(".permark").join("local"),
    )
    .unwrap();
    assert_eq!(f.store().location(), StateLocation::Profile);
    assert!(fs::read_dir(elsewhere.path()).unwrap().next().is_none());
}

#[test]
fn a_link_swapped_in_after_open_refuses_the_write() {
    let f = fx();
    fs::create_dir(f.project.path().join(".permark")).unwrap();
    let s = f.store();
    let local = f.root().path().join(".permark").join("local");
    fs::write(&local, "now a file").unwrap();
    assert!(s.file::<Sample>("x.json").save(&sample(1)).is_err());
    assert_eq!(fs::read_to_string(&local).unwrap(), "now a file");
}

#[test]
fn plugin_namespaces_are_separate_and_removable() {
    let f = fx();
    let s = f.store();
    let git = PluginId::of("git");
    let tags = PluginId::of("tags");
    s.plugin_file::<Sample>(&git, "a.json")
        .save(&sample(1))
        .unwrap();
    s.plugin_file::<Sample>(&tags, "a.json")
        .save(&sample(2))
        .unwrap();
    assert!(s.plugin_dir(&git).starts_with(s.dir().join("plugins")));
    s.remove_plugin_data(&git).unwrap();
    assert!(!s.plugin_dir(&git).exists());
    assert_eq!(
        s.plugin_file::<Sample>(&tags, "a.json").load(),
        Loaded::Found(sample(2))
    );
    s.remove_plugin_data(&git).unwrap();
}

#[test]
fn remove_everywhere_covers_app_known_projects_and_open_roots() {
    let f = fx();
    let git = PluginId::of("git");
    StateStore::app(&f.dirs)
        .plugin_file::<Sample>(&git, "a.json")
        .save(&sample(1))
        .unwrap();
    f.store()
        .plugin_file::<Sample>(&git, "a.json")
        .save(&sample(1))
        .unwrap();
    let other = tempfile::tempdir().unwrap();
    fs::create_dir(other.path().join(".permark")).unwrap();
    let other_root = Root::open(other.path()).unwrap();
    let other_store = StateStore::for_project(&f.dirs, &other_root);
    other_store
        .plugin_file::<Sample>(&git, "a.json")
        .save(&sample(1))
        .unwrap();
    remove_plugin_data_everywhere(&f.dirs, &git, &[other_root]).unwrap();
    assert!(!StateStore::app(&f.dirs).plugin_dir(&git).exists());
    assert!(!f.store().plugin_dir(&git).exists());
    assert!(!other_store.plugin_dir(&git).exists());
}

#[test]
fn init_project_creates_files_keeps_existing_and_carries_state() {
    let f = fx();
    let before = f.store();
    before
        .file::<Sample>("session.json")
        .save(&sample(5))
        .unwrap();
    before
        .profile_file::<Sample>("agent.json")
        .save(&sample(9))
        .unwrap();
    before
        .plugin_file::<Sample>(&PluginId::of("tags"), "t.json")
        .save(&sample(4))
        .unwrap();
    fs::create_dir(f.project.path().join(".permark")).unwrap();
    fs::write(
        f.project.path().join(".permark").join("config.toml"),
        "# mine\n",
    )
    .unwrap();
    // now .permark exists but init has not run: state folder is already project-side (empty)
    let after = init_project(&f.dirs, &f.root()).unwrap();
    assert_eq!(after.location(), StateLocation::Project);
    let permark = f.root().path().join(".permark");
    assert_eq!(
        fs::read_to_string(permark.join("config.toml")).unwrap(),
        "# mine\n"
    );
    assert!(fs::read_to_string(permark.join(".gitignore"))
        .unwrap()
        .contains("local/"));
    assert_eq!(
        fs::read_to_string(permark.join("cache").join(".gitignore")).unwrap(),
        "*\n"
    );
    // init twice: idempotent
    init_project(&f.dirs, &f.root()).unwrap();
}

#[test]
fn init_project_copies_profile_state_except_profile_only_files() {
    let f = fx();
    let before = f.store();
    before
        .file::<Sample>("session.json")
        .save(&sample(5))
        .unwrap();
    before
        .profile_file::<Sample>("agent.json")
        .save(&sample(9))
        .unwrap();
    before
        .plugin_file::<Sample>(&PluginId::of("tags"), "t.json")
        .save(&sample(4))
        .unwrap();
    let after = init_project(&f.dirs, &f.root()).unwrap();
    assert_eq!(
        after.file::<Sample>("session.json").load(),
        Loaded::Found(sample(5))
    );
    assert_eq!(
        after
            .plugin_file::<Sample>(&PluginId::of("tags"), "t.json")
            .load(),
        Loaded::Found(sample(4))
    );
    assert!(!after.dir().join("agent.json").exists());
    assert_eq!(
        after.profile_file::<Sample>("agent.json").load(),
        Loaded::Found(sample(9))
    );
    let default = fs::read_to_string(f.root().path().join(".permark").join("config.toml")).unwrap();
    assert_eq!(default, DEFAULT_CONFIG_TOML);
}

#[test]
fn init_project_refuses_a_file_in_the_way() {
    let f = fx();
    fs::write(f.project.path().join(".permark"), "x").unwrap();
    assert!(init_project(&f.dirs, &f.root()).is_err());
    assert_eq!(
        fs::read_to_string(f.project.path().join(".permark")).unwrap(),
        "x"
    );
}

#[test]
fn unicode_space_project_paths_work() {
    let base = tempfile::tempdir().unwrap();
    let dirs = AppDirs::under(&base.path().join("perfil ç 档"));
    let project = base.path().join("my 项目 ação");
    fs::create_dir(&project).unwrap();
    let root = Root::open(&project).unwrap();
    let file = StateStore::for_project(&dirs, &root).file::<Sample>("s.json");
    file.save(&sample(1)).unwrap();
    assert_eq!(file.load(), Loaded::Found(sample(1)));
}

// ---- raw files (the person's data in the layouts of earlier versions) ------------------------------------------------

#[test]
fn a_shared_file_is_in_permark_when_the_project_has_it_else_in_the_profile() {
    let f = fx();
    let before = f.store();
    let shared = before.shared_file("meta.toml");
    assert_eq!(shared.path(), before.dir().join("meta.toml"));
    assert!(before.legacy_shared_file("meta.toml").is_none());
    shared.write(b"x = 1\n").unwrap();
    assert!(f.project.path().join(".permark").read_dir().is_err());

    let after = init_project(&f.dirs, &f.root()).unwrap();
    let shared = after.shared_file("meta.toml");
    // the project as the root holds it (on Windows the temp folder may come as an 8.3 short name)
    assert_eq!(
        shared.path(),
        f.root().path().join(".permark").join("meta.toml")
    );
    // the profile's copy is the legacy one; it did not land in local/
    let legacy = after.legacy_shared_file("meta.toml").unwrap();
    assert_eq!(legacy.read().unwrap().as_deref(), Some(&b"x = 1\n"[..]));
    assert!(!after.dir().join("meta.toml").exists());
    assert_eq!(shared.read().unwrap(), None);
    shared.write(b"y = 2\n").unwrap();
    assert_eq!(shared.read().unwrap().as_deref(), Some(&b"y = 2\n"[..]));
    legacy.remove().unwrap();
    legacy.remove().unwrap();
    assert_eq!(legacy.read().unwrap(), None);
}

#[test]
fn a_raw_file_in_the_state_folder_is_guarded_like_the_others() {
    let f = fx();
    fs::create_dir(f.project.path().join(".permark")).unwrap();
    let s = f.store();
    let raw = s.raw_file("comments.json");
    raw.write(b"{}").unwrap();
    assert_eq!(
        fs::read(f.project.path().join(".permark/local/comments.json")).unwrap(),
        b"{}"
    );
    // the state folder ignores itself, `.permark/` itself does not get an ignore file
    assert!(f.project.path().join(".permark/local/.gitignore").exists());
    assert!(!f.project.path().join(".permark/.gitignore").exists());
    // `local/` replaced by a file after the store was made: refused, nothing written
    fs::remove_dir_all(f.project.path().join(".permark/local")).unwrap();
    fs::write(f.project.path().join(".permark/local"), "now a file").unwrap();
    assert!(raw.write(b"{ \"x\": 1 }").is_err());
    assert_eq!(
        fs::read_to_string(f.project.path().join(".permark/local")).unwrap(),
        "now a file"
    );
}

#[test]
fn a_shared_write_is_refused_when_permark_stops_being_a_folder() {
    let f = fx();
    fs::create_dir(f.project.path().join(".permark")).unwrap();
    let shared = f.store().shared_file("meta.toml");
    shared.write(b"a = 1\n").unwrap();
    fs::remove_dir_all(f.project.path().join(".permark")).unwrap();
    fs::write(f.project.path().join(".permark"), "now a file").unwrap();
    assert!(shared.write(b"a = 2\n").is_err());
    assert_eq!(
        fs::read_to_string(f.project.path().join(".permark")).unwrap(),
        "now a file"
    );
}

#[test]
fn a_raw_file_reads_bytes_exactly_and_tells_a_change_by_its_stamp() {
    let f = fx();
    let raw = f.store().raw_file("quiet.json");
    assert_eq!(raw.read().unwrap(), None);
    assert_eq!(raw.stamp(), None);
    let bytes = "{\r\n  \"é 档\": \"quiet\"\r\n}".as_bytes();
    raw.write(bytes).unwrap();
    assert_eq!(raw.read().unwrap().as_deref(), Some(bytes));
    let stamp = raw.stamp().unwrap();
    assert_eq!(stamp.0, bytes.len() as u64);
    raw.write(b"{}").unwrap();
    assert_ne!(raw.stamp().unwrap().0, stamp.0);
}

#[test]
fn a_checked_write_never_lands_over_what_another_program_wrote_since_the_read() {
    let f = fx();
    fs::create_dir(f.project.path().join(".permark")).unwrap();
    let shared = f.store().shared_file("meta.toml");
    let path = f.project.path().join(".permark/meta.toml");
    // no file yet, none expected: written
    assert!(shared.write_over(None, b"a = 1\n").unwrap());
    // the file as read: written
    assert!(shared.write_over(Some(b"a = 1\n"), b"a = 2\n").unwrap());
    assert_eq!(fs::read(&path).unwrap(), b"a = 2\n");
    // another program wrote it since: untouched, and no temp left behind
    fs::write(&path, "theirs = 1\n").unwrap();
    assert!(!shared.write_over(Some(b"a = 2\n"), b"a = 3\n").unwrap());
    assert_eq!(fs::read_to_string(&path).unwrap(), "theirs = 1\n");
    // ... or deleted it, or made it where there was none
    fs::remove_file(&path).unwrap();
    assert!(!shared
        .write_over(Some(b"theirs = 1\n"), b"a = 4\n")
        .unwrap());
    assert!(!path.exists());
    fs::write(&path, "new = 1\n").unwrap();
    assert!(!shared.write_over(None, b"a = 5\n").unwrap());
    assert_eq!(fs::read_to_string(&path).unwrap(), "new = 1\n");
    let left: Vec<_> = fs::read_dir(f.project.path().join(".permark"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(left, ["meta.toml"]);
}

/// N1: undo of an init removes `.permark/` in one step (renamed away, then deleted): nothing left behind, the state
/// back in the profile.
#[test]
fn uninit_project_removes_the_folder_whole_and_leaves_no_temp() {
    let f = fx();
    let store = init_project(&f.dirs, &f.root()).unwrap();
    store
        .file::<Sample>("session.json")
        .save(&sample(7))
        .unwrap();
    assert!(uninit_project(&f.dirs, &f.root()).unwrap());
    let names: Vec<String> = fs::read_dir(f.project.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.is_empty(), "nothing left: {names:?}");
    let back = f.store();
    assert_eq!(back.location(), StateLocation::Profile);
    assert_eq!(
        back.file::<Sample>("session.json").load().into_value(),
        sample(7)
    );
}

/// N1: a file held open inside `.permark/` (Windows: antivirus, editor) -> the folder stays whole, never half removed.
#[cfg(windows)]
#[test]
fn uninit_project_with_a_held_file_keeps_the_folder_whole() {
    let f = fx();
    let store = init_project(&f.dirs, &f.root()).unwrap();
    store
        .file::<Sample>("session.json")
        .save(&sample(3))
        .unwrap();
    let held = f.root().path().join(".permark/local/session.json");
    let _open = fs::File::open(&held).unwrap();
    assert!(uninit_project(&f.dirs, &f.root()).is_err());
    let permark = f.root().path().join(".permark");
    for name in ["config.toml", ".gitignore", "local/session.json", "cache"] {
        assert!(permark.join(name).exists(), "{name} kept");
    }
    assert_eq!(f.store().location(), StateLocation::Project);
}

/// Shares: 0.6 keyed their state by the `\\?\UNC\` spelling (std's canonical form). Opened plain now, they keep that
/// id -> their profile state (unread, sessions, agent connection) stays found, with no move.
#[cfg(windows)]
#[test]
fn a_share_keeps_the_project_id_0_6_gave_it() {
    let plain = crate::paths::RootKey::of(Path::new(r"\\wsl.localhost\Ubuntu-24.04\tmp\pm07wsl"));
    let verbatim =
        crate::paths::RootKey::of(Path::new(r"\\?\UNC\wsl.localhost\Ubuntu-24.04\tmp\pm07wsl"));
    let then = crate::fsx::hash(b"//?/unc/wsl.localhost/ubuntu-24.04/tmp/pm07wsl").hex();
    assert_eq!(ProjectId::of_key(&plain).0, then);
    assert_eq!(ProjectId::of_key(&verbatim).0, then);
    let drive = crate::paths::RootKey::of(Path::new(r"C:\Users\me"));
    assert_eq!(
        ProjectId::of_key(&drive).0,
        crate::fsx::hash(b"c:/users/me").hex()
    );
    // 0.5's key of a share kept the prefix too
    assert_eq!(
        older::older_project_key(Path::new(r"\\srv\Share\x\")),
        crate::fsx::hash(b"//?/unc/srv/share/x").hex()
    );
    assert_eq!(
        older::older_project_key(Path::new(r"\\?\UNC\srv\share\x")),
        older::older_project_key(Path::new(r"\\srv\share\x"))
    );
}

/// A real share (loopback admin share): the id of the root opened now == the id 0.6 computed from std's canonical
/// `\\?\UNC\` path; a move between spellings of one share moves nothing.
#[cfg(windows)]
#[test]
fn a_real_share_root_keeps_its_0_6_id() {
    let dir = tempfile::tempdir().unwrap();
    let Some(shared) = crate::paths::via_admin_share(dir.path()) else {
        eprintln!("skipped: no loopback admin share here");
        return;
    };
    let root = Root::open(&shared).unwrap();
    assert!(!root.path().to_string_lossy().starts_with(r"\\?\"));
    let then_canon = std::fs::canonicalize(&shared).unwrap();
    let then_key = crate::paths::fold(
        then_canon
            .to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches('/'),
    );
    assert_eq!(
        ProjectId::of(&root).as_str(),
        crate::fsx::hash(then_key.as_bytes()).hex()
    );
    let base = tempfile::tempdir().unwrap();
    let dirs = AppDirs::under(base.path());
    let kept = dirs
        .profile
        .join("projects")
        .join(ProjectId::of(&root).as_str());
    fs::create_dir_all(&kept).unwrap();
    assert!(!move_profile_state(&dirs, &then_canon, root.path()).unwrap());
    assert!(kept.is_dir());
}

/// The file at `path` cannot be read by this process until this goes (another program holding it on Windows, no
/// read permission on Unix).
struct Unreadable {
    #[cfg(windows)]
    _held: fs::File,
    #[cfg(not(windows))]
    path: std::path::PathBuf,
}

#[cfg(not(windows))]
impl Drop for Unreadable {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(0o644));
    }
}

fn unreadable(path: &Path) -> Unreadable {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(path)
            .unwrap();
        Unreadable { _held: held }
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
        Unreadable {
            path: path.to_path_buf(),
        }
    }
}

/// Unreadable is in force, yet this process reads the file anyway (root on Unix): the test has nothing to check.
fn readable_anyway(path: &Path) -> bool {
    let anyway = fs::read(path).is_ok();
    if anyway {
        eprintln!(
            "skipped: this process reads {} anyway (root?)",
            path.display()
        );
    }
    anyway
}

/// The file at `path` can be read but not moved (renamed) until this goes: held without delete sharing on Windows,
/// its folder read-only on Unix.
struct Unmovable {
    #[cfg(windows)]
    _held: fs::File,
    #[cfg(not(windows))]
    dir: std::path::PathBuf,
}

#[cfg(not(windows))]
impl Drop for Unmovable {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o755));
    }
}

fn unmovable(path: &Path) -> Unmovable {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 1;
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(path)
            .unwrap();
        Unmovable { _held: held }
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = path.parent().unwrap().to_path_buf();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
        Unmovable { dir }
    }
}

/// Unmovable is in force, yet this process moves the file anyway (root on Unix): the test has nothing to check.
fn movable_anyway(path: &Path) -> bool {
    let probe = path.with_extension("probe");
    let anyway = fs::rename(path, &probe).is_ok();
    if anyway {
        fs::rename(&probe, path).unwrap();
        eprintln!(
            "skipped: this process moves {} anyway (root?)",
            path.display()
        );
    }
    anyway
}

const KEPT: &[u8] = br#"{"version":3,"data":{"items":{"kept":7}}}"#;

/// Board item 15: a state file PerMark could not read (an IO error, not a missing or corrupt file) is never written
/// over with the defaults used meanwhile: it stays as it is, and saves are refused (by any handle on that file) until
/// a load reads it again.
#[test]
fn an_unreadable_state_file_is_never_written_over() {
    let f = fx();
    let file = f.store().file::<Sample>("x.json");
    fs::create_dir_all(file.path().parent().unwrap()).unwrap();
    fs::write(file.path(), KEPT).unwrap();
    let held = unreadable(file.path());
    if readable_anyway(file.path()) {
        return;
    }
    assert_eq!(file.load(), Loaded::Recovered { backup: None });
    assert!(file.path().exists(), "left in place, not moved aside");
    drop(held);
    // what the caller does with the defaults it got: never written over the person's data
    assert!(file.save(&Sample::default()).is_err());
    assert!(f.store().file::<Sample>("x.json").save(&sample(1)).is_err());
    assert_eq!(fs::read(file.path()).unwrap(), KEPT);
    // read again: the data is there and saves go through
    let Loaded::Found(data) = file.load() else {
        panic!("readable again");
    };
    assert_eq!(data.items.get("kept"), Some(&7));
    file.save(&sample(2)).unwrap();
    assert_eq!(file.load(), Loaded::Found(sample(2)));
}

/// A file held only for a moment (an antivirus | sync client) is read after a short retry: its data, no defaults.
#[test]
fn a_state_file_held_for_a_moment_is_read_after_a_retry() {
    let f = fx();
    let file = f.store().file::<Sample>("x.json");
    fs::create_dir_all(file.path().parent().unwrap()).unwrap();
    fs::write(file.path(), KEPT).unwrap();
    let held = unreadable(file.path());
    if readable_anyway(file.path()) {
        return;
    }
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(30));
        drop(held);
    });
    let loaded = file.load();
    release.join().unwrap();
    let Loaded::Found(data) = loaded else {
        panic!("read after the retry: {loaded:?}");
    };
    assert_eq!(data.items.get("kept"), Some(&7));
}

/// An update reads the file itself under its lock: once it can, it changes the data on disk (never the defaults), and
/// plain saves go through again.
#[test]
fn an_update_after_an_unreadable_load_changes_the_data_on_disk() {
    let f = fx();
    let file = f.store().file::<Sample>("x.json");
    fs::create_dir_all(file.path().parent().unwrap()).unwrap();
    fs::write(file.path(), KEPT).unwrap();
    let held = unreadable(file.path());
    if readable_anyway(file.path()) {
        return;
    }
    assert_eq!(file.load(), Loaded::Recovered { backup: None });
    assert!(
        file.update(|_| Some(Sample::default())).is_err(),
        "still unreadable: no write"
    );
    drop(held);
    let after = file
        .update(|now| {
            let mut next = Sample {
                items: now.map(|s| s.items.clone()).unwrap_or_default(),
            };
            next.items.insert("added".to_owned(), 1);
            Some(next)
        })
        .unwrap()
        .unwrap();
    assert_eq!(after.items.get("kept"), Some(&7));
    assert_eq!(after.items.get("added"), Some(&1));
    file.save(&after).unwrap();
}

/// Board item 15 (review S2): a load that reads the file from another handle does not unblock a handle that got the
/// defaults: its save would write them over the data the other one read.
#[test]
fn another_handle_reading_does_not_unblock_the_caller_holding_defaults() {
    let f = fx();
    let holding_defaults = f.store().file::<Sample>("x.json");
    fs::create_dir_all(holding_defaults.path().parent().unwrap()).unwrap();
    fs::write(holding_defaults.path(), KEPT).unwrap();
    let held = unreadable(holding_defaults.path());
    if readable_anyway(holding_defaults.path()) {
        return;
    }
    assert_eq!(holding_defaults.load(), Loaded::Recovered { backup: None });
    drop(held);
    let reader = f.store().file::<Sample>("x.json");
    assert!(matches!(reader.load(), Loaded::Found(_)));
    assert!(holding_defaults.save(&Sample::default()).is_err());
    assert!(
        f.store()
            .file::<Sample>("x.json")
            .save(&Sample::default())
            .is_err(),
        "a handle that never read it is not unblocked either"
    );
    assert_eq!(fs::read(holding_defaults.path()).unwrap(), KEPT);
    // the one that read it may write; the other one again after it reads the file itself
    reader.save(&sample(2)).unwrap();
    assert_eq!(holding_defaults.load(), Loaded::Found(sample(2)));
    holding_defaults.save(&sample(3)).unwrap();
    assert_eq!(reader.load(), Loaded::Found(sample(3)));
}

/// Review S1: an update that finds a file it cannot parse and cannot move aside fails and keeps it blocked: a later
/// save of the defaults never writes over it.
#[test]
fn a_failed_update_on_an_unmovable_unparsable_file_keeps_the_mark() {
    const BROKEN: &[u8] = b"{ not json";
    let f = fx();
    let file = f.store().file::<Sample>("x.json");
    fs::create_dir_all(file.path().parent().unwrap()).unwrap();
    fs::write(file.path(), BROKEN).unwrap();
    let held = unmovable(file.path());
    if movable_anyway(file.path()) {
        return;
    }
    assert!(file.update(|_| Some(sample(1))).is_err());
    assert_eq!(fs::read(file.path()).unwrap(), BROKEN);
    drop(held);
    assert!(file.save(&Sample::default()).is_err());
    assert!(f
        .store()
        .file::<Sample>("x.json")
        .save(&Sample::default())
        .is_err());
    assert_eq!(fs::read(file.path()).unwrap(), BROKEN);
}

/// Re-review N1 (P4): a handle that read the file keeps writing it after another handle fails to read it (two tabs,
/// the journal held by an antivirus when the second one opens): it holds the person's data, not defaults.
#[test]
fn a_handle_that_read_the_file_still_saves_after_another_one_fails() {
    let f = fx();
    let healthy = f.store().file::<Sample>("x.json");
    fs::create_dir_all(healthy.path().parent().unwrap()).unwrap();
    fs::write(healthy.path(), KEPT).unwrap();
    let Loaded::Found(mut data) = healthy.load() else {
        panic!("readable");
    };
    let failing = f.store().file::<Sample>("x.json");
    let held = unreadable(failing.path());
    if readable_anyway(failing.path()) {
        return;
    }
    assert_eq!(failing.load(), Loaded::Recovered { backup: None });
    drop(held);
    data.items.insert("more".to_owned(), 2);
    healthy.save(&data).unwrap();
    let saved = fs::read(healthy.path()).unwrap();
    // the one that got the defaults, and one that never read it, are still refused
    assert!(failing.save(&Sample::default()).is_err());
    assert!(f
        .store()
        .file::<Sample>("x.json")
        .save(&Sample::default())
        .is_err());
    assert_eq!(fs::read(healthy.path()).unwrap(), saved);
    assert_eq!(healthy.load(), Loaded::Found(data));
}

/// P3: on a file no load ever failed on, fresh handles (several threads at once) save as before.
#[test]
fn fresh_handles_on_a_file_that_never_failed_save_as_before() {
    let f = fx();
    let first = f.store().file::<Sample>("x.json");
    fs::create_dir_all(first.path().parent().unwrap()).unwrap();
    let store = f.store();
    std::thread::scope(|scope| {
        for n in 0..4u32 {
            let store = &store;
            scope.spawn(move || {
                for _ in 0..10 {
                    store.file::<Sample>("x.json").save(&sample(n)).unwrap();
                }
            });
        }
    });
    assert!(matches!(first.load(), Loaded::Found(_)));
    first.save(&sample(9)).unwrap();
    assert_eq!(first.load(), Loaded::Found(sample(9)));
}

/// P5: a load that moves a corrupt file aside leaves nothing to write over: that handle may save its defaults, even
/// on a file an earlier load could not read; the handle that got defaults then and a fresh one stay refused.
#[test]
fn a_handle_that_moved_the_file_aside_may_save() {
    let f = fx();
    let earlier = f.store().file::<Sample>("x.json");
    fs::create_dir_all(earlier.path().parent().unwrap()).unwrap();
    fs::write(earlier.path(), KEPT).unwrap();
    let held = unreadable(earlier.path());
    if readable_anyway(earlier.path()) {
        return;
    }
    assert_eq!(earlier.load(), Loaded::Recovered { backup: None });
    drop(held);
    fs::write(earlier.path(), b"{ not json").unwrap();
    let mover = f.store().file::<Sample>("x.json");
    let Loaded::Recovered {
        backup: Some(backup),
    } = mover.load()
    else {
        panic!("moved aside");
    };
    assert_eq!(fs::read(&backup).unwrap(), b"{ not json");
    mover.save(&Sample::default()).unwrap();
    assert!(earlier.save(&sample(1)).is_err());
    assert!(f.store().file::<Sample>("x.json").save(&sample(1)).is_err());
    assert_eq!(mover.load(), Loaded::Found(Sample::default()));
}

/// Delta check P7: an update that cannot read the file (an IO error) stamps it, so a fresh handle never saves the
/// defaults over it afterwards; a handle that read the file itself keeps writing.
#[test]
fn an_update_that_cannot_read_the_file_keeps_fresh_handles_from_saving_defaults() {
    let f = fx();
    let healthy = f.store().file::<Sample>("x.json");
    fs::create_dir_all(healthy.path().parent().unwrap()).unwrap();
    fs::write(healthy.path(), KEPT).unwrap();
    assert!(matches!(healthy.load(), Loaded::Found(_)));
    let held = unreadable(healthy.path());
    if readable_anyway(healthy.path()) {
        return;
    }
    assert!(f
        .store()
        .file::<Sample>("x.json")
        .update(|_| Some(sample(1)))
        .is_err());
    drop(held);
    assert!(f
        .store()
        .file::<Sample>("x.json")
        .save(&Sample::default())
        .is_err());
    assert_eq!(fs::read(healthy.path()).unwrap(), KEPT);
    healthy.save(&sample(2)).unwrap();
    assert_eq!(healthy.load(), Loaded::Found(sample(2)));
}
