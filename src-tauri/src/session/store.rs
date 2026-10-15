//! `StateStore`: where a project's session and last-visit data live, and `.trailmark/` initialization.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::config::{Config, DEFAULT_CONFIG_TOML};
use super::private;
use super::window::{self, WindowState};
use super::{SessionError, SessionState};
use crate::util::{hash_hex, write_atomic};

const SESSION_FILE: &str = "session.json";
const LAST_VISIT_FILE: &str = "last_visit.json";
const READ_FILE: &str = "read.json";
const NOTES_FILE: &str = "notes.json";
const AGENT_FILE: &str = "agent.json";
const META_FILE: &str = "meta.toml";
/// The agent connection's switch, in the project's folder in AppData (see [`StateStore::agent_connection`]).
const CONNECTION_FILE: &str = "agent-connection.json";
const TRAILMARK_DIR: &str = ".trailmark";
const PROJECT_GITIGNORE: &str = "\
# TrailMark private state: not shared.
cache/
local/
";

/// Where state is stored (the UI's `"appData" | "project"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StateLocation {
    AppData,
    Project,
}

/// A file as it was when the project was closed: its size and a hash of its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FileStamp {
    pub len: u64,
    pub hash: u64,
}

/// When the project was last closed, where HEAD pointed then, and which files were already
/// uncommitted (so "since your last visit" can tell them apart from changes made afterwards).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastVisit {
    pub time_ms: u64,
    pub oid: Option<String>,
    /// Files that differed from HEAD at the close, by vault path. Files saved by an older version
    /// of the app have none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dirty: BTreeMap<String, FileStamp>,
}

/// The agent connection's switch as it is kept.
#[derive(Serialize, Deserialize)]
struct ConnectionSwitch {
    enabled: bool,
}

#[derive(Debug)]
pub struct StateStore {
    /// Canonical project root.
    root: PathBuf,
    location: StateLocation,
    /// Directory holding `session.json` and `last_visit.json`.
    dir: PathBuf,
    /// This project's folder in AppData, which holds the state of a project that has no `.trailmark/`.
    appdata_dir: PathBuf,
}

/// Stable, case-insensitive-on-Windows key for a project folder.
pub(crate) fn project_key(root: &Path) -> String {
    let mut s = dunce::simplified(root).to_string_lossy().replace('\\', "/");
    while s.ends_with('/') && s.len() > 1 {
        s.pop();
    }
    if cfg!(windows) {
        s = s.to_lowercase();
    }
    hash_hex(s.as_bytes())
}

impl StateStore {
    /// Uses `.trailmark/local/` when the project has a `.trailmark/` folder, AppData otherwise. A `.trailmark/` or
    /// `.trailmark/local/` that is a link (a repository can carry one) is not used: the state stays in AppData, so
    /// nothing is written or deleted wherever the link points.
    pub fn open(app_data_dir: &Path, root: &Path) -> Self {
        // Canonical when possible, so every spelling of a folder (8.3 short names, `..`, case on Windows) maps to
        // one key. The workspace already passes the vault's canonical root, for which this changes nothing.
        let root =
            dunce::canonicalize(root).unwrap_or_else(|_| dunce::simplified(root).to_path_buf());
        let trailmark = root.join(TRAILMARK_DIR);
        let outside = app_data_dir.join("projects").join(project_key(&root));
        let (location, dir) = if private::usable_in(&trailmark) {
            (StateLocation::Project, trailmark.join("local"))
        } else {
            (StateLocation::AppData, outside.clone())
        };
        Self {
            root,
            location,
            dir,
            appdata_dir: outside,
        }
    }

    pub fn location(&self) -> StateLocation {
        self.location
    }

    /// Directory with the state files (it may not exist until something is saved).
    #[cfg(test)]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where the unread markers keep what was last read: `read.json` with the rest of the local state.
    pub fn read_path(&self) -> PathBuf {
        self.dir.join(READ_FILE)
    }

    /// Where the review notes are kept: `notes.json` with the rest of the local state. Personal, like `read.json`.
    pub fn notes_path(&self) -> PathBuf {
        self.dir.join(NOTES_FILE)
    }

