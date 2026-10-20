//! `ProjectCore`: everything one open folder needs, shared by every window on it. Built in one step; Drop tears
//! down its plugins in reverse dependency order.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock};

use serde::{Deserialize, Serialize};

use crate::activity::{system_clock, ActivityLog};
use crate::conditions::{Context, Distribution};
use crate::document::{DocScope, Documents};
use crate::events::{CoreEvent, EventSink};
use crate::fileops::FileOps;
use crate::format::FormatRegistry;
use crate::notes::{NotesCore, NotesScope};
use crate::organizing::{SilenceApi, TagsApi};
use crate::paths::{Root, Vault};
use crate::plugins::{
    resolve_enabled, AppServices, CxWorkspace, EnableInputs, Enablement, PluginCx, PluginId,
    PluginRegistry, PluginScope, PluginSet, ProjectFacts, ProjectServices, RpcTable, ServiceLease,
    Services,
};
use crate::search::{SearchFilter, SearchIndex, SearchOptions};
use crate::settings::{
    AppSettings, ProjectConfig, ProjectConfigFile, ProjectSettingChange, SettingsError,
};
use crate::state::{AppDirs, ProjectId, StateData, StateLocation, StateStore};
use crate::tree::Explorer;
use crate::watch::{FsBatch, LiveStatus, OwnWrites, StatusHook, Subscription, WatchHub, Watcher};

/// Process-wide services every workspace uses.
pub struct CoreEnv {
    pub dirs: AppDirs,
    pub registry: Arc<PluginRegistry>,
    pub sink: Arc<dyn EventSink>,
    pub distribution: Distribution,
    settings: Arc<RwLock<AppSettings>>,
    app_plugins: Mutex<PluginSet>,
    /// What App-scope plugins serve (the Notes window and the welcome screen call it; no project needed).
    rpc: Arc<RpcTable>,
    /// Typed APIs App-scope plugins hand to core and to each other.
    services: Arc<Services>,
    /// Where the Notes window's documents are (the `notes` plugin fills it).
    notes_scope: Arc<NotesScope>,
    _notes_lease: ServiceLease,
    /// The Notes workspace, built when first opened.
    notes: OnceLock<Arc<NotesCore>>,
    /// Formats of the app-scope plugins that are on (single-file windows read with these).
    formats: RwLock<Arc<FormatRegistry>>,
    /// Called when a project live updates change (the registry recomputes contexts).
    status_hook: Mutex<Option<StatusHook>>,
}

impl std::fmt::Debug for CoreEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoreEnv")
            .field("dirs", &self.dirs)
            .finish_non_exhaustive()
    }
}

impl CoreEnv {
    #[must_use]
    pub fn new(
        dirs: AppDirs,
        registry: Arc<PluginRegistry>,
        sink: Arc<dyn EventSink>,
        distribution: Distribution,
        settings: AppSettings,
    ) -> Self {
        let services = Services::new();
        let notes_scope = NotesScope::new();
        let notes_lease = services.provide(Arc::clone(&notes_scope));
        Self {
            dirs,
            registry,
            sink,
            distribution,
            settings: Arc::new(RwLock::new(settings)),
            app_plugins: Mutex::new(PluginSet::new(PluginScope::App)),
            rpc: RpcTable::new(),
            services,
            notes_scope,
            _notes_lease: notes_lease,
            notes: OnceLock::new(),
            formats: RwLock::new(Arc::new(FormatRegistry::core())),
            status_hook: Mutex::new(None),
        }
    }

    /// What App-scope plugins serve.
    #[must_use]
    pub fn rpc(&self) -> &Arc<RpcTable> {
        &self.rpc
    }

    /// The typed APIs App-scope plugins offer.
    #[must_use]
    pub fn services(&self) -> &Arc<Services> {
        &self.services
    }

    /// Where the Notes window's documents are.
    #[must_use]
    pub fn notes_scope(&self) -> &Arc<NotesScope> {
        &self.notes_scope
    }

    /// The Notes workspace (built on first use: it polls its open documents on a thread of its own).
    #[must_use]
    pub fn notes(&self) -> Arc<NotesCore> {
        Arc::clone(
            self.notes.get_or_init(|| {
                NotesCore::new(Arc::clone(&self.notes_scope), Arc::clone(&self.sink))
            }),
        )
    }

