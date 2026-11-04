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
