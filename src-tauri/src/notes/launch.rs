//! One Notes window, however many launchers.
//!
//! The Notes window is a program of its own (`trailmark --notes`) and there is only ever one for each app data folder. It
//! listens on a local socket that only the same user can reach (see [`Endpoint`]). Any other TrailMark window, and any
//! launch of the program that finds the window already there, asks it over that socket to come forward, with a new note
//! ready if they want one; when nobody listens, the one that asked starts the window and asks it once it is up. Two
//! programs that ask at the same moment start one window between them.
//!
//! The conversation is one line each way: the request as JSON, then `ok`.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::time::{sleep, timeout, Instant};

use crate::bridge::socket::{ConnectError, Endpoint, ListenError, Listening};
use crate::session::StartIn;

/// The argument that starts the program as the Notes window.
pub(crate) const NOTES_FLAG: &str = "--notes";
/// How long a request waits for the window to answer.
const ANSWER_WITHIN: Duration = Duration::from_secs(3);
/// How long a program that started the window waits for it to listen.
const START_WITHIN: Duration = Duration::from_secs(20);
/// The pause between tries while the window starts.
const START_POLL: Duration = Duration::from_millis(100);
/// A request is a few bytes; more than this is not one.
const MAX_REQUEST_BYTES: u64 = 4096;

/// Whether the program is to be the Notes window: it was started with `--notes`, or with nothing to open while the person
/// chose to start in Notes. `args` are the program's arguments, the program itself first.
pub(crate) fn is_notes_launch(args: &[std::ffi::OsString], start_in: StartIn) -> bool {
    match args.get(1) {
        Some(first) => first == NOTES_FLAG,
        None => start_in == StartIn::Notes,
    }
}

