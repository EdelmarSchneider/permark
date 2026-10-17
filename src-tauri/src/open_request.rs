//! Files and folders the system asks PerMark to open, other than through the command line.
//!
//! On macOS, Finder does not start the program with the path as an argument: a double-click, "Open With" or a drop
//! on the Dock icon reaches the app as an event (`RunEvent::Opened`), both when the app is starting and when it is
//! already running. [`OpenInbox`] decides what happens to each path:
//!
//! - while the app is starting, the first path is kept for the UI to pick up as its first project (the same place
//!   the command line argument goes, see [`OpenInbox::take_launch_path`]);
//! - while it runs, a path opens in this window when no project is open; when a project is open, a path inside it
//!   is shown in this window (a second process on the same project would write the same state files), and any other
//!   path gets a new PerMark process (one process per project, as everywhere else in the app).
//!
//! The decision and the conversion from URLs are plain functions with no window system in them, so they are
//! tested on every platform. Only the glue that receives the event and starts the process is macOS-only.
//!
//! The other half of "Open with" on macOS and Linux is declared in `tauri.macos.conf.json`,
//! `tauri.linux.conf.json` and `installer/permark.desktop`; the tests in `bundle_config` check those files.

#[cfg(test)]
mod bundle_config;

use std::sync::{Mutex, PoisonError};

#[cfg(any(target_os = "macos", test))]
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "macos", test))]
use tauri::Url;

/// What to do with a path that arrived while the app runs.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Open it in the window that is showing no project.
    OpenHere(String),
    /// It is the open project or inside it: bring the window forward and, for a file, show it. Holds the file's path
    /// in the project (`/`-separated), or `None` for the project itself and for a folder in it.
    ShowInProject(Option<String>),
    /// Start another PerMark for it.
    NewProcess(String),
}

/// What the window shows when a path arrives.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Showing {
    /// No project (the welcome screen).
    Nothing,
    /// A folder, with its root.
    Project(PathBuf),
    /// A single file.
    SingleFile,
}

/// Where the app is on its way to showing its first project.
#[derive(Debug)]
enum Stage {
    /// The UI has not asked what to open yet. Holds the first path the system delivered meanwhile.
    Launching(Option<String>),
    /// The UI has asked: paths now go to the running app.
    Running,
}

/// Paths delivered by the system, kept until the UI can use them. Managed Tauri state.
#[derive(Debug)]
pub(crate) struct OpenInbox(Mutex<Stage>);

impl Default for OpenInbox {
    fn default() -> Self {
        Self(Mutex::new(Stage::Launching(None)))
    }
}

impl OpenInbox {
    /// The path the system delivered while the app was starting, if any. The UI asks once, when it boots; from then
    /// on the app counts as running.
    pub(crate) fn take_launch_path(&self) -> Option<String> {
        let mut stage = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match std::mem::replace(&mut *stage, Stage::Running) {
            Stage::Launching(held) => held,
            Stage::Running => None,
        }
    }

    /// Takes in the paths of one delivery, in order, and returns what to do now. `showing` is what the window shows.
    /// Only the first path of a delivery can use an empty window (or become the launch path); a path in the open
    /// project is shown in it; every other path gets a process of its own.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn receive(&self, paths: Vec<String>, showing: &Showing) -> Vec<Action> {
        let mut stage = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let mut actions = Vec::new();
        for (n, path) in paths.into_iter().enumerate() {
            let first = n == 0;
            match (&mut *stage, showing) {
                (Stage::Launching(held @ None), _) if first => *held = Some(path),
                (Stage::Running, Showing::Nothing) if first => actions.push(Action::OpenHere(path)),
                (Stage::Running, Showing::Project(root)) => {
                    actions.push(inside(&path, root).unwrap_or(Action::NewProcess(path)));
                }
                _ => actions.push(Action::NewProcess(path)),
            }
        }
        actions
    }
}

/// What to do with `path` when the open project is `root`, if it is the project or inside it. Both are canonical, as
/// [`paths_from_urls`] makes them.
#[cfg(any(target_os = "macos", test))]
fn inside(path: &str, root: &Path) -> Option<Action> {
    let path = Path::new(path);
    let relative = path.strip_prefix(root).ok()?;
    let document = path.is_file().then(|| {
        relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    });
    Some(Action::ShowInProject(document))
}

/// The existing files and folders among `urls`, as the canonical paths the app opens. Anything else (another URL
/// scheme, a file on another machine, a path that is gone) is left out.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn paths_from_urls(urls: &[Url]) -> Vec<String> {
    urls.iter()
        .filter(|url| url.scheme() == "file")
        .filter_map(|url| url.to_file_path().ok())
        .filter_map(|path| dunce::canonicalize(path).ok())
        .map(|path| path.display().to_string())
        .collect()
}

