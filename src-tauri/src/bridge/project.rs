//! A project as `trailmark mcp` sees it when it is not talking to the window: found from the folder an agent works
//! in, with its setting read from the app's own data, and its unread documents and search index worked out from the
//! files. Nothing here needs the window, and nothing here changes the project.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use crate::comments::{CommentBook, CommentError, CommentFilter, CommentView, FileProblem};
use crate::git::{self, Discovery};
use crate::meta::filetags;
use crate::meta::store::Entries;
use crate::notes;
use crate::search::indexer::Indexer;
use crate::search::Bm25Index;
use crate::session::{AppSettingsStore, StateStore};
use crate::unread;
use crate::util::modified_ms;
use crate::vault::{NodeKind, Vault, VaultError};
use crate::watch::{PathFilter, SigStore};

use super::answers;
use super::protocol::{CommentList, Hit, Opened, ProtocolError, UnreadList};
use super::tags::{is_document, unreadable_meta, Doc, DocIndex};

/// The environment variable that points a debug build at another data folder, so that the tests which run the real
/// program never touch the person's own. Release builds ignore it.
pub(crate) const TEST_DATA_DIR_VARIABLE: &str = "TRAILMARK_TEST_DATA_DIR";

/// Where TrailMark keeps its data for the current user: the folder the window itself uses (`app_data_dir`), which
/// is named after the app's identifier. `None` when the system has no such folder.
///
/// The whole program asks for it here (the windows through `commands::app_data_dir`): a folder found two ways would split the
/// settings, the notes' socket and the saved state of the same app in two.
pub(crate) fn app_data_dir(identifier: &str) -> Option<PathBuf> {
    data_dir_from(
        identifier,
        std::env::var_os(TEST_DATA_DIR_VARIABLE),
        dirs::data_dir(),
        cfg!(debug_assertions),
    )
}

/// [`app_data_dir`] with what it reads from the system given: the test variable (honoured in a debug build only), and the
/// system's data folder.
fn data_dir_from(
    identifier: &str,
    test_dir: Option<std::ffi::OsString>,
    system: Option<PathBuf>,
    debug: bool,
) -> Option<PathBuf> {
    if debug {
        if let Some(dir) = test_dir.filter(|dir| !dir.is_empty()) {
            return Some(PathBuf::from(dir));
        }
    }
    system.map(|dir| dir.join(identifier))
}

/// The nearest folder at or above `start` that TrailMark knows: one with a `.trailmark/` folder, or one the app
/// kept state for. `None` when `start` is not inside any such folder.
///
/// The notes folder is never found, nor anything inside it: it has a `.trailmark/` folder like a project, but the notes
/// are private and no agent is shown them (the person shares a note on purpose, one at a time).
pub(crate) fn find_project(app_data_dir: &Path, start: &Path) -> Option<PathBuf> {
    if in_notes_tree(app_data_dir, start) {
        return None;
    }
    let start =
        dunce::canonicalize(start).unwrap_or_else(|_| dunce::simplified(start).to_path_buf());
    start
        .ancestors()
        .find(|dir| StateStore::open(app_data_dir, dir).is_known())
        .map(Path::to_path_buf)
}

/// Whether the person turned the agent connection on for the project in `root`. Only the app's own data says so: a
/// project's files (`.trailmark/` can be committed to a repository) have no say in who is shown the person's reading.
pub(crate) fn connection_is_on(app_data_dir: &Path, root: &Path) -> bool {
    !in_notes_tree(app_data_dir, root) && StateStore::open(app_data_dir, root).agent_connection()
}

