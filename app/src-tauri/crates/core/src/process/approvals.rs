//! Programs the person allowed to start: a program they set up in the app's own settings (the editor, the Assistant's
//! own command) starts only once they said yes to it in a system dialog naming its full path. A setting changed by
//! anything else than the person (a page script that got hold of the settings call) then meets that dialog instead of
//! starting a program at once.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::downloads::sha_hex;
use crate::plugins::{nesting, Nesting, ProgramConsent};
use crate::state::{Loaded, StateData, StateFile, StateStore};

/// File of the approvals, in the profile folder itself.
pub const APPROVALS_FILE: &str = "program-approvals.json";

/// A program as the person set it up: its full path, the arguments as set (an editor's template words, placeholders
/// unfilled) and the variables set on top of the app's.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProgramCall {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// One allowed program: the same path with other arguments | variables is another program to ask about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Approval {
    program: String,
    /// SHA-256 of the arguments and variables: never their words (an argument may hold a key).
    command: String,
}

impl Approval {
    fn of(call: &ProgramCall) -> Self {
        // a JSON array keeps word borders: `["a b"]` and `["a", "b"]` differ
        let words = serde_json::to_vec(&(&call.args, &call.env)).unwrap_or_default();
        Self {
            program: call.program.to_string_lossy().into_owned(),
            command: sha_hex(&Sha256::digest(&words).into()),
        }
    }
}

/// What [`APPROVALS_FILE`] holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Approved {
    #[serde(default)]
    programs: Vec<Approval>,
}

impl StateData for Approved {
    const VERSION: u32 = 1;
}

/// The allowed programs and who asks about a new one.
pub struct ProgramApprovals {
    /// `None`: no profile, nothing is kept (every start asks).
    file: Option<StateFile<Approved>>,
    /// `None`: nobody to ask, so nothing new starts.
    consent: Option<Arc<dyn ProgramConsent>>,
    /// Open project folders: a program inside one is never started (a project's own file is no program the person set
    /// up, whatever the settings say).
    projects: Vec<PathBuf>,
}

impl ProgramApprovals {
    #[must_use]
    pub fn new(
        file: Option<StateFile<Approved>>,
        consent: Option<Arc<dyn ProgramConsent>>,
    ) -> Self {
        Self {
            file,
            consent,
            projects: Vec::new(),
        }
    }

    /// The same, refusing every program inside one of `projects` (open project folders).
    #[must_use]
    pub fn outside(self, projects: Vec<PathBuf>) -> Self {
        self.outside_below(projects, dirs::home_dir().as_deref())
    }

    /// [`ProgramApprovals::outside`] with the home folder given. A project that holds the home folder (home itself, a
    /// drive root) is left out: per-user installs (`AppData`, `.local/bin`) sit below it, and a folder that came with
    /// someone else's files never holds the person's home.
    fn outside_below(mut self, mut projects: Vec<PathBuf>, home: Option<&Path>) -> Self {
        if let Some(home) = home {
            projects
                .retain(|folder| !matches!(nesting(home, folder), Nesting::Inside | Nesting::Same));
        }
        self.projects = projects;
        self
    }

    /// `program` lies inside an open project folder ([`ProgramApprovals::outside`]): never asked about, never started.
    #[must_use]
    pub fn in_project(&self, program: &Path) -> bool {
        self.projects
            .iter()
            .any(|folder| matches!(nesting(program, folder), Nesting::Inside | Nesting::Same))
    }

    /// The approvals on disk. One that could not be parsed (moved aside by the load) is replaced by an empty file at
    /// once: a later start finding no file would take itself for the first one and adopt whatever is set up then.
    fn load(file: &StateFile<Approved>) -> Loaded<Approved> {
        let loaded = file.load();
        if matches!(loaded, Loaded::Recovered { backup: Some(_) }) {
            let _ = file.update(|current| current.is_none().then(Approved::default));
        }
        loaded
    }

    /// The approvals of the profile `store` belongs to ([`APPROVALS_FILE`] in the profile folder itself).
    #[must_use]
    pub fn of(store: &StateStore, consent: Option<Arc<dyn ProgramConsent>>) -> Self {
        Self::new(Some(store.app_file(APPROVALS_FILE)), consent)
    }

    /// The person allowed `call` before (a file that cannot be read allows nothing).
    #[must_use]
    pub fn approved(&self, call: &ProgramCall) -> bool {
        let Some(Loaded::Found(approved)) = self.file.as_ref().map(Self::load) else {
            return false;
        };
        approved.programs.contains(&Approval::of(call))
    }

