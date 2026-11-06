// Test code: counts and sizes here are small, and plain `assert!(x.is_empty())` reads best.
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::items_after_statements,
    clippy::single_match_else
)]

use std::io::Read;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use permark_core::downloads::{Request, Response, Scripted, ScriptedTransport, Transport};

use super::*;
use crate::tests::{kit_of, pin_of, serving, TEST_URL};

/// A valid tiny rank file: the 256 bytes, then two merges.
fn rank_file() -> Vec<u8> {
    let mut tokens: Vec<Vec<u8>> = (0..=255_u8).map(|b| vec![b]).collect();
    tokens.push(b"ab".to_vec());
    tokens.push(b"abc".to_vec());
    let mut out = Vec::new();
    for (rank, token) in tokens.iter().enumerate() {
        out.extend_from_slice(STANDARD.encode(token).as_bytes());
        out.extend_from_slice(
            format!(
                " {rank}
"
            )
            .as_bytes(),
        );
    }
    out
}

/// A network answering `answers` in order at [`TEST_URL`] (the last one repeats).
fn scripted(answers: Vec<Scripted>) -> Arc<ScriptedTransport> {
    Arc::new(
        answers
            .into_iter()
            .fold(ScriptedTransport::new(), |t, a| t.answer(TEST_URL, a)),
    )
}

/// Serves `body` slowly (a pause before every 100 bytes): a download that runs long enough to be stopped.
struct Slow {
    body: Vec<u8>,
    pause: Duration,
}

struct SlowBody {
    bytes: Vec<u8>,
    at: usize,
    pause: Duration,
}

