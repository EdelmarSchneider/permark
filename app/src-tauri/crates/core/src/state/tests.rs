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
