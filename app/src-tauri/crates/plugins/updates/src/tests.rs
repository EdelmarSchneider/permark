use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use permark_core::ErrorKind;

use super::*;

fn available() -> UpdateState {
    UpdateState::Available {
        version: "0.7.0".into(),
        notes: None,
    }
}

#[test]
fn a_check_the_app_started_is_quiet() {
    let checking = UpdateState::Idle.next(Event::CheckStarted { manual: false });
    assert_eq!(checking, UpdateState::Checking { manual: false });
    assert_eq!(checking.next(Event::NothingNew), UpdateState::Idle);
    assert_eq!(
        checking.next(Event::CheckFailed {
            message: "offline".into()
        }),
        UpdateState::Idle
    );
}

#[test]
fn a_check_the_person_asked_for_answers() {
    let checking = UpdateState::Idle.next(Event::CheckStarted { manual: true });
    assert_eq!(checking.next(Event::NothingNew), UpdateState::UpToDate);
    let failed = checking.next(Event::CheckFailed {
        message: "offline".into(),
    });
    assert_eq!(
        failed,
        UpdateState::Error {
            stage: Stage::Check,
            message: "offline".into()
        }
    );
    assert_eq!(failed.next(Event::Dismiss), UpdateState::Idle);
    assert_eq!(
        UpdateState::UpToDate.next(Event::Dismiss),
        UpdateState::Idle
    );
    // a new check from "up to date" | an error
    assert_eq!(
        UpdateState::UpToDate.next(Event::CheckStarted { manual: true }),
        UpdateState::Checking { manual: true }
    );
}

#[test]
fn download_progress_and_ready() {
    let downloading = available().next(Event::DownloadStarted);
    assert_eq!(
        downloading,
        UpdateState::Downloading {
            version: "0.7.0".into(),
            percent: None
        }
    );
    let half = downloading.next(Event::Progress {
        done: 50,
        total: Some(100),
    });
    assert!(matches!(
        half,
        UpdateState::Downloading {
            percent: Some(50),
            ..
        }
    ));
    // unknown | zero size: no percent; more than the size: 100
    assert!(matches!(
        half.next(Event::Progress {
            done: 5,
            total: None
        }),
        UpdateState::Downloading { percent: None, .. }
    ));
    assert!(matches!(
        half.next(Event::Progress {
            done: 5,
            total: Some(0)
        }),
        UpdateState::Downloading { percent: None, .. }
    ));
    assert!(matches!(
        half.next(Event::Progress {
            done: u64::MAX,
            total: Some(10)
        }),
        UpdateState::Downloading {
            percent: Some(100),
            ..
        }
    ));
    let ready = half.next(Event::Downloaded);
    assert_eq!(
        ready,
        UpdateState::Ready {
            version: "0.7.0".into()
        }
    );
    assert!(matches!(
        ready.next(Event::RestartFailed {
            message: "x".into()
        }),
        UpdateState::Error {
            stage: Stage::Install,
            ..
        }
    ));
}

#[test]
fn late_and_out_of_place_events_change_nothing() {
    let events = [
        Event::CheckStarted { manual: true },
        Event::Found {
            version: "9".into(),
            notes: None,
        },
        Event::NothingNew,
        Event::CheckFailed {
            message: "m".into(),
        },
        Event::DownloadStarted,
        Event::Progress {
            done: 1,
            total: Some(2),
        },
        Event::Downloaded,
        Event::DownloadFailed {
            message: "m".into(),
        },
        Event::RestartFailed {
            message: "m".into(),
        },
        Event::Dismiss,
    ];
    let states = [
        UpdateState::Idle,
        UpdateState::Checking { manual: false },
        UpdateState::UpToDate,
        available(),
        UpdateState::Downloading {
            version: "1".into(),
            percent: None,
        },
        UpdateState::Ready {
            version: "1".into(),
        },
        UpdateState::Error {
            stage: Stage::Download,
            message: "m".into(),
        },
    ];
    let mut moved = 0;
    for state in &states {
        for event in &events {
            if state.next(event.clone()) != *state {
                moved += 1;
            }
        }
    }
    // the transitions the machine has, and no other (each state x event pair checked above, 70 in all)
    assert_eq!(moved, 13);
    // a check never starts while an update is known, downloading or ready
    for state in [
        available(),
        UpdateState::Ready {
            version: "1".into(),
        },
    ] {
        assert_eq!(state.next(Event::CheckStarted { manual: true }), state);
    }
}

#[test]
fn the_state_is_told_by_name() {
    assert_eq!(
        serde_json::to_value(available()).unwrap(),
        serde_json::json!({ "state": "available", "version": "0.7.0", "notes": null })
    );
    assert_eq!(
        serde_json::to_value(UpdateState::Error {
            stage: Stage::Install,
            message: "m".into()
        })
        .unwrap(),
        serde_json::json!({ "state": "error", "stage": "install", "message": "m" })
    );
}

// ---- with a fake driver ------------------------------------------------------------------------------------------

#[derive(Default)]
struct Fake {
    checks: AtomicUsize,
    offer: Mutex<Option<Found>>,
    fail_check: Mutex<Option<String>>,
    fail_download: Mutex<Option<String>>,
    unsaved: Mutex<Vec<String>>,
    restarts: AtomicUsize,
    slow: Option<Duration>,
}

