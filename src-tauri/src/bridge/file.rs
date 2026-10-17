//! `.permark/local/agent.json`: the `get_context` answer as a file, for a shell script or a tool that does not
//! speak MCP. It exists only while the agent connection is on, is rewritten at most every 300 ms, and is deleted
//! when the connection is turned off or the project is closed.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::context::Published;
use crate::session::{remove_local_state, write_local_state};

/// The file is rewritten at most this often. One constant to change.
pub const WRITE_INTERVAL: Duration = Duration::from_millis(300);

/// Decides when a file that changes often may be written: at once if it was not written within the interval,
/// otherwise when the interval is over. Pure, so the rule is tested with made-up instants.
#[derive(Debug)]
struct Throttle {
    interval: Duration,
    last_write: Option<Instant>,
    pending: bool,
}

impl Throttle {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            last_write: None,
            pending: false,
        }
    }

    /// There is something new to write.
    fn changed(&mut self) {
        self.pending = true;
    }

    /// When the pending change may be written; `None` when nothing is pending.
    fn due_at(&self, now: Instant) -> Option<Instant> {
        self.pending
            .then(|| self.last_write.map_or(now, |last| last + self.interval))
    }

    fn due(&self, now: Instant) -> bool {
        self.due_at(now).is_some_and(|at| now >= at)
    }

    fn written(&mut self, now: Instant) {
        self.last_write = Some(now);
        self.pending = false;
    }
}

enum Msg {
    Publish(Published),
    /// The file lives elsewhere now (the project got a `.permark/` folder).
    Relocate(PathBuf),
    /// Write what is pending now, and say so (a test waits for the file this way).
    #[cfg(test)]
    Flush(Sender<()>),
    /// Say so once everything sent before has been looked at (a test waits for the thread this way).
    #[cfg(test)]
    Settle(Sender<()>),
    Stop,
}

/// The state file of a running connection.
pub struct ContextFile {
    tx: Sender<Msg>,
    handle: Option<JoinHandle<()>>,
}

impl ContextFile {
    /// Starts keeping `path` up to date, beginning with `initial`.
    pub fn start(path: PathBuf, interval: Duration, initial: Published) -> Self {
        let (tx, rx) = mpsc::channel();
        let handle = thread::Builder::new()
            .name("permark-agent-file".to_owned())
            .spawn(move || run(&rx, path, interval, initial))
            .ok();
        Self { tx, handle }
    }

    pub fn publish(&self, published: Published) {
        let _ = self.tx.send(Msg::Publish(published));
    }

    pub fn relocate(&self, path: PathBuf) {
        let _ = self.tx.send(Msg::Relocate(path));
    }

    /// Writes what is pending now and waits until it is written.
    #[cfg(test)]
    pub fn flush(&self) {
        let (ack, done) = mpsc::channel();
        if self.tx.send(Msg::Flush(ack)).is_ok() {
            let _ = done.recv();
        }
    }

    #[cfg(test)]
    fn settle(&self) {
        let (ack, done) = mpsc::channel();
        if self.tx.send(Msg::Settle(ack)).is_ok() {
            let _ = done.recv();
        }
    }

