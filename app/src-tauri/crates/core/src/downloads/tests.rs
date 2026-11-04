use std::io::Read;

use super::*;
use crate::error::{ErrorKind, IpcError};

const START: &str = "https://huggingface.co/org/repo/resolve/0123abc/model.gguf";
const CDN: &str = "https://cdn-lfs.hf.co/signed/model.gguf?X-Sig=abc";

fn asset() -> Asset {
    Asset::new(
        Source::HUGGING_FACE,
        "/org/repo/resolve/0123abc/model.gguf",
        "model.gguf",
        5,
        [7; 32],
    )
    .unwrap()
    .with_revision("0123abc")
}

fn token() -> Secret {
    Secret::new("hf_secret_token")
}

fn body(response: Response) -> Vec<u8> {
    let mut out = Vec::new();
    response.body.take(1024).read_to_end(&mut out).unwrap();
    out
}

/// [`open`], never cancelled.
fn ask(
    web: &dyn Transport,
    asset: &Asset,
    from: Option<u64>,
    secret: Option<&Secret>,
) -> Result<Response, DownloadError> {
    open(web, asset, from, secret, &AtomicBool::new(false))
}

#[test]
fn asset_builds_its_url_from_the_source_only() {
    let a = asset();
    assert_eq!(a.url(), START);
    assert_eq!(a.revision(), Some("0123abc"));
    assert_eq!(
        (a.bytes(), a.name(), a.sha256()),
        (5, "model.gguf", &[7; 32])
    );
    let bad_path = |p: &str| Asset::new(Source::HUGGING_FACE, p, "m", 1, [0; 32]).unwrap_err();
    for p in [
        "org/x",
        "//evil.com/x",
        "@evil.com/x",
        "/a/../b",
        "/a?b",
        "/a#b",
        "/a b",
        "/a\\b",
    ] {
        assert_eq!(bad_path(p), InvalidAsset::Path, "{p}");
    }
    let bad_name = |n: &str| Asset::new(Source::HUGGING_FACE, "/x", n, 1, [0; 32]).unwrap_err();
    for n in ["", ".hidden.part", "a/b", "a\\b", "c:x"] {
        assert_eq!(bad_name(n), InvalidAsset::Name, "{n}");
    }
    assert!(Asset::new(Source::HUGGING_FACE, "/x", "João modelo.gguf", 1, [0; 32]).is_ok());
}

#[test]
fn direct_answer_carries_auth_to_the_first_host() {
    let web = ScriptedTransport::new().answer(START, Scripted::ok(b"hello"));
    let opened = ask(&web, &asset(), None, Some(&token())).unwrap();
    assert_eq!(opened.status, 200);
    assert_eq!(body(opened), b"hello");
    assert_eq!(
        web.asked(),
        [Asked {
            url: START.into(),
            from: None,
            authorized: true
        }]
    );
}

#[test]
fn redirect_to_an_allowed_cdn_host_is_followed_without_auth() {
    let web = ScriptedTransport::new()
        .answer(START, Scripted::redirect(302, CDN))
        .answer(CDN, Scripted::partial(2, 5, b"llo"));
    let opened = ask(&web, &asset(), Some(2), Some(&token())).unwrap();
    assert_eq!(opened.status, 206);
    assert_eq!(opened.content_range.as_deref(), Some("bytes 2-4/5"));
    let asked = web.asked();
    assert_eq!(asked.len(), 2);
    assert!(asked[0].authorized);
    assert!(
        !asked[1].authorized,
        "credential never sent to a redirect host"
    );
    assert!(asked.iter().all(|a| a.from == Some(2)));
}

#[test]
fn relative_redirect_on_the_first_host_keeps_auth() {
    let moved = "https://huggingface.co/new-org/repo/resolve/0123abc/model.gguf";
    let web = ScriptedTransport::new()
        .answer(
            START,
            Scripted::redirect(307, "/new-org/repo/resolve/0123abc/model.gguf"),
        )
        .answer(moved, Scripted::redirect(302, CDN))
        .answer(CDN, Scripted::ok(b"hello"));
    ask(&web, &asset(), None, Some(&token())).unwrap();
    let auth: Vec<bool> = web.asked().iter().map(|a| a.authorized).collect();
    assert_eq!(auth, [true, true, false]);
}

#[test]
fn subdomain_of_the_first_host_gets_no_auth() {
    let cdn = "https://cdn-lfs.huggingface.co/x";
    let web = ScriptedTransport::new()
        .answer(START, Scripted::redirect(302, cdn))
        .answer(cdn, Scripted::ok(b"hello"));
    ask(&web, &asset(), None, Some(&token())).unwrap();
    assert!(!web.asked()[1].authorized);
}

#[test]
fn hops_off_https_or_off_the_allowlist_are_refused_before_asking() {
    for location in [
        "http://cdn-lfs.hf.co/x",
        "https://evil.example/x",
        "https://huggingface.co.evil.example/x",
        "https://nothf.co/x",
        "https://127.0.0.1/x",
        "https://localhost/x",
        "https://user@hf.co/x",
        "ftp://hf.co/x",
    ] {
        let web = ScriptedTransport::new()
            .answer(START, Scripted::redirect(302, location))
            .answer(location, Scripted::ok(b"hello"));
        assert_eq!(
            ask(&web, &asset(), None, Some(&token())).unwrap_err(),
            DownloadError::HostNotAllowed,
            "{location}"
        );
        assert_eq!(web.asked().len(), 1, "{location}: never asked");
    }
}

#[test]
fn a_source_base_outside_its_allowlist_is_refused() {
    const ODD: Source = Source::new("odd", "https://example.com", &["hf.co"]);
    const PLAIN: Source = Source::new("plain", "http://hf.co", &["hf.co"]);
    for source in [ODD, PLAIN] {
        let a = Asset::new(source, "/x", "x", 1, [0; 32]).unwrap();
        let web = ScriptedTransport::new();
        assert_eq!(
            ask(&web, &a, None, None).unwrap_err(),
            DownloadError::HostNotAllowed
        );
        assert_eq!(web.asked(), []);
    }
}

#[test]
fn too_many_redirects_stop_with_the_last_status() {
    let hop = |n: usize| format!("https://cdn-lfs.hf.co/{n}");
    let mut web = ScriptedTransport::new().answer(START, Scripted::redirect(302, &hop(1)));
    for n in 1..=crate::remote::MAX_REDIRECTS + 1 {
        web = web.answer(&hop(n), Scripted::redirect(301, &hop(n + 1)));
    }
    assert_eq!(
        ask(&web, &asset(), None, None).unwrap_err(),
        DownloadError::Http { status: 301 }
    );
    assert_eq!(web.asked().len(), crate::remote::MAX_REDIRECTS + 1);
}

