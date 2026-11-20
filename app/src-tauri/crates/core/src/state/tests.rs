use super::*;
use std::collections::BTreeMap;
use std::fs;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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

/// A plugin's profile folder stays in the profile even when the project keeps its state in `.permark/local/`, and
/// "remove data" sweeps it.
#[test]
fn a_profile_plugin_folder_never_lives_in_the_project() {
    let f = fx();
    fs::create_dir(f.project.path().join(".permark")).unwrap();
    let s = f.store();
    assert_eq!(s.location(), StateLocation::Project);
    let acp = PluginId::of("acp");
    let dir = s.profile_plugin_dir(&acp);
    let expected = f
        .dirs
        .profile
        .join("projects")
        .join(ProjectId::of(&f.root()).as_str())
        .join("plugins")
        .join("acp");
    assert_eq!(dir, expected);
    assert!(!dir.starts_with(f.project.path()));
    let file = s.profile_plugin_file::<Sample>(&acp, "session.json");
    file.save(&sample(1)).unwrap();
    assert_eq!(file.path(), dir.join("session.json"));
    fs::create_dir_all(dir.join("work")).unwrap();
    remove_plugin_data_everywhere(&f.dirs, &acp, &[]).unwrap();
    assert!(!dir.exists());
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
    // the person's answers about included projects, the copy ledger, the profile card and the mark of the 0.5
    // take-over are read from the profile only: a copy in the project would never be read
    let read_from_profile = [
        "membership.json",
        "projects-copied.json",
        "card.json",
        "from-0.5",
    ];
    for name in read_from_profile {
        fs::write(before.dir().join(name), b"profile's").unwrap();
    }
    assert_eq!(
        PROFILE_ONLY[..],
        [
            "agent.json",
            "agent-refused.json",
            "plugins.json",
            "pictures.json",
            "membership.json",
            "projects-copied.json",
            "card.json",
            "from-0.5",
        ]
    );
    let after = init_project(&f.dirs, &f.root()).unwrap();
    for name in read_from_profile {
        assert!(!after.dir().join(name).exists(), "{name} stays out");
        assert_eq!(
            fs::read(before.dir().join(name)).unwrap(),
            b"profile's",
            "{name} kept in the profile"
        );
    }
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

/// Every entry below `dir`, relative, `/`-separated, folders ending in `/`, sorted.
fn entries_below(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if entry.file_type().unwrap().is_dir() {
                out.push(format!("{name}/"));
                walk(&entry.path(), &format!("{name}/"), out);
            } else {
                out.push(name);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, "", &mut out);
    out.sort();
    out
}

#[test]
fn init_project_makes_exactly_these_files() {
    let f = fx();
    init_project(&f.dirs, &f.root()).unwrap();
    let permark = f.root().path().join(".permark");
    assert_eq!(
        entries_below(&permark),
        [
            ".gitignore",
            "cache/",
            "cache/.gitignore",
            "cache/CACHEDIR.TAG",
            "config.toml",
            "local/",
            "local/.gitignore",
        ]
    );
    assert!(
        fs::read_to_string(permark.join("cache").join("CACHEDIR.TAG"))
            .unwrap()
            .starts_with("Signature: 8a477f597d28d172789f06886806bc55")
    );
    for state in ["local", "cache"] {
        assert_eq!(
            fs::read_to_string(permark.join(state).join(".gitignore")).unwrap(),
            "*\n"
        );
    }
    assert_eq!(
        fs::read_to_string(permark.join(".gitignore")).unwrap(),
        PROJECT_GITIGNORE
    );
}

#[test]
fn uninit_project_takes_back_a_cache_holding_more_files() {
    let f = fx();
    let store = init_project(&f.dirs, &f.root()).unwrap();
    store
        .cache_file::<Sample>("index.json")
        .save(&sample(1))
        .unwrap();
    fs::write(store.cache_dir().join("extra.bin"), b"x").unwrap();
    assert!(uninit_project(&f.dirs, &f.root()).unwrap());
    assert!(!f.root().path().join(".permark").exists());
}

#[test]
fn a_state_entry_is_a_folder_only_when_it_is_a_real_folder() {
    let f = fx();
    let base = f.project.path();
    fs::create_dir(base.join("folder")).unwrap();
    fs::write(base.join("file"), b"x").unwrap();
    assert!(matches!(
        private::entry(&base.join("folder")),
        private::Entry::Folder
    ));
    assert!(matches!(
        private::entry(&base.join("missing")),
        private::Entry::Missing
    ));
    assert!(matches!(
        private::entry(&base.join("file")),
        private::Entry::Other
    ));
    if crate::tree::tests::link_out(&base.join("folder link"), &base.join("folder")) {
        assert!(matches!(
            private::entry(&base.join("folder link")),
            private::Entry::Other
        ));
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(base.join("file"), base.join("file link")).unwrap();
        assert!(matches!(
            private::entry(&base.join("file link")),
            private::Entry::Other
        ));
    }
}

/// A folder where a state folder's ignore file belongs cannot be replaced without deleting what it holds: it is left
/// as it is and the save still goes through.
#[test]
fn a_folder_in_place_of_the_ignore_file_never_blocks_a_save() {
    let f = fx();
    let local = f.project.path().join(".permark").join("local");
    fs::create_dir_all(local.join(".gitignore")).unwrap();
    fs::write(local.join(".gitignore").join("kept"), b"x").unwrap();
    let file = f.store().file::<Sample>("x.json");
    file.save(&sample(1)).unwrap();
    assert_eq!(file.load(), Loaded::Found(sample(1)));
    assert_eq!(
        fs::read(local.join(".gitignore").join("kept")).unwrap(),
        b"x"
    );
}

/// A folder may arrive with its own ignore files in the state folders: they are set back to "everything" at the next
/// write, while the repository's ignore file and `.permark/.gitignore` stay as they are.
#[test]
fn the_state_folders_ignore_everything_whatever_the_folder_shipped() {
    let f = fx();
    let permark = f.project.path().join(".permark");
    for (path, text) in [
        (".gitignore", "build/\n"),
        (".permark/.gitignore", "# the team's own\n"),
        (".permark/local/.gitignore", "!*\n"),
        (".permark/cache/.gitignore", ""),
    ] {
        let path = f.project.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let store = f.store();
    store.file::<Sample>("x.json").save(&sample(1)).unwrap();
    store
        .cache_file::<Sample>("index.json")
        .save(&sample(2))
        .unwrap();
    for state in ["local", "cache"] {
        assert_eq!(
            fs::read_to_string(permark.join(state).join(".gitignore")).unwrap(),
            "*\n",
            "{state}"
        );
    }
    assert_eq!(
        fs::read_to_string(f.project.path().join(".gitignore")).unwrap(),
        "build/\n"
    );
    assert_eq!(
        fs::read_to_string(permark.join(".gitignore")).unwrap(),
        "# the team's own\n"
    );
    // a raw write sets it back too
    fs::write(permark.join("local").join(".gitignore"), "x\n").unwrap();
    store.raw_file("comments.json").write(b"{}").unwrap();
    assert_eq!(
        fs::read_to_string(permark.join("local").join(".gitignore")).unwrap(),
        "*\n"
    );
}

/// A link in place of a state folder's ignore file is replaced by a plain file, never written through.
#[cfg(unix)]
#[test]
fn a_link_in_place_of_an_ignore_file_is_replaced_never_followed() {
    let f = fx();
    let local = f.project.path().join(".permark").join("local");
    fs::create_dir_all(&local).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("ignore");
    fs::write(&target, "!*\n").unwrap();
    std::os::unix::fs::symlink(&target, local.join(".gitignore")).unwrap();
    f.store().file::<Sample>("x.json").save(&sample(1)).unwrap();
    let meta = fs::symlink_metadata(local.join(".gitignore")).unwrap();
    assert!(meta.file_type().is_file());
    assert_eq!(fs::read_to_string(local.join(".gitignore")).unwrap(), "*\n");
    assert_eq!(fs::read_to_string(&target).unwrap(), "!*\n");
    // a link to a folder is looked at, never followed (it could lead to a network share): replaced the same way
    let cache = f.project.path().join(".permark").join("cache");
    fs::create_dir_all(&cache).unwrap();
    let folder = outside.path().join("folder");
    fs::create_dir(&folder).unwrap();
    std::os::unix::fs::symlink(&folder, cache.join(".gitignore")).unwrap();
    f.store()
        .cache_file::<Sample>("index.json")
        .save(&sample(1))
        .unwrap();
    let meta = fs::symlink_metadata(cache.join(".gitignore")).unwrap();
    assert!(meta.file_type().is_file());
    assert_eq!(fs::read_dir(&folder).unwrap().count(), 0);
}

/// The cache folder tells backup tools it holds only rebuildable files (<https://bford.info/cachedir/>); a tag already
/// there is kept as it is, and the private state folder never gets one.
#[test]
fn the_cache_folder_carries_a_cache_directory_tag() {
    let f = fx();
    let permark = f.project.path().join(".permark");
    fs::create_dir(&permark).unwrap();
    let store = f.store();
    store.file::<Sample>("x.json").save(&sample(1)).unwrap();
    store
        .cache_file::<Sample>("index.json")
        .save(&sample(2))
        .unwrap();
    let tag = permark.join("cache").join("CACHEDIR.TAG");
    let text = fs::read_to_string(&tag).unwrap();
    assert!(text.starts_with("Signature: 8a477f597d28d172789f06886806bc55\n"));
    assert!(!permark.join("local").join("CACHEDIR.TAG").exists());
    fs::write(
        &tag,
        "Signature: 8a477f597d28d172789f06886806bc55\n# mine\n",
    )
    .unwrap();
    store
        .cache_file::<Sample>("index.json")
        .save(&sample(3))
        .unwrap();
    assert!(fs::read_to_string(&tag).unwrap().ends_with("# mine\n"));
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
fn a_shared_file_is_read_only_as_a_plain_file_of_at_most_a_mebibyte() {
    let f = fx();
    fs::create_dir(f.project.path().join(".permark")).unwrap();
    let shared = f.store().shared_file("spelling.json");
    let path = f.project.path().join(".permark/spelling.json");
    fs::write(&path, b"{}").unwrap();
    assert_eq!(shared.read().unwrap().as_deref(), Some(&b"{}"[..]));
    // the cap itself: read; one byte more: unreadable, and an update writes nothing
    let cap = usize::try_from(SHARED_MAX_BYTES).unwrap();
    fs::write(&path, vec![b' '; cap]).unwrap();
    assert_eq!(shared.read().unwrap().map(|b| b.len()), Some(cap));
    let big = vec![b' '; cap + 1];
    fs::write(&path, &big).unwrap();
    assert_eq!(
        shared.read().err().map(|e| e.kind()),
        Some(std::io::ErrorKind::InvalidData)
    );
    assert!(shared.update(|_| Some(b"{}".to_vec())).is_err());
    assert_eq!(fs::read(&path).unwrap(), big);
    // a folder in its place: unreadable
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(shared.read().is_err());
    fs::remove_dir(&path).unwrap();
    // a link in its place is never followed: refused, its target untouched
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("elsewhere.json");
    fs::write(&target, b"{\"x\": 1}").unwrap();
    // Windows makes file links only with the privilege (developer mode): `false` then
    let link = |at: &std::path::Path| -> bool {
        #[cfg(unix)]
        return std::os::unix::fs::symlink(&target, at).is_ok();
        #[cfg(windows)]
        return std::os::windows::fs::symlink_file(&target, at).is_ok();
    };
    if !link(&path) {
        eprintln!("skipped the link cases: no file links here");
        return;
    }
    assert_eq!(
        shared.read().err().map(|e| e.kind()),
        Some(std::io::ErrorKind::InvalidData)
    );
    assert!(shared.update(|_| Some(b"{}".to_vec())).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"{\"x\": 1}");
    // the same for a file in the state folder
    let private = f.store().raw_file("comments.json");
    private.write(b"{}").unwrap();
    let private_path = f.project.path().join(".permark/local/comments.json");
    fs::remove_file(&private_path).unwrap();
    assert!(link(&private_path));
    assert!(private.read().is_err());
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

const MIB: usize = 1024 * 1024;

/// State as large as people's gets today keeps reading: a 3 MiB state file (project and profile), a 1.5 MiB
/// `comments.json`, a 3 MiB profile raw file.
#[test]
fn state_files_of_todays_sizes_still_read() {
    let f = fx();
    fs::create_dir(f.project.path().join(".permark")).unwrap();
    let store = f.store();
    let big = Sample {
        items: BTreeMap::from([("k".repeat(3 * MIB), 1)]),
    };
    for file in [
        store.file::<Sample>("big.json"),
        store.profile_file::<Sample>("big.json"),
    ] {
        file.save(&big).unwrap();
        assert!(fs::metadata(file.path()).unwrap().len() > 3 * MIB as u64);
        assert_eq!(file.load(), Loaded::Found(big.clone()));
    }
    let comments = vec![b' '; 3 * MIB / 2];
    let raw = store.raw_file("comments.json");
    raw.write(&comments).unwrap();
    assert_eq!(raw.read().unwrap(), Some(comments));
    let profile = vec![b'#'; 3 * MIB];
    let legacy = store.legacy_shared_file("meta.toml").unwrap();
    legacy.write(&profile).unwrap();
    assert_eq!(legacy.read().unwrap(), Some(profile));
}

/// Names in `dir` that a load moved aside.
fn broken_in(dir: &Path) -> usize {
    fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".broken-"))
        .count()
}

/// A valid state file padded past the default cap.
fn over_cap() -> Vec<u8> {
    let mut bytes = KEPT.to_vec();
    bytes.resize(8 * MIB + 1, b' ');
    bytes
}

/// A state file larger than its kind's cap is never taken into memory: left as it is (not moved aside), and no handle
/// writes over it, also one that never loaded it.
#[test]
fn a_state_file_over_its_cap_is_left_unread_and_never_written_over() {
    let f = fx();
    fs::create_dir(f.project.path().join(".permark")).unwrap();
    let store = f.store();
    let big = over_cap();
    let file = store.file::<Sample>("x.json");
    let dir = file.path().parent().unwrap().to_path_buf();
    fs::create_dir_all(&dir).unwrap();
    fs::write(file.path(), &big).unwrap();
    assert_eq!(file.load(), Loaded::Recovered { backup: None });
    assert!(file.save(&Sample::default()).is_err());
    assert!(store.file::<Sample>("x.json").save(&sample(1)).is_err());
    assert!(store
        .file::<Sample>("x.json")
        .update(|_| Some(sample(2)))
        .is_err());
    assert_eq!(fs::read(file.path()).unwrap(), big);
    // a file nobody loaded yet: a handle that never read it is refused all the same
    let blind = store.file::<Sample>("y.json");
    fs::write(blind.path(), &big).unwrap();
    assert!(blind.save(&sample(3)).is_err());
    assert_eq!(fs::read(blind.path()).unwrap(), big);
    // raw files in the state folder: the same default cap
    let raw = store.raw_file("quiet.json");
    fs::write(raw.path(), &big).unwrap();
    assert_eq!(
        raw.read().err().map(|e| e.kind()),
        Some(std::io::ErrorKind::InvalidData)
    );
    assert!(raw.update(|_| Some(b"{}".to_vec())).is_err());
    assert_eq!(fs::read(raw.path()).unwrap(), big);
    assert_eq!(broken_in(&dir), 0);
}

/// A state file a newer version wrote stays where it is (no copy beside it), loads as unreadable every time, and is
/// never written over: an older build used again never destroys what the newer one keeps.
#[test]
fn a_state_file_of_a_newer_version_is_left_in_place_and_never_written_over() {
    const NEWER: &[u8] = br#"{"version":4,"data":{"items":{"kept":7},"more":[1]}}"#;
    let f = fx();
    let store = f.store();
    let file = store.file::<Sample>("x.json");
    let dir = file.path().parent().unwrap().to_path_buf();
    fs::create_dir_all(&dir).unwrap();
    fs::write(file.path(), NEWER).unwrap();
    assert_eq!(file.load(), Loaded::Recovered { backup: None });
    assert_eq!(file.load(), Loaded::Recovered { backup: None });
    assert!(file.save(&Sample::default()).is_err());
    assert!(store.file::<Sample>("x.json").save(&sample(1)).is_err());
    assert!(store
        .file::<Sample>("x.json")
        .update(|_| Some(sample(2)))
        .is_err());
    assert_eq!(fs::read(file.path()).unwrap(), NEWER);
    // never loaded by anyone: a save still leaves it
    let blind = store.file::<Sample>("y.json");
    fs::write(blind.path(), NEWER).unwrap();
    assert!(blind.save(&sample(3)).is_err());
    assert_eq!(fs::read(blind.path()).unwrap(), NEWER);
    assert_eq!(broken_in(&dir), 0);
}

/// A link on the way to a state file (a junction | folder link inside `.permark/local/`) never leads a read outside
/// `.permark/`: the file stays unread and nothing is written through the link.
#[test]
fn a_link_inside_the_state_folder_never_leads_a_read_outside() {
    let f = fx();
    fs::create_dir(f.project.path().join(".permark")).unwrap();
    let store = f.store();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("f.json"), KEPT).unwrap();
    let id = PluginId::of("git");
    let plugins = store.dir().join("plugins");
    fs::create_dir_all(&plugins).unwrap();
    if !crate::tree::tests::link_out(&plugins.join("git"), outside.path()) {
        eprintln!("skipped: no folder links here");
        return;
    }
    let file = store.plugin_file::<Sample>(&id, "f.json");
    assert_eq!(file.load(), Loaded::Recovered { backup: None });
    assert!(file.save(&sample(1)).is_err());
    assert!(store.plugin_raw_file(&id, "f.json").read().is_err());
    assert_eq!(fs::read(outside.path().join("f.json")).unwrap(), KEPT);
}

/// A folder link higher in `.permark/local/` is refused before any name below it is looked up (following one could
/// reach a network share): a read stops at it, and a save names it, never a path behind it.
#[test]
fn a_folder_link_on_the_way_is_refused_before_anything_below_it_is_looked_up() {
    let f = fx();
    fs::create_dir(f.project.path().join(".permark")).unwrap();
    let store = f.store();
    let id = PluginId::of("git");
    fs::create_dir_all(store.dir()).unwrap();
    let plugins = store.dir().join("plugins");
    let outside = tempfile::tempdir().unwrap();
    let gone = outside.path().join("gone");
    fs::create_dir(&gone).unwrap();
    if !crate::tree::tests::link_out(&plugins, &gone) {
        eprintln!("skipped: no folder links here");
        return;
    }
    // a lookup through the link would find no file (missing); stopped at the link, the file is left unread
    fs::remove_dir(&gone).unwrap();
    let file = store.plugin_file::<Sample>(&id, "f.json");
    assert_eq!(file.load(), Loaded::Recovered { backup: None });
    // a save of a file that was never read stops at the link too, even after a read of it was refused
    assert!(store
        .plugin_file::<Sample>(&id, "f.json")
        .save(&sample(1))
        .is_err());
    // the link leads to a folder with a file where the plugin's folder would be: a save names the link itself
    fs::create_dir(&gone).unwrap();
    fs::write(gone.join("git"), KEPT).unwrap();
    let error = store
        .plugin_file::<Sample>(&id, "g.json")
        .save(&sample(1))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(&format!("{} is a link", plugins.display())),
        "{error}"
    );
    assert_eq!(fs::read(gone.join("git")).unwrap(), KEPT);
    assert_eq!(fs::read_dir(&gone).unwrap().count(), 1);
}

