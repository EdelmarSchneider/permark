use std::cell::RefCell;
use std::collections::HashMap;

use super::*;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
const GIF: &[u8] = b"GIF89a\x01\x00\x01\x00";

/// A web of canned answers; records every address asked.
#[derive(Default)]
struct Web {
    answers: HashMap<String, Answer>,
    asked: RefCell<Vec<String>>,
}

impl Web {
    fn page(
        mut self,
        url: &str,
        status: u16,
        location: Option<&str>,
        mime: Option<&str>,
        body: &[u8],
    ) -> Self {
        self.answers.insert(
            url.to_owned(),
            Answer {
                status,
                location: location.map(str::to_owned),
                content_type: mime.map(str::to_owned),
                body: body.to_vec(),
            },
        );
        self
    }
}

impl Http for Web {
    fn get(&self, url: &str, limit: u64) -> Option<Answer> {
        self.asked.borrow_mut().push(url.to_owned());
        let mut answer = self.answers.get(url).cloned()?;
        let cap = usize::try_from(limit + 1).unwrap();
        answer.body.truncate(cap);
        Some(answer)
    }
}

fn kind(result: Result<Picture, IpcError>) -> (ErrorKind, Option<String>) {
    let error = result.unwrap_err();
    (error.kind, error.detail)
}

#[test]
fn an_https_picture_of_a_known_type_is_fetched() {
    let web = Web::default().page(
        "https://example.com/a.png",
        200,
        None,
        Some("image/png"),
        PNG,
    );
    let picture = fetch_picture(&web, "https://example.com/a.png").unwrap();
    assert_eq!(picture.kind, PictureType::Png);
    assert_eq!(picture.bytes, PNG);
    assert_eq!(picture.kind.mime(), "image/png");
    // a parameter on the type is fine
    let web = Web::default().page(
        "https://example.com/b",
        200,
        None,
        Some("image/gif; charset=binary"),
        GIF,
    );
    assert_eq!(
        fetch_picture(&web, "https://example.com/b").unwrap().kind,
        PictureType::Gif
    );
}

#[test]
fn only_https_addresses_are_asked() {
    let web = Web::default();
    for url in [
        "http://example.com/a.png",
        "ftp://example.com/a.png",
        "file:///C:/a.png",
        "data:image/png;base64,AAAA",
        "blob:https://example.com/x",
        "javascript:alert(1)",
        "//example.com/a.png",
        "example.com/a.png",
    ] {
        assert_eq!(
            kind(fetch_picture(&web, url)).0,
            ErrorKind::Unsupported,
            "{url}"
        );
    }
    for url in [
        "https://",
        "https:///a.png",
        "https://user:pw@example.com/a.png",
        "https://example.com/a b.png",
        "https://:443/a.png",
    ] {
        assert_eq!(
            kind(fetch_picture(&web, url)).0,
            ErrorKind::InvalidData,
            "{url}"
        );
    }
    let long = format!("https://example.com/{}", "a".repeat(MAX_URL_CHARS));
    assert_eq!(kind(fetch_picture(&web, &long)).0, ErrorKind::InvalidData);
    assert!(
        web.asked.borrow().is_empty(),
        "nothing was asked of the network"
    );
}