    /// Where the agent connection keeps what the person is looking at: `agent.json` with the rest of the local
    /// state, but only in a project that keeps its state in `.trailmark/` (nothing is written into AppData for
    /// other programs to find).
    pub fn agent_path(&self) -> Option<PathBuf> {
        (self.location == StateLocation::Project).then(|| self.dir.join(AGENT_FILE))
    }

    /// Whether TrailMark has been used on this folder: it has a `.trailmark/` folder, or the app kept state for it.
    /// This is how a program started somewhere inside a project finds the project.
    pub fn is_known(&self) -> bool {
        self.location == StateLocation::Project || self.dir.is_dir()
    }

    /// Where the tags and descriptions added in the app are kept: `.trailmark/meta.toml` (shared with
    /// the project) once the project has a `.trailmark/` folder, a file next to the AppData state before.
    pub fn meta_path(&self) -> PathBuf {
        match self.location {
            StateLocation::Project => self.root.join(TRAILMARK_DIR).join(META_FILE),
            StateLocation::AppData => self.appdata_dir.join(META_FILE),
        }
    }

    /// The AppData `meta.toml` of a project that has a `.trailmark/` folder now: tags added before the
    /// folder existed may still be in it, and have to be merged into the project's file.
    pub fn legacy_meta_path(&self) -> Option<PathBuf> {
        (self.location == StateLocation::Project).then(|| self.appdata_dir.join(META_FILE))
    }