    pub fn set_status_hook(&self, hook: StatusHook) {
        *self
            .status_hook
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(hook);
    }

    #[must_use]
    pub fn status_hook(&self) -> Option<StatusHook> {
        self.status_hook
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    #[must_use]
    pub fn settings(&self) -> AppSettings {
        self.settings
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn set_settings(&self, settings: AppSettings) {
        *self
            .settings
            .write()
            .unwrap_or_else(PoisonError::into_inner) = settings;
    }

    /// Tells the running App-scope plugins that the settings changed (after [`CoreEnv::set_settings`]).
    pub fn settings_changed(&self) {
        let settings = self.settings();
        self.app_plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .settings_changed(&settings);
    }

    /// App-level enablement (no project overrides).
    #[must_use]
    pub fn app_enablement(&self) -> BTreeMap<PluginId, Enablement> {
        let inputs = EnableInputs {
            app: self.settings().plugins,
            ..EnableInputs::default()
        };
        resolve_enabled(&self.registry, &inputs)
    }

    /// App plugins that are on (running set brought to it by [`CoreEnv::apply_app_plugins`]).
    #[must_use]
    pub fn app_plugins_on(&self) -> BTreeSet<PluginId> {
        let enablement = self.app_enablement();
        self.registry
            .ordered()
            .filter(|m| {
                m.scope == PluginScope::App && enablement.get(&m.id) == Some(&Enablement::On)
            })
            .map(|m| m.id.clone())
            .collect()
    }

    /// The formats of the plugins that are on, app-wide.
    #[must_use]
    pub fn formats(&self) -> Arc<FormatRegistry> {
        Arc::clone(&self.formats.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// Starts | stops app plugins to match settings.
    pub fn apply_app_plugins(&self) {
        let on = crate::plugins::on_ids(&self.app_enablement());
        *self.formats.write().unwrap_or_else(PoisonError::into_inner) =
            Arc::new(FormatRegistry::build(&self.registry.contributions(&on)));
        let settings = self.settings();
        let state = StateStore::app(&self.dirs);
        let context = Context::welcome(self.distribution);
        let failures = self
            .app_plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .apply(&self.registry, &on, |id| {
                PluginCx::new(
                    id.clone(),
                    CxWorkspace::App,
                    state.plugin_dir(id),
                    settings.plugin_tables.get(id).map(|t| t.0.clone()),
                    context.clone(),
                    Arc::clone(&self.sink),
                )
                .with_store(state.clone())
                .with_app(AppServices::new(
                    Arc::clone(&self.rpc),
                    Arc::clone(&self.services),
                    Arc::clone(&self.settings),
                ))
            });
        for failure in failures {
            self.sink.emit(CoreEvent::PluginFailed {
                message: failure.to_string(),
            });
        }
    }

    /// Stops app plugins (app exit; reverse order).
    pub fn stop_app_plugins(&self) {
        self.app_plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stop_all();
    }
}

/// Person's per-project plugin choices (profile only: a project's files never switch them).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectPluginChoices {
    pub choices: BTreeMap<PluginId, bool>,
}

impl StateData for ProjectPluginChoices {
    const VERSION: u32 = 1;
}

pub const PLUGIN_CHOICES_FILE: &str = "plugins.json";

pub struct ProjectCore {
    // Teardown order = declaration order after `Drop::drop` stops the plugins: the Explorer stops listening, the
    // watcher joins its thread, then file operations, the Explorer and the rest go.
    _explorer_sub: Subscription,
    _documents_sub: Subscription,
    _search_sub: Subscription,
    _activity_sub: Option<Subscription>,
    search: Arc<SearchIndex>,
    watcher: Watcher,
    fileops: FileOps,
    /// The person's actions + the changes made outside (one journal for every window on the folder).
    activity: Arc<ActivityLog>,
    documents: Arc<Documents>,
    formats: Arc<RwLock<Arc<FormatRegistry>>>,
    explorer: Arc<Explorer>,
    hub: Arc<WatchHub>,
    id: ProjectId,
    vault: Vault,
    /// Where the project's state lives; moves when PerMark is initialized in the folder (`reload_state`).
    store: RwLock<StateStore>,
    config: RwLock<ProjectConfig>,
    enablement: Mutex<BTreeMap<PluginId, Enablement>>,
    plugins: Mutex<PluginSet>,
    // what the plugins share; they outlive the plugins (stopped first in Drop)
    own: Arc<OwnWrites>,
    facts: Arc<ProjectFacts>,
    rpc: Arc<RpcTable>,
    services: Arc<Services>,
    app_services: Arc<Services>,
    /// Opened for a reader with no window: only these plugins run, each told `files_only`.
    files_only: Option<BTreeSet<PluginId>>,
}

impl std::fmt::Debug for ProjectCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectCore")
            .field("root", &self.vault.main().path())
            .finish_non_exhaustive()
    }
}

/// The project's watch, told which paths the tree holds as folders; the tree is built at once in the background.
fn start_watcher(
    vault: &Vault,
    explorer: &Arc<Explorer>,
    hub: &Arc<WatchHub>,
    own: &Arc<OwnWrites>,
    env: &CoreEnv,
) -> Watcher {
    let watcher = Watcher::start(
        vault,
        explorer.ignorers(),
        Arc::clone(hub),
        Arc::clone(own),
        env.status_hook(),
    );
    watcher.knows_folders(Box::new({
        let explorer = Arc::clone(explorer);
        move |path| explorer.holds_folder(path)
    }));
    // the tree is known before anything goes away (a window would list it now anyway; a core without one too)
    let explorer = Arc::clone(explorer);
    let _ = std::thread::Builder::new()
        .name("permark-tree".to_owned())
        .spawn(move || {
            let _ = explorer.listed("");
        });
    watcher
}

impl ProjectCore {
    /// Opens a project: state store, config, plugins started. Single step (no `activate`).
    #[must_use]
    pub fn open(root: Root, env: &CoreEnv) -> Arc<Self> {
        Self::build(root, env, None)
    }

