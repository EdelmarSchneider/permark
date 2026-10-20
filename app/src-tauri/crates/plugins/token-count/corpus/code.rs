//! Plugin `updates`: signed self-updates of the builds PerMark publishes itself (never a Store build: the store updates
//! those). A check a few seconds after the start and then once a day is the app's only background network call; it fails
//! quietly and tries again next time. Downloading and restarting are the person's actions; a restart waits until no
//! window holds unsaved edits.
//!
//! This crate is the state machine and the schedule. The network, the signature check and the installer are the app's:
//! it provides an [`UpdateDriver`] through the app services before the plugin starts.

mod state;

use std::borrow::Cow;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use permark_core::conditions::{Distribution, Need, Requirement};
use permark_core::plugins::{
    typed, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginEvents, PluginId, PluginKind,
    PluginScope, RpcRegistration, Running,
};
use permark_core::{ErrorKind, IpcError};

pub use state::{Event, Stage, UpdateState};

const ID: PluginId = PluginId::of("updates");

const NEEDS: &[Need] = &[Need::Distribution {
    distribution: Distribution::Direct,
}];

/// A newer version the update server offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub version: String,
    pub notes: Option<String>,
}

/// What the app does for the updater. Every method blocks (the plugin calls them on its own threads).
pub trait UpdateDriver: Send + Sync {
    /// Whether to check by itself (a development build checks only when told where to).
    fn automatic(&self) -> bool {
        true
    }
    /// Asks the update server; `None` = nothing newer.
    ///
    /// # Errors
    /// No answer | a bad answer (text for the person).
    fn check(&self) -> Result<Option<Found>, String>;
    /// Downloads the update `check` found and verifies its signature; keeps it for `restart`. `progress(done, total)`.
    ///
    /// # Errors
    /// The download | the signature check failed.
    fn download(&self, progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<(), String>;
    /// The documents with unsaved edits in any window (a restart would lose them).
    fn unsaved(&self) -> Vec<String>;
    /// Installs the downloaded update and starts PerMark again. Returns only when that failed.
    ///
    /// # Errors
    /// The installer could not start (text for the person).
    fn restart(&self) -> Result<(), String>;
}

/// How the app hands its driver to the plugin (`AppServices.services`).
pub struct DriverService(pub Arc<dyn UpdateDriver>);

/// When the automatic checks run.
#[derive(Debug, Clone, Copy)]
pub struct Schedule {
    pub first: Duration,
    pub every: Duration,
}

impl Schedule {
    /// 5 s after the start (the app settles first), then every 24 h.
    pub const DEFAULT: Self = Self {
        first: Duration::from_secs(5),
        every: Duration::from_hours(24),
    };
}

/// The updater of one running app.
pub struct Updates {
    state: Mutex<UpdateState>,
    driver: Arc<dyn UpdateDriver>,
    events: Option<PluginEvents>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Updates {
    #[must_use]
    pub fn new(driver: Arc<dyn UpdateDriver>, events: Option<PluginEvents>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(UpdateState::Idle),
            driver,
            events,
        })
    }

    #[must_use]
    pub fn state(&self) -> UpdateState {
        lock(&self.state).clone()
    }

    /// Applies `event`; tells the windows when the state changed. Returns whether it changed.
    fn apply(&self, event: Event) -> bool {
        let after = {
            let mut state = lock(&self.state);
            let after = state.next(event);
            if after == *state {
                return false;
            }
            *state = after.clone();
            after
        };
        if let (Some(events), Ok(payload)) = (&self.events, serde_json::to_value(&after)) {
            events.emit("state", payload);
        }
        true
    }

    /// One check (blocking). A check while another runs | a download | a ready update does nothing.
    pub fn check(&self, manual: bool) {
        // only the call that moved the state to "checking" asks the server
        if !self.apply(Event::CheckStarted { manual }) {
            return;
        }
        let event = match self.driver.check() {
            Ok(Some(found)) => Event::Found {
                version: found.version,
                notes: found.notes,
            },
            Ok(None) => Event::NothingNew,
            Err(message) => Event::CheckFailed { message },
        };
        self.apply(event);
    }

    /// Downloads the available update (blocking).
    pub fn download(&self) {
        if !self.apply(Event::DownloadStarted) {
            return;
        }
        let mut progress = |done: u64, total: Option<u64>| {
            self.apply(Event::Progress { done, total });
        };
        let event = match self.driver.download(&mut progress) {
            Ok(()) => Event::Downloaded,
            Err(message) => Event::DownloadFailed { message },
        };
        self.apply(event);
    }

    /// Installs the ready update and restarts; refused while a window holds unsaved edits.
    ///
    /// # Errors
    /// `Conflict` (subject: the unsaved documents, one per line) | `Unavailable` (nothing is ready).
    pub fn restart(&self) -> Result<(), IpcError> {
        if !matches!(self.state(), UpdateState::Ready { .. }) {
            return Err(IpcError::new(ErrorKind::Unavailable).subject("update"));
        }
        let unsaved = self.driver.unsaved();
        if !unsaved.is_empty() {
            return Err(IpcError::new(ErrorKind::Conflict).subject(unsaved.join("\n")));
        }
        if let Err(message) = self.driver.restart() {
            self.apply(Event::RestartFailed { message });
        }
        Ok(())
    }

    pub fn dismiss(&self) {
        self.apply(Event::Dismiss);
    }
}

/// The automatic checks: one thread, stopped (and joined) when dropped.
pub struct Scheduler {
    stop: Arc<(Mutex<bool>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl Scheduler {
    #[must_use]
    pub fn start(updates: Arc<Updates>, schedule: Schedule) -> Self {
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let signal = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("permark-updates".to_owned())
            .spawn(move || {
                let mut wait = schedule.first;
                loop {
                    let (flag, condvar) = &*signal;
                    let guard = lock(flag);
                    let (guard, _) = condvar
                        .wait_timeout_while(guard, wait, |stopped| !*stopped)
                        .unwrap_or_else(PoisonError::into_inner);