    /// Deletes the file and stops. Safe to call more than once.
    pub fn stop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ContextFile {
    fn drop(&mut self) {
        self.stop();
    }
}

fn write(path: &Path, published: &Published) {
    // Best effort: a project folder that cannot be written to just has no state file.
    if let Ok(bytes) = serde_json::to_vec_pretty(published) {
        let _ = write_local_state(path, &bytes);
    }
}

fn run(rx: &Receiver<Msg>, mut path: PathBuf, interval: Duration, initial: Published) {
    let mut latest = initial;
    let mut throttle = Throttle::new(interval);
    throttle.changed();
    loop {
        let now = Instant::now();
        let msg = match throttle.due_at(now) {
            Some(at) => rx.recv_timeout(at.saturating_duration_since(now)),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match msg {
            Ok(Msg::Publish(published)) => {
                latest = published;
                throttle.changed();
            }
            Ok(Msg::Relocate(new_path)) => {
                let _ = remove_local_state(&path);
                path = new_path;
                throttle.changed();
            }
            #[cfg(test)]
            Ok(Msg::Flush(ack)) => {
                if throttle.pending {
                    write(&path, &latest);
                    throttle.written(Instant::now());
                }
                let _ = ack.send(());
                continue;
            }
            #[cfg(test)]
            Ok(Msg::Settle(ack)) => {
                let _ = ack.send(());
            }
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        if throttle.due(now) {
            write(&path, &latest);
            throttle.written(now);
        }
    }
    let _ = remove_local_state(&path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::context::AgentContext;

    fn published(document: &str, at: u64) -> Published {
        Published {
            context: AgentContext {
                document: Some(document.to_owned()),
                ..AgentContext::default()
            },
            updated_at_ms: at,
        }
    }

    fn read(path: &PathBuf) -> Option<Published> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }

    /// Longer than any test runs, so only an explicit flush (or the first write) puts the file down.
    const FOREVER: Duration = Duration::from_secs(3600);

    #[test]
    fn the_first_change_is_written_at_once_and_the_next_waits_for_the_interval() {
        let t0 = Instant::now();
        let mut throttle = Throttle::new(Duration::from_millis(300));
        assert_eq!(throttle.due_at(t0), None, "nothing to write");
        throttle.changed();
        assert!(throttle.due(t0), "never written: now");
        throttle.written(t0);
        assert!(
            !throttle.due(t0 + Duration::from_millis(500)),
            "nothing pending"
        );
        throttle.changed();
        assert!(!throttle.due(t0 + Duration::from_millis(299)));
        assert!(throttle.due(t0 + Duration::from_millis(300)));
        assert_eq!(
            throttle.due_at(t0 + Duration::from_millis(100)),
            Some(t0 + Duration::from_millis(300))
        );
        // After a long pause a change is written right away.
        assert!(throttle.due(t0 + Duration::from_secs(60)));
    }

    #[test]
    fn many_changes_inside_the_interval_are_one_write() {
        let t0 = Instant::now();
        let mut throttle = Throttle::new(Duration::from_millis(300));
        throttle.changed();
        throttle.written(t0);
        for ms in [10, 20, 30, 200] {
            throttle.changed();
            assert!(!throttle.due(t0 + Duration::from_millis(ms)));
        }
        assert_eq!(
            throttle.due_at(t0 + Duration::from_millis(250)),
            Some(t0 + Duration::from_millis(300))
        );
    }

    #[test]
    fn the_file_appears_with_the_context_and_is_deleted_on_stop() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("local").join("agent.json");
        let mut file = ContextFile::start(path.clone(), FOREVER, published("a.md", 1));
        file.flush();
        assert_eq!(read(&path), Some(published("a.md", 1)));
        file.stop();
        assert!(!path.exists(), "the file is gone with the connection");
        file.stop();
    }

    #[test]
    fn a_change_inside_the_interval_waits_and_a_flush_writes_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("agent.json");
        let file = ContextFile::start(path.clone(), FOREVER, published("a.md", 1));
        file.flush();
        file.publish(published("b.md", 2));
        file.settle();
        assert_eq!(read(&path), Some(published("a.md", 1)), "not rewritten yet");
        file.flush();
        assert_eq!(read(&path), Some(published("b.md", 2)));
    }

    #[test]
    fn the_file_moves_when_the_state_moves() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = dir.path().join("one").join("agent.json");
        let second = dir.path().join("two").join("agent.json");
        let file = ContextFile::start(first.clone(), FOREVER, published("a.md", 1));
        file.flush();
        assert!(first.exists());
        file.relocate(second.clone());
        file.flush();
        assert!(!first.exists());
        assert_eq!(read(&second), Some(published("a.md", 1)));
    }

    /// A repository can commit `.permark/local` as a link to some other folder.
    #[test]
    fn nothing_is_written_or_deleted_behind_a_link() {
        use crate::util::testing::{dir_link, remove_link};

        let dir = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("outside");
        let theirs = outside.path().join("agent.json");
        std::fs::write(&theirs, "not ours").expect("write");
        let permark = dir.path().join(".permark");
        std::fs::create_dir(&permark).expect("mkdir");
        dir_link(&permark.join("local"), outside.path());
        let path = permark.join("local").join("agent.json");

        let mut file = ContextFile::start(path.clone(), FOREVER, published("a.md", 1));
        file.flush();
        file.relocate(permark.join("local").join("other.json"));
        file.flush();
        file.stop();
        assert_eq!(
            std::fs::read_to_string(&theirs).expect("read"),
            "not ours",
            "neither overwritten nor deleted"
        );
        assert!(!outside.path().join("other.json").exists());
        remove_link(&permark.join("local"));
    }

    #[test]
    fn dropping_the_handle_deletes_the_file_too() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("agent.json");
        let file = ContextFile::start(path.clone(), FOREVER, published("a.md", 1));
        file.flush();
        drop(file);
        assert!(!path.exists());
    }
}
