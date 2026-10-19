//! Process-wide shell state (Tauri managed): core env + workspace registry + stores, single-instance server,
//! session saver.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use permark_core::conditions::Distribution;
use permark_core::events::{CoreEvent, EventSink, OpenFailed, SettingsChanged, StateRecovered};
use permark_core::ipc_local::{Listening, Server};
use permark_core::open::{OpenAck, OpenRequest};
use permark_core::plugins::PluginId;
use permark_core::session::{app_session_file, AppSession, Geometry, RecentStore, SavedWindow};
use permark_core::settings::{
    AppSettings, SettingChange, SettingsError, SettingsFile, SettingsScope,
};
use permark_core::state::{AppDirs, Loaded, StateStore};
use permark_core::workspace::{CoreEnv, WorkspaceRegistry};
use tauri::{AppHandle, Manager};

use crate::saver::SessionSaver;
use crate::sink::TauriSink;

#[derive(Default)]
struct Delivery {
    listening: std::collections::HashSet<String>,
    pending: HashMap<String, Vec<OpenFailed>>,
}

pub struct Shell {
    pub env: Arc<CoreEnv>,
    pub registry: Arc<WorkspaceRegistry>,
    pub app_store: StateStore,
    pub recent: RecentStore,
    pub saver: SessionSaver,
    settings_file: SettingsFile,
    /// One settings write at a time (windows race).
    settings_lock: Mutex<()>,
    next_window: AtomicU64,
    /// Last non-maximized geometry per window (restore size while maximized).
    normal: Mutex<HashMap<String, Geometry>>,
    /// Startup notices, handed to the first window that boots.
    notices: Mutex<Vec<StateRecovered>>,
    /// Open failures: emitted to windows whose frontend listens, kept for the others until they boot.
    delivery: Mutex<Delivery>,
    server: Mutex<Option<Server>>,
}

/// The shell of a running app.
pub fn state(handle: &AppHandle) -> tauri::State<'_, Shell> {
    handle.state::<Shell>()
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn distribution() -> Distribution {
    if cfg!(feature = "store") {
        Distribution::Store
    } else {
        Distribution::Direct
    }
}

impl Shell {
    #[must_use]
    pub fn new(handle: AppHandle, dirs: &AppDirs) -> Self {
        let settings_file = SettingsFile::app(dirs);
        let settings = settings_file.load();
        let sink: Arc<dyn EventSink> = Arc::new(TauriSink::new(handle.clone()));
        let env = Arc::new(CoreEnv::new(
            dirs.clone(),
            Arc::new(crate::plugins::registry()),
            sink,
            distribution(),
            settings,
        ));
        env.apply_app_plugins();
        let app_store = StateStore::app(dirs);
        Self {
            registry: WorkspaceRegistry::new_shared(&env),
            recent: RecentStore::new(&app_store),
            saver: SessionSaver::start(handle),
            app_store,
            env,
            settings_file,
            settings_lock: Mutex::new(()),
            next_window: AtomicU64::new(1),
            normal: Mutex::new(HashMap::new()),
            notices: Mutex::new(Vec::new()),
            delivery: Mutex::new(Delivery::default()),
            server: Mutex::new(None),
        }
    }

    /// Serves forwarded open requests (second launches).
    pub fn serve(&self, listening: Listening) {
        let handle = self.saver.handle().clone();
        let server = Server::spawn(listening, move |request: OpenRequest| {
            let handle = handle.clone();
            // answered at once; the work runs on its own thread (never blocks the socket)
            std::thread::spawn(move || crate::open::forwarded(&handle, &request));
            OpenAck { accepted: true }
        });
        if let Ok(server) = server {
            *lock(&self.server) = Some(server);
        }
    }

    /// Next label for a new window.
    pub fn new_label(&self) -> String {
        format!("w-{}", self.next_window.fetch_add(1, Ordering::Relaxed))
    }

    pub fn remember_normal(&self, label: &str, geometry: Geometry) {
        lock(&self.normal).insert(label.to_owned(), geometry);
    }