#[test]
fn redirects_are_followed_only_to_https_and_only_three_times() {
    let web = Web::default()
        .page(
            "https://a.example/p",
            302,
            Some("https://b.example/p"),
            None,
            b"",
        )
        .page("https://b.example/p", 301, Some("/q.png"), None, b"")
        .page(
            "https://b.example/q.png",
            307,
            Some("//c.example/r.png"),
            None,
            b"",
        )
        .page("https://c.example/r.png", 200, None, Some("image/png"), PNG);
    assert_eq!(
        fetch_picture(&web, "https://a.example/p").unwrap().bytes,
        PNG
    );
    assert_eq!(web.asked.borrow().len(), 4);
    // to http: refused, the http address never asked
    let web = Web::default().page(
        "https://a.example/p",
        302,
        Some("http://a.example/p.png"),
        None,
        b"",
    );
    assert_eq!(
        kind(fetch_picture(&web, "https://a.example/p")),
        (ErrorKind::Unsupported, Some("https".to_owned()))
    );
    assert_eq!(*web.asked.borrow(), ["https://a.example/p"]);
    // a loop or a long chain ends
    let web = Web::default().page(
        "https://a.example/p",
        302,
        Some("https://a.example/p"),
        None,
        b"",
    );
    assert_eq!(
        kind(fetch_picture(&web, "https://a.example/p")),
        (ErrorKind::Unsupported, Some("redirects".to_owned()))
    );
    assert_eq!(web.asked.borrow().len(), MAX_REDIRECTS + 1);
    // a redirect without a place to go
    let web = Web::default().page("https://a.example/p", 302, None, None, b"");
    assert_eq!(
        kind(fetch_picture(&web, "https://a.example/p")).0,
        ErrorKind::Unavailable
    );
}

#[test]
fn what_is_not_a_picture_is_refused() {
    let cases: [(Option<&str>, &[u8]); 7] = [
        (Some("text/html"), b"<html><script>"),
        (Some("image/svg+xml"), b"<svg onload=alert(1)>"),
        (None, PNG),
        (Some("application/octet-stream"), PNG),
        // the type says png, the bytes say otherwise
        (Some("image/png"), b"<html>"),
        (Some("image/png"), GIF),
        (Some("image/webp"), b"RIFF\0\0\0\0WAVE"),
    ];
    for (mime, body) in cases {
        let web = Web::default().page("https://e.example/x", 200, None, mime, body);
        assert_eq!(
            kind(fetch_picture(&web, "https://e.example/x")).0,
            ErrorKind::InvalidData,
            "{mime:?}"
        );
    }
}

#[test]
fn a_huge_picture_stops_at_the_cap() {
    let mut big = PNG.to_vec();
    big.resize(usize::try_from(MAX_PICTURE_BYTES).unwrap() + 10, 0);
    let web = Web::default().page(
        "https://e.example/big.png",
        200,
        None,
        Some("image/png"),
        &big,
    );
    assert_eq!(
        kind(fetch_picture(&web, "https://e.example/big.png")).0,
        ErrorKind::TooLarge
    );
    // exactly at the cap is fine
    big.truncate(usize::try_from(MAX_PICTURE_BYTES).unwrap());
    let web = Web::default().page(
        "https://e.example/ok.png",
        200,
        None,
        Some("image/png"),
        &big,
    );
    assert!(fetch_picture(&web, "https://e.example/ok.png").is_ok());
}

#[test]
fn no_answer_or_another_status_is_unavailable() {
    let web = Web::default().page("https://e.example/404", 404, None, Some("image/png"), PNG);
    assert_eq!(
        kind(fetch_picture(&web, "https://e.example/404")).0,
        ErrorKind::Unavailable
    );
    assert_eq!(
        kind(fetch_picture(&Web::default(), "https://e.example/none")),
        (ErrorKind::Unavailable, Some("offline".to_owned()))
    );
}

#[test]
fn every_kept_type_knows_its_signature() {
    let samples: [(PictureType, &[u8]); 6] = [
        (PictureType::Png, PNG),
        (PictureType::Jpeg, &[0xFF, 0xD8, 0xFF, 0xE0]),
        (PictureType::Gif, GIF),
        (PictureType::Webp, b"RIFF\x10\0\0\0WEBPVP8 "),
        (PictureType::Avif, b"\0\0\0\x1cftypavif\0\0"),
        (PictureType::Bmp, b"BM\x36\0"),
    ];
    for (kind, bytes) in samples {
        assert!(kind.signed(bytes), "{kind:?}");
        assert_eq!(PictureType::of_mime(kind.mime()), Some(kind));
        assert!(!kind.signed(b""), "{kind:?} empty");
    }
}
