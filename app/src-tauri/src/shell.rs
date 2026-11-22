//! Process-wide shell state (Tauri managed): core env + workspace registry + stores, single-instance server,
//! session saver.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use permark_core::conditions::Distribution;
use permark_core::crash::CrashReport;
use permark_core::events::{CoreEvent, EventSink, OpenFailed, SettingsChanged, StateRecovered};
use permark_core::ipc_local::{Listening, Server};
use permark_core::open::{OpenAck, OpenRequest};
use permark_core::paths::Root;
use permark_core::plugins::{HtmlPrinterApi, PluginId, ServiceLease};
use permark_core::session::{
    app_session_file, AppSession, Geometry, RecentStore, SavedNotes, SavedWindow,
};
use permark_core::settings::{
    installer_language, AppSettings, SettingChange, SettingsError, SettingsFile,
};
use permark_core::state::{AppDirs, Loaded, StateStore};
use permark_core::workspace::{CoreEnv, WorkspaceRegistry};
use permark_core::IpcError;
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
    /// The tab views: what each shows on screen, titles, close prompts.
    pub views: crate::views::Views,
    settings_file: SettingsFile,
    /// One settings write at a time (windows race).
    settings_lock: Mutex<()>,
    next_window: AtomicU64,
    /// Last non-maximized geometry per window (restore size while maximized).
    normal: Mutex<HashMap<String, Geometry>>,
    /// Startup notices, handed to the first window that boots.
    notices: Mutex<Vec<StateRecovered>>,
    /// The last run's crash report the person has not dismissed yet (read at start).
    crash: Option<CrashReport>,
    /// The crash report went to a window already: offered once per start, like the notices.
    crash_handed: AtomicBool,
    /// The last session's windows and tabs are all back in the registry (a session saved before would lack some).
    restored: AtomicBool,
    /// Open failures: emitted to views whose frontend listens, kept for the others until they boot.
    delivery: Mutex<Delivery>,
    server: Mutex<Option<Server>>,
    /// The updater's IO, handed to the plugin `updates` for the app's whole run.
    _updater: Option<ServiceLease>,
    /// The files of a window's workspace, for App-scope plugins (the converter).
    _files: Vec<ServiceLease>,
    /// The downloads network + credential store (`DownloadKit`), for plugins fetching pinned files.
    _downloads: ServiceLease,
    /// PDF | HTML | print through the system web view (`HtmlPrinterApi`), for the export plugin.
    _print: ServiceLease,
    /// The question before a program the person set up first starts (`ProgramConsentApi`).
    _consent: ServiceLease,
}

/// The file manager entries go with their plugin (off = leaves nothing behind); quitting the app leaves them.
fn withdraw_if_switched_off(before: &BTreeSet<PluginId>, after: &BTreeSet<PluginId>) {
    let id = PluginId::of("shell-integration");
    if before.contains(&id) && !after.contains(&id) {
        #[cfg(feature = "plugin-shell-integration")]
        std::thread::spawn(|| {
            if let Err(reason) = permark_plugin_shell_integration::withdraw() {
                eprintln!("PerMark: file manager entries not removed: {reason}");
            }
        });
    }
}

/// The shell of a running app.
pub fn state(handle: &AppHandle) -> tauri::State<'_, Shell> {
    handle.state::<Shell>()
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The startup notice for a settings file that is there but cannot be used, so the person knows why their choices
/// are missing: one that is not TOML is set aside now (`backup`: where it went); one that cannot be read as text is
/// left untouched (no backup). The app runs on the defaults either way.
fn unreadable_settings(file: &SettingsFile) -> Option<StateRecovered> {
    let backup = match file.set_aside_if_broken() {
        Ok(None) => return None,
        Ok(Some(backup)) => Some(backup.display().to_string()),
        Err(_) => None,
    };
    Some(StateRecovered {
        file: file.path().display().to_string(),
        backup,
    })
}