    #[must_use]
    pub fn normal_geometry(&self, label: &str) -> Option<Geometry> {
        lock(&self.normal).get(label).copied()
    }

    pub fn add_notice(&self, notice: StateRecovered) {
        lock(&self.notices).push(notice);
    }

    pub fn take_notices(&self) -> Vec<StateRecovered> {
        std::mem::take(&mut *lock(&self.notices))
    }

    /// Open failure for `window`: emitted when its frontend listens, else kept until it boots (first launch races).
    pub fn report_open_failed(&self, failure: OpenFailed) {
        let mut delivery = lock(&self.delivery);
        if delivery.listening.contains(&failure.window) {
            drop(delivery);
            self.env.sink.emit(CoreEvent::OpenFailed(failure));
        } else {
            delivery
                .pending
                .entry(failure.window.clone())
                .or_default()
                .push(failure);
        }
    }

    /// A window's frontend booted: from now on it hears events. Returns the failures kept for it.
    pub fn window_booted(&self, window: &str) -> Vec<OpenFailed> {
        let mut delivery = lock(&self.delivery);
        delivery.listening.insert(window.to_owned());
        delivery.pending.remove(window).unwrap_or_default()
    }

    pub fn window_gone(&self, window: &str) {
        let mut delivery = lock(&self.delivery);
        delivery.listening.remove(window);
        delivery.pending.remove(window);
    }

    /// Loads the saved session (corrupt -> notice).
    pub fn load_session(&self) -> AppSession {
        let file = app_session_file(&self.app_store);
        match file.load() {
            Loaded::Found(session) => session,
            Loaded::Missing => AppSession::default(),
            Loaded::Recovered { backup } => {
                self.add_notice(StateRecovered {
                    file: file.path().display().to_string(),
                    backup: backup.map(|b| b.display().to_string()),
                });
                AppSession::default()
            }
        }
    }

    /// Snapshot of windows (oldest first) + their geometry.
    #[must_use]
    pub fn session_snapshot(&self, handle: &AppHandle) -> AppSession {
        let windows = self
            .registry
            .session_windows()
            .into_iter()
            .map(|(label, workspace)| SavedWindow {
                workspace,
                geometry: crate::windows::geometry(handle, &label, self),
            })
            .collect();
        AppSession { windows }
    }

    pub fn save_session_now(&self, handle: &AppHandle) {
        let session = self.session_snapshot(handle);
        // best effort: a session that cannot be saved is not worth an error dialog
        let _ = app_session_file(&self.app_store).save(&session);
    }

    /// Applies one setting change (file read fresh), then re-applies plugins when a switch changed.
    ///
    /// # Errors
    /// Settings file write failure.
    pub fn change_setting(&self, change: &SettingChange) -> Result<AppSettings, SettingsError> {
        let settings = {
            let _guard = lock(&self.settings_lock);
            self.settings_file.apply(change)?
        };
        self.env.set_settings(settings.clone());
        if matches!(change, SettingChange::GitBackend(_)) {
            // the git plugin of every open project starts again with the other backend
            for core in self.registry.cores() {
                core.restart_plugin(&self.env, &PluginId::of("git"));
            }
            self.registry.refresh();
        }
        if matches!(change, SettingChange::PluginEnabled { .. }) {
            self.env.apply_app_plugins();
            for core in self.registry.cores() {
                core.apply_plugins(&self.env);
            }
            self.env.sink.emit(CoreEvent::PluginsChanged);
            self.registry.refresh();
        }
        self.env
            .sink
            .emit(CoreEvent::SettingsChanged(SettingsChanged {
                scope: SettingsScope::App,
                keys: vec![change.key()],
                settings: settings.clone(),
            }));
        Ok(settings)
    }

    /// App exit: stop serving, stop plugins (reverse order), final session save already done on last close.
    pub fn shutdown(&self) {
        drop(lock(&self.server).take());
        self.saver.stop();
        self.env.stop_app_plugins();
    }
}