impl Read for SlowBody {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        std::thread::sleep(self.pause);
        let n = buf.len().min(100).min(self.bytes.len() - self.at);
        buf[..n].copy_from_slice(&self.bytes[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

impl Transport for Slow {
    fn get(&self, _request: &Request<'_>) -> Result<Response, DownloadError> {
        Ok(Response {
            status: 200,
            location: None,
            content_range: None,
            content_length: Some(self.body.len() as u64),
            body: Box::new(SlowBody {
                bytes: self.body.clone(),
                at: 0,
                pause: self.pause,
            }),
        })
    }
}

fn slow(body: &[u8], pause: Duration) -> Arc<Slow> {
    Arc::new(Slow {
        body: body.to_vec(),
        pause,
    })
}

fn silent() -> Listener {
    Arc::new(|_| {})
}

fn store_with(dir: &Path, body: &[u8], transport: Arc<dyn Transport>) -> Arc<VocabStore> {
    let pins = [pin_of(body), pin_of(body)];
    VocabStore::new(dir.to_path_buf(), pins, Some(&kit_of(transport)), silent())
}

fn wait_until(
    store: &VocabStore,
    encoding: Encoding,
    done: impl Fn(&VocabState) -> bool,
) -> VocabState {
    let start = Instant::now();
    loop {
        let state = store.states()[encoding.index()].clone();
        if done(&state) {
            return state;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "stuck in {state:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn settled(state: &VocabState) -> bool {
    !matches!(state, VocabState::Downloading { .. })
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn the_state_machine_moves_only_along_its_edges() {
    use Event::{Canceled, Failed, Finished, Found, Progress, Removed, Started};
    let absent = VocabState::Absent;
    let downloading = VocabState::Downloading { done: 5, total: 9 };
    let ready = VocabState::Ready;
    let failed = VocabState::Failed {
        reason: VocabFailure::Offline,
    };
    let started = VocabState::Downloading { done: 0, total: 9 };
    assert_eq!(absent.next(Found), ready);
    assert_eq!(absent.next(Started { total: 9 }), started);
    assert_eq!(failed.next(Started { total: 9 }), started);
    assert_eq!(
        downloading.next(Progress { done: 6, total: 9 }),
        VocabState::Downloading { done: 6, total: 9 }
    );
    assert_eq!(downloading.next(Finished), ready);
    assert_eq!(
        downloading.next(Failed(VocabFailure::Hash)),
        VocabState::Failed {
            reason: VocabFailure::Hash
        }
    );
    assert_eq!(downloading.next(Canceled), absent);
    assert_eq!(
        ready.next(Failed(VocabFailure::Hash)),
        VocabState::Failed {
            reason: VocabFailure::Hash
        }
    );
    assert_eq!(ready.next(Removed), absent);
    assert_eq!(failed.next(Removed), absent);
    // events that do not fit change nothing
    assert_eq!(ready.next(Started { total: 9 }), ready);
    assert_eq!(downloading.next(Started { total: 9 }), downloading);
    assert_eq!(downloading.next(Removed), downloading);
    assert_eq!(absent.next(Finished), absent);
    assert_eq!(absent.next(Progress { done: 1, total: 9 }), absent);
    assert_eq!(ready.next(Found), ready);
    assert_eq!(failed.next(Canceled), failed);
}

#[test]
fn the_pins_decode() {
    let pin = Pin::official(Encoding::O200k);
    assert_eq!(pin.sha256[0], 0x44);
    assert_eq!(pin.sha256[31], 0x2d);
    assert_eq!(pin.bytes, 3_613_922);
    assert_eq!(hex32(&"ab".repeat(32)), [0xab; 32]);
    // the official address, under the one allowed host
    let unit = unit_of(Encoding::O200k, &pin).unwrap();
    assert_eq!(
        unit.assets()[0].url(),
        "https://openaipublic.blob.core.windows.net/encodings/o200k_base.tiktoken"
    );
    assert_eq!(unit.assets()[0].name(), "o200k_base.tiktoken");
}

#[test]
fn nothing_is_fetched_until_a_download_is_asked_for() {
    let dir = tempfile::tempdir().unwrap();
    let body = rank_file();
    let net = serving(&body);
    let store = store_with(dir.path(), &body, Arc::clone(&net) as Arc<dyn Transport>);
    assert_eq!(store.states(), [VocabState::Absent, VocabState::Absent]);
    assert!(matches!(
        store.load(Encoding::O200k),
        Err(Unavailable::NoVocab)
    ));
    store.cancel(Encoding::O200k);
    assert!(store.remove(Encoding::O200k).is_ok());
    store.shutdown();
    assert!(net.asked().is_empty());
    // not even the folder is made
    assert!(files_in(dir.path()).is_empty());
}

#[test]
fn a_download_keeps_the_official_file_and_it_loads() {
    let dir = tempfile::tempdir().unwrap();
    let body = rank_file();
    let net = serving(&body);
    let store = store_with(
        &dir.path().join("cache"),
        &body,
        Arc::clone(&net) as Arc<dyn Transport>,
    );
    store.download(Encoding::O200k);
    assert_eq!(
        wait_until(&store, Encoding::O200k, settled),
        VocabState::Ready
    );
    assert_eq!(net.asked().len(), 1);
    // the file + the engine's install record, nothing partial
    assert_eq!(
        files_in(&dir.path().join("cache")),
        ["o200k.installed.json", "o200k_base.tiktoken"]
    );
    let bpe = store.load(Encoding::O200k).unwrap();
    assert_eq!(bpe.ranks().len(), 258);
    // the other encoding was not touched
    assert_eq!(store.states()[Encoding::Cl100k.index()], VocabState::Absent);
    // a second download asked while it is there does nothing
    store.download(Encoding::O200k);
    store.shutdown();
    assert_eq!(net.asked().len(), 1);
}

/// Files downloaded before the engine have no install record: present by size, never fetched again.
#[test]
fn a_file_of_the_right_size_is_there_at_start_and_checked_when_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let body = rank_file();
    fs::write(dir.path().join(Encoding::O200k.file()), &body).unwrap();
    let net = Arc::new(ScriptedTransport::new());
    let store = store_with(dir.path(), &body, Arc::clone(&net) as Arc<dyn Transport>);
    assert_eq!(store.states()[0], VocabState::Ready);
    assert!(store.load(Encoding::O200k).is_ok());
    store.download(Encoding::O200k);
    store.shutdown();
    assert!(net.asked().is_empty());
}

#[test]
fn a_damaged_cached_file_is_refused_deleted_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    let body = rank_file();
    let mut damaged = body.clone();
    damaged[3] ^= 1;
    fs::write(dir.path().join(Encoding::O200k.file()), &damaged).unwrap();
    let store = store_with(dir.path(), &body, serving(&body));
    assert_eq!(store.states()[0], VocabState::Ready);
    assert!(matches!(
        store.load(Encoding::O200k),
        Err(Unavailable::NoVocab)
    ));
    assert_eq!(
        store.states()[0],
        VocabState::Failed {
            reason: VocabFailure::Hash
        }
    );
    assert!(files_in(dir.path()).is_empty());
}

#[test]
fn a_file_of_the_wrong_size_does_not_count_as_there() {
    let dir = tempfile::tempdir().unwrap();
    let body = rank_file();
    fs::write(dir.path().join(Encoding::O200k.file()), &body[..100]).unwrap();
    let store = store_with(dir.path(), &body, serving(&body));
    assert_eq!(store.states()[0], VocabState::Absent);
}

#[test]
fn another_file_of_the_same_size_is_refused_and_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let body = rank_file();
    let mut other = body.clone();
    let last = other.len() - 2;
    other[last] = b'9';
    let store = store_with(dir.path(), &body, serving(&other));
    store.download(Encoding::Cl100k);
    assert_eq!(
        wait_until(&store, Encoding::Cl100k, settled),
        VocabState::Failed {
            reason: VocabFailure::Hash
        }
    );
    assert!(
        files_in(dir.path()).is_empty(),
        "{:?}",
        files_in(dir.path())
    );
}

#[test]
fn a_short_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let body = rank_file();
    let store = store_with(dir.path(), &body, serving(&body[..body.len() - 10]));
    store.download(Encoding::O200k);
    assert_eq!(
        wait_until(&store, Encoding::O200k, settled),
        VocabState::Failed {
            reason: VocabFailure::Hash
        }
    );
    assert!(files_in(dir.path()).is_empty());
}

/// A body announced longer than the pin is refused before it is read (the engine never reads past the pinned size).
#[test]
fn a_file_longer_than_the_pin_stops_early() {
    let dir = tempfile::tempdir().unwrap();
    let body = rank_file();
    let mut long = body.clone();
    long.extend(std::iter::repeat_n(b'x', 1_000_000));
    let net = serving(&long);
    let store = store_with(dir.path(), &body, Arc::clone(&net) as Arc<dyn Transport>);
    store.download(Encoding::O200k);
    assert_eq!(
        wait_until(&store, Encoding::O200k, settled),
        VocabState::Failed {
            reason: VocabFailure::Hash
        }
    );
    assert_eq!(net.asked().len(), 1);
    assert!(files_in(dir.path()).is_empty());
}

#[test]
fn offline_and_http_failures_say_why_and_retry_works() {
    let dir = tempfile::tempdir().unwrap();
    let body = rank_file();
    // a transfer dropped midway, then a 404, then the file
    let net = scripted(vec![
        Scripted::ok(&body).cut_after(100),
        Scripted::status(404),
        Scripted::ok(&body),
    ]);
    let store = store_with(dir.path(), &body, Arc::clone(&net) as Arc<dyn Transport>);
    store.download(Encoding::O200k);
    assert_eq!(
        wait_until(&store, Encoding::O200k, settled),
        VocabState::Failed {
            reason: VocabFailure::Offline
        }
    );
    // the dropped transfer left nothing partial
    assert!(files_in(dir.path()).iter().all(|n| !n.contains(".part")));
    store.download(Encoding::O200k);
    assert_eq!(
        wait_until(&store, Encoding::O200k, |s| matches!(
            s,
            VocabState::Failed {
                reason: VocabFailure::Http { .. }
            }
        )),
        VocabState::Failed {
            reason: VocabFailure::Http { status: 404 }
        }
    );
    store.download(Encoding::O200k);
    assert_eq!(
        wait_until(&store, Encoding::O200k, |s| *s == VocabState::Ready),
        VocabState::Ready
    );
    assert_eq!(net.asked().len(), 3);
    assert!(files_in(dir.path()).iter().all(|n| !n.contains(".part")));
}

#[test]
fn a_redirect_off_the_source_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let body = rank_file();
    let net = scripted(vec![Scripted::redirect(302, "https://elsewhere.example/x")]);
    let store = store_with(dir.path(), &body, Arc::clone(&net) as Arc<dyn Transport>);
    store.download(Encoding::O200k);
    assert_eq!(
        wait_until(&store, Encoding::O200k, settled),
        VocabState::Failed {
            reason: VocabFailure::Offline
        }
    );
    // the other host was never asked
    assert_eq!(net.asked().len(), 1);
    assert!(files_in(dir.path()).is_empty());
}

#[test]
fn without_a_download_service_a_download_fails_offline() {
    let dir = tempfile::tempdir().unwrap();
    let body = rank_file();
    let pins = [pin_of(&body), pin_of(&body)];
    let store = VocabStore::new(dir.path().to_path_buf(), pins, None, silent());
    store.download(Encoding::O200k);
    assert_eq!(
        store.states()[0],
        VocabState::Failed {
            reason: VocabFailure::Offline
        }
    );
    store.cancel(Encoding::O200k);
    store.shutdown();
    assert!(files_in(dir.path()).is_empty());
}

#[test]
fn a_cancelled_download_leaves_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let body = rank_file();
    let store = store_with(dir.path(), &body, slow(&body, Duration::from_millis(5)));
    store.download(Encoding::O200k);
    assert!(matches!(
        store.states()[0],
        VocabState::Downloading { done: 0, .. }
    ));
    store.cancel(Encoding::O200k);
    assert_eq!(
        wait_until(&store, Encoding::O200k, settled),
        VocabState::Absent
    );
    // the engine keeps a cancelled part for resume; the store removes it before it says so
    assert!(files_in(dir.path()).is_empty());
    // and it can start again
    store.download(Encoding::O200k);
    store.shutdown();
    assert!(files_in(dir.path()).is_empty());
}