/// One setting change (file read fresh), then plugins re-applied when a switch changed, every window told. The file,
/// the cached settings, the app's own plugins and the windows' settings news inside `writes`: they follow the order of
/// the writes, so a write that finished earlier never lands over a newer one, and a window never ends on older
/// settings than the file holds. The open projects' plugins after `writes` is let go: each project waits for its
/// settings turn first (an Initialize holds it while it copies the folder's data), which must never hold every other
/// setting; each apply reads the settings as they are then, so the last change wins in whatever order the threads get
/// there. Nothing reachable from an app plugin's start may write a setting (it would wait on `writes` forever); only
/// the IPC commands call this.
fn change_setting(
    env: &CoreEnv,
    registry: &WorkspaceRegistry,
    file: &SettingsFile,
    writes: &Mutex<()>,
    change: &SettingChange,
) -> Result<AppSettings, SettingsError> {
    let switched = matches!(change, SettingChange::PluginEnabled { .. });
    let settings = {
        let _guard = lock(writes);
        let settings = file.apply(change)?;
        env.set_settings(settings.clone());
        env.settings_changed();
        if switched {
            let before = env.app_plugins_on();
            env.apply_app_plugins();
            withdraw_if_switched_off(&before, &env.app_plugins_on());
        }
        env.sink
            .emit(CoreEvent::SettingsChanged(Box::new(SettingsChanged {
                keys: vec![change.key()],
                settings: settings.clone(),
            })));
        settings
    };
    if matches!(change, SettingChange::GitBackend(_)) {
        // the git plugin of every open project starts again with the other backend
        for core in registry.cores() {
            core.restart_plugin(env, &PluginId::of("git"));
        }
        registry.refresh();
    }
    if switched {
        for core in registry.cores() {
            core.apply_plugins(env);
        }
        env.sink.emit(CoreEvent::PluginsChanged);
        registry.refresh();
    }
    Ok(settings)
}

/// The groups file read again (nothing watches it): changed -> the cached settings take it, app plugins and every
/// window are told (`true`); unchanged -> nothing. Inside `writes`, as a setting change: no write lands between the
/// read of the cached settings and their replacement, and the news keep the order of the changes.
fn refresh_characters(env: &CoreEnv, file: &SettingsFile, writes: &Mutex<()>) -> bool {
    let _guard = lock(writes);
    let Some(settings) = file.refresh_characters(&env.settings()) else {
        return false;
    };
    env.set_settings(settings.clone());
    env.settings_changed();
    env.sink
        .emit(CoreEvent::SettingsChanged(Box::new(SettingsChanged {
            // the groups come from the file this key names
            keys: vec!["characters_file".to_owned()],
            settings,
        })));
    true
}

/// Writes the built-in groups to the groups file only while none is there; a new file is read at once so the windows
/// see it as theirs.
fn create_characters(
    env: &CoreEnv,
    file: &SettingsFile,
    writes: &Mutex<()>,
) -> Result<bool, IpcError> {
    let created = file.create_characters(&env.settings())?;
    if created {
        refresh_characters(env, file, writes);
    }
    Ok(created)
}

/// `Store` = a store updates this copy: the Microsoft Store build (feature `store`), or the .deb inside a snap.
pub(crate) fn distribution() -> Distribution {
    let in_snap = cfg!(all(unix, not(target_os = "macos")))
        && std::env::var_os("SNAP").is_some_and(|v| !v.is_empty());
    if cfg!(feature = "store") || in_snap {
        Distribution::Store
    } else {
        Distribution::Direct
    }
}