#[test]
fn other_statuses_and_no_answer_are_typed() {
    let web = ScriptedTransport::new().answer(START, Scripted::status(401));
    assert_eq!(
        ask(&web, &asset(), None, None).unwrap_err(),
        DownloadError::Http { status: 401 }
    );
    let web = ScriptedTransport::new().answer(START, Scripted::status(302));
    assert_eq!(
        ask(&web, &asset(), None, None).unwrap_err(),
        DownloadError::Http { status: 302 },
        "redirect without Location"
    );
    let web = ScriptedTransport::new();
    assert_eq!(
        ask(&web, &asset(), None, None).unwrap_err(),
        DownloadError::Offline
    );
}

#[test]
fn no_secret_means_no_auth_anywhere() {
    let web = ScriptedTransport::new().answer(START, Scripted::ok(b"hello"));
    ask(&web, &asset(), None, None).unwrap();
    assert!(!web.asked()[0].authorized);
}

#[test]
fn scripted_answers_repeat_the_last_and_cut_bodies_fail() {
    let web = ScriptedTransport::new()
        .answer(START, Scripted::ok(b"hello").cut_after(2))
        .answer(START, Scripted::partial(2, 5, b"llo"));
    let first = ask(&web, &asset(), None, None).unwrap();
    let mut out = Vec::new();
    let failed = first.body.take(16).read_to_end(&mut out);
    assert_eq!(out, b"he");
    assert_eq!(failed.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
    for _ in 0..2 {
        assert_eq!(ask(&web, &asset(), Some(2), None).unwrap().status, 206);
    }
}

#[test]
fn secrets_are_per_source_and_never_shown() {
    let store = MemorySecrets::default();
    let other = Source::new("other", "https://example.com", &["example.com"]);
    assert!(!store.is_set(&Source::HUGGING_FACE).unwrap());
    store.set(&Source::HUGGING_FACE, token()).unwrap();
    assert!(store.is_set(&Source::HUGGING_FACE).unwrap());
    assert!(!store.is_set(&other).unwrap());
    let got = store.get(&Source::HUGGING_FACE).unwrap().unwrap();
    assert_eq!(got.expose(), "hf_secret_token");
    assert!(!format!("{got:?}").contains("hf_secret"), "debug masked");
    store.clear(&Source::HUGGING_FACE).unwrap();
    store.clear(&Source::HUGGING_FACE).unwrap();
    assert!(store.get(&Source::HUGGING_FACE).unwrap().is_none());
}

#[test]
fn errors_serialize_as_tagged_state_and_map_to_existing_kinds() {
    assert_eq!(
        serde_json::to_string(&DownloadError::NoSpace { needed: 9, free: 3 }).unwrap(),
        r#"{"kind":"noSpace","needed":9,"free":3}"#
    );
    assert_eq!(
        serde_json::to_string(&DownloadError::HostNotAllowed).unwrap(),
        r#"{"kind":"hostNotAllowed"}"#
    );
    let kind = |e: DownloadError| IpcError::from(e).kind;
    assert_eq!(kind(DownloadError::Offline), ErrorKind::Unavailable);
    assert_eq!(
        kind(DownloadError::Http { status: 404 }),
        ErrorKind::Unavailable
    );
    assert_eq!(kind(DownloadError::HostNotAllowed), ErrorKind::Denied);
    assert_eq!(kind(DownloadError::Integrity), ErrorKind::InvalidData);
    assert_eq!(
        kind(DownloadError::NoSpace { needed: 2, free: 1 }),
        ErrorKind::Io
    );
    let disk = DownloadError::disk(&std::io::Error::other("full"));
    assert_eq!(kind(disk), ErrorKind::Io);
    assert_eq!(kind(DownloadError::Cancelled), ErrorKind::Superseded);
    assert_eq!(
        serde_json::to_string(&Progress {
            done: 1,
            total: 2,
            phase: Phase::Verifying
        })
        .unwrap(),
        r#"{"done":1,"total":2,"phase":"verifying"}"#
    );
}

// ---- engine: part + meta, resume, queue, lock, sweep, units ----

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::state::shared_write::lock_within;

const WAIT: Duration = Duration::from_secs(20);

fn sha(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn data(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| u8::try_from((i * 31 + usize::from(seed)) % 251).unwrap())
        .collect()
}

fn url_of(name: &str) -> String {
    format!("https://huggingface.co/org/repo/resolve/0123abc/{name}")
}

fn pin(name: &str, body: &[u8]) -> Asset {
    Asset::new(
        Source::HUGGING_FACE,
        format!("/org/repo/resolve/0123abc/{name}"),
        name,
        body.len() as u64,
        sha(body),
    )
    .unwrap()
    .with_revision("0123abc")
}

fn unit(id: &str, assets: Vec<Asset>) -> Unit {
    Unit::new(id, "models", assets).unwrap()
}

/// Store under a folder with a space + non-ASCII letter (AC-12).
fn store() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Jo\u{e3}o docs").join("store");
    (tmp, root)
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[allow(clippy::unnecessary_wraps)]
fn plenty(_: &Path) -> std::io::Result<u64> {
    Ok(u64::MAX / 2)
}

struct Run {
    downloads: Downloads,
    events: Receiver<(String, Update)>,
}

fn start(root: &Path, web: Arc<dyn Transport>) -> Run {
    start_with(root, web, None, plenty)
}

fn start_with(
    root: &Path,
    web: Arc<dyn Transport>,
    secrets: Option<Arc<dyn SecretStore>>,
    free: FreeSpace,
) -> Run {
    let seams = Seams {
        free_space: free,
        ..Seams::SYSTEM
    };
    start_seams(root, web, secrets, seams)
}

fn start_seams(
    root: &Path,
    web: Arc<dyn Transport>,
    secrets: Option<Arc<dyn SecretStore>>,
    seams: Seams,
) -> Run {
    let (tx, events) = mpsc::channel();
    let listener: Listener = Arc::new(move |id: &str, update: &Update| {
        let _ = tx.send((id.to_owned(), update.clone()));
    });
    Run {
        downloads: Downloads::with_seams(root.to_path_buf(), web, secrets, listener, seams),
        events,
    }
}

impl Run {
    /// Progress of `id` + its final update.
    fn end(&self, id: &str) -> (Vec<Progress>, Update) {
        let mut seen = Vec::new();
        loop {
            let (got, update) = self.events.recv_timeout(WAIT).expect("an update");
            assert_eq!(got, id);
            match update {
                Update::Progress(p) => seen.push(p),
                other => return (seen, other),
            }
        }
    }

    /// Waits for progress of `id` past `done` bytes.
    fn until_past(&self, id: &str, done: u64) {
        loop {
            let (got, update) = self.events.recv_timeout(WAIT).expect("progress");
            assert_eq!(got, id);
            match update {
                Update::Progress(p) if p.done > done => return,
                Update::Progress(_) => {}
                other => panic!("ended early: {other:?}"),
            }
        }
    }
}

/// Answers `200` with `body`, `step` bytes per read after `pause` (slow: a transfer to cancel; fast: many reads).
struct Trickle {
    body: Vec<u8>,
    step: usize,
    pause: Duration,
}

impl Trickle {
    /// One byte per 2 ms.
    fn slow(body: Vec<u8>) -> Self {
        Self {
            body,
            step: 1,
            pause: Duration::from_millis(2),
        }
    }
}

struct TrickleBody {
    bytes: Vec<u8>,
    at: usize,
    step: usize,
    pause: Duration,
}

impl Read for TrickleBody {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if !self.pause.is_zero() {
            std::thread::sleep(self.pause);
        }
        let n = self.step.min(buf.len()).min(self.bytes.len() - self.at);
        buf[..n].copy_from_slice(&self.bytes[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

impl Transport for Trickle {
    fn get(&self, _: &Request<'_>) -> Result<Response, DownloadError> {
        Ok(Response {
            status: 200,
            location: None,
            content_range: None,
            content_length: Some(self.body.len() as u64),
            body: Box::new(TrickleBody {
                bytes: self.body.clone(),
                at: 0,
                step: self.step,
                pause: self.pause,
            }),
        })
    }
}

#[test]
fn part_and_meta_names_are_not_scratch_names() {
    for name in [
        "model.gguf.part",
        "model.gguf.part.meta",
        "m.installed.json",
    ] {
        assert!(!crate::fsx::is_scratch_name(name), "{name}");
    }
}

#[test]
fn units_refuse_bad_ids_folders_and_file_names() {
    let a = || pin("model.gguf", b"x");
    assert!(Unit::new("m", "", vec![a()]).is_ok());
    assert!(Unit::new("m", "models/qwen", vec![a()]).is_ok());
    for id in ["", ".m", "a/b", "a\\b"] {
        assert_eq!(
            Unit::new(id, "", vec![a()]).unwrap_err(),
            InvalidAsset::Unit,
            "{id}"
        );
    }
    for dir in ["../x", "a/../b", "a\\b", "c:x", "a/.hidden"] {
        assert_eq!(
            Unit::new("m", dir, vec![a()]).unwrap_err(),
            InvalidAsset::Unit,
            "{dir}"
        );
    }
    assert_eq!(Unit::new("m", "", vec![]).unwrap_err(), InvalidAsset::Unit);
    let twins = vec![pin("A.bin", b"x"), pin("a.bin", b"y")];
    assert_eq!(Unit::new("m", "", twins).unwrap_err(), InvalidAsset::Unit);
    for name in ["x.part", "x.part.meta"] {
        assert_eq!(
            Unit::new("m", "", vec![pin(name, b"x")]).unwrap_err(),
            InvalidAsset::Unit,
            "{name}"
        );
    }
}

#[test]
fn a_unit_downloads_verifies_and_installs_under_a_non_ascii_path() {
    let (_tmp, root) = store();
    let body = data(300_000, 1);
    let web = Arc::new(ScriptedTransport::new().answer(&url_of("model.gguf"), Scripted::ok(&body)));
    let run = start(&root, web.clone());
    let u = unit("m", vec![pin("model.gguf", &body)]);
    run.downloads.enqueue(u.clone());
    let (progress, end) = run.end("m");
    assert_eq!(end, Update::Done);
    let dir = root.join("models");
    assert_eq!(fs::read(dir.join("model.gguf")).unwrap(), body);
    assert_eq!(names(&dir), ["m.installed.json", "model.gguf"]);
    assert!(run.downloads.is_installed(&u));
    let last = progress.last().unwrap();
    assert_eq!(
        (last.done, last.total, last.phase),
        (300_000, 300_000, Phase::Verifying)
    );
    assert!(progress.iter().any(|p| p.phase == Phase::Downloading));
    assert!(progress.windows(2).all(|w| w[0].done <= w[1].done));
    // installed: asked again -> done, nothing fetched
    run.downloads.enqueue(u);
    assert_eq!(run.end("m").1, Update::Done);
    assert_eq!(web.asked().len(), 1);
}

#[test]
fn the_sources_credential_goes_to_the_first_host_only() {
    let (_tmp, root) = store();
    let body = data(1_000, 2);
    let web = Arc::new(
        ScriptedTransport::new()
            .answer(&url_of("model.gguf"), Scripted::redirect(302, CDN))
            .answer(CDN, Scripted::ok(&body)),
    );
    let secrets = Arc::new(MemorySecrets::default());
    secrets.set(&Source::HUGGING_FACE, token()).unwrap();
    let run = start_with(&root, web.clone(), Some(secrets), plenty);
    run.downloads
        .enqueue(unit("m", vec![pin("model.gguf", &body)]));
    assert_eq!(run.end("m").1, Update::Done);
    let auth: Vec<bool> = web.asked().iter().map(|a| a.authorized).collect();
    assert_eq!(auth, [true, false]);
}

#[test]
fn a_dropped_transfer_resumes_by_range_after_a_restart_and_a_rehash() {
    let (_tmp, root) = store();
    let body = data(200_000, 3);
    let web = Arc::new(
        ScriptedTransport::new()
            .answer(&url_of("model.gguf"), Scripted::redirect(302, CDN))
            .answer(CDN, Scripted::ok(&body).cut_after(70_000))
            .answer(CDN, Scripted::partial(70_000, 200_000, &body[70_000..])),
    );
    let u = unit("m", vec![pin("model.gguf", &body)]);
    let dir = root.join("models");
    {
        let run = start(&root, web.clone());
        run.downloads.enqueue(u.clone());
        assert_eq!(run.end("m").1, Update::Failed(DownloadError::Offline));
        assert!(!run.downloads.is_installed(&u));
    }
    assert_eq!(names(&dir), ["model.gguf.part", "model.gguf.part.meta"]);
    assert_eq!(
        fs::metadata(dir.join("model.gguf.part")).unwrap().len(),
        70_000
    );
    let meta = fs::read_to_string(dir.join("model.gguf.part.meta")).unwrap();
    assert!(meta.contains(&url_of("model.gguf")), "{meta}");
    assert!(
        !meta.contains("X-Sig") && !meta.contains("cdn-lfs"),
        "{meta}"
    );
    // app restart: a new engine on the same store
    let run = start(&root, web.clone());
    run.downloads.enqueue(u);
    let (progress, end) = run.end("m");
    assert_eq!(end, Update::Done);
    let first = |phase| progress.iter().position(|p| p.phase == phase).unwrap();
    assert!(
        first(Phase::Verifying) < first(Phase::Downloading),
        "rehash first"
    );
    assert_eq!(fs::read(dir.join("model.gguf")).unwrap(), body);
    let froms: Vec<_> = web.asked().iter().map(|a| a.from).collect();
    assert_eq!(froms, [None, None, Some(70_000), Some(70_000)]);
}

/// Store + requests + end of a second run after a first run dropped after `cut` bytes; the second gets `then`.
struct Resumed {
    asked: Vec<Asked>,
    end: Update,
    dir: PathBuf,
    _tmp: tempfile::TempDir,
}

fn resume_with(body: &[u8], cut: usize, then: Vec<Scripted>) -> Resumed {
    let (tmp, root) = store();
    let mut web =
        ScriptedTransport::new().answer(&url_of("model.gguf"), Scripted::ok(body).cut_after(cut));
    for answer in then {
        web = web.answer(&url_of("model.gguf"), answer);
    }
    let web = Arc::new(web);
    let run = start(&root, web.clone());
    let u = unit("m", vec![pin("model.gguf", body)]);
    run.downloads.enqueue(u.clone());
    assert_eq!(run.end("m").1, Update::Failed(DownloadError::Offline));
    run.downloads.enqueue(u);
    let end = run.end("m").1;
    Resumed {
        asked: web.asked(),
        end,
        dir: root.join("models"),
        _tmp: tmp,
    }
}

#[test]
fn a_full_answer_to_a_range_request_restarts_from_zero() {
    let body = data(100_000, 4);
    let r = resume_with(&body, 50_000, vec![Scripted::ok(&body)]);
    assert_eq!(r.end, Update::Done);
    assert_eq!(fs::read(r.dir.join("model.gguf")).unwrap(), body);
    let froms: Vec<_> = r.asked.iter().map(|a| a.from).collect();
    assert_eq!(froms, [None, Some(50_000)]);
}

#[test]
fn another_range_start_restarts_without_range() {
    let body = data(100_000, 5);
    let r = resume_with(
        &body,
        50_000,
        vec![
            Scripted::partial(10_000, 100_000, &body[10_000..]),
            Scripted::ok(&body),
        ],
    );
    assert_eq!(r.end, Update::Done);
    assert_eq!(fs::read(r.dir.join("model.gguf")).unwrap(), body);
    let froms: Vec<_> = r.asked.iter().map(|a| a.from).collect();
    assert_eq!(froms, [None, Some(50_000), None]);
}

#[test]
fn another_total_in_the_range_is_an_integrity_failure_leaving_nothing() {
    let body = data(100_000, 6);
    let r = resume_with(
        &body,
        50_000,
        vec![Scripted::partial(50_000, 100_001, &body[50_000..])],
    );
    assert_eq!(r.end, Update::Failed(DownloadError::Integrity));
    assert_eq!(names(&r.dir), Vec::<String>::new());
}

#[test]
fn a_corrupted_part_fails_the_final_check_and_is_removed() {
    let body = data(100_000, 7);
    let (_tmp, root) = store();
    let web = Arc::new(
        ScriptedTransport::new()
            .answer(&url_of("model.gguf"), Scripted::ok(&body).cut_after(50_000))
            .answer(
                &url_of("model.gguf"),
                Scripted::partial(50_000, 100_000, &body[50_000..]),
            ),
    );
    let run = start(&root, web);
    let u = unit("m", vec![pin("model.gguf", &body)]);
    run.downloads.enqueue(u.clone());
    assert_eq!(run.end("m").1, Update::Failed(DownloadError::Offline));
    let part = root.join("models").join("model.gguf.part");
    let mut bytes = fs::read(&part).unwrap();
    bytes[10] ^= 0xff;
    fs::write(&part, bytes).unwrap();
    run.downloads.enqueue(u);
    assert_eq!(run.end("m").1, Update::Failed(DownloadError::Integrity));
    assert_eq!(names(&root.join("models")), Vec::<String>::new());
}

#[test]
fn bytes_beyond_the_pin_are_never_kept() {
    let body = data(10_000, 8);
    let other = data(10_000, 9);
    let longer = |b: &[u8]| [b, b"junk".as_slice()].concat();
    let unannounced = |b: &[u8]| {
        let mut answer = Scripted::ok(&longer(b));
        answer.content_length = None;
        answer
    };
    let cases = [
        // announced longer: refused before reading
        (
            Scripted::ok(&longer(&body)),
            Update::Failed(DownloadError::Integrity),
        ),
        // not announced: reading stops at the pinned size, the pin decides
        (unannounced(&body), Update::Done),
        (
            unannounced(&other),
            Update::Failed(DownloadError::Integrity),
        ),
    ];
    for (answer, want) in cases {
        let (_tmp, root) = store();
        let web = Arc::new(ScriptedTransport::new().answer(&url_of("model.gguf"), answer));
        let run = start(&root, web);
        run.downloads
            .enqueue(unit("m", vec![pin("model.gguf", &body)]));
        let done = want == Update::Done;
        assert_eq!(run.end("m").1, want);
        let dir = root.join("models");
        if done {
            assert_eq!(fs::read(dir.join("model.gguf")).unwrap(), body);
        } else {
            assert_eq!(names(&dir), Vec::<String>::new());
        }
    }
}

#[test]
fn a_sha256_mismatch_leaves_nothing() {
    let (_tmp, root) = store();
    let body = data(10_000, 10);
    let web = Arc::new(
        ScriptedTransport::new().answer(&url_of("model.gguf"), Scripted::ok(&data(10_000, 11))),
    );
    let run = start(&root, web);
    let u = unit("m", vec![pin("model.gguf", &body)]);
    run.downloads.enqueue(u.clone());
    assert_eq!(run.end("m").1, Update::Failed(DownloadError::Integrity));
    assert_eq!(names(&root.join("models")), Vec::<String>::new());
    assert!(!run.downloads.is_installed(&u));
}

#[test]
fn cancel_stops_promptly_keeps_the_part_and_delete_part_removes_it() {
    let (_tmp, root) = store();
    let body = data(5_000, 12);
    let run = start(&root, Arc::new(Trickle::slow(body.clone())));
    let u = unit("m", vec![pin("model.gguf", &body)]);
    let dir = root.join("models");
    run.downloads.enqueue(u.clone());
    run.until_past("m", 10);
    let asked = Instant::now();
    run.downloads.cancel("m");
    assert_eq!(run.end("m").1, Update::Failed(DownloadError::Cancelled));
    assert!(asked.elapsed() < Duration::from_secs(2), "prompt");
    assert_eq!(names(&dir), ["model.gguf.part", "model.gguf.part.meta"]);
    let kept = fs::metadata(dir.join("model.gguf.part")).unwrap().len();
    assert!(kept > 0 && kept < 5_000, "{kept}");
    run.downloads.delete_part(&u).unwrap();
    assert_eq!(names(&dir), Vec::<String>::new());
    // delete_part on the running unit: stops it, the worker removes the part
    run.downloads.enqueue(u.clone());
    run.until_past("m", 10);
    run.downloads.delete_part(&u).unwrap();
    assert_eq!(run.end("m").1, Update::Failed(DownloadError::Cancelled));
    assert_eq!(names(&dir), Vec::<String>::new());
}

#[test]
fn not_enough_space_is_refused_before_anything_is_asked_or_made() {
    #[allow(clippy::unnecessary_wraps)]
    fn tight(_: &Path) -> std::io::Result<u64> {
        Ok(1_000)
    }
    let (_tmp, root) = store();
    let web = Arc::new(ScriptedTransport::new());
    let run = start_with(&root, web.clone(), None, tight);
    run.downloads
        .enqueue(unit("m", vec![pin("model.gguf", &data(10_000, 13))]));
    assert_eq!(
        run.end("m").1,
        Update::Failed(DownloadError::NoSpace {
            needed: 10_000 + (64 << 20),
            free: 1_000
        })
    );
    assert_eq!(web.asked(), []);
    assert!(!root.exists());
}

#[test]
fn units_run_one_at_a_time_in_order_and_a_queued_one_can_be_cancelled() {
    let (_tmp, root) = store();
    let mut web = ScriptedTransport::new();
    let mut units = Vec::new();
    for id in ["a", "b", "c", "d"] {
        let name = format!("{id}.bin");
        let body = data(2_000, id.as_bytes()[0]);
        web = web.answer(&url_of(&name), Scripted::ok(&body));
        units.push(unit(id, vec![pin(&name, &body)]));
    }
    let web = Arc::new(web);
    let run = start(&root, web.clone());
    // the store lock held elsewhere: everything waits in the queue
    let held = lock_within(&root, Duration::from_secs(5)).unwrap();
    for u in &units {
        run.downloads.enqueue(u.clone());
    }
    run.downloads.cancel("c");
    assert_eq!(
        run.events.recv_timeout(WAIT).unwrap(),
        ("c".to_owned(), Update::Failed(DownloadError::Cancelled))
    );
    drop(held);
    let mut finished = Vec::new();
    while finished.len() < 3 {
        let (id, update) = run.events.recv_timeout(WAIT).unwrap();
        if update == Update::Done {
            finished.push(id);
        }
    }
    assert_eq!(finished, ["a", "b", "d"]);
    let urls: Vec<String> = web.asked().into_iter().map(|a| a.url).collect();
    assert_eq!(urls, [url_of("a.bin"), url_of("b.bin"), url_of("d.bin")]);
}

#[test]
fn a_store_locked_by_another_instance_waits_and_refuses_cleanup() {
    let (_tmp, root) = store();
    let body = data(2_000, 14);
    let web = Arc::new(ScriptedTransport::new().answer(&url_of("model.gguf"), Scripted::ok(&body)));
    let run = start(&root, web.clone());
    let u = unit("m", vec![pin("model.gguf", &body)]);
    let held = lock_within(&root, Duration::from_secs(5)).unwrap();
    assert_eq!(
        run.downloads.sweep(std::slice::from_ref(&u)),
        Err(DownloadError::Busy)
    );
    assert_eq!(run.downloads.delete_part(&u), Err(DownloadError::Busy));
    assert_eq!(run.downloads.remove(&u), Err(DownloadError::Busy));
    run.downloads.enqueue(u);
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(web.asked(), [], "waits for the other instance");
    drop(held);
    assert_eq!(run.end("m").1, Update::Done);
}

#[test]
fn sweep_removes_orphans_and_lists_valid_parts_as_paused_without_restarting_them() {
    let (_tmp, root) = store();
    let dir = root.join("models");
    let body = data(50_000, 15);
    let valid = unit("v", vec![pin("valid.bin", &body)]);
    {
        let web = Arc::new(
            ScriptedTransport::new()
                .answer(&url_of("valid.bin"), Scripted::ok(&body).cut_after(20_000)),
        );
        let run = start(&root, web);
        run.downloads.enqueue(valid.clone());
        assert_eq!(run.end("v").1, Update::Failed(DownloadError::Offline));
    }
    let meta_of = |a: &Asset| serde_json::to_vec(&part::PartMeta::of(a)).unwrap();
    let stale_pin = pin("stale.bin", &data(50_000, 16));
    let long_pin = pin("long.bin", &data(10, 17));
    let stale = unit("s", vec![stale_pin.clone(), long_pin.clone()]);
    fs::write(dir.join("lonely.bin.part"), b"x").unwrap();
    fs::write(dir.join("ghost.bin.part.meta"), meta_of(&stale_pin)).unwrap();
    // meta of another pin
    fs::write(dir.join("stale.bin.part"), b"x").unwrap();
    fs::write(
        dir.join("stale.bin.part.meta"),
        meta_of(&pin("stale.bin", b"old")),
    )
    .unwrap();
    // part longer than its pin
    fs::write(dir.join("long.bin.part"), [0; 11]).unwrap();
    fs::write(dir.join("long.bin.part.meta"), meta_of(&long_pin)).unwrap();
    // no unit names it
    fs::write(dir.join("unknown.bin.part"), b"x").unwrap();
    fs::write(dir.join("unknown.bin.part.meta"), meta_of(&stale_pin)).unwrap();
    // a write_atomic temp: not ours
    fs::write(dir.join(".valid.bin.123.4.part"), b"tmp").unwrap();
    let web = Arc::new(ScriptedTransport::new());
    let run = start(&root, web.clone());
    let paused = run.downloads.sweep(&[valid, stale]).unwrap();
    assert_eq!(
        paused,
        [Paused {
            unit: "v".into(),
            name: "valid.bin".into(),
            done: 20_000,
            total: 50_000
        }]
    );
    assert_eq!(
        names(&dir),
        [
            ".valid.bin.123.4.part",
            "valid.bin.part",
            "valid.bin.part.meta"
        ]
    );
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(web.asked(), [], "never restarted by itself");
    assert!(run.events.try_recv().is_err());
}

#[test]
fn a_multi_file_unit_is_installed_only_after_every_file_verifies() {
    let (_tmp, root) = store();
    let dir = root.join("models");
    let model = data(30_000, 18);
    let vocab = data(3_000, 19);
    let u = unit(
        "qwen",
        vec![pin("model.gguf", &model), pin("tokenizer.json", &vocab)],
    );
    {
        let web = Arc::new(
            ScriptedTransport::new()
                .answer(&url_of("model.gguf"), Scripted::ok(&model))
                .answer(&url_of("tokenizer.json"), Scripted::status(404)),
        );
        let run = start(&root, web);
        run.downloads.enqueue(u.clone());
        assert_eq!(
            run.end("qwen").1,
            Update::Failed(DownloadError::Http { status: 404 })
        );
        assert!(!run.downloads.is_installed(&u));
    }
    assert_eq!(
        names(&dir),
        ["model.gguf"],
        "verified file kept, no manifest"
    );
    // the verified file is checked again, not fetched again
    let web =
        Arc::new(ScriptedTransport::new().answer(&url_of("tokenizer.json"), Scripted::ok(&vocab)));
    let run = start(&root, web.clone());
    run.downloads.enqueue(u.clone());
    let (progress, end) = run.end("qwen");
    assert_eq!(end, Update::Done);
    assert_eq!(progress.last().unwrap().total, 33_000);
    assert_eq!(
        names(&dir),
        ["model.gguf", "qwen.installed.json", "tokenizer.json"]
    );
    assert!(run.downloads.is_installed(&u));
    let urls: Vec<String> = web.asked().into_iter().map(|a| a.url).collect();
    assert_eq!(urls, [url_of("tokenizer.json")]);
    // a file of another size: not installed
    fs::write(dir.join("tokenizer.json"), b"x").unwrap();
    assert!(!run.downloads.is_installed(&u));
    run.downloads.remove(&u).unwrap();
    assert_eq!(names(&dir), Vec::<String>::new());
    assert!(!run.downloads.is_installed(&u));
}

#[test]
fn busy_maps_to_unavailable() {
    assert_eq!(
        IpcError::from(DownloadError::Busy).kind,
        ErrorKind::Unavailable
    );
}

// ---- review fixes (U5) ----

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

#[test]
fn a_fresh_engine_starts_no_thread_makes_no_folder_and_asks_nothing() {
    let (_tmp, root) = store();
    let web = Arc::new(ScriptedTransport::new());
    let run = start(&root, web.clone());
    let u = unit("m", vec![pin("model.gguf", b"hello")]);
    assert!(!run.downloads.is_installed(&u));
    run.downloads.cancel("m");
    assert_eq!(run.downloads.sweep(std::slice::from_ref(&u)), Ok(vec![]));
    run.downloads.delete_part(&u).unwrap();
    run.downloads.remove(&u).unwrap();
    assert!(!run.downloads.started(), "worker starts on first enqueue");
    assert!(!root.exists());
    assert_eq!(web.asked(), []);
    run.downloads.enqueue(u);
    assert!(run.downloads.started());
    assert_eq!(run.end("m").1, Update::Failed(DownloadError::Offline));
}

#[test]
fn cleanup_of_another_unit_is_never_busy_while_this_engine_works() {
    let (_tmp, root) = store();
    let mut web = ScriptedTransport::new();
    let mut units = Vec::new();
    for n in 0..40 {
        let name = format!("f{n}.bin");
        let body = data(64, u8::try_from(n).unwrap());
        web = web.answer(&url_of(&name), Scripted::ok(&body));
        units.push(unit(&format!("u{n}"), vec![pin(&name, &body)]));
    }
    let other = unit("other", vec![pin("other.bin", b"x")]);
    let run = start(&root, Arc::new(web));
    let stop = AtomicBool::new(false);
    let busy = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            while !stop.load(Ordering::Relaxed) {
                for result in [
                    run.downloads.remove(&other),
                    run.downloads.delete_part(&other),
                ] {
                    if result == Err(DownloadError::Busy) {
                        busy.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        });
        for u in &units {
            run.downloads.enqueue(u.clone());
        }
        let mut done = 0;
        while done < units.len() {
            if let (_, Update::Done) = run.events.recv_timeout(WAIT).unwrap() {
                done += 1;
            }
        }
        stop.store(true, Ordering::Relaxed);
    });
    assert_eq!(
        busy.load(Ordering::Relaxed),
        0,
        "own worker never answers Busy"
    );
}

#[test]
fn dropping_the_engine_mid_download_joins_the_worker_and_keeps_the_part() {
    let (_tmp, root) = store();
    let body = data(5_000, 20);
    let run = start(&root, Arc::new(Trickle::slow(body.clone())));
    run.downloads
        .enqueue(unit("m", vec![pin("model.gguf", &body)]));
    run.until_past("m", 10);
    let Run { downloads, events } = run;
    let asked = Instant::now();
    drop(downloads);
    assert!(asked.elapsed() < Duration::from_secs(2), "prompt");
    // joined: the last update was already told
    let mut last = None;
    while let Ok((_, update)) = events.try_recv() {
        last = Some(update);
    }
    assert_eq!(last, Some(Update::Failed(DownloadError::Cancelled)));
    let dir = root.join("models");
    assert_eq!(names(&dir), ["model.gguf.part", "model.gguf.part.meta"]);
}

#[test]
fn a_listener_dropping_the_last_handle_on_the_worker_neither_deadlocks_nor_panics() {
    let (_tmp, root) = store();
    let body = data(1_000, 21);
    let web: Arc<dyn Transport> =
        Arc::new(ScriptedTransport::new().answer(&url_of("model.gguf"), Scripted::ok(&body)));
    let slot: Arc<Mutex<Option<Downloads>>> = Arc::new(Mutex::new(None));
    let (tx, rx) = mpsc::channel();
    let held = Arc::clone(&slot);
    let listener: Listener = Arc::new(move |_: &str, update: &Update| {
        if *update == Update::Done {
            let last = held.lock().unwrap().take();
            drop(last);
            let _ = tx.send(std::thread::current().name().map(str::to_owned));
        }
    });
    let downloads = Downloads::new(root.clone(), web, None, listener);
    *slot.lock().unwrap() = Some(downloads);
    if let Some(downloads) = slot.lock().unwrap().as_ref() {
        downloads.enqueue(unit("m", vec![pin("model.gguf", &body)]));
    }
    let thread = rx.recv_timeout(WAIT).expect("listener returned");
    assert_eq!(thread.as_deref(), Some("downloads"));
    assert!(slot.lock().unwrap().is_none());
}

#[test]
fn progress_of_a_fast_transfer_is_throttled() {
    let (_tmp, root) = store();
    let body = data(1 << 20, 22);
    let fast = Trickle {
        body: body.clone(),
        step: 1024,
        pause: Duration::ZERO,
    };
    let run = start(&root, Arc::new(fast));
    run.downloads
        .enqueue(unit("m", vec![pin("model.gguf", &body)]));
    let (progress, end) = run.end("m");
    assert_eq!(end, Update::Done);
    // 1024 reads; forced: downloading start + verifying end
    assert!(progress.len() < 40, "{} updates", progress.len());
}

/// Counts credential reads.
#[derive(Default)]
struct CountingSecrets {
    inner: MemorySecrets,
    gets: AtomicUsize,
}

impl SecretStore for CountingSecrets {
    fn get(&self, source: &Source) -> Result<Option<Secret>, SecretError> {
        self.gets.fetch_add(1, Ordering::Relaxed);
        self.inner.get(source)
    }
    fn set(&self, source: &Source, secret: Secret) -> Result<(), SecretError> {
        self.inner.set(source, secret)
    }
    fn clear(&self, source: &Source) -> Result<(), SecretError> {
        self.inner.clear(source)
    }
}

#[test]
fn a_source_without_credentials_never_reads_the_credential_store() {
    const PUBLIC: Source = Source::new("public", "https://huggingface.co", &["huggingface.co"]);
    let (_tmp, root) = store();
    let body = data(100, 23);
    let web = Arc::new(ScriptedTransport::new().answer(&url_of("model.gguf"), Scripted::ok(&body)));
    let secrets = Arc::new(CountingSecrets::default());
    secrets.set(&PUBLIC, token()).unwrap();
    let run = start_with(&root, web.clone(), Some(secrets.clone()), plenty);
    let asset = Asset::new(
        PUBLIC,
        "/org/repo/resolve/0123abc/model.gguf",
        "model.gguf",
        100,
        sha(&body),
    )
    .unwrap();
    run.downloads
        .enqueue(Unit::new("m", "models", vec![asset]).unwrap());
    assert_eq!(run.end("m").1, Update::Done);
    assert_eq!(secrets.gets.load(Ordering::Relaxed), 0);
    assert!(!web.asked()[0].authorized);
}

/// Answers the first hop only after the test let it go (a cancel arriving between hops).
struct Gate {
    inner: ScriptedTransport,
    asked: mpsc::SyncSender<()>,
    go: Mutex<Receiver<()>>,
}

impl Transport for Gate {
    fn get(&self, request: &Request<'_>) -> Result<Response, DownloadError> {
        let answer = self.inner.get(request);
        if request.url == url_of("model.gguf") {
            let _ = self.asked.send(());
            let _ = self.go.lock().unwrap().recv_timeout(WAIT);
        }
        answer
    }
}

#[test]
fn a_cancel_between_redirect_hops_asks_no_further() {
    let (_tmp, root) = store();
    let body = data(100, 24);
    let (asked, on_asked) = mpsc::sync_channel(1);
    let (let_go, go) = mpsc::channel();
    let web = Arc::new(Gate {
        inner: ScriptedTransport::new()
            .answer(&url_of("model.gguf"), Scripted::redirect(302, CDN))
            .answer(CDN, Scripted::ok(&body)),
        asked,
        go: Mutex::new(go),
    });
    let run = start(&root, web.clone());
    run.downloads
        .enqueue(unit("m", vec![pin("model.gguf", &body)]));
    on_asked.recv_timeout(WAIT).unwrap();
    run.downloads.cancel("m");
    let_go.send(()).unwrap();
    assert_eq!(run.end("m").1, Update::Failed(DownloadError::Cancelled));
    assert_eq!(web.inner.asked().len(), 1, "CDN never asked");
}

#[test]
fn range_not_satisfiable_on_resume_restarts_from_zero_once() {
    let body = data(100_000, 25);
    let r = resume_with(
        &body,
        50_000,
        vec![Scripted::status(416), Scripted::ok(&body)],
    );
    assert_eq!(r.end, Update::Done);
    assert_eq!(fs::read(r.dir.join("model.gguf")).unwrap(), body);
    let froms: Vec<_> = r.asked.iter().map(|a| a.from).collect();
    assert_eq!(froms, [None, Some(50_000), None]);
}

#[test]
fn a_206_without_a_usable_range_is_typed_and_keeps_the_part() {
    let body = data(100_000, 26);
    for range in ["bytes */100000", "bytes abc", "pages 1-2/3"] {
        let mut odd = Scripted::partial(50_000, 100_000, &body[50_000..]);
        odd.content_range = Some(range.to_owned());
        let r = resume_with(&body, 50_000, vec![odd]);
        assert_eq!(
            r.end,
            Update::Failed(DownloadError::Http { status: 206 }),
            "{range}"
        );
        assert_eq!(
            names(&r.dir),
            ["model.gguf.part", "model.gguf.part.meta"],
            "{range}"
        );
        assert_eq!(
            fs::metadata(r.dir.join("model.gguf.part")).unwrap().len(),
            50_000
        );
    }
}

#[test]
fn the_credential_goes_only_to_the_first_hosts_https_port() {
    for (hop, auth) in [
        ("https://huggingface.co:8443/org/x", false),
        ("https://huggingface.co:443/org/x", true),
        ("https://HuggingFace.co./org/x", true),
    ] {
        let web = ScriptedTransport::new()
            .answer(START, Scripted::redirect(302, hop))
            .answer(hop, Scripted::ok(b"hello"));
        ask(&web, &asset(), None, Some(&token())).unwrap();
        assert_eq!(web.asked()[1].authorized, auth, "{hop}");
    }
}

#[test]
fn hosts_with_backslashes_or_escapes_are_refused_before_asking() {
    for location in [
        "https://evil.com\\.hf.co/x",
        "https://evil%2Ecom.hf.co/x",
        "https://cdn-lfs.hf.co%2F.evil.com/x",
        "https://cdn-lfs.hf.co:x/x",
    ] {
        let web = ScriptedTransport::new()
            .answer(START, Scripted::redirect(302, location))
            .answer(location, Scripted::ok(b"hello"));
        assert_eq!(
            ask(&web, &asset(), None, Some(&token())).unwrap_err(),
            DownloadError::HostNotAllowed,
            "{location}"
        );
        assert_eq!(web.asked().len(), 1, "{location}: never asked");
    }
}

#[test]
fn reserved_and_unsafe_windows_names_are_refused() {
    let bad_name = |n: &str| Asset::new(Source::HUGGING_FACE, "/x", n, 1, [0; 32]).unwrap_err();
    for n in [
        "CON",
        "con.txt",
        "Nul.tar.gz",
        "aux",
        "PRN.x",
        "COM1",
        "lpt9.bin",
        "conin$",
        "a.",
        "a ",
        "a<b",
        "a>b",
        "a\"b",
        "a|b",
        "a?b",
        "a*b",
    ] {
        assert_eq!(bad_name(n), InvalidAsset::Name, "{n}");
    }
    for ok in ["console.txt", "com10", "lpt", "a.b", "nul_x"] {
        assert!(
            Asset::new(Source::HUGGING_FACE, "/x", ok, 1, [0; 32]).is_ok(),
            "{ok}"
        );
    }
    let a = || pin("model.gguf", b"x");
    for id in ["CON", "a.", "a?"] {
        assert_eq!(
            Unit::new(id, "", vec![a()]).unwrap_err(),
            InvalidAsset::Unit,
            "{id}"
        );
    }
    assert_eq!(
        Unit::new("m", "nul/x", vec![a()]).unwrap_err(),
        InvalidAsset::Unit
    );
    for name in ["m.installed.json", "M.Installed.JSON"] {
        assert_eq!(
            Unit::new("m", "", vec![pin(name, b"x")]).unwrap_err(),
            InvalidAsset::Unit,
            "{name}"
        );
    }
}

#[test]
fn sweep_skips_a_folder_named_like_a_part() {
    let (_tmp, root) = store();
    let dir = root.join("models");
    fs::create_dir_all(dir.join("x.bin.part")).unwrap();
    fs::create_dir_all(dir.join("y.bin.part.meta")).unwrap();
    let run = start(&root, Arc::new(ScriptedTransport::new()));
    let u = unit("m", vec![pin("x.bin", b"x"), pin("y.bin", b"y")]);
    assert_eq!(run.downloads.sweep(&[u]), Ok(vec![]));
    assert_eq!(names(&dir), ["x.bin.part", "y.bin.part.meta"]);
}

#[test]
fn secret_values_with_line_breaks_or_nul_are_refused() {
    let store = MemorySecrets::default();
    for bad in ["a\r\nX-Evil: 1", "a\nb", "a\rb", "a\0b"] {
        assert!(
            store.set(&Source::HUGGING_FACE, Secret::new(bad)).is_err(),
            "{bad:?}"
        );
    }
    assert!(store.get(&Source::HUGGING_FACE).unwrap().is_none());
}

/// Folder | file link at `link` -> `target`; `false`: links cannot be made here (Windows without the privilege).
fn link(target: &Path, link: &Path, dir: bool) -> bool {
    #[cfg(unix)]
    {
        let _ = dir;
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        if dir {
            std::os::windows::fs::symlink_dir(target, link).is_ok()
        } else {
            std::os::windows::fs::symlink_file(target, link).is_ok()
        }
    }
}

#[test]
fn a_linked_unit_folder_is_refused() {
    let (_tmp, root) = store();
    let elsewhere = root.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    if !link(&elsewhere, &root.join("models"), true) {
        eprintln!("skipped: no symlinks here");
        return;
    }
    let body = data(100, 27);
    let web = Arc::new(ScriptedTransport::new().answer(&url_of("model.gguf"), Scripted::ok(&body)));
    let run = start(&root, web.clone());
    let u = unit("m", vec![pin("model.gguf", &body)]);
    run.downloads.enqueue(u.clone());
    assert!(matches!(
        run.end("m").1,
        Update::Failed(DownloadError::Disk { .. })
    ));
    assert_eq!(web.asked(), []);
    assert_eq!(names(&elsewhere), Vec::<String>::new());
    assert!(run.downloads.sweep(std::slice::from_ref(&u)).is_err());
    assert!(run.downloads.remove(&u).is_err());
}

#[test]
fn a_linked_part_with_a_valid_meta_is_discarded_not_resumed() {
    let (_tmp, root) = store();
    let dir = root.join("models");
    fs::create_dir_all(&dir).unwrap();
    let body = data(1_000, 28);
    let asset = pin("model.gguf", &body);
    let outside = root.join("outside.bin");
    fs::write(&outside, &body[..500]).unwrap();
    if !link(&outside, &dir.join("model.gguf.part"), false) {
        eprintln!("skipped: no symlinks here");
        return;
    }
    fs::write(
        dir.join("model.gguf.part.meta"),
        serde_json::to_vec(&part::PartMeta::of(&asset)).unwrap(),
    )
    .unwrap();
    let run = start(&root, Arc::new(ScriptedTransport::new()));
    assert_eq!(run.downloads.sweep(&[unit("m", vec![asset])]), Ok(vec![]));
    assert_eq!(names(&dir), Vec::<String>::new());
    assert_eq!(
        fs::read(&outside).unwrap(),
        &body[..500],
        "target untouched"
    );
}

#[test]
fn without_a_store_lock_a_download_is_refused_before_anything_is_asked_or_made() {
    #[allow(clippy::unnecessary_wraps)]
    fn no_lock(_: &Path, _: Duration) -> std::io::Result<Option<FileLock>> {
        Ok(None)
    }
    let (_tmp, root) = store();
    let web = Arc::new(ScriptedTransport::new().answer(&url_of("model.gguf"), Scripted::ok(b"x")));
    let seams = Seams {
        free_space: plenty,
        lock: no_lock,
    };
    let run = start_seams(&root, web.clone(), None, seams);
    run.downloads
        .enqueue(unit("m", vec![pin("model.gguf", b"x")]));
    assert_eq!(
        run.end("m").1,
        Update::Failed(DownloadError::Disk {
            detail: "store lock unavailable".into()
        })
    );
    assert_eq!(web.asked(), []);
    assert!(!root.exists());
}

#[test]
fn only_sources_marked_so_accept_a_credential() {
    assert!(Source::HUGGING_FACE.accepts_credential());
    let plain = Source::new("x", "https://huggingface.co", &["huggingface.co"]);
    assert!(!plain.accepts_credential());
    assert!(plain.with_credential().accepts_credential());
    // `open` never sends one for a source without
    let a = Asset::new(plain, "/x", "x", 1, [0; 32]).unwrap();
    let web = ScriptedTransport::new().answer("https://huggingface.co/x", Scripted::ok(b"x"));
    ask(&web, &a, None, Some(&token())).unwrap();
    assert!(!web.asked()[0].authorized);
}

#[test]
fn adopt_records_files_put_in_place_and_refuses_missing_or_resized_ones() {
    let (_tmp, root) = store();
    let body = data(1000, 3);
    let run = start(&root, Arc::new(ScriptedTransport::new()));
    let u = unit("m", vec![pin("model.gguf", &body)]);
    let dir = root.join("models");
    assert_eq!(run.downloads.adopt(&u), Err(DownloadError::Integrity));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("model.gguf"), &body[..999]).unwrap();
    assert_eq!(run.downloads.adopt(&u), Err(DownloadError::Integrity));
    assert!(!run.downloads.is_installed(&u));
    fs::write(dir.join("model.gguf"), &body).unwrap();
    run.downloads.adopt(&u).unwrap();
    assert!(run.downloads.is_installed(&u));
    assert_eq!(names(&dir), ["m.installed.json", "model.gguf"]);
}