/// Whether `path` is the person's notes folder or inside it (see [`notes::folder`]).
fn in_notes_tree(app_data_dir: &Path, path: &Path) -> bool {
    let chosen = AppSettingsStore::new(app_data_dir).load().notes_folder;
    notes::folder::contains(app_data_dir, chosen.as_deref(), path)
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ProjectError {
    #[error(transparent)]
    Vault(#[from] VaultError),
}

/// What is built the first time something is searched, and kept up to date from then on.
struct Searcher {
    index: Arc<RwLock<Bm25Index>>,
    indexer: Indexer,
}

/// The tags a document's front matter had, as they were when its size and time were last seen.
struct Seen {
    len: u64,
    mtime_ms: u64,
    tags: Vec<String>,
}

/// A project read from its files.
pub(crate) struct Project {
    vault: Arc<Vault>,
    filter: Arc<PathFilter>,
    read_path: PathBuf,
    /// Where the tags and descriptions added in TrailMark are: `.trailmark/meta.toml`, or a file in the app's data
    /// before the project has a `.trailmark/` folder.
    meta_path: PathBuf,
    /// The file in the app's data that the tags lived in before the project had a `.trailmark/` folder, when it is
    /// still there (the window merges it into `meta_path` the next time it opens the project).
    older_meta_path: Option<PathBuf>,
    front_matter: Mutex<HashMap<String, Seen>>,
    comments: CommentBook,
    searcher: Mutex<Option<Searcher>>,
    /// The notes folder, when it lies inside the project: never shown, opened or commented on.
    private: answers::PrivateTree,
}

impl Project {
    /// Opens the project in `root` for reading. `home` keeps a repository in the user's home folder from being
    /// taken for the project's (see `git::discover`).
    pub(crate) fn open(
        root: &Path,
        app_data_dir: &Path,
        home: Option<&Path>,
    ) -> Result<Self, ProjectError> {
        let vault = Arc::new(Vault::open(root)?);
        let store = StateStore::open(app_data_dir, vault.root());
        let config = store.config();
        // Only what the repository ignores is needed from git; linked worktrees are outside the project.
        let info_exclude = match git::discover(vault.root(), home) {
            Discovery::Repo(context) => context.info_exclude(),
            Discovery::NoRepo | Discovery::Error(_) => None,
        };
        // the notes are private: a notes folder inside the project is not part of what is searched or listed
        let mut ignore = config.ignore.clone();
        ignore.extend(notes::folder::ignore_inside(
            app_data_dir,
            AppSettingsStore::new(app_data_dir)
                .load()
                .notes_folder
                .as_deref(),
            vault.root(),
        ));
        let filter = Arc::new(PathFilter::new(
            &[(vault.root().to_path_buf(), info_exclude)],
            &ignore,
        ));
        let private = answers::PrivateTree::of(app_data_dir, vault.root());
        let comments = CommentBook::new(Arc::clone(&vault), store.comments_path());
        Ok(Self {
            vault,
            filter,
            read_path: store.read_path(),
            meta_path: store.meta_path(),
            older_meta_path: store.legacy_meta_path(),
            front_matter: Mutex::new(HashMap::new()),
            comments,
            searcher: Mutex::new(None),
            private,
        })
    }

    /// The documents that changed since the person last read them, newest change first.
    pub(crate) fn unread(&self) -> UnreadList {
        let paths = unread::unread_on_disk(&self.vault, &self.filter, &self.read_path);
        UnreadList::new(answers::unread_docs(&self.vault, &paths))
    }

    /// The Markdown documents with their tags and descriptions, worked out from the files: the tags in each document's
    /// front matter and the ones added in TrailMark (read from `meta.toml`, which is never written here). Front matter
    /// is read again only for the documents whose size or time changed since the last call.
    pub(crate) fn docs(&self) -> DocIndex {
        let (entries, problem) = self.app_entries();
        let mut seen = self
            .front_matter
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let docs = self
            .vault
            .walk()
            .into_iter()
            .filter(|item| {
                item.kind == NodeKind::File
                    && is_document(&self.filter, &item.rel)
                    && !self.private.covers_path(&self.vault, &item.rel)
            })
            .map(|item| {
                let entry = entries.get(&item.rel);
                Doc {
                    file_tags: front_matter_tags(&mut seen, &item.rel, &item.abs),
                    app_tags: entry.map(|e| e.tags.clone()).unwrap_or_default(),
                    description: entry.map(|e| e.description.clone()).unwrap_or_default(),
                    path: item.rel,
                }
            })
            .collect();
        DocIndex::new(docs, problem.map(|problem| unreadable_meta(&problem)))
    }

    /// The tags and descriptions added in TrailMark, or why they cannot be read (then there are none, as in the window).
    fn app_entries(&self) -> (Entries, Option<crate::meta::store::Problem>) {
        let mut entries = match Entries::read_file(&self.meta_path) {
            Ok(entries) => entries,
            Err(problem) => return (Entries::default(), Some(problem)),
        };
        // what was added before the project had a `.trailmark/` folder already counts: the window only has not moved it yet
        if let Some(older) = self
            .older_meta_path
            .as_deref()
            .and_then(|path| Entries::read_file(path).ok())
        {
            entries.merge(older);
        }
        (entries, None)
    }

    /// The best sections for `query`, in the documents that have all of `tags` (any document when there are none),
    /// each with its document's tags and description. The index is built from the files on the first call and brought
    /// up to date on the later ones (only files whose size or time changed are read again).
    pub(crate) fn search(&self, query: &str, limit: usize, tags: &[String]) -> Vec<Hit> {
        let docs = self.docs();
        let allow = docs.allowing(tags);
        let mut searcher = self.searcher.lock().unwrap_or_else(PoisonError::into_inner);
        let never = std::sync::atomic::AtomicBool::new(false);
        let searcher = searcher.get_or_insert_with(|| {
            let index = Arc::new(RwLock::new(Bm25Index::new()));
            let mut indexer = Indexer::new(
                Arc::clone(&self.vault),
                Arc::clone(&self.filter),
                Arc::clone(&index),
                Arc::new(SigStore::new()),
            );
            indexer.build(&never, &mut |_, _| {});
            Searcher { index, indexer }
        });
        searcher.indexer.rescan(&never);
        let found = searcher
            .index
            .read()
            .map(|index| index.search_in(query, limit, &*allow))
            .unwrap_or_default();
        docs.describe(answers::plain_hits(found))
    }

    /// The comments of the kind asked for. Read-only: the places that moved are not written down here. A comment on a note of a
    /// private notes folder that lies inside the project is not listed.
    pub(crate) fn comments(&self, which: CommentFilter) -> CommentList {
        answers::comment_list(self.visible_comments(), which)
    }

    fn visible_comments(&self) -> Vec<CommentView> {
        self.comments
            .list(false)
            .into_iter()
            .filter(|comment| !self.private.covers_path(&self.vault, &comment.path))
            .collect()
    }

    /// What is wrong with the comments file, when that is why there are no comments to list.
    pub(crate) fn comments_problem(&self) -> Option<FileProblem> {
        self.comments.problem()
    }

    /// Marks a comment done by an agent.
    pub(crate) fn resolve_comment(
        &self,
        id: &str,
        reply: Option<&str>,
    ) -> Result<CommentView, CommentError> {
        // a comment on a private note is a comment that is not there
        if !self
            .visible_comments()
            .iter()
            .any(|comment| comment.id == id)
        {
            return Err(CommentError::NotFound(id.to_owned()));
        }
        self.comments.resolve(id, reply)
    }

    /// Checks a place an agent wants shown (see [`answers::prepare_open_hiding`]).
    pub(crate) fn check_open(
        &self,
        path: &str,
        line: Option<u32>,
        heading: Option<&str>,
    ) -> Result<Opened, ProtocolError> {
        answers::prepare_open_hiding(&self.vault, &self.private, path, line, heading)
    }
}

/// The tags in the front matter of the document `rel` (at `abs`), read again only when its size or time changed.
fn front_matter_tags(seen: &mut HashMap<String, Seen>, rel: &str, abs: &Path) -> Vec<String> {
    let Ok(meta) = std::fs::metadata(abs) else {
        return Vec::new();
    };
    let (len, mtime_ms) = (meta.len(), modified_ms(&meta));
    if let Some(known) = seen
        .get(rel)
        .filter(|known| known.len == len && known.mtime_ms == mtime_ms)
    {
        return known.tags.clone();
    }
    let tags = filetags::read(abs);
    seen.insert(
        rel.to_owned(),
        Seen {
            len,
            mtime_ms,
            tags: tags.clone(),
        },
    );
    tags
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::session::SessionState;

    struct Fx {
        app: tempfile::TempDir,
        project: tempfile::TempDir,
    }

    fn fx() -> Fx {
        let fx = Fx {
            app: tempfile::tempdir().expect("app data"),
            project: tempfile::tempdir().expect("project"),
        };
        write(&fx, "README.md", "# Home\n\nWelcome to the lighthouse.\n");
        write(
            &fx,
            "docs/plan.md",
            "# Plan\n\nIntro.\n\n## Rollout\n\nShip the beacon on Friday.\n",
        );
        fx
    }

    fn write(fx: &Fx, rel: &str, text: &str) {
        let path = fx.project.path().join(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, text).expect("write");
    }

    fn root(fx: &Fx) -> PathBuf {
        dunce::canonicalize(fx.project.path()).expect("canonical")
    }

    fn store(fx: &Fx) -> StateStore {
        StateStore::open(fx.app.path(), &root(fx))
    }

    fn store_for(fx: &Fx, folder: &Path) -> StateStore {
        StateStore::open(fx.app.path(), folder)
    }

    #[test]
    fn the_data_folder_is_named_after_the_app() {
        let dir = app_data_dir("dev.example.app").expect("a data folder");
        assert!(dir.ends_with("dev.example.app"));
        assert!(dir.is_absolute());
    }

    #[test]
    fn the_test_variable_moves_the_data_folder_in_a_debug_build_only() {
        let test = Some(std::ffi::OsString::from("/tmp/test-data"));
        let system = Some(PathBuf::from("/home/ana/.local/share"));
        assert_eq!(
            data_dir_from("dev.example.app", test.clone(), system.clone(), true),
            Some(PathBuf::from("/tmp/test-data"))
        );
        assert_eq!(
            data_dir_from("dev.example.app", test, system.clone(), false),
            Some(PathBuf::from("/home/ana/.local/share/dev.example.app"))
        );
        // an empty variable is no variable, and a system without a data folder has none
        assert_eq!(
            data_dir_from(
                "dev.example.app",
                Some(std::ffi::OsString::new()),
                system,
                true
            ),
            Some(PathBuf::from("/home/ana/.local/share/dev.example.app"))
        );
        assert_eq!(data_dir_from("dev.example.app", None, None, true), None);
    }

    #[test]
    fn a_program_started_inside_a_project_finds_the_project() {
        let fx = fx();
        let nested = fx.project.path().join("docs").join("deep").join("er");
        fs::create_dir_all(&nested).expect("mkdir");
        // TrailMark has never seen this folder.
        assert_eq!(find_project(fx.app.path(), &nested), None);
        // Once the app kept state for it, any folder inside finds it.
        store(&fx)
            .save_session(&SessionState::default())
            .expect("save");
        assert_eq!(find_project(fx.app.path(), &nested), Some(root(&fx)));
        assert_eq!(
            find_project(fx.app.path(), fx.project.path()),
            Some(root(&fx))
        );
    }

    #[test]
    fn the_nearest_known_folder_wins() {
        let fx = fx();
        store(&fx)
            .save_session(&SessionState::default())
            .expect("save");
        let inner = fx.project.path().join("docs");
        fs::create_dir(inner.join(".trailmark")).expect("mkdir");
        let found = find_project(fx.app.path(), &inner);
        assert_eq!(found, Some(dunce::canonicalize(&inner).expect("canonical")));
    }

    #[test]
    fn a_folder_with_a_trailmark_folder_is_a_project() {
        let fx = fx();
        fs::create_dir(fx.project.path().join(".trailmark")).expect("mkdir");
        assert_eq!(
            find_project(fx.app.path(), fx.project.path()),
            Some(root(&fx))
        );
    }

    #[test]
    fn the_setting_is_the_one_the_app_kept_and_is_off_without_one() {
        let fx = fx();
        assert!(!connection_is_on(fx.app.path(), &root(&fx)));
        store(&fx)
            .save_session(&SessionState::default())
            .expect("save");
        assert!(!connection_is_on(fx.app.path(), &root(&fx)));
        store(&fx).set_agent_connection(true).expect("on");
        assert!(connection_is_on(fx.app.path(), &root(&fx)));
        // It stays with the person when the project starts keeping its other settings in `.trailmark/`.
        let mut moved = store(&fx);
        moved.init_project_folder().expect("init");
        assert!(connection_is_on(fx.app.path(), &root(&fx)));
        store(&fx).set_agent_connection(false).expect("off");
        assert!(!connection_is_on(fx.app.path(), &root(&fx)));
    }

    /// A repository decides what is in its `.trailmark/` folder (it can be committed); it must not decide who sees
    /// the person's reading.
    #[test]
    fn a_project_cannot_turn_the_connection_on_for_itself() {
        let fx = fx();
        write(
            &fx,
            ".trailmark/local/session.json",
            r#"{"agentConnection": true}"#,
        );
        write(
            &fx,
            ".trailmark/local/agent-connection.json",
            r#"{"enabled": true}"#,
        );
        write(
            &fx,
            ".trailmark/agent-connection.json",
            r#"{"enabled": true}"#,
        );
        assert!(!connection_is_on(fx.app.path(), &root(&fx)));
        // the folder is known (it has a `.trailmark/`), and still answers off
        assert_eq!(
            find_project(fx.app.path(), fx.project.path()),
            Some(root(&fx))
        );
        // and the person's own choice is the only thing that changes it
        store(&fx).set_agent_connection(true).expect("on");
        assert!(connection_is_on(fx.app.path(), &root(&fx)));
    }

    /// The notes folder has a `.trailmark/` folder like a project, and is private: it is not found, whatever is inside it.
    #[test]
    fn the_notes_folder_is_never_a_project() {
        let fx = fx();
        let notes = root(&fx).join("TrailMark Notes");
        fs::create_dir_all(notes.join(".trailmark")).expect("mkdir");
        fs::create_dir_all(notes.join("sub")).expect("mkdir");
        // before it is the notes folder, it looks like any folder with a `.trailmark/`
        assert_eq!(find_project(fx.app.path(), &notes), Some(notes.clone()));

        AppSettingsStore::new(fx.app.path())
            .set_notes_folder(&notes.display().to_string())
            .expect("choose the notes folder");
        for inside in [
            notes.clone(),
            notes.join("sub"),
            notes.join("not-yet").join("x.md"),
        ] {
            assert_eq!(find_project(fx.app.path(), &inside), None, "{inside:?}");
        }
        // the connection answers off for it, even if a switch was somehow stored
        store_for(&fx, &notes)
            .set_agent_connection(true)
            .expect("on");
        assert!(!connection_is_on(fx.app.path(), &notes));
        // the project around it, outside the notes, is found as before
        assert_eq!(find_project(fx.app.path(), fx.project.path()), None);
        store(&fx)
            .save_session(&SessionState::default())
            .expect("save");
        assert_eq!(
            find_project(fx.app.path(), &root(&fx).join("docs")),
            Some(root(&fx))
        );
    }

    #[test]
    fn search_builds_the_index_from_the_files_and_follows_edits() {
        let fx = fx();
        let project = Project::open(&root(&fx), fx.app.path(), None).expect("open");
        let hits = project.search("beacon", 10, &[]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "docs/plan.md");
        assert_eq!(hits[0].heading_path, ["Plan", "Rollout"]);
        assert_eq!(hits[0].line, 5);
        assert!(hits[0].snippet.contains("beacon"));
        assert!(!hits[0].snippet.contains("<mark>"), "plain text");
        assert_eq!(
            project.search("nonexistentword", 10, &[]),
            Vec::<Hit>::new()
        );

        // A document written after the first search is found by the next one.
        write(&fx, "docs/new.md", "# New\n\nA second beacon.\n");
        let hits = project.search("beacon", 10, &[]);
        assert_eq!(hits.len(), 2);
        // And one that was deleted is gone.
        fs::remove_file(fx.project.path().join("docs/plan.md")).expect("rm");
        let hits = project.search("beacon", 10, &[]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "docs/new.md");
    }

    #[test]
    fn search_respects_the_projects_ignore_rules() {
        let fx = fx();
        write(&fx, ".gitignore", "secret/\n");
        write(&fx, "secret/keys.md", "# Keys\n\nthe beacon passphrase\n");
        let project = Project::open(&root(&fx), fx.app.path(), None).expect("open");
        let paths: Vec<_> = project
            .search("beacon", 10, &[])
            .into_iter()
            .map(|h| h.path)
            .collect();
        assert_eq!(paths, ["docs/plan.md"]);
    }

    /// A project can hold the notes folder (a notes folder chosen inside it): the notes are private, and nothing the
    /// project's agent can ask for (the search, the unread documents) shows a word of them.
    #[test]
    fn the_notes_folder_inside_a_project_is_not_searched_nor_listed_as_unread() {
        let fx = fx();
        write(
            &fx,
            "notes/secret.md",
            "# Secret\n\nThe beacon passphrase is pelican.\n",
        );
        write(&fx, "notes/.trailmark/state.json", "{}");
        let notes = root(&fx).join("notes");
        let project = Project::open(&root(&fx), fx.app.path(), None).expect("open");
        // before the folder is the notes folder it is a folder of the project like any other
        let before = project.search("pelican", 10, &[]);
        assert_eq!(before.len(), 1, "the control: the note is found");
        drop(project);

        AppSettingsStore::new(fx.app.path())
            .set_notes_folder(&notes.display().to_string())
            .expect("choose the notes folder");
        let project = Project::open(&root(&fx), fx.app.path(), None).expect("open");
        assert_eq!(project.search("pelican", 10, &[]), Vec::<Hit>::new());
        // the rest of the project is still searched
        assert_eq!(project.search("beacon", 10, &[]).len(), 1);
        // and an edit to a note does not bring it in later
        write(&fx, "notes/more.md", "# More\n\nA second pelican.\n");
        assert_eq!(project.search("pelican", 10, &[]), Vec::<Hit>::new());
        let hash = format!("{:016x}", crate::util::hash_bytes(b"old"));
        let read = store(&fx).read_path();
        fs::create_dir_all(read.parent().expect("parent")).expect("mkdir");
        fs::write(&read, format!("{{\"README.md\": \"{hash}\"}}")).expect("write");
        let unread = project.unread();
        assert!(
            unread.unread.iter().all(|d| !d.path.starts_with("notes/")),
            "{:?}",
            unread.unread.iter().map(|d| &d.path).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_unread_documents_come_from_the_saved_state() {
        let fx = fx();
        let project = Project::open(&root(&fx), fx.app.path(), None).expect("open");
        assert_eq!(project.unread().total, 0, "never looked at: the baseline");
        // The app saved that README.md was read as it was; then both documents changed.
        let hash = format!("{:016x}", crate::util::hash_bytes(b"old readme"));
        let read = store(&fx).read_path();
        fs::create_dir_all(read.parent().expect("parent")).expect("mkdir");
        fs::write(&read, format!("{{\"README.md\": \"{hash}\"}}")).expect("write");
        let unread = project.unread();
        let mut paths: Vec<_> = unread.unread.iter().map(|d| d.path.as_str()).collect();
        paths.sort_unstable();
        assert_eq!(paths, ["README.md", "docs/plan.md"]);
        assert_eq!(unread.total, 2);
    }

    #[test]
    fn comments_are_read_and_resolved_from_the_saved_state() {
        use crate::comments::{CommentFilter, NewComment};
        let fx = fx();
        let project = Project::open(&root(&fx), fx.app.path(), None).expect("open");
        assert_eq!(project.comments(CommentFilter::All).total, 0);
        // the window wrote a comment
        let book = CommentBook::new(
            Arc::new(Vault::open(&root(&fx)).expect("vault")),
            store(&fx).comments_path(),
        );
        let comment = book
            .add(NewComment {
                path: "docs/plan.md".to_owned(),
                quote: "Ship the beacon".to_owned(),
                text: "Which beacon?".to_owned(),
                heading_path: Vec::new(),
                line: 0,
                end_line: 0,
            })
            .expect("a comment");
        let open = project.comments(CommentFilter::Open);
        assert_eq!(open.total, 1);
        assert_eq!(open.comments[0].line, 7);
        let resolved = project
            .resolve_comment(&comment.id, Some("Named it"))
            .expect("resolved");
        assert!(resolved.done);
        assert_eq!(project.comments(CommentFilter::Open).total, 0);
        assert_eq!(project.comments(CommentFilter::Done).total, 1);
        assert!(project.resolve_comment("nope", None).is_err());
    }

    /// The program an agent starts when the window is closed answers about the private notes folder as the window does: as if the
    /// notes were not there.
    #[test]
    fn the_notes_folder_inside_a_project_is_not_shown_nor_commented_on_to_an_agent() {
        use crate::comments::{CommentFilter, NewComment};
        let fx = fx();
        write(
            &fx,
            "notes/secret.md",
            "# Secret\n\n## Hidden heading\n\nthe pelican\n",
        );
        let notes = root(&fx).join("notes");
        AppSettingsStore::new(fx.app.path())
            .set_notes_folder(&notes.display().to_string())
            .expect("choose the notes folder");
        let project = Project::open(&root(&fx), fx.app.path(), None).expect("open");

        let missing = project
            .check_open("notes/zzz.md", Some(2), Some("Hidden heading"))
            .expect_err("missing");
        for path in ["notes/secret.md", "NOTES\\secret.md", "./notes/secret.md"] {
            let refused = project
                .check_open(path, Some(2), Some("Hidden heading"))
                .expect_err(path);
            assert_eq!(refused.kind, missing.kind, "{path}");
            assert_eq!(
                refused.message.replace(path, "<path>"),
                missing.message.replace("notes/zzz.md", "<path>"),
                "{path}"
            );
        }
        assert!(project.check_open("docs/plan.md", None, None).is_ok());

        // a comment the person wrote on a note is not listed, and cannot be resolved by an agent that learned its id
        let book = CommentBook::new(
            Arc::new(Vault::open(&root(&fx)).expect("vault")),
            store(&fx).comments_path(),
        );
        let on_note = book
            .add(NewComment {
                path: "notes/secret.md".to_owned(),
                quote: "the pelican".to_owned(),
                text: "private".to_owned(),
                heading_path: Vec::new(),
                line: 0,
                end_line: 0,
            })
            .expect("a comment");
        assert_eq!(project.comments(CommentFilter::All).total, 0);
        assert!(project.resolve_comment(&on_note.id, None).is_err());
        assert!(
            !book
                .list(false)
                .iter()
                .any(|comment| comment.id == on_note.id && comment.done),
            "it was not resolved"
        );
    }

    #[test]
    fn what_is_shown_must_be_a_document_of_the_project() {
        let fx = fx();
        let project = Project::open(&root(&fx), fx.app.path(), None).expect("open");
        assert!(project
            .check_open("docs/plan.md", None, Some("Rollout"))
            .is_ok());
        assert!(project.check_open("../x.md", None, None).is_err());
    }
}
