//! Files and folders the system asks TrailMark to open, other than through the command line.
//!
//! On macOS, Finder does not start the program with the path as an argument: a double-click, "Open With" or a drop
//! on the Dock icon reaches the app as an event (`RunEvent::Opened`), both when the app is starting and when it is
//! already running. [`OpenInbox`] decides what happens to each path:
//!
//! - while the app is starting, the first path is kept for the UI to pick up as its first project (the same place
//!   the command line argument goes, see [`OpenInbox::take_launch_path`]);
//! - while it runs, a path opens in this window when no project is open, and in a new TrailMark process otherwise
//!   (one process per project, as everywhere else in the app).
//!
//! The decision and the conversion from URLs are plain functions with no window system in them, so they are
//! tested on every platform. Only the glue that receives the event and starts the process is macOS-only.
//!
//! The other half of "Open with" on macOS and Linux is declared in `tauri.macos.conf.json`,
//! `tauri.linux.conf.json` and `installer/trailmark.desktop`; the tests in `bundle_config` check those files.

#[cfg(test)]
mod bundle_config;

use std::sync::{Mutex, PoisonError};

#[cfg(any(target_os = "macos", test))]
use tauri::Url;

/// What to do with a path that arrived while the app runs.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Open it in the window that is showing no project.
    OpenHere(String),
    /// Start another TrailMark for it.
    NewProcess(String),
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

    /// Takes in the paths of one delivery, in order, and returns what to do now. `project_open` says whether the
    /// window already shows a project. Only the first path of a delivery can use the window (or become the launch
    /// path): the rest each get a process of their own.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn receive(&self, paths: Vec<String>, project_open: bool) -> Vec<Action> {
        let mut stage = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let mut paths = paths.into_iter();
        let mut actions = Vec::new();
        if let Some(first) = paths.next() {
            match &mut *stage {
                Stage::Launching(held @ None) => *held = Some(first),
                Stage::Launching(Some(_)) => actions.push(Action::NewProcess(first)),
                Stage::Running if project_open => actions.push(Action::NewProcess(first)),
                Stage::Running => actions.push(Action::OpenHere(first)),
            }
        }
        actions.extend(paths.map(Action::NewProcess));
        actions
    }
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

/// Starts another TrailMark for `path` (a folder, or a file for Simple mode) and lets it go its own way.
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
        assert_eq!(inbox.receive(paths(&["/notes"]), false), vec![]);
        assert_eq!(inbox.take_launch_path(), Some("/notes".to_owned()));
    }

    #[test]
    fn the_launch_path_is_handed_over_once_and_the_app_then_counts_as_running() {
        let inbox = OpenInbox::default();
        inbox.receive(paths(&["/notes"]), false);
        assert_eq!(inbox.take_launch_path(), Some("/notes".to_owned()));
        assert_eq!(inbox.take_launch_path(), None);
        assert_eq!(
            inbox.receive(paths(&["/other"]), false),
            vec![open_here("/other")]
        );
    }

    #[test]
    fn nothing_delivered_while_starting_means_no_launch_path() {
        let inbox = OpenInbox::default();
        assert_eq!(inbox.receive(vec![], false), vec![]);
        assert_eq!(inbox.take_launch_path(), None);
    }

    #[test]
    fn an_empty_delivery_while_starting_keeps_the_app_waiting_for_the_next_one() {
        let inbox = OpenInbox::default();
        inbox.receive(vec![], false);
        inbox.receive(paths(&["/notes"]), false);
        assert_eq!(inbox.take_launch_path(), Some("/notes".to_owned()));
    }

    #[test]
    fn when_several_paths_arrive_while_starting_the_first_is_kept_and_the_rest_get_processes() {
        let inbox = OpenInbox::default();
        assert_eq!(
            inbox.receive(paths(&["/a.md", "/b.md", "/c.md"]), false),
            vec![new_process("/b.md"), new_process("/c.md")]
        );
        assert_eq!(inbox.take_launch_path(), Some("/a.md".to_owned()));
    }

    #[test]
    fn a_second_delivery_before_the_ui_asks_gets_a_process_and_keeps_the_first_path() {
        let inbox = OpenInbox::default();
        inbox.receive(paths(&["/a.md"]), false);
        assert_eq!(
            inbox.receive(paths(&["/b.md"]), false),
            vec![new_process("/b.md")]
        );
        assert_eq!(inbox.take_launch_path(), Some("/a.md".to_owned()));
    }

    #[test]
    fn a_running_app_with_no_project_opens_the_first_path_in_its_window() {
        let inbox = OpenInbox::default();
        inbox.take_launch_path();
        assert_eq!(
            inbox.receive(paths(&["/notes", "/more"]), false),
            vec![open_here("/notes"), new_process("/more")]
        );
    }

    #[test]
    fn a_running_app_with_a_project_starts_a_process_for_every_path() {
        let inbox = OpenInbox::default();
        inbox.take_launch_path();
        assert_eq!(
            inbox.receive(paths(&["/notes", "/more"]), true),
            vec![new_process("/notes"), new_process("/more")]
        );
        assert_eq!(inbox.receive(vec![], true), vec![]);
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
            inbox.receive(paths(&["/notes"]), false),
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
            "trailmark://open/a.md".parse().expect("url"),
            "file://other-machine.invalid/share/a.md"
                .parse()
                .expect("url"),
            file_url(&gone),
            file_url(&kept),
        ];

        assert_eq!(paths_from_urls(&urls), vec![canonical(&kept)]);
    }
}