    /// Whether the person turned the agent connection on for this project.
    ///
    /// The switch lives only in the app's own data folder, never in the project: a project's files are content that
    /// somebody else may have written (a cloned repository can carry a `.trailmark/` folder), and what they say must
    /// not be able to share the person's reading with an agent. Nothing in `.trailmark/` is read for it.
    pub fn agent_connection(&self) -> bool {
        std::fs::read(self.appdata_dir.join(CONNECTION_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ConnectionSwitch>(&bytes).ok())
            .is_some_and(|switch| switch.enabled)
    }

    /// Remembers the switch of the agent connection (see [`StateStore::agent_connection`]). Turning it off for a
    /// project that never had it on writes nothing.
    pub fn set_agent_connection(&self, enabled: bool) -> Result<(), SessionError> {
        let path = self.appdata_dir.join(CONNECTION_FILE);
        if !enabled && !path.exists() {
            return Ok(());
        }
        let bytes = serde_json::to_vec(&ConnectionSwitch { enabled })
            .map_err(|e| SessionError::Json(e.to_string()))?;
        write_atomic(&path, &bytes).map_err(|e| SessionError::io(&path, e))
    }

    /// Project settings from `.trailmark/config.toml`; defaults when missing or invalid.
    pub fn config(&self) -> Config {
        let trailmark = self.root.join(TRAILMARK_DIR);
        if !private::is_real_folder(&trailmark) {
            return Config::default();
        }
        std::fs::read_to_string(trailmark.join("config.toml"))
            .ok()
            .and_then(|t| Config::parse(&t).ok())
            .unwrap_or_default()
    }

    fn read<T: for<'de> Deserialize<'de>>(&self, file: &str) -> Option<T> {
        let bytes = std::fs::read(self.dir.join(file)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn write<T: Serialize>(&self, file: &str, value: &T) -> Result<(), SessionError> {
        let bytes =
            serde_json::to_vec_pretty(value).map_err(|e| SessionError::Json(e.to_string()))?;
        let path = self.dir.join(file);
        private::write_local_state(&path, &bytes).map_err(|e| SessionError::io(&path, e))
    }

    /// The saved session, if any (unreadable data counts as none).
    pub fn load_session(&self) -> Option<SessionState> {
        self.read(SESSION_FILE)
    }

    pub fn save_session(&self, session: &SessionState) -> Result<(), SessionError> {
        self.write(SESSION_FILE, session)
    }

    pub fn load_last_visit(&self) -> Option<LastVisit> {
        self.read(LAST_VISIT_FILE)
    }

    pub fn save_last_visit(&self, visit: &LastVisit) -> Result<(), SessionError> {
        self.write(LAST_VISIT_FILE, visit)
    }

    /// The window geometry this project had the last time it was open, if any.
    pub fn load_window(&self) -> Option<WindowState> {
        WindowState::load(&self.dir)
    }

    pub fn save_window(&self, state: &WindowState) -> Result<(), SessionError> {
        state.save(&self.dir)
    }

    /// Creates `.trailmark/{config.toml, .gitignore, cache/, local/}` (existing files are kept; `cache/` and `local/`
    /// ignore themselves), carries the AppData state over, and switches to project storage. A `.trailmark/` or one
    /// of its folders that is a link is refused.
    pub fn init_project_folder(&mut self) -> Result<(), SessionError> {
        let tm = self.root.join(TRAILMARK_DIR);
        let io = |p: &Path| {
            let p = p.to_path_buf();
            move |e| SessionError::io(&p, e)
        };
        for sub in ["cache", "local"] {
            let dir = tm.join(sub);
            private::prepare(&dir).map_err(io(&dir))?;
        }
        for (name, content) in [
            ("config.toml", DEFAULT_CONFIG_TOML),
            (".gitignore", PROJECT_GITIGNORE),
        ] {
            let path = tm.join(name);
            private::create_if_missing(&path, content).map_err(io(&path))?;
        }
        let local = tm.join("local");
        if self.location == StateLocation::AppData {
            for file in [
                SESSION_FILE,
                LAST_VISIT_FILE,
                READ_FILE,
                NOTES_FILE,
                window::FILE_NAME,
            ] {
                let (from, to) = (self.dir.join(file), local.join(file));
                // nothing there at all: a link in place of the file would be written through
                if from.is_file() && std::fs::symlink_metadata(&to).is_err() {
                    std::fs::copy(&from, &to).map_err(io(&from))?;
                }
            }
        }
        self.location = StateLocation::Project;
        self.dir = local;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Theme;
    use std::fs;

    struct Fx {
        app: tempfile::TempDir,
        project: tempfile::TempDir,
    }

    fn fx() -> Fx {
        Fx {
            app: tempfile::tempdir().expect("tempdir"),
            project: tempfile::tempdir().expect("tempdir"),
        }
    }

    impl Fx {
        fn store(&self) -> StateStore {
            StateStore::open(self.app.path(), self.project.path())
        }
    }

    #[test]
    fn every_spelling_of_a_folder_shares_one_key() {
        let f = fx();
        std::fs::create_dir(f.project.path().join("sub")).expect("mkdir");
        let roundabout = f.project.path().join("sub").join("..");
        assert_eq!(
            StateStore::open(f.app.path(), &roundabout).dir(),
            f.store().dir()
        );
    }

    #[test]
    fn default_location_is_appdata_keyed_by_the_root_hash() {
        let f = fx();
        let s = f.store();
        assert_eq!(s.location(), StateLocation::AppData);
        let projects = f.app.path().join("projects");
        assert_eq!(s.dir().parent(), Some(projects.as_path()));
        let key = s
            .dir()
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        assert_eq!(key.len(), 16);
        // Same root, same folder; another root, another folder.
        assert_eq!(f.store().dir(), s.dir());
        let other = tempfile::tempdir().expect("tempdir");
        assert_ne!(StateStore::open(f.app.path(), other.path()).dir(), s.dir());
        // The project folder itself is untouched.
        assert!(fs::read_dir(f.project.path()).expect("ls").next().is_none());
    }

    #[test]
    fn tags_live_in_appdata_until_the_project_is_initialized_and_then_in_meta_toml() {
        let f = fx();
        let mut s = f.store();
        assert_eq!(s.meta_path(), s.dir().join("meta.toml"));
        assert!(s.meta_path().starts_with(f.app.path()));
        s.init_project_folder().expect("init");
        let root = dunce::canonicalize(f.project.path()).expect("canonical");
        let project_file = root.join(".trailmark").join("meta.toml");
        assert_eq!(s.meta_path(), project_file);
        // The shared file is not in the gitignored local/ folder.
        assert!(!s.meta_path().starts_with(s.dir()));
        assert_eq!(f.store().meta_path(), project_file);
    }

    #[test]
    fn the_appdata_meta_file_is_only_a_legacy_once_the_project_has_a_trailmark_folder() {
        let f = fx();
        let s = f.store();
        assert_eq!(
            s.legacy_meta_path(),
            None,
            "in AppData it is the file itself"
        );
        fs::create_dir(f.project.path().join(".trailmark")).expect("mkdir");
        let s = f.store();
        let legacy = s.legacy_meta_path().expect("a legacy path");
        assert!(legacy.starts_with(f.app.path().join("projects")));
        assert!(legacy.ends_with("meta.toml"));
        // it is where the store kept it before the folder existed
        let before = StateStore::open(f.app.path(), tempfile::tempdir().expect("tmp").path());
        assert_ne!(before.meta_path(), legacy, "another project, another file");
    }

    #[test]
    fn project_key_ignores_trailing_separators() {
        let a = project_key(Path::new("/some/where/"));
        let b = project_key(Path::new("/some/where"));
        assert_eq!(a, b);
        assert_eq!(
            project_key(Path::new("C:\\Work\\Notes")) == project_key(Path::new("c:/work/notes")),
            cfg!(windows)
        );
    }

    #[test]
    fn session_round_trips_and_survives_garbage() {
        let f = fx();
        let s = f.store();
        assert_eq!(s.load_session(), None);
        let session = SessionState {
            last_doc: Some("docs/a.md".to_owned()),
            scroll_top: 42.0,
            theme: Theme::Dark,
            follow: true,
            ..SessionState::default()
        };
        s.save_session(&session).expect("save");
        assert_eq!(s.load_session(), Some(session));
        fs::write(s.dir().join(SESSION_FILE), "{ not json").expect("write");
        assert_eq!(s.load_session(), None);
    }

    #[test]
    fn last_visit_round_trips() {
        let f = fx();
        let s = f.store();
        assert_eq!(s.load_last_visit(), None);
        let v = LastVisit {
            time_ms: 1_700_000_000_000,
            oid: Some("abc123".to_owned()),
            dirty: BTreeMap::from([(
                "docs/wip.md".to_owned(),
                FileStamp {
                    len: 12,
                    hash: u64::MAX,
                },
            )]),
        };
        s.save_last_visit(&v).expect("save");
        assert_eq!(s.load_last_visit(), Some(v));
        let v2 = LastVisit {
            time_ms: 5,
            ..LastVisit::default()
        };
        s.save_last_visit(&v2).expect("save");
        assert_eq!(s.load_last_visit(), Some(v2));
    }

    #[test]
    fn a_last_visit_saved_by_an_older_version_has_no_dirty_files() {
        let f = fx();
        let s = f.store();
        let dir = s.dir().to_path_buf();
        fs::create_dir_all(&dir).expect("mkdir");
        fs::write(dir.join(LAST_VISIT_FILE), r#"{"timeMs": 7, "oid": "abc"}"#).expect("write");
        assert_eq!(
            s.load_last_visit(),
            Some(LastVisit {
                time_ms: 7,
                oid: Some("abc".to_owned()),
                dirty: BTreeMap::new(),
            })
        );
    }

    #[test]
    fn existing_trailmark_folder_switches_state_to_the_project() {
        let f = fx();
        fs::create_dir(f.project.path().join(".trailmark")).expect("mkdir");
        let s = f.store();
        assert_eq!(s.location(), StateLocation::Project);
        assert!(s.dir().ends_with(Path::new(".trailmark").join("local")));
        s.save_session(&SessionState::default()).expect("save");
        assert!(f
            .project
            .path()
            .join(".trailmark/local/session.json")
            .is_file());
        assert!(!f.app.path().join("projects").exists());
    }

    /// A folder `outside` the project that a link can point to, with a marker file in it.
    fn outside() -> tempfile::TempDir {
        let outside = tempfile::tempdir().expect("outside");
        fs::write(outside.path().join("marker.txt"), "precious").expect("write");
        outside
    }

    /// Nothing but the marker is in `outside`: the project's state did not go there.
    fn untouched(outside: &tempfile::TempDir) {
        let mut names: Vec<_> = fs::read_dir(outside.path())
            .expect("ls")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            ["marker.txt"],
            "something was written behind the link"
        );
    }

    #[test]
    fn a_trailmark_folder_that_is_a_link_is_not_used() {
        use crate::util::testing::{dir_link, remove_link};

        let f = fx();
        let outside = outside();
        let link = f.project.path().join(".trailmark");
        dir_link(&link, outside.path());
        let mut s = f.store();
        assert_eq!(s.location(), StateLocation::AppData);
        assert_eq!(s.agent_path(), None);
        assert!(s.dir().starts_with(f.app.path()));
        // the state is kept, but not behind the link
        s.save_session(&SessionState::default()).expect("save");
        s.save_last_visit(&LastVisit::default()).expect("save");
        assert!(s.dir().join("session.json").is_file());
        // what the link points to has no say either
        fs::write(outside.path().join("config.toml"), "baseline = \"head\"\n").expect("write");
        assert_eq!(s.config(), Config::default());
        fs::remove_file(outside.path().join("config.toml")).expect("rm");
        // and it is not made into a project folder
        assert!(s.init_project_folder().is_err());
        untouched(&outside);
        assert_eq!(s.location(), StateLocation::AppData);
        remove_link(&link);
    }

    #[test]
    fn a_local_folder_that_is_a_link_is_not_used() {
        use crate::util::testing::{dir_link, remove_link};

        let f = fx();
        let outside = outside();
        let trailmark = f.project.path().join(".trailmark");
        fs::create_dir(&trailmark).expect("mkdir");
        let link = trailmark.join("local");
        dir_link(&link, outside.path());
        let mut s = f.store();
        assert_eq!(
            s.location(),
            StateLocation::AppData,
            "the state stays in the app's data"
        );
        assert_eq!(s.agent_path(), None);
        s.save_session(&SessionState::default()).expect("save");
        s.save_window(&WindowState {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
            scale: 1.0,
            maximized: false,
        })
        .expect("save");
        assert!(s.init_project_folder().is_err());
        untouched(&outside);
        remove_link(&link);
    }

    #[test]
    fn a_state_folder_that_turns_into_a_link_while_the_app_runs_is_not_written_through() {
        use crate::util::testing::{dir_link, remove_link};

        let f = fx();
        fs::create_dir(f.project.path().join(".trailmark")).expect("mkdir");
        let s = f.store();
        assert_eq!(s.location(), StateLocation::Project);
        s.save_session(&SessionState::default()).expect("save");
        // a branch that was checked out brings a link in place of the folder
        let outside = outside();
        let local = f.project.path().join(".trailmark/local");
        fs::remove_dir_all(&local).expect("rm");
        dir_link(&local, outside.path());
        assert!(s.save_session(&SessionState::default()).is_err());
        assert!(s.save_last_visit(&LastVisit::default()).is_err());
        untouched(&outside);
        remove_link(&local);
    }

    #[test]
    fn the_private_folders_ignore_themselves() {
        let f = fx();
        let mut s = f.store();
        s.init_project_folder().expect("init");
        let tm = f.project.path().join(".trailmark");
        for folder in ["local", "cache"] {
            let ignore = fs::read_to_string(tm.join(folder).join(".gitignore")).expect(folder);
            assert!(
                ignore.lines().any(|line| line == "*"),
                "{folder}: {ignore:?}"
            );
        }
    }

    /// A project can commit `.trailmark/` without a `.gitignore` (or with one that says nothing about `local/`).
    #[test]
    fn the_local_state_stays_out_of_git_whatever_the_projects_gitignore_says() {
        let f = fx();
        let repo = git2::Repository::init(f.project.path()).expect("git init");
        let tm = f.project.path().join(".trailmark");
        fs::create_dir(&tm).expect("mkdir");
        fs::write(tm.join("config.toml"), "").expect("write");
        fs::write(tm.join(".gitignore"), "# nothing about local\n").expect("write");
        let s = f.store();
        s.save_session(&SessionState::default()).expect("save");
        fs::write(s.notes_path(), "{}").expect("write");
        assert!(s.dir().join(".gitignore").is_file());
        for file in ["session.json", "notes.json"] {
            let path = Path::new(".trailmark/local").join(file);
            assert!(repo.status_should_ignore(&path).expect("status"), "{file}");
        }
        // the shared part is not hidden
        assert!(!repo
            .status_should_ignore(Path::new(".trailmark/config.toml"))
            .expect("status"));
        // a folder that ignores itself is kept as the project wrote it when it already has a .gitignore
        fs::write(s.dir().join(".gitignore"), "mine\n").expect("write");
        s.save_session(&SessionState::default()).expect("save");
        assert_eq!(
            fs::read_to_string(s.dir().join(".gitignore")).expect("read"),
            "mine\n"
        );
    }

    #[test]
    fn init_creates_the_documented_layout() {
        let f = fx();
        let mut s = f.store();
        s.init_project_folder().expect("init");
        let tm = f.project.path().join(".trailmark");
        assert!(tm.join("config.toml").is_file());
        assert!(tm.join("cache").is_dir());
        assert!(tm.join("local").is_dir());
        let gi = fs::read_to_string(tm.join(".gitignore")).expect("gitignore");
        assert!(gi.lines().any(|l| l == "cache/") && gi.lines().any(|l| l == "local/"));
        assert_eq!(s.location(), StateLocation::Project);
        // The generated config parses and means "all defaults".
        assert_eq!(s.config(), Config::default());
    }

    #[test]
    fn init_is_idempotent_and_keeps_user_edits() {
        let f = fx();
        let mut s = f.store();
        s.init_project_folder().expect("init");
        let cfg = f.project.path().join(".trailmark/config.toml");
        fs::write(&cfg, "baseline = \"head\"\nignore = [\"x/\"]\n").expect("write");
        s.init_project_folder().expect("init again");
        assert_eq!(
            fs::read_to_string(&cfg).expect("read"),
            "baseline = \"head\"\nignore = [\"x/\"]\n"
        );
        let c = f.store().config();
        assert_eq!(c.baseline, crate::git::BaselineKind::Head);
        assert_eq!(c.ignore, ["x/"]);
    }

    #[test]
    fn init_carries_appdata_state_over() {
        let f = fx();
        let mut s = f.store();
        let session = SessionState {
            last_doc: Some("keep.md".to_owned()),
            ..SessionState::default()
        };
        s.save_session(&session).expect("save");
        s.save_last_visit(&LastVisit {
            time_ms: 9,
            oid: Some("deadbeef".to_owned()),
            ..LastVisit::default()
        })
        .expect("save");
        s.init_project_folder().expect("init");
        assert_eq!(s.load_session(), Some(session.clone()));
        assert_eq!(s.load_last_visit().map(|v| v.time_ms), Some(9));
        // A store opened afterwards finds the same data in the project.
        assert_eq!(f.store().load_session(), Some(session));
    }

    #[test]
    fn what_was_read_is_kept_with_the_local_state_and_follows_it_into_the_project() {
        let f = fx();
        let mut s = f.store();
        assert_eq!(s.read_path(), s.dir().join("read.json"));
        assert!(s.read_path().starts_with(f.app.path()));
        fs::create_dir_all(s.dir()).expect("mkdir");
        fs::write(s.read_path(), r#"{"a.md": "00000000000000ab"}"#).expect("write");
        s.init_project_folder().expect("init");
        let local = Path::new(".trailmark").join("local").join("read.json");
        assert!(s.read_path().ends_with(&local));
        assert_eq!(
            fs::read_to_string(s.read_path()).expect("carried over"),
            r#"{"a.md": "00000000000000ab"}"#
        );
        // never in the part of .trailmark that is shared
        assert_eq!(f.store().read_path(), s.read_path());
    }

    #[test]
    fn review_notes_are_kept_with_the_local_state_and_follow_it_into_the_project() {
        let f = fx();
        let mut s = f.store();
        assert_eq!(s.notes_path(), s.dir().join("notes.json"));
        assert!(s.notes_path().starts_with(f.app.path()));
        fs::create_dir_all(s.dir()).expect("mkdir");
        fs::write(s.notes_path(), r#"{"version": 1, "notes": []}"#).expect("write");
        s.init_project_folder().expect("init");
        let local = Path::new(".trailmark").join("local").join("notes.json");
        assert!(s.notes_path().ends_with(&local));
        assert_eq!(
            fs::read_to_string(s.notes_path()).expect("carried over"),
            r#"{"version": 1, "notes": []}"#
        );
        // in the part of .trailmark that is not shared
        assert_eq!(f.store().notes_path(), s.notes_path());
    }

    #[test]
    fn the_agent_state_file_is_written_only_into_a_project_that_keeps_its_state_there() {
        let f = fx();
        let mut s = f.store();
        assert_eq!(s.agent_path(), None, "nothing is written into AppData");
        s.init_project_folder().expect("init");
        let local = Path::new(".trailmark").join("local").join("agent.json");
        assert!(s.agent_path().expect("a path").ends_with(&local));
        assert_eq!(f.store().agent_path(), s.agent_path());
    }

    #[test]
    fn a_folder_is_known_once_the_app_kept_state_for_it_or_it_has_a_trailmark_folder() {
        let f = fx();
        assert!(!f.store().is_known(), "never opened");
        let s = f.store();
        s.save_session(&SessionState::default()).expect("save");
        assert!(f.store().is_known(), "the app kept state for it");
        let other = tempfile::tempdir().expect("tempdir");
        fs::create_dir(other.path().join(".trailmark")).expect("mkdir");
        assert!(StateStore::open(f.app.path(), other.path()).is_known());
    }

    #[test]
    fn the_window_is_kept_with_the_rest_of_the_state_and_follows_it_into_the_project() {
        let f = fx();
        let mut s = f.store();
        assert_eq!(s.load_window(), None);
        let window = WindowState {
            x: -20,
            y: 30,
            width: 1280,
            height: 800,
            scale: 1.25,
            maximized: false,
        };
        s.save_window(&window).expect("save");
        assert_eq!(s.load_window(), Some(window));
        assert!(s.dir().join("window.json").is_file());
        // another project has its own
        let other = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            StateStore::open(f.app.path(), other.path()).load_window(),
            None
        );
        // in the project's private folder once it has one, never in the part that is shared
        s.init_project_folder().expect("init");
        assert_eq!(s.load_window(), Some(window));
        assert_eq!(f.store().load_window(), Some(window));
        let tm = f.project.path().join(".trailmark");
        assert!(tm.join("local").join("window.json").is_file());
        assert!(!tm.join("window.json").exists());
        let moved = WindowState { x: 5, ..window };
        s.save_window(&moved).expect("save");
        assert_eq!(f.store().load_window(), Some(moved));
        // damaged data counts as nothing saved
        fs::write(tm.join("local").join("window.json"), "{ not json").expect("write");
        assert_eq!(f.store().load_window(), None);
    }

    #[test]
    fn invalid_config_falls_back_to_defaults() {
        let f = fx();
        fs::create_dir(f.project.path().join(".trailmark")).expect("mkdir");
        fs::write(
            f.project.path().join(".trailmark/config.toml"),
            "baseline = [",
        )
        .expect("write");
        assert_eq!(f.store().config(), Config::default());
    }

    #[test]
    fn config_is_default_without_a_project_folder() {
        assert_eq!(fx().store().config(), Config::default());
    }

    #[test]
    fn location_serializes_as_the_ui_strings() {
        assert_eq!(
            serde_json::to_string(&StateLocation::AppData).expect("json"),
            "\"appData\""
        );
        assert_eq!(
            serde_json::to_string(&StateLocation::Project).expect("json"),
            "\"project\""
        );
    }
}