/// Starts another PerMark for `path` (a folder, or a file for Simple mode) and lets it go its own way.
#[cfg(target_os = "macos")]
pub(crate) fn spawn_instance(path: &str) -> std::io::Result<()> {
    use std::process::{Command, Stdio};

    let mut child = Command::new(std::env::current_exe()?)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Collects the exit status when it ends, so the finished process is not left behind as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn open_here(path: &str) -> Action {
        Action::OpenHere(path.to_owned())
    }

    fn new_process(path: &str) -> Action {
        Action::NewProcess(path.to_owned())
    }

    #[test]
    fn the_first_path_of_a_launch_is_kept_for_the_ui() {
        let inbox = OpenInbox::default();
        assert_eq!(inbox.receive(paths(&["/notes"]), &Showing::Nothing), vec![]);
        assert_eq!(inbox.take_launch_path(), Some("/notes".to_owned()));
    }

    #[test]
    fn the_launch_path_is_handed_over_once_and_the_app_then_counts_as_running() {
        let inbox = OpenInbox::default();
        inbox.receive(paths(&["/notes"]), &Showing::Nothing);
        assert_eq!(inbox.take_launch_path(), Some("/notes".to_owned()));
        assert_eq!(inbox.take_launch_path(), None);
        assert_eq!(
            inbox.receive(paths(&["/other"]), &Showing::Nothing),
            vec![open_here("/other")]
        );
    }

    #[test]
    fn nothing_delivered_while_starting_means_no_launch_path() {
        let inbox = OpenInbox::default();
        assert_eq!(inbox.receive(vec![], &Showing::Nothing), vec![]);
        assert_eq!(inbox.take_launch_path(), None);
    }

    #[test]
    fn an_empty_delivery_while_starting_keeps_the_app_waiting_for_the_next_one() {
        let inbox = OpenInbox::default();
        inbox.receive(vec![], &Showing::Nothing);
        inbox.receive(paths(&["/notes"]), &Showing::Nothing);
        assert_eq!(inbox.take_launch_path(), Some("/notes".to_owned()));
    }

    #[test]
    fn when_several_paths_arrive_while_starting_the_first_is_kept_and_the_rest_get_processes() {
        let inbox = OpenInbox::default();
        assert_eq!(
            inbox.receive(paths(&["/a.md", "/b.md", "/c.md"]), &Showing::Nothing),
            vec![new_process("/b.md"), new_process("/c.md")]
        );
        assert_eq!(inbox.take_launch_path(), Some("/a.md".to_owned()));
    }

    #[test]
    fn a_second_delivery_before_the_ui_asks_gets_a_process_and_keeps_the_first_path() {
        let inbox = OpenInbox::default();
        inbox.receive(paths(&["/a.md"]), &Showing::Nothing);
        assert_eq!(
            inbox.receive(paths(&["/b.md"]), &Showing::Nothing),
            vec![new_process("/b.md")]
        );
        assert_eq!(inbox.take_launch_path(), Some("/a.md".to_owned()));
    }

    #[test]
    fn a_running_app_with_no_project_opens_the_first_path_in_its_window() {
        let inbox = OpenInbox::default();
        inbox.take_launch_path();
        assert_eq!(
            inbox.receive(paths(&["/notes", "/more"]), &Showing::Nothing),
            vec![open_here("/notes"), new_process("/more")]
        );
    }

    #[test]
    fn a_running_app_with_a_single_file_starts_a_process_for_every_path() {
        let inbox = OpenInbox::default();
        inbox.take_launch_path();
        assert_eq!(
            inbox.receive(paths(&["/notes", "/more"]), &Showing::SingleFile),
            vec![new_process("/notes"), new_process("/more")]
        );
        assert_eq!(inbox.receive(vec![], &Showing::SingleFile), vec![]);
    }

    /// A project folder with a document and a subfolder in it, and one folder beside it.
    fn project() -> (tempfile::TempDir, PathBuf, Showing) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dunce::canonicalize(dir.path()).expect("canonical");
        std::fs::create_dir_all(root.join("project").join("docs")).expect("mkdir");
        std::fs::create_dir_all(root.join("project-two")).expect("mkdir");
        std::fs::write(root.join("project").join("docs").join("a b.md"), "x").expect("write");
        std::fs::write(root.join("project-two").join("c.md"), "x").expect("write");
        let open = root.join("project");
        let showing = Showing::Project(open);
        (dir, root, showing)
    }

    fn running() -> OpenInbox {
        let inbox = OpenInbox::default();
        inbox.take_launch_path();
        inbox
    }

    #[test]
    fn a_file_in_the_open_project_is_shown_in_its_window_and_not_in_a_process() {
        let (_dir, root, showing) = project();
        let file = root.join("project").join("docs").join("a b.md");
        assert_eq!(
            running().receive(vec![file.display().to_string()], &showing),
            vec![Action::ShowInProject(Some("docs/a b.md".to_owned()))]
        );
    }

    #[test]
    fn the_open_project_itself_and_a_folder_in_it_only_bring_the_window_forward() {
        let (_dir, root, showing) = project();
        let project = root.join("project");
        let docs = project.join("docs");
        assert_eq!(
            running().receive(
                vec![project.display().to_string(), docs.display().to_string()],
                &showing
            ),
            vec![Action::ShowInProject(None), Action::ShowInProject(None)]
        );
    }

    #[test]
    fn a_path_that_only_starts_like_the_project_is_not_in_it() {
        // `project-two` begins with `project`, but it is another folder
        let (_dir, root, showing) = project();
        let other = root.join("project-two");
        let file = other.join("c.md");
        assert_eq!(
            running().receive(
                vec![other.display().to_string(), file.display().to_string()],
                &showing
            ),
            vec![
                new_process(&other.display().to_string()),
                new_process(&file.display().to_string())
            ]
        );
    }

    #[test]
    fn paths_of_one_delivery_are_each_placed_by_where_they_are() {
        let (_dir, root, showing) = project();
        let inside = root.join("project").join("docs").join("a b.md");
        let outside = root.join("project-two").join("c.md");
        assert_eq!(
            running().receive(
                vec![outside.display().to_string(), inside.display().to_string()],
                &showing
            ),
            vec![
                new_process(&outside.display().to_string()),
                Action::ShowInProject(Some("docs/a b.md".to_owned())),
            ]
        );
    }

    #[test]
    fn a_path_of_the_open_project_that_arrives_while_starting_is_still_held_for_the_ui() {
        // no project can be open before the UI has asked what to open
        let inbox = OpenInbox::default();
        assert_eq!(inbox.receive(paths(&["/notes"]), &Showing::Nothing), vec![]);
        assert_eq!(inbox.take_launch_path(), Some("/notes".to_owned()));
    }

    #[test]
    fn a_poisoned_lock_does_not_stop_paths_from_opening() {
        let inbox = std::sync::Arc::new(OpenInbox::default());
        let held = std::sync::Arc::clone(&inbox);
        let _ = std::thread::spawn(move || {
            let _guard = held.0.lock().expect("lock");
            panic!("poison the lock");
        })
        .join();
        assert_eq!(inbox.take_launch_path(), None);
        assert_eq!(
            inbox.receive(paths(&["/notes"]), &Showing::Nothing),
            vec![open_here("/notes")]
        );
    }

    fn file_url(path: &std::path::Path) -> Url {
        Url::from_file_path(path).expect("an absolute path")
    }

    fn canonical(path: &std::path::Path) -> String {
        dunce::canonicalize(path)
            .expect("canonical")
            .display()
            .to_string()
    }

    #[test]
    fn file_and_folder_urls_become_the_paths_the_app_opens() {
        let dir = tempfile::tempdir().expect("tempdir");
        let folder = dir.path().join("my notes ü");
        std::fs::create_dir(&folder).expect("mkdir");
        let file = folder.join("read me.md");
        std::fs::write(&file, "x").expect("write");

        let got = paths_from_urls(&[file_url(&folder), file_url(&file)]);

        assert_eq!(got, vec![canonical(&folder), canonical(&file)]);
    }

    #[test]
    fn a_folder_url_with_a_trailing_slash_is_the_folder() {
        // Finder writes folders this way: `file:///Users/me/notes/`.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut url = file_url(dir.path());
        if !url.path().ends_with('/') {
            url.set_path(&format!("{}/", url.path()));
        }

        assert_eq!(paths_from_urls(&[url]), vec![canonical(dir.path())]);
    }

    #[test]
    fn urls_that_are_not_local_existing_files_are_left_out() {
        let dir = tempfile::tempdir().expect("tempdir");
        let gone = dir.path().join("gone.md");
        let kept = dir.path().join("kept.md");
        std::fs::write(&kept, "x").expect("write");
        let urls = [
            "https://example.com/a.md".parse().expect("url"),
            "permark://open/a.md".parse().expect("url"),
            "file://other-machine.invalid/share/a.md"
                .parse()
                .expect("url"),
            file_url(&gone),
            file_url(&kept),
        ];

        assert_eq!(paths_from_urls(&urls), vec![canonical(&kept)]);
    }
}
