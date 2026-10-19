use super::*;
use std::sync::atomic::AtomicU32;

static NEXT: AtomicU32 = AtomicU32::new(0);

/// Short private folder (Unix path limit) + a unique endpoint name per test.
fn endpoint() -> (tempfile::TempDir, Endpoint) {
    let dir = tempfile::Builder::new()
        .prefix("pm")
        .tempdir_in(std::env::temp_dir())
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let id = format!("t{}-{n}", std::process::id());
    let endpoint = Endpoint::in_folder(dir.path(), &id).unwrap();
    (dir, endpoint)
}

#[derive(Debug, Serialize, serde::Deserialize, PartialEq)]
struct Ping {
    text: String,
}

fn echo_server(endpoint: &Endpoint) -> Server {
    Server::spawn(endpoint.listen().unwrap(), |ping: Ping| Ping {
        text: format!("{}!", ping.text),
    })
    .unwrap()
}

#[test]
fn request_round_trip_unicode() {
    let (_d, endpoint) = endpoint();
    let _server = echo_server(&endpoint);
    let answer: Ping = request(
        &endpoint,
        &Ping {
            text: "olá 文档 C:\\a b".to_owned(),
        },
    )
    .unwrap();
    assert_eq!(answer.text, "olá 文档 C:\\a b!");
}

#[test]
fn nothing_listening_is_unreachable() {
    let (_d, endpoint) = endpoint();
    assert!(matches!(
        request::<Ping, Ping>(
            &endpoint,
            &Ping {
                text: "x".to_owned()
            }
        ),
        Err(IpcLocalError::Unreachable { .. })
    ));
}

#[test]
fn second_listener_is_refused_and_endpoint_reusable_after_drop() {
    let (_d, endpoint) = endpoint();
    let server = echo_server(&endpoint);
    assert!(
        endpoint.listen().is_err(),
        "second listener must not steal the endpoint"
    );
    drop(server);
    let _again = echo_server(&endpoint);
    let answer: Ping = request(
        &endpoint,
        &Ping {
            text: "b".to_owned(),
        },
    )
    .unwrap();
    assert_eq!(answer.text, "b!");
}

#[test]
fn many_concurrent_clients_all_answered() {
    let (_d, endpoint) = endpoint();
    let _server = echo_server(&endpoint);
    std::thread::scope(|s| {
        for i in 0..16 {
            let endpoint = &endpoint;
            s.spawn(move || {
                let answer: Ping = request(
                    endpoint,
                    &Ping {
                        text: format!("{i}"),
                    },
                )
                .unwrap();
                assert_eq!(answer.text, format!("{i}!"));
            });
        }
    });
}

#[test]
fn malformed_and_oversized_requests_get_no_answer() {
    let (_d, endpoint) = endpoint();
    let _server = echo_server(&endpoint);
    for payload in [
        b"not json\n".to_vec(),
        vec![b'a'; usize::try_from(MAX_LINE).unwrap() + 10],
    ] {
        let mut stream = endpoint.connect().unwrap();
        let _ = stream.write_all(&payload);
        let _ = stream.write_all(b"\n");
        let mut reader = BufReader::new(stream);
        assert!(read_line::<Ping>(&mut reader).is_err());
    }
    // still serving afterwards
    let answer: Ping = request(
        &endpoint,
        &Ping {
            text: "ok".to_owned(),
        },
    )
    .unwrap();
    assert_eq!(answer.text, "ok!");
}

#[test]
fn read_line_limits() {
    let mut ok = BufReader::new(&b"{\"text\":\"a\"}\nrest"[..]);
    assert_eq!(read_line::<Ping>(&mut ok).unwrap().text, "a");
    let mut cut = BufReader::new(&b"{\"text\":\"a\"}"[..]);
    assert!(matches!(
        read_line::<Ping>(&mut cut),
        Err(IpcLocalError::BadMessage(_))
    ));
    let long = vec![b'x'; usize::try_from(MAX_LINE).unwrap() + 5];
    assert!(matches!(
        read_line::<Ping>(&mut BufReader::new(&long[..])),
        Err(IpcLocalError::BadMessage(_))
    ));
}

#[test]
fn app_endpoint_differs_per_profile() {
    let a = Endpoint::app(&AppDirs::under(Path::new("/x/a"))).unwrap();
    let b = Endpoint::app(&AppDirs::under(Path::new("/x/b"))).unwrap();
    assert_ne!(a, b);
    assert_eq!(
        a,
        Endpoint::app(&AppDirs::under(Path::new("/x/a"))).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn private_folder_rule() {
    assert!(is_private_folder(true, 1000, 0o40700, 1000));
    assert!(!is_private_folder(true, 1000, 0o40750, 1000));
    assert!(!is_private_folder(true, 0, 0o40700, 1000));
    assert!(!is_private_folder(false, 1000, 0o700, 1000));
}

#[cfg(unix)]
#[test]
fn a_shared_folder_is_untrusted() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, endpoint) = endpoint();
    let _server = echo_server(&endpoint);
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(matches!(
        endpoint.connect(),
        Err(IpcLocalError::Untrusted(_))
    ));
}
