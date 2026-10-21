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
    // Unix: a child process another test starts at this moment holds a copy of the lock + socket descriptors from
    // its fork until its exec closes them, so the endpoint frees up a moment later, not at once
    let started = std::time::Instant::now();
    let listening = loop {
        match endpoint.listen() {
            Err(IpcLocalError::InUse(_)) if started.elapsed() < Duration::from_secs(10) => {
                std::thread::sleep(Duration::from_millis(20));
            }
            other => break other.unwrap(),
        }
    };
    let _again = Server::spawn(listening, |ping: Ping| Ping {
        text: format!("{}!", ping.text),
    })
    .unwrap();
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

#[test]
fn a_slow_first_line_is_dropped_and_the_server_keeps_serving() {
    let (_d, endpoint) = endpoint();
    let _server = echo_server(&endpoint);
    // slowloris: a few bytes, then nothing; another sends nothing at all
    let mut slow = endpoint.connect().unwrap();
    slow.write_all(b"{\"te").unwrap();
    let _silent = endpoint.connect().unwrap();
    let started = Instant::now();
    let mut reader = BufReader::new(slow);
    let mut rest = Vec::new();
    // the server gives up on it: the connection closes without an answer
    let read = reader.read_to_end(&mut rest);
    assert!(read.is_err() || rest.is_empty());
    let waited = started.elapsed();
    assert!(
        waited + Duration::from_millis(200) >= FIRST_LINE_DEADLINE,
        "{waited:?}"
    );
    assert!(waited < FIRST_LINE_DEADLINE * 3, "{waited:?}");
    let answer: Ping = request(&endpoint, &Ping { text: "x".into() }).unwrap();
    assert_eq!(answer.text, "x!");
}

#[test]
fn a_line_sent_byte_by_byte_within_the_deadline_is_answered() {
    let (_d, endpoint) = endpoint();
    let _server = echo_server(&endpoint);
    let mut stream = endpoint.connect().unwrap();
    for byte in b"{\"text\":\"slow\"}\n" {
        stream.write_all(&[*byte]).unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut reader = BufReader::new(stream);
    let answer: Ping = read_line(&mut reader).unwrap();
    assert_eq!(answer.text, "slow!");
}

#[test]
fn a_flood_of_silent_connections_is_capped_and_answers_resume() {
    let (_d, endpoint) = endpoint();
    let server = echo_server(&endpoint);
    let mut held = Vec::new();
    for _ in 0..(MAX_CONNECTIONS + 8) {
        if let Ok(stream) = endpoint.connect() {
            held.push(stream);
        }
    }
    std::thread::sleep(Duration::from_millis(300));
    assert!(server.open_connections() <= MAX_CONNECTIONS);
    drop(held);
    // the silent ones time out; afterwards everybody is served again
    std::thread::sleep(FIRST_LINE_DEADLINE + Duration::from_millis(500));
    let answer: Ping = request(&endpoint, &Ping { text: "y".into() }).unwrap();
    assert_eq!(answer.text, "y!");
}

struct Flag(Arc<AtomicBool>);

impl Drop for Flag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

fn session_server(endpoint: &Endpoint, released: &Arc<AtomicBool>) -> Server {
    let released = Arc::clone(released);
    Server::spawn_with(endpoint.listen().unwrap(), move |ping: Ping| {
        let answer = Ping {
            text: format!("{}!", ping.text),
        };
        if ping.text == "hello" {
            Reply::Hold(answer, Box::new(Flag(Arc::clone(&released))))
        } else {
            Reply::Answer(answer)
        }
    })
    .unwrap()
}

#[test]
fn a_held_session_ends_when_the_peer_leaves() {
    let (_d, endpoint) = endpoint();
    let released = Arc::new(AtomicBool::new(false));
    let _server = session_server(&endpoint, &released);
    let (answer, stream): (Ping, _) = open_session(
        &endpoint,
        &Ping {
            text: "hello".into(),
        },
    )
    .unwrap();
    assert_eq!(answer.text, "hello!");
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        !released.load(Ordering::Acquire),
        "held while the peer is there"
    );
    // calls still work beside the session
    let other: Ping = request(&endpoint, &Ping { text: "a".into() }).unwrap();
    assert_eq!(other.text, "a!");
    drop(stream);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !released.load(Ordering::Acquire) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(released.load(Ordering::Acquire));
}

#[test]
fn stopping_the_server_ends_held_sessions() {
    let (_d, endpoint) = endpoint();
    let released = Arc::new(AtomicBool::new(false));
    let server = session_server(&endpoint, &released);
    let (_answer, _stream): (Ping, _) = open_session(
        &endpoint,
        &Ping {
            text: "hello".into(),
        },
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    drop(server);
    assert!(
        released.load(Ordering::Acquire),
        "guard dropped by the time Drop returns"
    );
}

#[test]
fn sessions_over_the_cap_are_answered_then_closed() {
    let (_d, endpoint) = endpoint();
    let released = Arc::new(AtomicBool::new(false));
    let server = session_server(&endpoint, &released);
    let mut sessions = Vec::new();
    for _ in 0..(MAX_SESSIONS + 2) {
        let (answer, stream): (Ping, _) = open_session(
            &endpoint,
            &Ping {
                text: "hello".into(),
            },
        )
        .unwrap();
        assert_eq!(answer.text, "hello!");
        sessions.push(stream);
    }
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(server.open_connections(), MAX_SESSIONS);
}

#[test]
fn project_endpoints_differ_per_project_and_profile() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let ra = crate::paths::Root::open(a.path()).unwrap();
    let rb = crate::paths::Root::open(b.path()).unwrap();
    let dirs = AppDirs::under(a.path());
    let other = AppDirs::under(b.path());
    let ea = Endpoint::project(&dirs.profile, &ra).unwrap();
    assert_eq!(ea, Endpoint::project(&dirs.profile, &ra).unwrap());
    assert_ne!(ea, Endpoint::project(&dirs.profile, &rb).unwrap());
    assert_ne!(ea, Endpoint::project(&other.profile, &ra).unwrap());
    assert_ne!(ea, Endpoint::app(&dirs).unwrap());
}