    /// `call` may start: outside the open projects, and allowed before, else the person says yes now (kept; one that
    /// cannot be kept asks again next time). Blocks while the person answers: MUST NOT run on the UI thread.
    #[must_use]
    pub fn allow(&self, call: &ProgramCall) -> bool {
        if self.in_project(&call.program) {
            return false;
        }
        if self.approved(call) {
            return true;
        }
        let Some(consent) = &self.consent else {
            return false;
        };
        if !consent.allow(call) {
            return false;
        }
        if let Some(file) = &self.file {
            let approval = Approval::of(call);
            let _ = file.update(|current| {
                let mut next = current.cloned().unwrap_or_default();
                if !next.programs.contains(&approval) {
                    next.programs.push(approval.clone());
                }
                Some(next)
            });
        }
        true
    }

    /// First start with approvals (no file yet): the programs set up before they existed count as allowed, so an
    /// update asks nobody about the editor | own command they already use. Any later start leaves the file as it is
    /// (one that could not be read: replaced by an empty one, nothing adopted).
    pub fn adopt(&self, calls: &[ProgramCall]) {
        let Some(file) = &self.file else {
            return;
        };
        if !matches!(Self::load(file), Loaded::Missing) {
            return;
        }
        // written even when empty: a program set up after this start is asked about
        let _ = file.update(|current| {
            current.is_none().then(|| Approved {
                programs: calls.iter().map(Approval::of).collect(),
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    use super::*;
    use crate::state::AppDirs;

    /// Answers with `yes`; keeps what it was asked about.
    #[derive(Default)]
    struct Asked {
        yes: AtomicBool,
        asks: Mutex<Vec<(PathBuf, Vec<String>)>>,
    }

    impl ProgramConsent for Asked {
        fn allow(&self, call: &ProgramCall) -> bool {
            self.asks
                .lock()
                .unwrap()
                .push((call.program.clone(), call.args.clone()));
            self.yes.load(Ordering::SeqCst)
        }
    }

    impl Asked {
        fn count(&self) -> usize {
            self.asks.lock().unwrap().len()
        }
    }

    fn call(program: &str, args: &[&str], env: &[(&str, &str)]) -> ProgramCall {
        ProgramCall {
            program: PathBuf::from(program),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            env: env
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        }
    }

    fn store(dir: &Path) -> StateStore {
        StateStore::app(&AppDirs {
            profile: dir.to_path_buf(),
            cache: dir.join("cache"),
            install: dir.join("install"),
        })
    }

    fn approvals(dir: &Path, asked: &Arc<Asked>) -> ProgramApprovals {
        let consent: Arc<dyn ProgramConsent> = Arc::clone(asked) as _;
        ProgramApprovals::of(&store(dir), Some(consent))
    }

    #[test]
    fn a_yes_is_kept_and_asked_once() {
        let dir = tempfile::tempdir().unwrap();
        let asked = Arc::new(Asked::default());
        asked.yes.store(true, Ordering::SeqCst);
        let editor = call("/opt/code/bin/code", &["-g", "{path}:{line}"], &[]);
        assert!(approvals(dir.path(), &asked).allow(&editor));
        assert_eq!(
            asked.asks.lock().unwrap().as_slice(),
            [(editor.program.clone(), editor.args.clone())]
        );
        // another handle on the same file (the next start): allowed without a question
        let again = approvals(dir.path(), &asked);
        assert!(again.approved(&editor));
        assert!(again.allow(&editor));
        assert_eq!(asked.count(), 1);
        // the file names the program, never the words it starts with
        let text = std::fs::read_to_string(dir.path().join(APPROVALS_FILE)).unwrap();
        assert!(text.contains("code"), "{text}");
        assert!(!text.contains("{line}"), "{text}");
    }

    #[test]
    fn other_arguments_variables_or_path_ask_again() {
        let dir = tempfile::tempdir().unwrap();
        let asked = Arc::new(Asked::default());
        asked.yes.store(true, Ordering::SeqCst);
        let gate = approvals(dir.path(), &asked);
        assert!(gate.allow(&call("/bin/agent", &["--acp"], &[])));
        for other in [
            call("/bin/agent", &["--acp", "--evil"], &[]),
            call("/bin/agent", &["--acp"], &[("NODE_OPTIONS", "--require x")]),
            call("/bin/agent", &["--a", "cp"], &[]),
            call("/bin/agent", &["--a cp"], &[]),
            call("/tmp/agent", &["--acp"], &[]),
        ] {
            assert!(!gate.approved(&other), "{other:?}");
        }
        asked.yes.store(false, Ordering::SeqCst);
        assert!(!gate.allow(&call("/bin/agent", &["--acp", "--evil"], &[])));
        assert_eq!(asked.count(), 2);
    }

    #[test]
    fn a_no_starts_nothing_and_keeps_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let asked = Arc::new(Asked::default());
        let editor = call("/opt/code/bin/code", &["{path}"], &[]);
        let gate = approvals(dir.path(), &asked);
        assert!(!gate.allow(&editor));
        assert!(!gate.allow(&editor));
        // asked each time: a no is not remembered as a yes, nor kept as a no
        assert_eq!(asked.count(), 2);
        assert!(!dir.path().join(APPROVALS_FILE).exists());
        // nobody to ask: nothing new starts
        let alone = ProgramApprovals::of(&store(dir.path()), None);
        assert!(!alone.allow(&editor));
        // no profile: asked every time, nothing kept
        asked.yes.store(true, Ordering::SeqCst);
        let consent: Arc<dyn ProgramConsent> = Arc::clone(&asked) as _;
        let unkept = ProgramApprovals::new(None, Some(consent));
        assert!(unkept.allow(&editor));
        assert!(!unkept.approved(&editor));
    }

    #[test]
    fn the_first_start_adopts_what_was_set_up_and_later_starts_do_not() {
        let dir = tempfile::tempdir().unwrap();
        let asked = Arc::new(Asked::default());
        let editor = call("/opt/code/bin/code", &["-g", "{path}"], &[]);
        let own = call("/usr/bin/my-agent", &["--acp"], &[]);
        let gate = approvals(dir.path(), &asked);
        gate.adopt(&[editor.clone(), own.clone()]);
        assert!(gate.approved(&editor) && gate.approved(&own));
        // a later start: the file is there, nothing set up since is adopted
        let changed = call("/opt/evil", &["{path}"], &[]);
        gate.adopt(std::slice::from_ref(&changed));
        assert!(!gate.approved(&changed));
        assert!(!gate.allow(&changed));
        assert_eq!(asked.count(), 1);
        // nothing set up: the file is written all the same
        let fresh = tempfile::tempdir().unwrap();
        let gate = approvals(fresh.path(), &asked);
        gate.adopt(&[]);
        assert!(fresh.path().join(APPROVALS_FILE).exists());
        gate.adopt(std::slice::from_ref(&changed));
        assert!(!gate.approved(&changed));
    }

    /// A file that cannot be read was no first start: replaced by an empty one, so neither this start nor a later one
    /// adopts what is set up then.
    #[test]
    fn a_corrupt_file_is_no_first_start() {
        let dir = tempfile::tempdir().unwrap();
        let asked = Arc::new(Asked::default());
        let path = dir.path().join(APPROVALS_FILE);
        std::fs::write(&path, "{ not json").unwrap();
        let editor = call("/opt/code/bin/code", &["{path}"], &[]);
        approvals(dir.path(), &asked).adopt(std::slice::from_ref(&editor));
        assert!(path.is_file());
        let changed = call("/opt/evil", &["{path}"], &[]);
        let next = approvals(dir.path(), &asked);
        next.adopt(std::slice::from_ref(&changed));
        assert!(!next.approved(&changed));
        assert!(!next.approved(&editor));
        // a corrupt file met by a check during the run: replaced as well
        std::fs::write(
            &path,
            r#"{ "version": 1, "data": { "programs": [], "more": 1 } }"#,
        )
        .unwrap();
        assert!(!next.approved(&editor));
        next.adopt(std::slice::from_ref(&changed));
        assert!(!next.approved(&changed));
        assert_eq!(asked.count(), 0);
    }

    /// A program inside an open project is never asked about nor started, whoever set it up; one beside it is.
    #[test]
    fn a_program_inside_an_open_project_is_refused_unasked() {
        let dir = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let asked = Arc::new(Asked::default());
        asked.yes.store(true, Ordering::SeqCst);
        let gate = approvals(dir.path(), &asked).outside(vec![project.path().to_path_buf()]);
        let inside = project.path().join("tools").join("edit.cmd");
        let program = |path: &Path| call(&path.to_string_lossy(), &["{path}"], &[]);
        assert!(gate.in_project(&inside));
        assert!(gate.in_project(project.path()));
        assert!(!gate.allow(&program(&inside)));
        assert_eq!(asked.count(), 0);
        let beside = dir.path().join("edit.cmd");
        assert!(!gate.in_project(&beside));
        assert!(gate.allow(&program(&beside)));
        assert_eq!(asked.count(), 1);
    }

    /// The home folder (or a folder above it) opened as a project does not refuse the person's own installs below it; a
    /// project inside home still refuses its own files.
    #[test]
    fn a_project_holding_the_home_folder_leaves_per_user_installs_alone() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let asked = Arc::new(Asked::default());
        let repo = home.path().join("repo");
        let gate = approvals(dir.path(), &asked).outside_below(
            vec![home.path().to_path_buf(), repo.clone()],
            Some(home.path()),
        );
        let install = home
            .path()
            .join("AppData")
            .join("Local")
            .join("Programs")
            .join("code.cmd");
        assert!(!gate.in_project(&install));
        assert!(gate.in_project(&repo.join("bin").join("code.cmd")));
    }
}