/// Runs `read` on another thread; `None` when it did not answer in time (a read waiting on a FIFO forever).
#[cfg(unix)]
fn answered<T: Send + 'static>(read: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(read());
    });
    rx.recv_timeout(std::time::Duration::from_secs(10)).ok()
}

/// Makes a FIFO at `path`.
#[cfg(unix)]
fn fifo(path: &Path) {
    let made = std::process::Command::new("mkfifo").arg(path).status();
    assert!(made.is_ok_and(|s| s.success()), "mkfifo {}", path.display());
}

/// A file link in place of a state file is never followed, and a FIFO (in `.permark/` or in the profile) never makes a
/// read wait: both stay as they are and nothing is written over them.
#[cfg(unix)]
#[test]
fn a_fifo_or_a_file_link_in_place_of_a_state_file_is_never_read_through() {
    let f = fx();
    fs::create_dir(f.project.path().join(".permark")).unwrap();
    let store = f.store();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("t.json");
    fs::write(&target, KEPT).unwrap();
    let file = store.file::<Sample>("x.json");
    fs::create_dir_all(file.path().parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, file.path()).unwrap();
    assert_eq!(file.load(), Loaded::Recovered { backup: None });
    assert!(fs::symlink_metadata(file.path())
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(store.file::<Sample>("x.json").save(&sample(1)).is_err());
    assert_eq!(fs::read(&target).unwrap(), KEPT);

    let local = store.file::<Sample>("fifo.json");
    fifo(local.path());
    let profile = store.profile_file::<Sample>("fifo.json");
    fs::create_dir_all(profile.path().parent().unwrap()).unwrap();
    fifo(profile.path());
    let legacy = store.legacy_shared_file("fifo.toml").unwrap();
    fifo(legacy.path());
    let blind = store.profile_file::<Sample>("blind.json");
    fifo(blind.path());
    let paths = [local.path(), profile.path(), legacy.path(), blind.path()].map(Path::to_path_buf);
    let answers = answered(move || {
        (
            local.load(),
            profile.load(),
            legacy.read().is_err(),
            blind.save(&sample(1)).is_err(),
        )
    })
    .expect("a read waited on a FIFO");
    assert_eq!(answers.0, Loaded::Recovered { backup: None });
    assert_eq!(answers.1, Loaded::Recovered { backup: None });
    assert!(answers.2 && answers.3);
    for path in paths {
        use std::os::unix::fs::FileTypeExt;
        assert!(fs::symlink_metadata(&path).unwrap().file_type().is_fifo());
    }
}

/// The team's files in `.permark/` (settings, project list, vocabulary, card) arrive with the folder: a link in place of
/// one is never followed, and a FIFO never makes the read wait.
#[cfg(unix)]
#[test]
fn team_files_are_read_only_as_plain_files_in_permark() {
    use crate::projects::{card, record, vocabulary};
    let f = fx();
    let root = f.root();
    let permark = root.path().join(PERMARK_DIR);
    fs::create_dir(&permark).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("config.toml");
    fs::write(&target, "[agent]\ndeny = [\"notes\"]\n").unwrap();
    std::os::unix::fs::symlink(&target, permark.join("config.toml")).unwrap();
    assert_eq!(
        crate::settings::ProjectConfig::load(&root),
        crate::settings::ProjectConfig::default()
    );
    fs::remove_file(permark.join("config.toml")).unwrap();
    for name in [
        "config.toml",
        record::RECORD_FILE,
        vocabulary::VOCABULARY_FILE,
        card::CARD_FILE,
    ] {
        fifo(&permark.join(name));
    }
    let dirs = f.dirs.clone();
    let answers = answered(move || {
        let card = match card::file_of(&dirs, root.path()) {
            Some(file @ card::CardFile::Project { .. }) => file.read(),
            _ => panic!("the card is the project's"),
        };
        (
            crate::settings::ProjectConfig::load(&root),
            record::read(root.path()),
            vocabulary::read(root.path()),
            card,
        )
    })
    .expect("a read waited on a FIFO");
    assert_eq!(answers.0, crate::settings::ProjectConfig::default());
    assert!(matches!(
        answers.1,
        crate::projects::RecordRead::Unreadable { .. }
    ));
    assert!(matches!(
        answers.2,
        crate::projects::VocabRead::Unreadable { .. }
    ));
    assert_eq!(answers.3.name, None);
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

/// Undo of an init carries `local/` back into the profile, but never the files that only the profile may hold: a
/// project's own `local/agent.json` must not switch the agent connection (or plugins) on in the profile.
#[test]
fn uninit_project_never_carries_profile_only_files_back() {
    let f = fx();
    let store = init_project(&f.dirs, &f.root()).unwrap();
    let profile = f
        .dirs
        .profile
        .join("projects")
        .join(ProjectId::of(&f.root()).as_str());
    fs::create_dir_all(&profile).unwrap();
    fs::write(profile.join("agent.json"), b"person's choice").unwrap();
    store
        .file::<Sample>("session.json")
        .save(&sample(5))
        .unwrap();
    let local = f.root().path().join(".permark/local");
    for name in PROFILE_ONLY.iter().chain(&SHARED_FILES) {
        fs::write(local.join(name), b"from the project").unwrap();
    }
    assert!(uninit_project(&f.dirs, &f.root()).unwrap());
    assert_eq!(
        fs::read(profile.join("agent.json")).unwrap(),
        b"person's choice"
    );
    for name in PROFILE_ONLY.iter().chain(&SHARED_FILES).skip(1) {
        assert!(!profile.join(name).exists(), "{name} stays out");
    }
    assert_eq!(
        f.store().file::<Sample>("session.json").load().into_value(),
        sample(5)
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

/// The PR #67 check: a file that could not be read once and is gone now holds nothing to lose. A handle that never read
/// it may write it again (the review keeps its copies of a version this way after it removed them as unused); one that
/// failed to read it still may not, and while the file is there a fresh handle is refused as before. Once a fresh
/// handle wrote it, the file holds only PerMark's own data: the stamp is gone and later fresh handles save it.
#[test]
fn a_handle_that_never_read_a_stamped_file_may_write_it_once_it_is_gone() {
    let f = fx();
    let failed = f.store().file::<Sample>("x.json");
    fs::create_dir_all(failed.path().parent().unwrap()).unwrap();
    fs::write(failed.path(), KEPT).unwrap();
    let held = unreadable(failed.path());
    if readable_anyway(failed.path()) {
        return;
    }
    assert_eq!(failed.load(), Loaded::Recovered { backup: None });
    drop(held);
    assert!(f.store().file::<Sample>("x.json").save(&sample(1)).is_err());
    assert_eq!(fs::read(failed.path()).unwrap(), KEPT);
    fs::remove_file(failed.path()).unwrap();
    f.store().file::<Sample>("x.json").save(&sample(2)).unwrap();
    assert_eq!(
        f.store().file::<Sample>("x.json").load(),
        Loaded::Found(sample(2))
    );
    assert!(
        failed.save(&Sample::default()).is_err(),
        "the handle holding defaults is still refused"
    );
    f.store()
        .file::<Sample>("x.json")
        .save(&sample(3))
        .expect("PerMark's own write cleared the stamp: a later fresh handle saves");
    assert_eq!(
        f.store().file::<Sample>("x.json").load(),
        Loaded::Found(sample(3))
    );
    assert!(
        failed.save(&Sample::default()).is_err(),
        "the handle holding defaults stays refused after the stamp is cleared"
    );
}

/// PR #67 delta check: only a not-found answer counts as gone; any entry (a folder here) counts as there, and a write
/// that did not happen leaves the stamp: the person's file put back afterwards is still never written over.
#[test]
fn a_folder_in_place_of_a_stamped_file_counts_as_there_and_keeps_the_stamp() {
    let f = fx();
    let failed = f.store().file::<Sample>("x.json");
    fs::create_dir_all(failed.path().parent().unwrap()).unwrap();
    fs::write(failed.path(), KEPT).unwrap();
    let held = unreadable(failed.path());
    if readable_anyway(failed.path()) {
        return;
    }
    assert_eq!(failed.load(), Loaded::Recovered { backup: None });
    drop(held);
    fs::remove_file(failed.path()).unwrap();
    fs::create_dir(failed.path()).unwrap();
    fs::write(failed.path().join("inside.txt"), b"k").unwrap();
    assert!(f.store().file::<Sample>("x.json").save(&sample(1)).is_err());
    assert_eq!(fs::read(failed.path().join("inside.txt")).unwrap(), b"k");
    fs::remove_dir_all(failed.path()).unwrap();
    fs::write(failed.path(), KEPT).unwrap();
    assert!(
        f.store().file::<Sample>("x.json").save(&sample(2)).is_err(),
        "the refused write kept the stamp"
    );
    assert_eq!(fs::read(failed.path()).unwrap(), KEPT);
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

/// P3: on a file no load ever failed on, fresh handles (several threads at once) save as before. Windows refuses a
/// rename over a file another rename is replacing right then (access denied); saves of one file in this process take
/// turns, so many savers at once never run out of retries (they did under a loaded runner with 4 savers).
#[test]
fn fresh_handles_on_a_file_that_never_failed_save_as_before() {
    let f = fx();
    let first = f.store().file::<Sample>("x.json");
    fs::create_dir_all(first.path().parent().unwrap()).unwrap();
    let store = f.store();
    std::thread::scope(|scope| {
        for n in 0..8u32 {
            let store = &store;
            scope.spawn(move || {
                for _ in 0..40 {
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

/// Every path a store hands out, in both locations and for the app-wide store: new state roots never move the files
/// earlier versions wrote.
#[test]
fn store_paths_are_pinned() {
    let f = fx();
    let git = PluginId::of("git");
    let id = ProjectId::of(&f.root());
    let profile = f.dirs.profile.join("projects").join(id.as_str());
    let cache = f.dirs.cache.join("projects").join(id.as_str());
    let path_of = |file: Option<RawFile>| file.map(|raw| raw.path().to_path_buf());
    let pinned = |s: &StateStore, dir: &Path, cache: &Path, shared: &Path, own: &Path| {
        assert_eq!(s.dir(), dir);
        assert_eq!(s.cache_dir(), cache);
        assert_eq!(s.file::<Sample>("x.json").path(), dir.join("x.json"));
        assert_eq!(s.raw_file("quiet.json").path(), dir.join("quiet.json"));
        assert_eq!(s.shared_file("meta.toml").path(), shared.join("meta.toml"));
        assert_eq!(
            s.profile_file::<Sample>("agent.json").path(),
            own.join("agent.json")
        );
        assert_eq!(s.app_dir(), f.dirs.profile);
        assert_eq!(
            s.app_file::<Sample>("a.json").path(),
            f.dirs.profile.join("a.json")
        );
        assert_eq!(
            s.cache_file::<Sample>("c.json").path(),
            cache.join("c.json")
        );
        let plugin = dir.join("plugins").join("git");
        assert_eq!(s.plugin_dir(&git), plugin);
        assert_eq!(
            s.plugin_file::<Sample>(&git, "p.json").path(),
            plugin.join("p.json")
        );
        assert_eq!(
            s.plugin_raw_file(&git, "r.json").path(),
            plugin.join("r.json")
        );
        let own_plugin = own.join("plugins").join("git");
        assert_eq!(s.profile_plugin_dir(&git), own_plugin);
        assert_eq!(
            s.profile_plugin_file::<Sample>(&git, "s.json").path(),
            own_plugin.join("s.json")
        );
    };
    // no `.permark/`: everything in the profile, keyed by the project id
    let s = f.store();
    assert_eq!(s.location(), StateLocation::Profile);
    pinned(&s, &profile, &cache, &profile, &profile);
    assert_eq!(path_of(s.legacy_shared_file("meta.toml")), None);
    // a real `.permark/`: private state in `local/`, caches in `cache/`, shared files in `.permark/` itself
    let permark = f.root().path().join(".permark");
    fs::create_dir(&permark).unwrap();
    let s = f.store();
    assert_eq!(s.location(), StateLocation::Project);
    pinned(
        &s,
        &permark.join("local"),
        &permark.join("cache"),
        &permark,
        &profile,
    );
    assert_eq!(
        path_of(s.legacy_shared_file("meta.toml")),
        Some(profile.join("meta.toml"))
    );
    // a file in place of `cache/`: caches stay in the profile cache
    fs::write(permark.join("cache"), "x").unwrap();
    assert_eq!(f.store().cache_dir(), cache);
    // app-wide: the profile folder itself
    let app = StateStore::app(&f.dirs);
    assert_eq!(app.location(), StateLocation::Profile);
    pinned(
        &app,
        &f.dirs.profile,
        &f.dirs.cache,
        &f.dirs.profile,
        &f.dirs.profile,
    );
}

/// Every path a project's store hands out.
#[derive(Debug, PartialEq)]
struct Resolved {
    location: StateLocation,
    team: bool,
    dir: PathBuf,
    cache_dir: PathBuf,
    cache_file: PathBuf,
    machine: Option<PathBuf>,
    shared_file: PathBuf,
    legacy_shared_file: Option<PathBuf>,
    profile_file: PathBuf,
    plugin_dir: PathBuf,
    profile_plugin_dir: PathBuf,
}

impl Resolved {
    fn of(s: &StateStore) -> Self {
        let git = PluginId::of("git");
        Self {
            location: s.location(),
            team: s.has_team_folder(),
            dir: s.dir().to_path_buf(),
            cache_dir: s.cache_dir().to_path_buf(),
            cache_file: s.cache_file::<Sample>("c.json").path().to_path_buf(),
            machine: s.machine().map(|m| m.dir().to_path_buf()),
            shared_file: s.shared_file("meta.toml").path().to_path_buf(),
            legacy_shared_file: s
                .legacy_shared_file("meta.toml")
                .map(|raw| raw.path().to_path_buf()),
            profile_file: s.profile_file::<Sample>("agent.json").path().to_path_buf(),
            plugin_dir: s.plugin_dir(&git),
            profile_plugin_dir: s.profile_plugin_dir(&git),
        }
    }

    /// The answer for a store of `f`'s folder whose state sits at `place` (install id `iid`).
    fn at(f: &Fx, iid: &str, place: Place) -> Self {
        let id = ProjectId::of(&f.root());
        let profile = f.dirs.profile.join("projects").join(id.as_str());
        let profile_cache = f.dirs.cache.join("projects").join(id.as_str());
        let permark = f.root().path().join(PERMARK_DIR);
        let (dir, cache) = match place {
            Place::Profile | Place::TeamOnly => (profile.clone(), profile_cache),
            Place::Project => (permark.join("local"), permark.join("cache")),
            Place::ProjectStateOnly => (permark.join("local"), profile_cache),
        };
        // shared files in `.permark/` (the profile copy kept apart) whenever it is the team folder
        let team = !matches!(place, Place::Profile);
        let shared = if team { &permark } else { &profile };
        Self {
            location: if matches!(place, Place::Profile | Place::TeamOnly) {
                StateLocation::Profile
            } else {
                StateLocation::Project
            },
            team,
            cache_file: cache.join("c.json"),
            cache_dir: cache,
            machine: Some(dir.join("machine").join(iid)),
            shared_file: shared.join("meta.toml"),
            legacy_shared_file: team.then(|| profile.join("meta.toml")),
            profile_file: profile.join("agent.json"),
            plugin_dir: dir.join("plugins").join("git"),
            profile_plugin_dir: profile.join("plugins").join("git"),
            dir,
        }
    }
}

/// Where a project store's state sits.
#[derive(Clone, Copy)]
enum Place {
    Profile,
    Project,
    /// In the project, caches in the profile cache.
    ProjectStateOnly,
    /// In the profile; only the team files in `.permark/`.
    TeamOnly,
}

impl Fx {
    fn permark(&self) -> PathBuf {
        self.project.path().join(PERMARK_DIR)
    }
}

/// Where a project's store sits for each layout its folder can have: `.permark/` a real folder (with `local/` real or
/// missing) -> `local/`, caches in `cache/` unless something else is there; anything else (no `.permark/`, a file or a
/// folder link in its place, a file | link as `local/`) -> the profile. Team files follow `.permark/` whenever it is a
/// real folder, also while the private state stays in the profile. Resolving writes nothing into the folder and
/// nothing through a link; a second store of the same folder gives the same answer.
#[test]
fn every_path_a_store_hands_out_is_pinned_for_each_folder_layout() {
    // (layout, made in the folder: `false` -> the system cannot make it here, skipped)
    type Make = fn(&Fx, &Path) -> bool;
    let mut layouts: Vec<(&str, Make, Place)> = vec![
        ("no .permark", |_, _| true, Place::Profile),
        (
            "profile state only",
            |f, _| {
                f.store()
                    .file::<Sample>("session.json")
                    .save(&sample(1))
                    .is_ok()
            },
            Place::Profile,
        ),
        (
            ".permark, local missing",
            |f, _| fs::create_dir(f.permark()).is_ok(),
            Place::Project,
        ),
        (
            ".permark with local",
            |f, _| fs::create_dir_all(f.permark().join("local")).is_ok(),
            Place::Project,
        ),
        (
            ".permark a file",
            |f, _| fs::write(f.permark(), "x").is_ok(),
            Place::Profile,
        ),
        (
            ".permark a junction | symlink",
            |f, out| crate::tree::tests::link_out(&f.permark(), out),
            Place::Profile,
        ),
        (
            "local a file",
            |f, _| {
                fs::create_dir(f.permark()).is_ok()
                    && fs::write(f.permark().join("local"), "x").is_ok()
            },
            Place::TeamOnly,
        ),
        (
            "local a junction | symlink",
            |f, out| {
                fs::create_dir(f.permark()).is_ok()
                    && crate::tree::tests::link_out(&f.permark().join("local"), out)
            },
            Place::TeamOnly,
        ),
        (
            "cache a file",
            |f, _| {
                fs::create_dir(f.permark()).is_ok()
                    && fs::write(f.permark().join("cache"), "x").is_ok()
            },
            Place::ProjectStateOnly,
        ),
    ];
    // Windows makes directory symlinks only with the privilege (developer mode): skipped without it
    #[cfg(windows)]
    layouts.push((
        ".permark a directory symlink",
        |f, out| std::os::windows::fs::symlink_dir(out, f.permark()).is_ok(),
        Place::Profile,
    ));
    for (name, make, place) in layouts {
        let f = fx();
        let outside = tempfile::tempdir().unwrap();
        if !make(&f, outside.path()) {
            eprintln!("skipped {name}: cannot be made here");
            continue;
        }
        let before = entries_below(f.project.path());
        let s = f.store();
        let got = Resolved::of(&s);
        assert_eq!(
            has_team_folder(&f.root()),
            got.team,
            "{name}: the folder's own answer"
        );
        let iid = s.machine().unwrap().iid().to_owned();
        assert_eq!(got, Resolved::at(&f, &iid, place), "{name}");
        assert_eq!(Resolved::of(&f.store()), got, "{name}: a second store");
        assert_eq!(
            entries_below(f.project.path()),
            before,
            "{name}: folder untouched"
        );
        assert!(
            fs::read_dir(outside.path()).unwrap().next().is_none(),
            "{name}: nothing behind a link"
        );
    }
}

/// Undo of an init takes `.permark/` back only while `local/` and `cache/` hold no link anywhere: a junction | symlink
/// inside or in their place, or a file in place of `local/`, leaves `.permark/` as it is (nothing carried back,
/// nothing removed, the link's target untouched).
#[test]
fn uninit_project_takes_back_only_state_folders_without_a_link() {
    fn local(permark: &Path) -> PathBuf {
        permark.join("local")
    }
    // (what is put into the fresh `.permark/`, taken back; `false` from the maker -> cannot be made here, skipped)
    type Make = fn(&Path, &Path) -> bool;
    let mut cases: Vec<(&str, Make, bool)> = vec![
        (
            "plain folder + file in local",
            |p, _| {
                fs::create_dir(local(p).join("sub")).is_ok()
                    && fs::write(local(p).join("sub").join("a.json"), "{}").is_ok()
            },
            true,
        ),
        (
            "junction | symlink in local",
            |p, out| crate::tree::tests::link_out(&local(p).join("elsewhere"), out),
            false,
        ),
        (
            "junction | symlink as cache",
            |p, out| {
                fs::remove_dir_all(p.join("cache")).is_ok()
                    && crate::tree::tests::link_out(&p.join("cache"), out)
            },
            false,
        ),
        (
            "file in place of local",
            |p, _| fs::remove_dir_all(local(p)).is_ok() && fs::write(local(p), "x").is_ok(),
            false,
        ),
    ];
    #[cfg(unix)]
    cases.push((
        "file symlink in local",
        |p, out| {
            std::os::unix::fs::symlink(out.join("kept.txt"), local(p).join("kept.txt")).is_ok()
        },
        false,
    ));
    // Windows makes symlinks only with the privilege (developer mode): skipped without it
    #[cfg(windows)]
    cases.push((
        "directory symlink in local",
        |p, out| std::os::windows::fs::symlink_dir(out, local(p).join("elsewhere")).is_ok(),
        false,
    ));
    for (name, make, taken) in cases {
        let f = fx();
        init_project(&f.dirs, &f.root()).unwrap();
        let permark = f.root().path().join(PERMARK_DIR);
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("kept.txt"), b"theirs").unwrap();
        if !make(&permark, outside.path()) {
            eprintln!("skipped {name}: cannot be made here");
            continue;
        }
        assert_eq!(uninit_project(&f.dirs, &f.root()).unwrap(), taken, "{name}");
        assert_eq!(
            fs::symlink_metadata(&permark).is_ok(),
            !taken,
            "{name}: .permark kept"
        );
        assert_eq!(
            entries_below(outside.path()),
            ["kept.txt"],
            "{name}: target untouched"
        );
        assert_eq!(
            fs::read(outside.path().join("kept.txt")).unwrap(),
            b"theirs"
        );
    }
}

/// A cloud placeholder folder (OneDrive Files On-Demand) carries a reparse point that is no link: std reports a plain
/// folder. Where the private state lives keeps the rule of earlier versions, which counted any reparse point as a link
/// (the state stays in the profile, where they kept it), while `.permark/` is the team folder: team files are written
/// there, the project list, the tag vocabulary and the card are read there, and its `local/` is where a context file
/// sits. No test can make a cloud folder: the earlier rule's predicate is swapped for one that calls everything a link,
/// and std answers for the folder as for a plain one.
#[test]
fn a_cloud_folder_is_the_team_folder_while_the_private_state_stays_where_earlier_versions_kept_it()
{
    use crate::projects::{card, record, vocabulary};
    let f = fx();
    let permark = f.permark();
    fs::create_dir_all(permark.join("local")).unwrap();
    fs::write(permark.join(record::RECORD_FILE), "version = 1\n").unwrap();
    fs::write(
        permark.join(vocabulary::VOCABULARY_FILE),
        "version = 1\n\n[tags.spec]\n",
    )
    .unwrap();
    let cloud = StateStore::for_project_as(&f.dirs, &f.root(), |_| true);
    let iid = cloud.machine().unwrap().iid().to_owned();
    assert_eq!(
        Resolved::of(&cloud),
        Resolved::at(&f, &iid, Place::TeamOnly)
    );
    assert!(has_team_folder(&f.root()));
    cloud.shared_file("meta.toml").write(b"x = 1\n").unwrap();
    assert_eq!(fs::read(permark.join("meta.toml")).unwrap(), b"x = 1\n");
    assert!(matches!(
        record::read(f.root().path()),
        crate::projects::RecordRead::Read(_)
    ));
    let vocab = vocabulary::read(f.root().path());
    assert!(
        vocab.vocab().is_some_and(|v| v.tags.contains_key("spec")),
        "{vocab:?}"
    );
    let place = card::file_of(&f.dirs, f.root().path()).map(|file| file.place());
    assert_eq!(place, Some(card::CardPlace::Project));
    assert_eq!(
        cloud
            .local_raw_file(context::FILE)
            .map(|file| file.path().to_path_buf()),
        Some(permark.join("local").join(context::FILE))
    );
    // the same folder under the earlier rule's own predicate: a plain folder, so the state is in `local/`
    assert_eq!(f.store().location(), StateLocation::Project);
}

/// The project's `local/` is offered whatever place holds the private state, only while `.permark/` is a real folder
/// and `local/` is a real folder or missing; it is the store's own state folder when the state is there.
#[test]
fn the_local_folder_is_offered_only_inside_a_real_team_folder() {
    // (layout, made in the folder: `false` -> the system cannot make it here, skipped; offered)
    type Make = fn(&Fx, &Path) -> bool;
    let layouts: Vec<(&str, Make, bool)> = vec![
        ("no .permark", |_, _| true, false),
        (
            ".permark, local missing",
            |f, _| fs::create_dir(f.permark()).is_ok(),
            true,
        ),
        (
            ".permark a file",
            |f, _| fs::write(f.permark(), "x").is_ok(),
            false,
        ),
        (
            ".permark a junction | symlink",
            |f, out| crate::tree::tests::link_out(&f.permark(), out),
            false,
        ),
        (
            "local a file",
            |f, _| {
                fs::create_dir(f.permark()).is_ok()
                    && fs::write(f.permark().join("local"), "x").is_ok()
            },
            false,
        ),
        (
            "local a junction | symlink",
            |f, out| {
                fs::create_dir(f.permark()).is_ok()
                    && crate::tree::tests::link_out(&f.permark().join("local"), out)
            },
            false,
        ),
    ];
    for (name, make, offered) in layouts {
        let f = fx();
        let outside = tempfile::tempdir().unwrap();
        if !make(&f, outside.path()) {
            eprintln!("skipped {name}: cannot be made here");
            continue;
        }
        let s = f.store();
        let file = s.local_raw_file("x.json");
        assert_eq!(file.is_some(), offered, "{name}");
        if let Some(file) = file {
            assert_eq!(file.path(), s.raw_file("x.json").path(), "{name}");
            file.write(b"{}").unwrap();
            assert_eq!(
                fs::read(f.permark().join("local").join("x.json")).unwrap(),
                b"{}"
            );
        }
        assert!(
            fs::read_dir(outside.path()).unwrap().next().is_none(),
            "{name}: nothing behind a link"
        );
    }
}

/// Pins a known defect, kept visible until it is fixed: Initialize copies a plugin's profile-only folder
/// ([`StateStore::profile_plugin_dir`]) into `.permark/local/` with the rest of the profile state. The copy is not
/// wanted: those folders stay in the profile on purpose (the Assistant's work copy of the project and its saved
/// conversation), since inside the project a tool walking up from the work copy finds the project, and a synced or
/// shared folder carries the conversation to other computers. Nothing reads the copy (the store keeps answering the
/// profile folder).
#[test]
fn init_project_today_copies_profile_pinned_plugin_folders() {
    let f = fx();
    let before = f.store();
    let acp = PluginId::of("acp");
    let pinned = before.profile_plugin_dir(&acp);
    fs::create_dir_all(pinned.join("work").join("proj")).unwrap();
    fs::write(pinned.join("work").join("proj").join("doc.md"), b"# copy").unwrap();
    fs::create_dir_all(pinned.join("run")).unwrap();
    before
        .profile_plugin_file::<Sample>(&acp, "session.json")
        .save(&sample(1))
        .unwrap();
    let after = init_project(&f.dirs, &f.root()).unwrap();
    assert_eq!(after.location(), StateLocation::Project);
    let copied = after.dir().join("plugins").join("acp");
    assert_eq!(
        entries_below(&copied),
        [
            "run/",
            "session.json",
            "work/",
            "work/proj/",
            "work/proj/doc.md"
        ]
    );
    assert_eq!(
        fs::read(copied.join("work").join("proj").join("doc.md")).unwrap(),
        b"# copy"
    );
    // the profile keeps its folder whole, and every store of the folder still answers it
    assert_eq!(after.profile_plugin_dir(&acp), pinned);
    assert_eq!(entries_below(&pinned), entries_below(&copied));
    assert_eq!(
        after
            .profile_plugin_file::<Sample>(&acp, "session.json")
            .load(),
        Loaded::Found(sample(1))
    );
}

#[test]
fn app_dirs_under_layout_is_pinned() {
    let base = Path::new("base");
    let dirs = AppDirs::under(base);
    assert_eq!(dirs.profile, base.join("profile"));
    assert_eq!(dirs.cache, base.join("cache"));
    let Some(system) = AppDirs::system() else {
        return;
    };
    // debug builds (the tests) never touch the person's real state
    let name = if cfg!(debug_assertions) {
        "PerMark-dev"
    } else {
        "PerMark"
    };
    assert!(system.profile.ends_with(name));
    assert!(system.cache.ends_with(name));
}

#[test]
fn the_install_folder_stays_apart_and_a_snap_keeps_it_across_revisions() {
    let base = Path::new("base");
    assert_eq!(AppDirs::under(base).install, base.join("install"));
    if let Some(system) = AppDirs::system() {
        if std::env::var_os("SNAP").is_none() {
            let name = if cfg!(debug_assertions) {
                "PerMark-dev"
            } else {
                "PerMark"
            };
            assert!(system.install.ends_with(Path::new(name).join("install")));
        }
    }
    let snap = Some(std::ffi::OsStr::new("/snap/permark/12"));
    let common = Some(std::ffi::OsStr::new("/home/a/snap/permark/common"));
    assert_eq!(
        snap_install(snap, common),
        Some(Path::new("/home/a/snap/permark/common").join("install"))
    );
    assert_eq!(snap_install(Some(std::ffi::OsStr::new("")), common), None);
    assert_eq!(snap_install(None, common), None);
    assert_eq!(snap_install(snap, None), None);
    assert_eq!(
        snap_install(snap, Some(std::ffi::OsStr::new("snap/permark/common"))),
        None
    );
}

#[test]
fn machine_files_sit_under_the_install_id_in_both_locations() {
    let f = fx();
    let journal = |dir: &Path| dir.join("fileops").join("journal.json");
    // no `.permark/`: the project's profile folder
    let s = f.store();
    let m = s.machine().unwrap();
    assert_eq!(m.iid().len(), 16);
    assert!(m.iid().bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(m.dir(), s.dir().join("machine").join(m.iid()));
    let file = s.machine_file::<Sample>("fileops/journal.json").unwrap();
    assert_eq!(file.path(), journal(m.dir()));
    file.save(&sample(1)).unwrap();
    assert_eq!(file.load(), Loaded::Found(sample(1)));
    assert_eq!(
        s.machine_raw_file("x.json").unwrap().path(),
        m.dir().join("x.json")
    );
    assert_eq!(
        s.install_file::<Sample>("marks.json").unwrap().path(),
        f.dirs.install.join("marks.json")
    );
    assert_eq!(s.app_dirs(), &f.dirs);
    // a real `.permark/`: inside `local/`, same install id, same folder id
    let permark = f.root().path().join(".permark");
    fs::create_dir(&permark).unwrap();
    let p = f.store();
    let pm = p.machine().unwrap();
    assert_eq!((pm.iid(), pm.rid()), (m.iid(), m.rid()));
    assert_eq!(
        pm.dir(),
        permark.join("local").join("machine").join(pm.iid())
    );
    p.machine_file::<Sample>("x.json")
        .unwrap()
        .save(&sample(2))
        .unwrap();
    assert!(pm.dir().join("x.json").is_file());
    // app-wide: no folder to bind data to; the install folder is still there
    let app = StateStore::app(&f.dirs);
    assert!(app.machine().is_none());
    assert!(app.machine_file::<Sample>("x.json").is_none());
    assert!(app.machine_raw_file("x.json").is_none());
    assert_eq!(
        app.install_file::<Sample>("marks.json").unwrap().path(),
        f.dirs.install.join("marks.json")
    );
}

#[test]
fn a_machine_file_is_guarded_like_the_other_state_files() {
    let f = fx();
    fs::create_dir(f.project.path().join(".permark")).unwrap();
    let s = f.store();
    let file = s.machine_file::<Sample>("x.json").unwrap();
    let local = f.root().path().join(".permark").join("local");
    fs::write(&local, "now a file").unwrap();
    assert!(file.save(&sample(1)).is_err());
    assert!(s.machine_raw_file("y.json").unwrap().write(b"y").is_err());
    assert_eq!(fs::read_to_string(&local).unwrap(), "now a file");
}

#[test]
fn the_folder_id_follows_the_folder_and_the_install_and_is_never_stored() {
    let f = fx();
    let store = f.store();
    let rid = store.machine().unwrap().rid().to_owned();
    assert_eq!(rid.len(), 32);
    assert!(rid.bytes().all(|b| b.is_ascii_hexdigit()));
    // the same folder opened again: the same id
    assert_eq!(f.store().machine().unwrap().rid(), rid);
    // another folder: another id under the same install id
    let other = tempfile::tempdir().unwrap();
    let o = StateStore::for_project(&f.dirs, &Root::open(other.path()).unwrap());
    assert_ne!(o.machine().unwrap().rid(), rid);
    assert_eq!(o.machine().unwrap().iid(), store.machine().unwrap().iid());
    // a folder made again at the same path: another id wherever the disk tells the two apart
    let again = other.path().join("again");
    fs::create_dir(&again).unwrap();
    let first = crate::fsx::FolderIdentity::of(&again);
    let before = StateStore::for_project(&f.dirs, &Root::open(&again).unwrap());
    let before = before.machine().unwrap().rid().to_owned();
    fs::remove_dir(&again).unwrap();
    fs::create_dir(&again).unwrap();
    let after = StateStore::for_project(&f.dirs, &Root::open(&again).unwrap());
    let told_apart = matches!(
        (first, crate::fsx::FolderIdentity::of(&again)),
        (Some(a), Some(b)) if !a.same(b)
    );
    if told_apart {
        assert_ne!(after.machine().unwrap().rid(), before);
    }
    // another install (another key): other ids for the same folder
    let base = tempfile::tempdir().unwrap();
    let elsewhere = StateStore::for_project(&AppDirs::under(base.path()), &f.root());
    let em = elsewhere.machine().unwrap();
    assert_ne!(em.rid(), rid);
    assert_ne!(em.iid(), store.machine().unwrap().iid());
    // only the key is kept: the folder id is worked out again at each open
    let kept: Vec<String> = fs::read_dir(&f.dirs.install)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(kept, ["install.key"]);
}

#[test]
fn clones_of_a_store_share_one_machine_and_no_key_means_none() {
    let f = fx();
    let s = f.store();
    let copy = s.clone();
    assert!(std::ptr::eq(s.machine().unwrap(), copy.machine().unwrap()));
    // an install folder that cannot be made (a file in the way): no key, no machine data, no install files
    let base = tempfile::tempdir().unwrap();
    let mut dirs = AppDirs::under(base.path());
    fs::write(base.path().join("blocked"), "x").unwrap();
    dirs.install = base.path().join("blocked").join("install");
    let blocked = StateStore::for_project(&dirs, &f.root());
    assert!(blocked.machine().is_none());
    assert!(blocked.machine_file::<Sample>("x.json").is_none());
    assert!(blocked.install_file::<Sample>("marks.json").is_none());
}