    /// The same project for a reader with no window (`permark mcp` while the app is closed): the same services, built
    /// from the files; only the plugins in `only` that are on run, each started `files_only` (no state written of
    /// their own accord).
    #[must_use]
    pub fn from_files(root: Root, env: &CoreEnv, only: &[PluginId]) -> Arc<Self> {
        Self::build(root, env, Some(only.iter().cloned().collect()))
    }

    fn build(root: Root, env: &CoreEnv, files_only: Option<BTreeSet<PluginId>>) -> Arc<Self> {
        let id = ProjectId::of(&root);
        let store = StateStore::for_project(&env.dirs, &root);
        if files_only.is_none() {
            // the person's data 0.5 kept in its own profile folder, taken over once (a reader writes nothing)
            crate::state::older::take_over_project(&env.dirs, &root, &store);
        }
        let config = ProjectConfig::load(&root);
        // plugins that are on may offer extra roots (linked worktrees) before the project is built
        let (enablement, on) = project_enablement(env, &config, &store, files_only.as_ref());
        let extra_roots = env.registry.roots(&on, &root);
        let vault = Vault::new(root).with_worktrees(extra_roots);
        let hub = WatchHub::new();
        let own = Arc::new(OwnWrites::new());
        let explorer = Arc::new(Explorer::new(
            &vault,
            &config,
            Arc::clone(&env.sink),
            crate::tree::Ignorer::global_excludes().as_ref(),
        ));
        // the Explorer hears every batch first: the tree is current before plugins react
        let explorer_sub = hub.subscribe({
            let explorer = Arc::clone(&explorer);
            move |batch| explorer.apply_batch(batch)
        });
        let documents =
            Documents::for_project(DocScope::Project(vault.clone()), Arc::clone(&env.sink));
        let documents_sub = hub.subscribe({
            let documents = Arc::clone(&documents);
            move |batch| documents.on_batch(batch)
        });
        // the index walks the project at once and follows the watch from the first batch on
        let formats = Arc::new(RwLock::new(Arc::new(FormatRegistry::core())));
        let search = Arc::new(SearchIndex::start_with(
            vault.clone(),
            Arc::clone(&explorer),
            Arc::clone(&env.sink),
            vault.main().path().display().to_string(),
            Arc::clone(&formats),
        ));
        let search_sub = hub.subscribe(search.feeder());
        let watcher = start_watcher(&vault, &explorer, &hub, &own, env);
        // a reader with no window reads the journal, never writes it
        let activity = Arc::new(if files_only.is_some() {
            ActivityLog::read_only(&store, system_clock())
        } else {
            ActivityLog::open(&store, system_clock())
        });
        let activity_sub = files_only.is_none().then(|| {
            hub.subscribe({
                let activity = Arc::clone(&activity);
                move |batch| activity.outside(batch)
            })
        });
        let fileops = FileOps::new(
            vault.clone(),
            Arc::clone(&explorer),
            Arc::clone(&own),
            &store,
        )
        .with_activity(Arc::clone(&activity));
        let core = Arc::new(Self {
            _explorer_sub: explorer_sub,
            _documents_sub: documents_sub,
            _search_sub: search_sub,
            _activity_sub: activity_sub,
            search,
            watcher,
            fileops,
            activity,
            documents,
            formats,
            explorer,
            hub,
            id,
            store: RwLock::new(store),
            config: RwLock::new(config),
            vault,
            enablement: Mutex::new(BTreeMap::new()),
            plugins: Mutex::new(PluginSet::new(PluginScope::Project)),
            own,
            facts: ProjectFacts::new(env.status_hook()),
            rpc: RpcTable::new(),
            services: Services::new(),
            app_services: Arc::clone(env.services()),
            files_only,
        });
        core.start_plugins(env, enablement, &on);
        core
    }

