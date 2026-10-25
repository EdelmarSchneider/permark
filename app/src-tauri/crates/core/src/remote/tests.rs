use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use super::*;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
const GIF: &[u8] = b"GIF89a\x01\x00\x01\x00";

/// A web of canned answers; records every address asked. An address in `local` stands for a name that resolves to
/// an address that is not public (the app's resolver refuses it before connecting).
#[derive(Default)]
struct Web {
    answers: HashMap<String, Answer>,
    local: HashSet<String>,
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

    fn resolves_locally(mut self, url: &str) -> Self {
        self.local.insert(url.to_owned());
        self
    }
}

impl Http for Web {
    fn get(&self, url: &str, limit: u64) -> Result<Answer, NoAnswer> {
        self.asked.borrow_mut().push(url.to_owned());
        if self.local.contains(url) {
            return Err(NoAnswer::LocalAddress);
        }
        let mut answer = self.answers.get(url).cloned().ok_or(NoAnswer::Offline)?;
        let cap = usize::try_from(limit + 1).unwrap();
        answer.body.truncate(cap);
        Ok(answer)
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

fn v4(text: &str) -> IpAddr {
    IpAddr::V4(text.parse::<Ipv4Addr>().unwrap())
}

fn v6(text: &str) -> IpAddr {
    IpAddr::V6(text.parse::<Ipv6Addr>().unwrap())
}

#[test]
fn only_addresses_on_the_open_internet_are_public() {
    for ip in [
        "0.0.0.0",
        "0.1.2.3",
        "10.0.0.1",
        "10.255.255.255",
        "100.64.0.1",
        "100.127.255.255",
        "127.0.0.1",
        "127.255.255.254",
        "169.254.0.1",
        "169.254.169.254",
        "172.16.0.1",
        "172.31.255.255",
        "192.0.0.1",
        "192.0.2.1",
        "192.168.0.1",
        "192.168.255.255",
        "198.18.0.1",
        "198.19.255.255",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "239.255.255.250",
        "240.0.0.1",
        "255.255.255.255",
    ] {
        assert!(!is_public(v4(ip)), "{ip} is not public");
    }
    for ip in [
        "::",
        "::1",
        "fc00::1",
        "fd12:3456:789a::1",
        "fe80::1",
        "febf::1",
        "fec0::1",
        "ff02::1",
        "100::1",
        "2001:db8::1",
        "64:ff9b:1::1",
        // an IPv6 form that carries a local IPv4 address
        "::ffff:127.0.0.1",
        "::ffff:7f00:1",
        "::ffff:169.254.169.254",
        "::ffff:10.0.0.1",
        "::ffff:0:192.168.1.1",
        "::127.0.0.1",
        "::10.0.0.1",
        "64:ff9b::127.0.0.1",
        "64:ff9b::a9fe:a9fe",
        "2002:7f00:1::1",
        "2002:c0a8:101::1",
        "2002:a9fe:a9fe::",
    ] {
        assert!(!is_public(v6(ip)), "{ip} is not public");
    }
    for ip in [
        "1.1.1.1",
        "8.8.8.8",
        "93.184.215.14",
        "100.63.255.255",
        "100.128.0.1",
        "126.255.255.255",
        "128.0.0.1",
        "169.253.255.255",
        "172.15.255.255",
        "172.32.0.1",
        "192.0.1.1",
        "192.167.255.255",
        "198.17.255.255",
        "198.20.0.1",
        "223.255.255.254",
    ] {
        assert!(is_public(v4(ip)), "{ip} is public");
    }
    for ip in [
        "2606:4700:4700::1111",
        "2a00:1450:4001::200e",
        "2001:4860:4860::8888",
        "::ffff:8.8.8.8",
        "64:ff9b::808:808",
        "2002:808:808::1",
    ] {
        assert!(is_public(v6(ip)), "{ip} is public");
    }
}

#[test]
fn a_local_or_private_host_is_refused_before_anything_is_asked() {
    let web = Web::default();
    for (url, host) in [
        ("https://localhost/a.png", "localhost"),
        ("https://LOCALHOST:8443/a.png", "LOCALHOST"),
        ("https://localhost./a.png", "localhost."),
        ("https://router.localhost/a.png", "router.localhost"),
        ("https://127.0.0.1/a.png", "127.0.0.1"),
        ("https://127.0.0.1.:443/a.png", "127.0.0.1."),
        ("https://10.1.2.3/a.png", "10.1.2.3"),
        ("https://172.20.0.5/a.png", "172.20.0.5"),
        ("https://192.168.1.1/a.png", "192.168.1.1"),
        (
            "https://169.254.169.254/latest/meta-data/",
            "169.254.169.254",
        ),
        ("https://100.100.100.200/a.png", "100.100.100.200"),
        ("https://0.0.0.0/a.png", "0.0.0.0"),
        ("https://[::1]/a.png", "[::1]"),
        ("https://[::1]:8443/a.png", "[::1]"),
        ("https://[fd00::1]/a.png", "[fd00::1]"),
        ("https://[fe80::1]/a.png", "[fe80::1]"),
        ("https://[::ffff:127.0.0.1]/a.png", "[::ffff:127.0.0.1]"),
        ("https://[::ffff:a9fe:a9fe]/a.png", "[::ffff:a9fe:a9fe]"),
        ("https://[::]/a.png", "[::]"),
        ("https://192.168.1.1?x", "192.168.1.1"),
        ("https://127.0.0.1#x", "127.0.0.1"),
    ] {
        let error = fetch_picture(&web, url).unwrap_err();
        assert_eq!(
            (
                error.kind,
                error.detail.as_deref(),
                error.subject.as_deref()
            ),
            (ErrorKind::Denied, Some("address"), Some(host)),
            "{url}"
        );
    }
    // numbers that a resolver would read as an address, but are not a plain dotted IPv4 one
    for url in [
        "https://127.1/a.png",
        "https://2130706433/a.png",
        "https://0x7f000001/a.png",
        "https://0x7f.0.0.1/a.png",
        "https://0177.0.0.1/a.png",
        "https://127.000.000.001/a.png",
        "https://1.2.3.4.5/a.png",
        "https://8.8.8.256/a.png",
        "https://example.0x10/a.png",
        "https://[fe80::1%25eth0]/a.png",
        "https://[127.0.0.1]/a.png",
        "https://[::1/a.png",
    ] {
        assert_eq!(
            kind(fetch_picture(&web, url)),
            (ErrorKind::InvalidData, Some("address".to_owned())),
            "{url}"
        );
    }
    assert!(
        web.asked.borrow().is_empty(),
        "nothing was asked of the network"
    );
}

#[test]
fn a_public_host_is_still_asked() {
    for url in [
        "https://93.184.215.14/a.png",
        "https://[2606:4700:4700::1111]/a.png",
        "https://localhost.example.com/a.png",
        "https://my-localhost/a.png",
        "https://127.0.0.1.example.com/a.png",
        "https://10.example/a.png",
    ] {
        let web = Web::default().page(url, 200, None, Some("image/png"), PNG);
        assert_eq!(fetch_picture(&web, url).unwrap().bytes, PNG, "{url}");
    }
}

#[test]
fn a_redirect_to_a_local_address_is_refused_and_never_asked() {
    for target in [
        "https://127.0.0.1/a.png",
        "https://localhost/a.png",
        "https://169.254.169.254/latest/meta-data/",
        "https://[::ffff:127.0.0.1]/a.png",
        "//192.168.1.1/a.png",
        "https://127.1/a.png",
    ] {
        let web = Web::default().page("https://a.example/p", 302, Some(target), None, b"");
        let error = fetch_picture(&web, "https://a.example/p").unwrap_err();
        assert!(
            matches!(error.kind, ErrorKind::Denied | ErrorKind::InvalidData),
            "{target}: {error:?}"
        );
        assert_eq!(error.detail.as_deref(), Some("address"), "{target}");
        assert_eq!(*web.asked.borrow(), ["https://a.example/p"], "{target}");
    }
}

#[test]
fn a_name_that_resolves_to_a_local_address_is_refused() {
    let web = Web::default().resolves_locally("https://rebind.example/a.png");
    let error = fetch_picture(&web, "https://rebind.example/a.png").unwrap_err();
    assert_eq!(
        (
            error.kind,
            error.detail.as_deref(),
            error.subject.as_deref()
        ),
        (ErrorKind::Denied, Some("address"), Some("rebind.example"))
    );
    // on a redirect hop too, with the host of that hop
    let web = Web::default()
        .page(
            "https://a.example/p",
            302,
            Some("https://inside.example:8443/a.png"),
            None,
            b"",
        )
        .resolves_locally("https://inside.example:8443/a.png");
    let error = fetch_picture(&web, "https://a.example/p").unwrap_err();
    assert_eq!(
        (
            error.kind,
            error.detail.as_deref(),
            error.subject.as_deref()
        ),
        (ErrorKind::Denied, Some("address"), Some("inside.example"))
    );
    assert_eq!(web.asked.borrow().len(), 2);
}