#[test]
fn shutdown_stops_a_running_download_and_waits_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let body = rank_file();
    let store = store_with(dir.path(), &body, slow(&body, Duration::from_millis(20)));
    store.download(Encoding::O200k);
    store.download(Encoding::Cl100k);
    store.shutdown();
    assert_eq!(store.states(), [VocabState::Absent, VocabState::Absent]);
    assert!(files_in(dir.path()).is_empty());
}

#[test]
fn removing_deletes_the_file_and_unloads_the_tables() {
    let dir = tempfile::tempdir().unwrap();
    let body = rank_file();
    let store = store_with(dir.path(), &body, serving(&body));
    store.download(Encoding::O200k);
    wait_until(&store, Encoding::O200k, settled);
    assert!(store.load(Encoding::O200k).is_ok());
    store.remove(Encoding::O200k).unwrap();
    assert_eq!(store.states()[0], VocabState::Absent);
    // file and install record gone
    assert!(files_in(dir.path()).is_empty());
    assert!(matches!(
        store.load(Encoding::O200k),
        Err(Unavailable::NoVocab)
    ));
}

#[test]
fn the_listener_hears_every_change() {
    let dir = tempfile::tempdir().unwrap();
    let body = rank_file();
    let heard: Arc<Mutex<Vec<VocabState>>> = Arc::default();
    let sink = Arc::clone(&heard);
    let listener: Listener = Arc::new(move |states| lock(&sink).push(states[0].clone()));
    let pins = [pin_of(&body), pin_of(&body)];
    let store = VocabStore::new(
        dir.path().to_path_buf(),
        pins,
        Some(&kit_of(serving(&body))),
        listener,
    );
    store.download(Encoding::O200k);
    wait_until(&store, Encoding::O200k, |s| *s == VocabState::Ready);
    let states = lock(&heard).clone();
    assert!(matches!(
        states.first(),
        Some(VocabState::Downloading { done: 0, .. })
    ));
    assert_eq!(states.last(), Some(&VocabState::Ready));
}

#[test]
fn stale_part_files_are_removed_at_start() {
    let dir = tempfile::tempdir().unwrap();
    let body = rank_file();
    // a part of an older version (no meta), and a resumable part the engine kept after a dropped transfer
    fs::write(dir.path().join("cl100k_base.tiktoken.part"), b"half").unwrap();
    let kit = kit_of(scripted(vec![Scripted::ok(&body).cut_after(100)]));
    let (tx, rx) = std::sync::mpsc::channel();
    let engine = kit.downloads(
        dir.path().to_path_buf(),
        Arc::new(move |_, update| {
            if !matches!(update, Update::Progress(_)) {
                let _ = tx.send(update.clone());
            }
        }),
    );
    engine.enqueue(unit_of(Encoding::O200k, &pin_of(&body)).unwrap());
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(10)).unwrap(),
        Update::Failed(DownloadError::Offline)
    );
    drop(engine);
    assert!(files_in(dir.path()).contains(&"o200k_base.tiktoken.part".to_owned()));
    let _store = store_with(dir.path(), &body, serving(&body));
    assert!(files_in(dir.path()).is_empty());
}