/// What a request to the Notes window asks for. Matches the UI's `NotesRequest`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NotesRequest {
    /// Start a new note (and put the caret in it).
    pub new_note: bool,
    /// Show this note.
    pub open: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum LaunchError {
    #[error(transparent)]
    Connect(#[from] ConnectError),
    #[error("the Notes window did not answer")]
    NoAnswer,
    #[error("the Notes window did not understand the request")]
    Refused,
    #[error("cannot start the Notes window: {0}")]
    Start(io::Error),
    #[error("the Notes window did not start in time")]
    Timeout,
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Asks the Notes window to come forward. Fails when nobody listens.
pub(crate) async fn ask(endpoint: &Endpoint, request: &NotesRequest) -> Result<(), LaunchError> {
    let stream = timeout(ANSWER_WITHIN, endpoint.connect())
        .await
        .map_err(|_| LaunchError::NoAnswer)??;
    let (received, mut sent) = tokio::io::split(stream);
    let mut line = serde_json::to_string(request).map_err(io::Error::other)?;
    line.push('\n');
    sent.write_all(line.as_bytes()).await?;
    sent.flush().await?;
    let mut answer = String::new();
    timeout(
        ANSWER_WITHIN,
        BufReader::new(received).read_line(&mut answer),
    )
    .await
    .map_err(|_| LaunchError::NoAnswer)??;
    if answer.trim() == "ok" {
        Ok(())
    } else {
        Err(LaunchError::Refused)
    }
}

/// Asks until the window answers or `within` has passed.
async fn ask_until(
    endpoint: &Endpoint,
    request: &NotesRequest,
    within: Duration,
) -> Result<(), LaunchError> {
    let deadline = Instant::now() + within;
    loop {
        match ask(endpoint, request).await {
            Ok(()) => return Ok(()),
            Err(_) if Instant::now() < deadline => sleep(START_POLL).await,
            Err(_) => return Err(LaunchError::Timeout),
        }
    }
}

/// Brings the Notes window forward with `request`, starting it with `start` when it is not running. Programs that ask at
/// the same moment start one window: the one that takes the turn starts it, the others wait for it.
pub(crate) async fn show_or_start(
    endpoint: &Endpoint,
    request: &NotesRequest,
    start: impl FnOnce() -> io::Result<()>,
) -> Result<(), LaunchError> {
    if ask(endpoint, request).await.is_ok() {
        return Ok(());
    }
    match endpoint.try_launch_lock()? {
        Some(turn) => {
            // the window may have come up between the ask and the turn
            if ask(endpoint, request).await.is_ok() {
                return Ok(());
            }
            start().map_err(LaunchError::Start)?;
            let asked = ask_until(endpoint, request, START_WITHIN).await;
            drop(turn);
            asked
        }
        None => ask_until(endpoint, request, START_WITHIN).await,
    }
}

/// What a program that wants to be the Notes window finds out first.
pub(crate) enum Claim {
    /// A Notes window is running and has been asked to come forward: this program has nothing more to do.
    Forwarded,
    /// Nobody was running one: this program is the Notes window, and listens.
    Listening(Listening),
    /// Neither: the socket cannot be used (the path is too long, the system refused). The program runs without one.
    Unavailable,
}

/// Becomes the one Notes window, or forwards `request` to the one that is. Call it where a Tokio runtime is current.
pub(crate) async fn claim(endpoint: &Endpoint, request: &NotesRequest) -> Claim {
    if ask(endpoint, request).await.is_ok() {
        return Claim::Forwarded;
    }
    match endpoint.listen() {
        Ok(listening) => Claim::Listening(listening),
        // another window took the socket in the meantime
        Err(ListenError::InUse(_)) => match ask_until(endpoint, request, ANSWER_WITHIN).await {
            Ok(()) => Claim::Forwarded,
            Err(_) => Claim::Unavailable,
        },
        Err(_) => Claim::Unavailable,
    }
}

/// Answers the requests that come in, for as long as the program lives. `on_request` is told of each one.
pub(crate) async fn serve(
    listening: Listening,
    on_request: Arc<dyn Fn(NotesRequest) + Send + Sync>,
) {
    loop {
        match listening.accept().await {
            Ok(stream) => {
                let on_request = Arc::clone(&on_request);
                tokio::spawn(async move {
                    // a client that says nothing must not hold the next one up
                    let _ = timeout(ANSWER_WITHIN, answer(stream, on_request.as_ref())).await;
                });
            }
            Err(_) => sleep(START_POLL).await,
        }
    }
}

async fn answer<S>(stream: S, on_request: &(dyn Fn(NotesRequest) + Send + Sync)) -> io::Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite,
{
    let (received, mut sent) = tokio::io::split(stream);
    let mut line = String::new();
    BufReader::new(received.take(MAX_REQUEST_BYTES))
        .read_line(&mut line)
        .await?;
    match serde_json::from_str::<NotesRequest>(line.trim()) {
        Ok(request) => {
            on_request(request);
            sent.write_all(b"ok\n").await?;
        }
        Err(_) => sent.write_all(b"error\n").await?,
    }
    sent.flush().await
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use super::*;
    use crate::bridge::socket::socket_folder;

    fn endpoint(dir: &tempfile::TempDir, name: &str) -> Endpoint {
        Endpoint::in_folder(dir.path(), Path::new(name), "tester").expect("endpoint")
    }

    /// A Notes window that remembers what it was asked.
    struct Window {
        asked: Arc<Mutex<Vec<NotesRequest>>>,
    }

    impl Window {
        fn listen(endpoint: &Endpoint) -> Self {
            let asked = Arc::new(Mutex::new(Vec::new()));
            let heard = Arc::clone(&asked);
            let listening = endpoint.listen().expect("the socket is free");
            tokio::spawn(serve(
                listening,
                Arc::new(move |request| heard.lock().expect("lock").push(request)),
            ));
            Self { asked }
        }

        fn requests(&self) -> Vec<NotesRequest> {
            self.asked.lock().expect("lock").clone()
        }
    }

    fn new_note() -> NotesRequest {
        NotesRequest {
            new_note: true,
            open: None,
        }
    }

    #[tokio::test]
    async fn a_request_reaches_the_window_that_listens() {
        let dir = socket_folder();
        let endpoint = endpoint(&dir, "/work/notes-window");
        let window = Window::listen(&endpoint);
        ask(&endpoint, &new_note()).await.expect("asked");
        ask(
            &endpoint,
            &NotesRequest {
                new_note: false,
                open: Some("a.md".to_owned()),
            },
        )
        .await
        .expect("asked");
        ask(&endpoint, &NotesRequest::default())
            .await
            .expect("asked");
        assert_eq!(
            window.requests(),
            [
                new_note(),
                NotesRequest {
                    new_note: false,
                    open: Some("a.md".to_owned())
                },
                NotesRequest::default()
            ]
        );
    }

    #[tokio::test]
    async fn nobody_to_ask_is_an_error_and_a_junk_request_is_refused() {
        let dir = socket_folder();
        let endpoint = endpoint(&dir, "/work/nobody-home");
        assert!(ask(&endpoint, &new_note()).await.is_err());

        let window = Window::listen(&endpoint);
        // a client that speaks nonsense is told so and changes nothing
        let stream = endpoint.connect().await.expect("connect");
        let (received, mut sent) = tokio::io::split(stream);
        sent.write_all(b"this is not json\n").await.expect("write");
        sent.flush().await.expect("flush");
        let mut answer = String::new();
        BufReader::new(received)
            .read_line(&mut answer)
            .await
            .expect("read");
        assert_eq!(answer, "error\n");
        assert_eq!(window.requests(), Vec::<NotesRequest>::new());
        // and the window is still there for the next one
        ask(&endpoint, &new_note()).await.expect("asked");
    }

    #[tokio::test]
    async fn the_second_window_finds_the_first_and_does_not_listen() {
        let dir = socket_folder();
        let endpoint = endpoint(&dir, "/work/claim");
        let first = Window::listen(&endpoint);
        assert!(matches!(
            claim(&endpoint, &new_note()).await,
            Claim::Forwarded
        ));
        assert_eq!(first.requests(), [new_note()]);
    }

    #[tokio::test]
    async fn a_window_that_is_not_running_is_started_once_and_asked_when_it_listens() {
        let dir = socket_folder();
        let endpoint = endpoint(&dir, "/work/start");
        let started = AtomicUsize::new(0);
        let window: Mutex<Option<Window>> = Mutex::new(None);
        show_or_start(&endpoint, &new_note(), || {
            started.fetch_add(1, Ordering::SeqCst);
            // the program that was started comes up and listens
            *window.lock().expect("lock") = Some(Window::listen(&endpoint));
            Ok(())
        })
        .await
        .expect("shown");
        assert_eq!(started.load(Ordering::SeqCst), 1);
        let requests = window
            .lock()
            .expect("lock")
            .as_ref()
            .expect("window")
            .requests();
        assert_eq!(requests, [new_note()]);

        // it is there now: nothing is started again
        show_or_start(&endpoint, &NotesRequest::default(), || {
            started.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .await
        .expect("shown");
        assert_eq!(started.load(Ordering::SeqCst), 1);
        let requests = window
            .lock()
            .expect("lock")
            .as_ref()
            .expect("window")
            .requests();
        assert_eq!(requests.len(), 2);
    }

    #[tokio::test]
    async fn a_program_that_cannot_start_the_window_says_so() {
        let dir = socket_folder();
        let endpoint = endpoint(&dir, "/work/cannot-start");
        let error = show_or_start(&endpoint, &new_note(), || {
            Err(io::Error::from(io::ErrorKind::NotFound))
        })
        .await
        .expect_err("not started");
        assert!(matches!(error, LaunchError::Start(_)), "{error}");
        // the turn was given back: the next try can start it
        let again = show_or_start(&endpoint, &new_note(), || {
            Err(io::Error::from(io::ErrorKind::NotFound))
        })
        .await
        .expect_err("not started");
        assert!(matches!(again, LaunchError::Start(_)), "{again}");
    }

    #[test]
    fn the_program_is_the_notes_window_when_asked_to_be_or_when_it_has_nothing_to_open_and_the_person_chose_notes(
    ) {
        let args = |rest: &[&str]| -> Vec<std::ffi::OsString> {
            std::iter::once("trailmark")
                .chain(rest.iter().copied())
                .map(Into::into)
                .collect()
        };
        for start in [StartIn::Welcome, StartIn::Notes] {
            assert!(is_notes_launch(&args(&["--notes"]), start));
            // something to open is a project, whatever the start setting says
            assert!(!is_notes_launch(&args(&["F:/docs"]), start));
            assert!(!is_notes_launch(&args(&["mcp"]), start));
        }
        assert!(!is_notes_launch(&args(&[]), StartIn::Welcome));
        assert!(is_notes_launch(&args(&[]), StartIn::Notes));
    }

    #[test]
    fn the_notes_window_has_a_socket_of_its_own_for_each_data_folder() {
        let (one, other) = (
            Path::new("/data/dev.trailmark.app"),
            Path::new("/data/dev.trailmark.e2e"),
        );
        let a = Endpoint::for_notes(one).expect("endpoint");
        assert_eq!(a, Endpoint::for_notes(one).expect("endpoint"));
        assert_ne!(a, Endpoint::for_notes(other).expect("endpoint"));
        // and it is not the socket of a project in the same folder
        assert_ne!(a, Endpoint::for_project(one).expect("endpoint"));
    }
}
