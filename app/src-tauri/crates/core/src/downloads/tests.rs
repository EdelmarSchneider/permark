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

fn body(opened: Opened) -> Vec<u8> {
    let mut out = Vec::new();
    opened
        .response
        .body
        .take(1024)
        .read_to_end(&mut out)
        .unwrap();
    out
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
    let opened = open(&web, &asset(), None, Some(&token())).unwrap();
    assert_eq!(opened.url, START);
    assert_eq!(opened.response.status, 200);
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
    let opened = open(&web, &asset(), Some(2), Some(&token())).unwrap();
    // signed address handed to the caller only
    assert_eq!(opened.url, CDN);
    assert_eq!(opened.response.status, 206);
    assert_eq!(
        opened.response.content_range.as_deref(),
        Some("bytes 2-4/5")
    );
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
    open(&web, &asset(), None, Some(&token())).unwrap();
    let auth: Vec<bool> = web.asked().iter().map(|a| a.authorized).collect();
    assert_eq!(auth, [true, true, false]);
}

#[test]
fn subdomain_of_the_first_host_gets_no_auth() {
    let cdn = "https://cdn-lfs.huggingface.co/x";
    let web = ScriptedTransport::new()
        .answer(START, Scripted::redirect(302, cdn))
        .answer(cdn, Scripted::ok(b"hello"));
    open(&web, &asset(), None, Some(&token())).unwrap();
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
            open(&web, &asset(), None, Some(&token())).unwrap_err(),
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
            open(&web, &a, None, None).unwrap_err(),
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
        open(&web, &asset(), None, None).unwrap_err(),
        DownloadError::Http { status: 301 }
    );
    assert_eq!(web.asked().len(), crate::remote::MAX_REDIRECTS + 1);
}

#[test]
fn other_statuses_and_no_answer_are_typed() {
    let web = ScriptedTransport::new().answer(START, Scripted::status(401));
    assert_eq!(
        open(&web, &asset(), None, None).unwrap_err(),
        DownloadError::Http { status: 401 }
    );
    let web = ScriptedTransport::new().answer(START, Scripted::status(302));
    assert_eq!(
        open(&web, &asset(), None, None).unwrap_err(),
        DownloadError::Http { status: 302 },
        "redirect without Location"
    );
    let web = ScriptedTransport::new();
    assert_eq!(
        open(&web, &asset(), None, None).unwrap_err(),
        DownloadError::Offline
    );
}

#[test]
fn no_secret_means_no_auth_anywhere() {
    let web = ScriptedTransport::new().answer(START, Scripted::ok(b"hello"));
    open(&web, &asset(), None, None).unwrap();
    assert!(!web.asked()[0].authorized);
}

#[test]
fn scripted_answers_repeat_the_last_and_cut_bodies_fail() {
    let web = ScriptedTransport::new()
        .answer(START, Scripted::ok(b"hello").cut_after(2))
        .answer(START, Scripted::partial(2, 5, b"llo"));
    let first = open(&web, &asset(), None, None).unwrap();
    let mut out = Vec::new();
    let failed = first.response.body.take(16).read_to_end(&mut out);
    assert_eq!(out, b"he");
    assert_eq!(failed.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
    for _ in 0..2 {
        assert_eq!(
            open(&web, &asset(), Some(2), None).unwrap().response.status,
            206
        );
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
    let (tx, events) = mpsc::channel();
    let listener: Listener = Arc::new(move |id: &str, update: &Update| {
        let _ = tx.send((id.to_owned(), update.clone()));
    });
    Run {
        downloads: Downloads::spawn(root.to_path_buf(), web, secrets, listener, free).unwrap(),
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

/// Answers `200` with a body trickling out one byte per 2 ms (a slow transfer to cancel).
struct Trickle(Vec<u8>);

struct TrickleBody {
    bytes: Vec<u8>,
    at: usize,
}

impl Read for TrickleBody {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        std::thread::sleep(Duration::from_millis(2));
        if self.at >= self.bytes.len() || buf.is_empty() {
            return Ok(0);
        }
        buf[0] = self.bytes[self.at];
        self.at += 1;
        Ok(1)
    }
}

impl Transport for Trickle {
    fn get(&self, _: &Request<'_>) -> Result<Response, DownloadError> {
        Ok(Response {
            status: 200,
            location: None,
            content_range: None,
            content_length: Some(self.0.len() as u64),
            body: Box::new(TrickleBody {
                bytes: self.0.clone(),
                at: 0,
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
    let run = start(&root, Arc::new(Trickle(body.clone())));
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