impl Shell {
    #[must_use]
    pub fn new(handle: AppHandle, dirs: &AppDirs) -> Self {
        let settings_file = SettingsFile::app(dirs);
        // first start after 0.5: the choices it kept (notes folder, editor, language), read from its own folder
        if let Some(older) = permark_core::state::older::older_profile(dirs) {
            let _ = settings_file.adopt_older(&older.join("settings.json"));
        }
        // first start: the language the installer recorded (a failed write leaves the system language; a file that is
        // not TOML is left to the check below)
        let _ = settings_file.adopt_language(installer_language());
        let unreadable = unreadable_settings(&settings_file);
        let settings = settings_file.load();
        let app_store = StateStore::app(dirs);
        // before any window: what was set up before approvals existed counts as allowed, nothing a page sets up later
        crate::consent::adopt(&app_store, &settings.editor);
        let sink: Arc<dyn EventSink> = Arc::new(TauriSink::new(handle.clone()));
        let env = Arc::new(CoreEnv::new(
            dirs.clone(),
            Arc::new(crate::plugins::registry()),
            sink,
            distribution(),
            settings,
        ));
        #[cfg(feature = "plugin-updates")]
        let updater = (env.distribution == Distribution::Direct)
            .then(|| crate::updater::provide(&env, &handle));
        #[cfg(not(feature = "plugin-updates"))]
        let updater = None;
        let files = crate::files::provide(&env, &handle);
        let downloads = crate::downloads_http::provide(&env);
        let print = crate::print::provide(&env, &handle);
        let consent = crate::consent::provide(&env, &handle);
        env.apply_app_plugins();
        // also removes all but the newest reports (never done by the panic hook)
        let crash = permark_core::crash::offer(&app_store);
        Self {
            registry: WorkspaceRegistry::new_shared(&env),
            recent: RecentStore::new(&app_store),
            saver: SessionSaver::start(handle),
            views: crate::views::Views::default(),
            app_store,
            env,
            settings_file,
            settings_lock: Mutex::new(()),
            next_window: AtomicU64::new(1),
            normal: Mutex::new(HashMap::new()),
            notices: Mutex::new(unreadable.into_iter().collect()),
            crash,
            crash_handed: AtomicBool::new(false),
            restored: AtomicBool::new(false),
            delivery: Mutex::new(Delivery::default()),
            server: Mutex::new(None),
            _updater: updater,
            _files: files,
            _downloads: downloads,
            _print: print,
            _consent: consent,
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

    /// The offer of the last run's crash report, for the first window that boots; `None` afterwards.
    pub fn take_crash(&self) -> Option<crate::crash::CrashOffer> {
        if self.crash_handed.swap(true, Ordering::Relaxed) {
            return None;
        }
        self.crash.as_ref().map(crate::crash::offer_of)
    }

    /// The person dismissed the crash report offer: it is not offered again (nor an older one).
    ///
    /// # Errors
    /// The record cannot be written.
    pub fn dismiss_crash(&self) -> Result<(), IpcError> {
        match &self.crash {
            Some(report) => permark_core::crash::dismiss(&self.app_store, report),
            None => Ok(()),
        }
    }

    /// The last session is back in the registry: saving the session now keeps all of it.
    pub fn restore_done(&self) {
        self.restored.store(true, Ordering::SeqCst);
    }

    /// Whether the last session is back in the registry ([`Self::restore_done`]).
    #[must_use]
    pub fn restored(&self) -> bool {
        self.restored.load(Ordering::SeqCst)
    }

    /// The offered crash report's file.
    #[must_use]
    pub fn crash_file(&self) -> Option<&std::path::Path> {
        self.crash.as_ref().map(|report| report.path.as_path())
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

    pub fn view_gone(&self, view: &str) {
        let mut delivery = lock(&self.delivery);
        delivery.listening.remove(view);
        delivery.pending.remove(view);
        drop(delivery);
        // the file an export wrote for this view: nothing can open | show it any more, and the map must not grow
        if let Some(printer) = self.env.services().get::<HtmlPrinterApi>() {
            printer.0.forget(view);
        }
        // what App-scope plugins keep for this view (a converter's job) goes with it
        self.env.rpc().caller_gone(view);
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

    /// Snapshot of the work windows (creation order): geometry, tabs (pending ones as their ref), active tab.
    #[must_use]
    pub fn session_snapshot(&self, handle: &AppHandle) -> AppSession {
        let windows = self
            .registry
            .session_windows()
            .into_iter()
            .map(|saved| SavedWindow {
                geometry: crate::windows::geometry(handle, &saved.window, self),
                tabs: saved.tabs,
                active: saved.active,
            })
            .collect();
        AppSession {
            windows,
            // the Notes are a tab: `geometry` stays in the format (0.6.0 wrote it), unused
            notes: SavedNotes {
                open: self.registry.notes_open(),
                geometry: None,
            },
        }
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
        change_setting(
            &self.env,
            &self.registry,
            &self.settings_file,
            &self.settings_lock,
            change,
        )
    }

    /// Reads the groups file again; a change reaches every window as `SettingsChanged`.
    pub fn refresh_characters(&self) {
        refresh_characters(&self.env, &self.settings_file, &self.settings_lock);
    }

    /// Writes the built-in groups file when none is there (`false`: one is there, or no name is set).
    ///
    /// # Errors
    /// The write failed.
    pub fn create_characters(&self) -> Result<bool, IpcError> {
        create_characters(&self.env, &self.settings_file, &self.settings_lock)
    }

    /// What "show in folder" reveals for the groups file (the file, else the profile folder).
    #[must_use]
    pub fn characters_target(&self) -> std::path::PathBuf {
        self.settings_file.characters_target(&self.env.settings())
    }

    /// Creates `.permark/` in `path` ([`WorkspaceRegistry::init_project`]): open projects follow, windows are told.
    ///
    /// # Errors
    /// The folder does not exist | is not a folder; a link or a file is in the way; both places hold data; IO failure.
    pub fn init_project(&self, path: &std::path::Path) -> Result<(), IpcError> {
        self.registry.init_project(&Root::open(path)?)
    }

    /// Keeps the private data of the folder at `path` on this computer ([`WorkspaceRegistry::keep_data`]): windows are
    /// told.
    ///
    /// # Errors
    /// The folder does not exist | is not a folder; the profile's row file cannot be written.
    pub fn keep_data(&self, path: &std::path::Path) -> Result<(), IpcError> {
        self.registry.keep_data(&Root::open(path)?)
    }

    /// Writes what every open project's activity journal holds now (its writer waits a moment to gather entries, and an
    /// exit that skips destructors would lose them).
    #[cfg(feature = "plugin-updates")]
    pub fn flush_activity(&self) {
        for core in self.registry.cores() {
            core.activity().flush();
        }
    }

    /// App exit: saves the session unless the last window's close did (frozen: the windows may be gone by now), then
    /// stops serving, stops plugins (reverse order). An exit no window close led to (macOS Dock Quit, logout, a system
    /// shutdown) keeps the windows of this run.
    pub fn shutdown(&self, handle: &AppHandle) {
        // an exit after the windows already went (logout, system shutdown) has nothing to keep: the last saved
        // session stays as it is
        if !self.saver.is_frozen() {
            let session = self.session_snapshot(handle);
            if !session.windows.is_empty() {
                let _ = app_session_file(&self.app_store).save(&session);
            }
        }
        drop(lock(&self.server).take());
        self.saver.stop();
        self.env.stop_app_plugins();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use permark_core::settings::CharactersStatus;

    #[test]
    fn a_settings_file_that_cannot_be_read_is_told_at_start_a_missing_one_is_not() {
        let base = tempfile::tempdir().unwrap();
        let file = SettingsFile::app(&AppDirs::under(base.path()));
        assert_eq!(unreadable_settings(&file), None);
        std::fs::create_dir_all(file.path().parent().unwrap()).unwrap();
        std::fs::write(file.path(), b"theme = \"dark\"\n# caf\xe9\n").unwrap();
        // no backup: the file was left in place, never set aside
        assert_eq!(
            unreadable_settings(&file),
            Some(StateRecovered {
                file: file.path().display().to_string(),
                backup: None,
            })
        );
        assert_eq!(
            std::fs::read(file.path()).unwrap(),
            b"theme = \"dark\"\n# caf\xe9\n"
        );
    }

    #[test]
    fn a_settings_file_that_is_not_toml_is_set_aside_at_start_and_the_notice_names_the_backup() {
        let base = tempfile::tempdir().unwrap();
        let file = SettingsFile::app(&AppDirs::under(base.path()));
        std::fs::create_dir_all(file.path().parent().unwrap()).unwrap();
        let broken = "theme = \"dark\"\n<<<<<<< HEAD\n";
        std::fs::write(file.path(), broken).unwrap();
        // the installer's language never replaces it, as at a start
        assert!(file
            .adopt_language(Some(permark_core::settings::LanguagePref::De))
            .unwrap()
            .is_none());
        let notice = unreadable_settings(&file).unwrap();
        assert_eq!(notice.file, file.path().display().to_string());
        let backup = notice.backup.unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), broken);
        assert!(!file.path().exists());
        // told once: the next start finds nothing to tell
        assert_eq!(unreadable_settings(&file), None);
    }

    #[derive(Default)]
    struct Told(Mutex<Vec<CoreEvent>>);

    impl EventSink for Told {
        fn emit(&self, event: CoreEvent) {
            lock(&self.0).push(event);
        }
    }

    impl Told {
        /// The groups status each `SettingsChanged` carried, in order.
        fn statuses(&self) -> Vec<CharactersStatus> {
            std::mem::take(&mut *lock(&self.0))
                .into_iter()
                .filter_map(|event| match event {
                    CoreEvent::SettingsChanged(changed) => Some(changed.settings.characters_status),
                    _ => None,
                })
                .collect()
        }
    }

    fn env_with(base: &std::path::Path, told: &Arc<Told>) -> (CoreEnv, SettingsFile) {
        let dirs = AppDirs::under(base);
        let file = SettingsFile::app(&dirs);
        let env = CoreEnv::new(
            dirs,
            Arc::new(crate::plugins::registry()),
            Arc::clone(told) as Arc<dyn EventSink>,
            Distribution::Direct,
            file.load(),
        );
        (env, file)
    }

    #[test]
    fn a_groups_file_read_again_tells_the_windows_only_when_it_changed() {
        let base = tempfile::tempdir().unwrap();
        let told = Arc::new(Told::default());
        let (env, file) = env_with(base.path(), &told);
        let writes = Mutex::new(());
        assert!(!refresh_characters(&env, &file, &writes));
        assert_eq!(told.statuses(), vec![]);
        std::fs::create_dir_all(&env.dirs.profile).unwrap();
        std::fs::write(
            env.dirs.profile.join("characters.toml"),
            "[[groups]]\nname = \"mine\"\nentries = [{ char = \"→\", names = [\"right arrow\"] }]\n",
        )
        .unwrap();
        assert!(refresh_characters(&env, &file, &writes));
        assert_eq!(told.statuses(), vec![CharactersStatus::Read]);
        assert_eq!(env.settings().characters_status, CharactersStatus::Read);
        assert_eq!(env.settings().characters[0].name, "mine");
        // read again with nothing new: no second notice
        assert!(!refresh_characters(&env, &file, &writes));
        assert_eq!(told.statuses(), vec![]);
    }

    /// Records the keyboard map size each `SettingsChanged` carried; every tenth one is slow to deliver (a busy
    /// window), so news sent outside the writes' turn would be recorded out of order.
    #[derive(Default)]
    struct Slow {
        sizes: Mutex<Vec<usize>>,
        seen: AtomicU64,
    }

    impl EventSink for Slow {
        fn emit(&self, event: CoreEvent) {
            if let CoreEvent::SettingsChanged(changed) = event {
                if self.seen.fetch_add(1, Ordering::Relaxed).is_multiple_of(10) {
                    std::thread::sleep(std::time::Duration::from_millis(15));
                }
                lock(&self.sizes).push(changed.settings.keymap.len());
            }
        }
    }

    #[test]
    fn settings_changed_from_two_threads_keep_every_change_in_the_file_the_cache_and_the_news_order(
    ) {
        let base = tempfile::tempdir().unwrap();
        let dirs = AppDirs::under(base.path());
        let file = SettingsFile::app(&dirs);
        let told = Arc::new(Slow::default());
        let env = Arc::new(CoreEnv::new(
            dirs,
            Arc::new(crate::plugins::registry()),
            Arc::clone(&told) as Arc<dyn EventSink>,
            Distribution::Direct,
            file.load(),
        ));
        let registry = WorkspaceRegistry::new(Arc::clone(&env));
        let writes = Mutex::new(());
        std::thread::scope(|scope| {
            for thread in 0..2 {
                let (env, registry, file, writes) = (&env, &registry, &file, &writes);
                scope.spawn(move || {
                    for n in 0..100 {
                        let change = SettingChange::Keymap {
                            command: format!("test.t{thread}.c{n}"),
                            combo: Some(format!("Ctrl+{n}")),
                        };
                        change_setting(env, registry, file, writes, &change).unwrap();
                    }
                });
            }
        });
        assert_eq!(file.load().keymap.len(), 200);
        assert_eq!(env.settings().keymap.len(), 200);
        assert_eq!(*lock(&told.sizes), (1..=200).collect::<Vec<_>>());
    }

    /// A plugin switch waits for a project whose settings turn is held (an Initialize copying the folder's data), but
    /// no other setting waits with it: the writes' turn is let go before the projects are waited for.
    #[test]
    fn a_plugin_switch_waiting_for_a_project_holds_no_other_setting() {
        use permark_core::workspace::{TabOpen, WorkspaceRef};
        let base = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let dirs = AppDirs::under(base.path());
        let file = SettingsFile::app(&dirs);
        let env = Arc::new(CoreEnv::new(
            dirs,
            Arc::new(permark_core::plugins::PluginRegistry::new(Vec::new()).unwrap()),
            Arc::new(Told::default()) as Arc<dyn EventSink>,
            Distribution::Direct,
            file.load(),
        ));
        let registry = Arc::new(WorkspaceRegistry::new(Arc::clone(&env)));
        registry.add_window("main");
        let _ = registry.show_welcome_view("main");
        let reference = WorkspaceRef::Project(Root::open(folder.path()).unwrap());
        let TabOpen::Opening { outcome, .. } = registry.open_tab("main", &reference, None).unwrap()
        else {
            panic!("the folder was open already");
        };
        for (ticket, reference) in outcome.loads {
            registry.load(ticket, &reference).unwrap();
        }
        let core = registry.cores().pop().expect("the project is open");
        let file = Arc::new(file);
        let writes = Arc::new(Mutex::new(()));
        let turn = core.hold_config_turn();
        let switch = std::thread::spawn({
            let (env, registry, file, writes) = (
                Arc::clone(&env),
                Arc::clone(&registry),
                Arc::clone(&file),
                Arc::clone(&writes),
            );
            move || {
                let change = SettingChange::PluginEnabled {
                    plugin: PluginId::of("git"),
                    enabled: Some(false),
                };
                change_setting(&env, &registry, &file, &writes, &change).map(|_| ())
            }
        });
        // the switch is in the cache (written inside the writes' turn): it waits for the project now
        let started = std::time::Instant::now();
        while env.settings().plugins.is_empty() {
            assert!(started.elapsed() < std::time::Duration::from_secs(10));
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let (done, other) = std::sync::mpsc::channel();
        std::thread::spawn({
            let (env, registry, file, writes) = (
                Arc::clone(&env),
                Arc::clone(&registry),
                Arc::clone(&file),
                Arc::clone(&writes),
            );
            move || {
                let change = SettingChange::Keymap {
                    command: "test.other".to_owned(),
                    combo: Some("Ctrl+1".to_owned()),
                };
                let _ = done.send(change_setting(&env, &registry, &file, &writes, &change).is_ok());
            }
        });
        let answered = other.recv_timeout(std::time::Duration::from_secs(5));
        drop(turn);
        assert_eq!(answered, Ok(true), "another setting waited for the project");
        assert!(switch.join().unwrap().is_ok());
        assert_eq!(file.load().keymap.len(), 1);
    }

    #[test]
    fn creating_the_groups_file_writes_once_and_never_over_a_file() {
        let base = tempfile::tempdir().unwrap();
        let told = Arc::new(Told::default());
        let (env, file) = env_with(base.path(), &told);
        let writes = Mutex::new(());
        assert_eq!(env.settings().characters_status, CharactersStatus::Builtin);
        assert!(create_characters(&env, &file, &writes).unwrap());
        assert_eq!(told.statuses(), vec![CharactersStatus::Read]);
        assert_eq!(env.settings().characters_status, CharactersStatus::Read);
        let path = env.dirs.profile.join("characters.toml");
        assert_eq!(file.characters_target(&env.settings()), path);
        let written = std::fs::read(&path).unwrap();
        assert!(!create_characters(&env, &file, &writes).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), written);
        assert_eq!(told.statuses(), vec![]);
    }
}
