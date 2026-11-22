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
fn answers_may_outgrow_a_request_line_up_to_their_own_limit() {
    let (_d, endpoint) = endpoint();
    let long = usize::try_from(MAX_LINE).unwrap() * 4;
    let _server = Server::spawn(endpoint.listen().unwrap(), move |ping: Ping| Ping {
        text: if ping.text == "over" {
            "x".repeat(usize::try_from(MAX_ANSWER).unwrap())
        } else {
            "x".repeat(long)
        },
    })
    .unwrap();
    let answer: Ping = request(
        &endpoint,
        &Ping {
            text: "long".to_owned(),
        },
    )
    .unwrap();
    assert_eq!(answer.text.len(), long);
    assert!(matches!(
        request::<Ping, Ping>(
            &endpoint,
            &Ping {
                text: "over".to_owned()
            }
        ),
        Err(IpcLocalError::BadMessage(_))
    ));
    // the server still reads requests with the short limit
    let mut stream = endpoint.connect().unwrap();
    let _ = write_line(
        &mut stream,
        &Ping {
            text: "y".repeat(long),
        },
    );
    assert!(read_line::<Ping>(&mut BufReader::new(stream)).is_err());
}

#[test]
fn a_caller_may_wait_longer_than_the_default_answer_time() {
    let (_d, endpoint) = endpoint();
    let _server = Server::spawn(endpoint.listen().unwrap(), |ping: Ping| {
        std::thread::sleep(Duration::from_millis(300));
        ping
    })
    .unwrap();
    let ping = Ping {
        text: "slow".to_owned(),
    };
    assert!(matches!(
        request_within::<Ping, Ping>(&endpoint, &ping, Duration::from_millis(50)),
        Err(IpcLocalError::NoAnswer(_))
    ));
    let answer: Ping = request_within(&endpoint, &ping, Duration::from_secs(5)).unwrap();
    assert_eq!(answer, ping);
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

/// A socket folder that cannot be closed to others (its mode change refused) is never listened in.
#[cfg(unix)]
#[test]
fn a_socket_folder_that_refuses_its_mode_change_is_not_used() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, endpoint) = endpoint();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let listening = crate::fsx::tests::with_modes_refused(|| endpoint.listen());
    assert!(matches!(listening, Err(IpcLocalError::Folder { .. })));
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
    session_server_ending(
        endpoint,
        released,
        &Arc::new(AtomicBool::new(false)),
        &Arc::new(AtomicUsize::new(0)),
    )
}

/// Holds every "hello"; setting `end` lets every held connection go; `made` counts the guards made (one per
/// connection actually held).
fn session_server_ending(
    endpoint: &Endpoint,
    released: &Arc<AtomicBool>,
    end: &Arc<AtomicBool>,
    made: &Arc<AtomicUsize>,
) -> Server {
    let released = Arc::clone(released);
    let end = Arc::clone(end);
    let made = Arc::clone(made);
    Server::spawn_with(endpoint.listen().unwrap(), move |ping: Ping| {
        let answer = Ping {
            text: format!("{}!", ping.text),
        };
        if ping.text == "hello" {
            let released = Arc::clone(&released);
            let made = Arc::clone(&made);
            Reply::Hold {
                answer,
                full: Ping {
                    text: "full".into(),
                },
                held: Box::new(move || {
                    made.fetch_add(1, Ordering::AcqRel);
                    Box::new(Flag(released))
                }),
                end: Arc::clone(&end),
            }
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
fn the_end_flag_of_a_held_session_closes_it_from_the_server_side() {
    let (_d, endpoint) = endpoint();
    let released = Arc::new(AtomicBool::new(false));
    let end = Arc::new(AtomicBool::new(false));
    let server = session_server_ending(&endpoint, &released, &end, &Arc::new(AtomicUsize::new(0)));
    let (_answer, stream): (Ping, _) = open_session(
        &endpoint,
        &Ping {
            text: "hello".into(),
        },
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(!released.load(Ordering::Acquire));
    assert!(!peer_gone(&stream));
    end.store(true, Ordering::Release);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !(released.load(Ordering::Acquire) && peer_gone(&stream)) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(released.load(Ordering::Acquire), "guard dropped");
    assert!(peer_gone(&stream), "the client sees its connection closed");
    // the server keeps answering others
    let other: Ping = request(&endpoint, &Ping { text: "a".into() }).unwrap();
    assert_eq!(other.text, "a!");
    drop(server);
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
fn sessions_over_the_cap_get_the_full_answer_then_are_closed() {
    assert_eq!(MAX_SESSIONS, 16);
    let (_d, endpoint) = endpoint();
    let released = Arc::new(AtomicBool::new(false));
    let made = Arc::new(AtomicUsize::new(0));
    let server = session_server_ending(
        &endpoint,
        &released,
        &Arc::new(AtomicBool::new(false)),
        &made,
    );
    let hello = || {
        open_session::<_, Ping>(
            &endpoint,
            &Ping {
                text: "hello".into(),
            },
        )
        .unwrap()
    };
    let mut sessions = Vec::new();
    for _ in 0..MAX_SESSIONS {
        let (answer, stream) = hello();
        assert_eq!(answer.text, "hello!");
        sessions.push(stream);
    }
    // a client over the cap must learn it holds no session (not the answer of a held one)
    for _ in 0..2 {
        let (answer, stream) = hello();
        assert_eq!(answer.text, "full");
        let deadline = Instant::now() + Duration::from_secs(3);
        while !peer_gone(&stream) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(peer_gone(&stream), "closed by the server");
    }
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(server.open_connections(), MAX_SESSIONS);
    // the server's side never learned of the ones over the cap: no guard was made for them
    assert_eq!(made.load(Ordering::Acquire), MAX_SESSIONS);
    // a slot given back is taken by the next hello
    sessions.pop();
    let deadline = Instant::now() + Duration::from_secs(3);
    while server.open_connections() >= MAX_SESSIONS && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let (answer, _stream) = hello();
    assert_eq!(answer.text, "hello!");
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

/// Polls `check` for up to 5 s.
fn within(check: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn peer_gone_sees_a_closed_peer_and_leaves_waiting_data_to_the_reader() {
    let (_d, endpoint) = endpoint();
    let listening = endpoint.listen().unwrap();
    let client = endpoint.connect().unwrap();
    let mut server = listening.accept_trusted().unwrap().unwrap();
    // an open idle peer is not gone
    assert!(!peer_gone(&client));
    write_line(
        &mut server,
        &Ping {
            text: "kept".into(),
        },
    )
    .unwrap();
    // looking twice with data waiting neither reports the peer gone nor eats the line
    assert!(!peer_gone(&client));
    assert!(!peer_gone(&client));
    let line: Ping = read_line_by(&client, Instant::now() + Duration::from_secs(5)).unwrap();
    assert_eq!(line.text, "kept");
    assert!(!peer_gone(&client));
    drop(server);
    assert!(within(|| peer_gone(&client)), "a closed peer is seen");
}
