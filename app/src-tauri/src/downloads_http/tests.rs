//! Real `ureq` stack, no network: a fake proxy on this computer (the proxy's own address is allowed) answers through
//! a CONNECT tunnel; a target address on this computer without a proxy is refused before any connection.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use permark_core::downloads::{DownloadError, Request, Secret, SecretStore, Source, Transport};

use super::{agent, unavailable, Keychain, Patience, UreqTransport};

/// Reads up to the blank line ending a request head.
fn head(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut byte = [0_u8; 1];
    while !bytes.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => bytes.push(byte[0]),
            _ => break,
        }
    }
    String::from_utf8_lossy(&bytes).to_ascii_lowercase()
}

/// A proxy for one request: opens the tunnel, hands over the request head (lowercase), writes `answer`, holds the
/// connection for `hold`.
fn proxy(answer: &'static [u8], hold: Duration) -> (u16, Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (seen, asked) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        head(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
            .unwrap();
        seen.send(head(&mut stream)).unwrap();
        stream.write_all(answer).unwrap();
        stream.flush().unwrap();
        std::thread::sleep(hold);
    });
    (port, asked)
}

fn transport(proxy_port: Option<u16>, idle: Duration) -> UreqTransport {
    let proxy =
        proxy_port.map(|port| ureq::Proxy::new(&format!("http://127.0.0.1:{port}")).unwrap());
    UreqTransport {
        agent: OnceLock::from(agent(proxy, Patience::Idle(idle))),
    }
}

const URL: &str = "http://files.example/model.bin";

#[test]
fn a_partial_answer_is_parsed_and_its_body_streamed() {
    let (port, asked) = proxy(
        b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 5-9/10\r\nContent-Length: 5\r\n\r\n56789",
        Duration::ZERO,
    );
    let secret = Secret::new("tok-123");
    let mut response = transport(Some(port), Duration::from_secs(5))
        .get(&Request {
            url: URL,
            from: Some(5),
            auth: Some(&secret),
        })
        .unwrap();
    assert_eq!(response.status, 206);
    assert_eq!(response.content_range.as_deref(), Some("bytes 5-9/10"));
    assert_eq!(response.content_length, Some(5));
    assert_eq!(response.location, None);
    let mut body = Vec::new();
    response.body.read_to_end(&mut body).unwrap();
    assert_eq!(body, b"56789");
    let request = asked.recv().unwrap();
    assert!(request.contains("\r\nrange: bytes=5-\r\n"), "{request}");
    assert!(
        request.contains("\r\nauthorization: bearer tok-123\r\n"),
        "{request}"
    );
}

#[test]
fn a_redirect_comes_back_unfollowed_and_nothing_extra_is_sent() {
    let (port, asked) = proxy(
        b"HTTP/1.1 302 Found\r\nLocation: https://cdn.example/x?sig=1\r\nContent-Length: 0\r\n\r\n",
        Duration::ZERO,
    );
    let response = transport(Some(port), Duration::from_secs(5))
        .get(&Request {
            url: URL,
            from: None,
            auth: None,
        })
        .unwrap();
    assert_eq!(response.status, 302);
    assert_eq!(
        response.location.as_deref(),
        Some("https://cdn.example/x?sig=1")
    );
    let request = asked.recv().unwrap();
    assert!(!request.contains("authorization"), "{request}");
    assert!(!request.contains("range"), "{request}");
}

#[test]
fn an_error_status_passes_through() {
    let (port, _asked) = proxy(
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n",
        Duration::ZERO,
    );
    let response = transport(Some(port), Duration::from_secs(5))
        .get(&Request {
            url: URL,
            from: None,
            auth: None,
        })
        .unwrap();
    assert_eq!(response.status, 404);
}

#[test]
fn a_stalled_body_ends_within_the_idle_time() {
    let (port, _asked) = proxy(
        b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc",
        Duration::from_secs(10),
    );
    let mut response = transport(Some(port), Duration::from_millis(300))
        .get(&Request {
            url: URL,
            from: None,
            auth: None,
        })
        .unwrap();
    let started = Instant::now();
    let mut body = Vec::new();
    assert!(response.body.read_to_end(&mut body).is_err());
    assert_eq!(body, b"abc");
    assert!(started.elapsed() < Duration::from_secs(5), "no total wait");
}

#[test]
fn no_answer_is_offline() {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let error = transport(Some(port), Duration::from_secs(5))
        .get(&Request {
            url: URL,
            from: None,
            auth: None,
        })
        .unwrap_err();
    assert_eq!(error, DownloadError::Offline);
}

#[test]
fn a_local_address_is_never_connected_to() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let error = transport(None, Duration::from_secs(5))
        .get(&Request {
            url: &format!("https://localhost:{port}/model.bin"),
            from: None,
            auth: None,
        })
        .unwrap_err();
    assert_eq!(error, DownloadError::HostNotAllowed);
    assert!(matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
}

#[test]
fn keychain_errors_never_carry_the_value() {
    let error = unavailable(&keyring::Error::BadEncoding(b"tok-123".to_vec()));
    assert!(!error.to_string().contains("tok-123"));
    assert!(!format!("{error:?}").contains("tok-123"));
}

#[test]
fn the_keychain_refuses_a_value_with_line_breaks_before_touching_the_store() {
    const SOURCE: Source = Source::new("permark-test", "https://example.org", &["example.org"]);
    for bad in ["tok\r\nX-Evil: 1", "tok\n", "tok\0"] {
        assert_eq!(
            Keychain.set(&SOURCE, Secret::new(bad)),
            Err(permark_core::downloads::SecretError::InvalidValue)
        );
    }
}

/// Real OS credential store, run by hand: `cargo test -p permark downloads_http -- --ignored`. CI machines have no
/// unlocked keychain | Secret Service session.
#[test]
#[ignore = "touches the real OS credential store"]
fn the_keychain_keeps_and_forgets_a_credential() {
    const SOURCE: Source = Source::new("permark-test", "https://example.org", &["example.org"]);
    let keychain = Keychain;
    keychain.clear(&SOURCE).unwrap();
    assert!(!keychain.is_set(&SOURCE).unwrap());
    keychain.set(&SOURCE, Secret::new("tok-123")).unwrap();
    assert_eq!(keychain.get(&SOURCE).unwrap(), Some(Secret::new("tok-123")));
    keychain.clear(&SOURCE).unwrap();
    keychain.clear(&SOURCE).unwrap();
    assert_eq!(keychain.get(&SOURCE).unwrap(), None);
}