    /// What plugins publish about the project (the git condition windows read).
    #[must_use]
    pub fn facts(&self) -> &Arc<ProjectFacts> {
        &self.facts
    }

    /// The methods running plugins serve (`plugin_call`).
    #[must_use]
    pub fn rpc(&self) -> &Arc<RpcTable> {
        &self.rpc
    }

    /// What a Project plugin gets: the composition every reader of the project shares (windows and `permark mcp`).
    #[must_use]
    pub fn services(&self) -> ProjectServices {
        self.project_services()
    }

    fn project_services(&self) -> ProjectServices {
        ProjectServices::new(
            Arc::clone(&self.documents),
            Arc::clone(&self.explorer),
            Arc::clone(&self.own),
            Arc::clone(&self.facts),
            Arc::clone(&self.services),
            Arc::clone(&self.search),
            Arc::clone(&self.app_services),
            Arc::clone(&self.activity),
            Arc::clone(&self.rpc),
            Arc::clone(&self.formats),
        )
    }

    /// The activity journal of the project.
    #[must_use]
    pub fn activity(&self) -> &Arc<ActivityLog> {
        &self.activity
    }

    /// The person saved `path` with `edits` (base lines): the journal notes the sections the change touched.
    pub fn note_save(&self, path: &str, edits: &[crate::edit::LineEdit]) {
        let Ok(parsed) = crate::paths::VaultPath::parse(path) else {
            return;
        };
        let Ok(abs) = self.vault.resolve(&parsed) else {
            return;
        };
        let text = crate::fsx::read_text(&abs, crate::activity::SAVE_READ_CAP)
            .map(|d| d.text)
            .unwrap_or_default();
        self.activity
            .person(crate::activity::save_fact(path, &text, edits));
    }

    /// Reads documents and tells windows when the ones they show change.
    #[must_use]
    pub fn documents(&self) -> &Arc<Documents> {
        &self.documents
    }