impl UpdateDriver for Fake {
    fn check(&self) -> Result<Option<Found>, String> {
        self.checks.fetch_add(1, Ordering::SeqCst);
        if let Some(slow) = self.slow {
            std::thread::sleep(slow);
        }
        if let Some(message) = self.fail_check.lock().unwrap().clone() {
            return Err(message);
        }
        Ok(self.offer.lock().unwrap().clone())
    }
    fn download(&self, progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<(), String> {
        progress(0, Some(200));
        progress(100, Some(200));
        if let Some(message) = self.fail_download.lock().unwrap().clone() {
            return Err(message);
        }
        progress(200, Some(200));
        Ok(())
    }
    fn unsaved(&self) -> Vec<String> {
        self.unsaved.lock().unwrap().clone()
    }
    fn restart(&self) -> Result<(), String> {
        self.restarts.fetch_add(1, Ordering::SeqCst);
        Err("the installer did not start".into())
    }
}

fn offer() -> Found {
    Found {
        version: "0.7.0".into(),
        notes: Some("Fixes".into()),
    }
}

#[test]
fn check_download_and_restart_through_the_driver() {
    let fake = Arc::new(Fake::default());
    *fake.offer.lock().unwrap() = Some(offer());
    let updates = Updates::new(fake.clone(), None);
    updates.check(false);
    assert!(
        matches!(updates.state(), UpdateState::Available { ref notes, .. } if notes.as_deref() == Some("Fixes"))
    );
    // nothing is ready yet
    assert_eq!(updates.restart().unwrap_err().kind, ErrorKind::Unavailable);
    updates.download();
    assert_eq!(
        updates.state(),
        UpdateState::Ready {
            version: "0.7.0".into()
        }
    );
    // unsaved edits keep the app running and say where
    *fake.unsaved.lock().unwrap() = vec!["notes/a.md".into(), "b.md".into()];
    let refused = updates.restart().unwrap_err();
    assert_eq!(refused.kind, ErrorKind::Conflict);
    assert_eq!(refused.subject.as_deref(), Some("notes/a.md\nb.md"));
    assert_eq!(fake.restarts.load(Ordering::SeqCst), 0);
    fake.unsaved.lock().unwrap().clear();
    updates.restart().unwrap();
    assert_eq!(fake.restarts.load(Ordering::SeqCst), 1);
    assert!(matches!(
        updates.state(),
        UpdateState::Error {
            stage: Stage::Install,
            ..
        }
    ));
}

#[test]
fn a_failed_download_is_an_error_and_a_new_check_offers_it_again() {
    let fake = Arc::new(Fake::default());
    *fake.offer.lock().unwrap() = Some(offer());
    *fake.fail_download.lock().unwrap() = Some("bad signature".into());
    let updates = Updates::new(fake.clone(), None);
    updates.check(true);
    updates.download();
    assert_eq!(
        updates.state(),
        UpdateState::Error {
            stage: Stage::Download,
            message: "bad signature".into()
        }
    );
    updates.check(false);
    assert!(matches!(updates.state(), UpdateState::Available { .. }));
}

#[test]
fn concurrent_checks_ask_the_server_once() {
    let fake = Arc::new(Fake {
        slow: Some(Duration::from_millis(100)),
        ..Fake::default()
    });
    let updates = Updates::new(fake.clone(), None);
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let u = Arc::clone(&updates);
            std::thread::spawn(move || u.check(true))
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(fake.checks.load(Ordering::SeqCst), 1);
    assert_eq!(updates.state(), UpdateState::UpToDate);
}

#[test]
fn the_schedule_checks_at_start_and_again_and_stops_at_once() {
    let fake = Arc::new(Fake::default());
    let updates = Updates::new(fake.clone(), None);
    let scheduler = Scheduler::start(
        Arc::clone(&updates),
        Schedule {
            first: Duration::from_millis(10),
            every: Duration::from_millis(20),
        },
    );
    let start = Instant::now();
    while fake.checks.load(Ordering::SeqCst) < 3 && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(fake.checks.load(Ordering::SeqCst) >= 3);
    // automatic checks that found nothing leave no trace
    assert_eq!(updates.state(), UpdateState::Idle);
    let stopping = Instant::now();
    drop(scheduler);
    assert!(stopping.elapsed() < Duration::from_secs(1));
    let after = fake.checks.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(
        fake.checks.load(Ordering::SeqCst),
        after,
        "no check after the stop"
    );
}

#[test]
fn a_long_schedule_still_stops_at_once() {
    let fake = Arc::new(Fake::default());
    let updates = Updates::new(fake.clone(), None);
    let scheduler = Scheduler::start(updates, Schedule::DEFAULT);
    let stopping = Instant::now();
    drop(scheduler);
    assert!(stopping.elapsed() < Duration::from_secs(1));
    assert_eq!(fake.checks.load(Ordering::SeqCst), 0);
}

#[test]
fn an_automatic_check_failure_stays_quiet_and_a_manual_one_reports() {
    let fake = Arc::new(Fake::default());
    *fake.fail_check.lock().unwrap() = Some("offline".into());
    let updates = Updates::new(fake.clone(), None);
    updates.check(false);
    assert_eq!(updates.state(), UpdateState::Idle);
    updates.check(true);
    assert!(matches!(
        updates.state(),
        UpdateState::Error {
            stage: Stage::Check,
            ..
        }
    ));
    updates.dismiss();
    assert_eq!(updates.state(), UpdateState::Idle);
}

#[test]
fn the_manifest_needs_a_direct_build() {
    let plugin = plugin();
    let manifest = plugin.manifest();
    assert_eq!(manifest.id.as_str(), "updates");
    assert_eq!(manifest.scope, PluginScope::App);
    assert!(manifest.requires.needs.contains(&Need::Distribution {
        distribution: Distribution::Direct
    }));
}
