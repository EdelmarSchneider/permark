//! Session saver: changes poke it; it saves once things are quiet for [`QUIET`]. One thread, joined on stop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::shell::Shell;

const QUIET: Duration = Duration::from_millis(500);

enum Msg {
    Poke,
    Stop,
}

pub struct SessionSaver {
    handle: AppHandle,
    tx: Mutex<Option<Sender<Msg>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
    /// Last window closing: the session already holds it; later changes (its destruction) are not saved.
    frozen: Arc<AtomicBool>,
}

impl SessionSaver {
    pub fn start(handle: AppHandle) -> Self {
        let (tx, rx) = mpsc::channel::<Msg>();
        let frozen = Arc::new(AtomicBool::new(false));
        let (worker, flag) = (handle.clone(), Arc::clone(&frozen));
        let thread = std::thread::Builder::new()
            .name("permark-session".to_owned())
            .spawn(move || {
                while let Ok(Msg::Poke) = rx.recv() {
                    // debounce: wait for quiet
                    let stop = loop {
                        match rx.recv_timeout(QUIET) {
                            Ok(Msg::Poke) => {}
                            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break true,
                            Err(RecvTimeoutError::Timeout) => break false,
                        }
                    };
                    if !flag.load(Ordering::Acquire) {
                        if let Some(shell) = worker.try_state::<Shell>() {
                            shell.save_session_now(&worker);
                        }
                    }
                    if stop {
                        break;
                    }
                }
            })
            .ok();
        Self {
            handle,
            tx: Mutex::new(Some(tx)),
            thread: Mutex::new(thread),
            frozen,
        }
    }

    pub fn handle(&self) -> &AppHandle {
        &self.handle
    }

    pub fn poke(&self) {
        if let Some(tx) = self
            .tx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            let _ = tx.send(Msg::Poke);
        }
    }

    pub fn freeze(&self) {
        self.frozen.store(true, Ordering::Release);
    }

    /// App exit (main thread): freezes, stops the thread. Not joined: a save in flight waits on the main thread
    /// (window geometry), so joining here could deadlock; the thread ends with the process.
    pub fn stop(&self) {
        self.freeze();
        if let Some(tx) = self
            .tx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = tx.send(Msg::Stop);
        }
        drop(
            self.thread
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
    }
}