    /// The formats of the plugins that are on for this project.
    #[must_use]
    pub fn formats(&self) -> Arc<FormatRegistry> {
        Arc::clone(&self.formats.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// The search index (sections, file list, links).
    #[must_use]
    pub fn search(&self) -> &SearchIndex {
        &self.search
    }

    /// The path filter a search asks for: `tags` keep the files that have all of them (the `tags` plugin answers),
    /// silenced paths are left out unless `include_silenced` (the `quiet` plugin answers; the agent never asks).
    ///
    /// # Errors
    /// `Unsupported` (subject `tags`): tags were asked for and the tags plugin is not on.
    pub fn search_filter(&self, options: &SearchOptions) -> Result<SearchFilter, crate::IpcError> {
        let mut filter = SearchFilter {
            hidden_last: options.hidden_last,
            ..SearchFilter::default()
        };
        if !options.tags.is_empty() {
            let api = self.services.get::<TagsApi>().ok_or_else(|| {
                crate::IpcError::new(crate::ErrorKind::Unsupported).subject("tags")
            })?;
            filter.only = Some(api.0.files_with_all(&options.tags));
        }
        if !options.include_silenced {
            filter.muted = self.services.get::<SilenceApi>().map(|api| api.0.clone());
        }
        Ok(filter)
    }

    /// The project tree service.
    #[must_use]
    pub fn explorer(&self) -> &Arc<Explorer> {
        &self.explorer
    }

    /// File operations (move, rename, delete, new, undo).
    #[must_use]
    pub fn fileops(&self) -> &FileOps {
        &self.fileops
    }

    /// File watch: own writes, polled documents, status.
    #[must_use]
    pub fn watcher(&self) -> &Watcher {
        &self.watcher
    }

    /// Whether changes of the project files reach the app as they happen.
    #[must_use]
    pub fn live_status(&self) -> LiveStatus {
        self.watcher.status()
    }

    #[must_use]
    pub fn id(&self) -> &ProjectId {
        &self.id
    }

    #[must_use]
    pub fn root(&self) -> &Root {
        self.vault.main()
    }

    #[must_use]
    pub fn vault(&self) -> &Vault {
        &self.vault
    }

    /// The project's state store now (a clone: it moves when PerMark is initialized in the folder).
    #[must_use]
    pub fn store(&self) -> StateStore {
        self.store
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The project's shared settings now.
    #[must_use]
    pub fn config(&self) -> ProjectConfig {
        self.config
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    #[must_use]
    pub fn state_location(&self) -> StateLocation {
        self.store().location()
    }

    /// PerMark was initialized in this folder: the state folder and the config are read again, the file operations'
    /// journal moves, the project plugins start over on the new state, the tree is walked again. The caller tells the
    /// windows (`WorkspaceRegistry::refresh`).
    pub fn reload_state(&self, env: &CoreEnv) {
        let store = StateStore::for_project(&env.dirs, self.vault.main());
        let config = ProjectConfig::load(self.vault.main());
        *self.store.write().unwrap_or_else(PoisonError::into_inner) = store.clone();
        *self.config.write().unwrap_or_else(PoisonError::into_inner) = config.clone();
        self.fileops.use_store(&store);
        self.activity.use_store(&store);
        self.explorer.reconfigure(&config);
        self.plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stop_all();
        self.apply_plugins(env);
        self.rescan();
    }

    /// One setting of `.permark/config.toml` changed: written surgically, then live (the Explorer's default filter, the
    /// ignore rules; the agent reads the file at each call).
    ///
    /// # Errors
    /// `NotInitialized`, an invalid value, an IO failure.
    pub fn change_config(
        &self,
        change: &ProjectSettingChange,
    ) -> Result<ProjectConfig, SettingsError> {
        let config = ProjectConfigFile::of(self.vault.main()).apply(change)?;
        *self.config.write().unwrap_or_else(PoisonError::into_inner) = config.clone();
        self.explorer.reconfigure(&config);
        self.rescan();
        Ok(config)
    }

    /// Everything that keeps a picture of the disk builds it again (the rules that decide what is listed changed).
    fn rescan(&self) {
        self.hub.publish(&FsBatch {
            rescan: true,
            bulk: true,
            ..FsBatch::default()
        });
    }

    /// The person's own choice for plugin `id` in this project (`None`: follow the app), kept in the profile; the
    /// project's plugins are brought to it.
    ///
    /// # Errors
    /// The choices file cannot be written.
    pub fn set_plugin_choice(
        &self,
        env: &CoreEnv,
        id: &PluginId,
        choice: Option<bool>,
    ) -> Result<(), crate::state::StateError> {
        let file = self
            .store()
            .profile_file::<ProjectPluginChoices>(PLUGIN_CHOICES_FILE);
        let mut choices = file.load().into_value();
        match choice {
            Some(on) => {
                choices.choices.insert(id.clone(), on);
            }
            None => {
                choices.choices.remove(id);
            }
        }
        file.save(&choices)?;
        self.apply_plugins(env);
        Ok(())
    }

    /// The person's per-project plugin choices.
    #[must_use]
    pub fn plugin_choices(&self) -> BTreeMap<PluginId, bool> {
        self.store()
            .profile_file::<ProjectPluginChoices>(PLUGIN_CHOICES_FILE)
            .load()
            .into_value()
            .choices
    }

    /// Enablement with this project's overrides.
    #[must_use]
    pub fn enablement(&self) -> BTreeMap<PluginId, Enablement> {
        self.enablement
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Plugins on for this project (app + project scope).
    #[must_use]
    pub fn plugins_on(&self) -> BTreeSet<PluginId> {
        crate::plugins::on_ids(&self.enablement())
    }

    #[must_use]
    pub fn running_plugins(&self) -> Vec<PluginId> {
        self.plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .running_ids()
    }

    /// Re-resolves enablement (settings | choices changed) and starts | stops project plugins to match.
    pub fn apply_plugins(&self, env: &CoreEnv) {
        let (enablement, on) =
            project_enablement(env, &self.config(), &self.store(), self.files_only.as_ref());
        self.start_plugins(env, enablement, &on);
    }

    /// Stops plugin `id` and its dependents, then starts them again (a setting of `id` changed).
    pub fn restart_plugin(&self, env: &CoreEnv, id: &PluginId) {
        self.plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stop_with_dependents(&env.registry, id);
        self.apply_plugins(env);
    }

    fn start_plugins(
        &self,
        env: &CoreEnv,
        enablement: BTreeMap<PluginId, Enablement>,
        on: &BTreeSet<PluginId>,
    ) {
        let settings = env.settings();
        let store = self.store();
        let config = self.config();
        *self
            .enablement
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = enablement;
        *self.formats.write().unwrap_or_else(PoisonError::into_inner) =
            Arc::new(FormatRegistry::build(&env.registry.contributions(on)));
        // config files are searched by their keys while a config format is on
        self.search.formats_changed();
        let mut context = Context::welcome(env.distribution);
        context.workspace = crate::conditions::WorkspaceKind::Project;
        context.plugins.clone_from(on);
        let failures = self
            .plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .apply(&env.registry, on, |id| {
                let table =
                    merged_table(settings.plugin_tables.get(id), config.plugin_tables.get(id));
                let cx = PluginCx::new(
                    id.clone(),
                    CxWorkspace::Project(self.vault.clone()),
                    store.plugin_dir(id),
                    table,
                    context.clone(),
                    Arc::clone(&env.sink),
                )
                .with_watch(Arc::clone(&self.hub))
                .with_project(self.project_services())
                .with_store(store.clone());
                if self.files_only.is_some() {
                    cx.with_files_only()
                } else {
                    cx
                }
            });
        for failure in failures {
            env.sink.emit(CoreEvent::PluginFailed {
                message: failure.to_string(),
            });
        }
    }
}

/// The plugin's table: the app's keys, the project's keys over them.
fn merged_table(
    app: Option<&crate::settings::RawTable>,
    project: Option<&crate::settings::RawTable>,
) -> Option<toml_edit::Table> {
    match (app, project) {
        (None, None) => None,
        (Some(table), None) | (None, Some(table)) => Some(table.0.clone()),
        (Some(app), Some(project)) => {
            let mut merged = app.0.clone();
            for (key, item) in &project.0 {
                merged.insert(key, item.clone());
            }
            Some(merged)
        }
    }
}

/// Enablement of every plugin for a project (app settings, the project's config, the person's choices) and the
/// ids that are on.
fn project_enablement(
    env: &CoreEnv,
    config: &ProjectConfig,
    store: &StateStore,
    only: Option<&BTreeSet<PluginId>>,
) -> (BTreeMap<PluginId, Enablement>, BTreeSet<PluginId>) {
    let choices = store
        .profile_file::<ProjectPluginChoices>(PLUGIN_CHOICES_FILE)
        .load()
        .into_value();
    let inputs = EnableInputs {
        app: env.settings().plugins,
        project_config_off: config.plugins_off.clone(),
        project_choice: choices.choices,
    };
    let enablement = resolve_enabled(&env.registry, &inputs);
    let mut on = crate::plugins::on_ids(&enablement);
    if let Some(only) = only {
        on.retain(|id| only.contains(id));
    }
    (enablement, on)
}

impl Drop for ProjectCore {
    fn drop(&mut self) {
        // the next action starts a new session
        self.activity.closed();
        // explicit: plugins stop before vault + store go (reverse dependency order inside the set)
        self.plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stop_all();
    }
}
