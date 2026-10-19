//! `ProjectCore`: everything one open folder needs, shared by every window on it. Built in one step; Drop tears
//! down its plugins in reverse dependency order.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use serde::{Deserialize, Serialize};

use crate::conditions::{Context, Distribution};
use crate::events::{CoreEvent, EventSink};
use crate::fileops::FileOps;
use crate::paths::{Root, Vault};
use crate::plugins::{
    resolve_enabled, CxWorkspace, EnableInputs, Enablement, PluginCx, PluginId, PluginRegistry,
    PluginScope, PluginSet,
};
use crate::settings::{AppSettings, ProjectConfig};
use crate::state::{AppDirs, ProjectId, StateData, StateLocation, StateStore};
use crate::tree::Explorer;
use crate::watch::{LiveStatus, OwnWrites, StatusHook, Subscription, WatchHub, Watcher};

/// Process-wide services every workspace uses.
pub struct CoreEnv {
    pub dirs: AppDirs,
    pub registry: Arc<PluginRegistry>,
    pub sink: Arc<dyn EventSink>,
    pub distribution: Distribution,
    settings: RwLock<AppSettings>,
    app_plugins: Mutex<PluginSet>,
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
        Self {
            dirs,
            registry,
            sink,
            distribution,
            settings: RwLock::new(settings),
            app_plugins: Mutex::new(PluginSet::new(PluginScope::App)),
            status_hook: Mutex::new(None),
        }
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

    /// Starts | stops app plugins to match settings.
    pub fn apply_app_plugins(&self) {
        let on = crate::plugins::on_ids(&self.app_enablement());
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
    watcher: Watcher,
    fileops: FileOps,
    explorer: Arc<Explorer>,
    hub: Arc<WatchHub>,
    id: ProjectId,
    vault: Vault,
    store: StateStore,
    config: ProjectConfig,
    enablement: Mutex<BTreeMap<PluginId, Enablement>>,
    plugins: Mutex<PluginSet>,
}

impl std::fmt::Debug for ProjectCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectCore")
            .field("root", &self.vault.main().path())
            .finish_non_exhaustive()
    }
}

impl ProjectCore {
    /// Opens a project: state store, config, plugins started. Single step (no `activate`).
    #[must_use]
    pub fn open(root: Root, env: &CoreEnv) -> Arc<Self> {
        let id = ProjectId::of(&root);
        let store = StateStore::for_project(&env.dirs, &root);
        let config = ProjectConfig::load(&root);
        let vault = Vault::new(root);
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
        let watcher = Watcher::start(
            &vault,
            explorer.ignorers(),
            Arc::clone(&hub),
            Arc::clone(&own),
            env.status_hook(),
        );
        let fileops = FileOps::new(vault.clone(), Arc::clone(&explorer), own, &store);
        let core = Arc::new(Self {
            _explorer_sub: explorer_sub,
            watcher,
            fileops,
            explorer,
            hub,
            id,
            store,
            config,
            vault,
            enablement: Mutex::new(BTreeMap::new()),
            plugins: Mutex::new(PluginSet::new(PluginScope::Project)),
        });
        core.apply_plugins(env);
        core
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

    #[must_use]
    pub fn store(&self) -> &StateStore {
        &self.store
    }

    #[must_use]
    pub fn config(&self) -> &ProjectConfig {
        &self.config
    }

    #[must_use]
    pub fn state_location(&self) -> StateLocation {
        self.store.location()
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
        let settings = env.settings();
        let choices = self
            .store
            .profile_file::<ProjectPluginChoices>(PLUGIN_CHOICES_FILE)
            .load()
            .into_value();
        let inputs = EnableInputs {
            app: settings.plugins.clone(),
            project_config_off: self.config.plugins_off.clone(),
            project_choice: choices.choices,
        };
        let enablement = resolve_enabled(&env.registry, &inputs);
        let on = crate::plugins::on_ids(&enablement);
        *self
            .enablement
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = enablement;
        let mut context = Context::welcome(env.distribution);
        context.workspace = crate::conditions::WorkspaceKind::Project;
        context.plugins.clone_from(&on);
        let failures = self
            .plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .apply(&env.registry, &on, |id| {
                let table = self
                    .config
                    .plugin_tables
                    .get(id)
                    .or_else(|| settings.plugin_tables.get(id))
                    .map(|t| t.0.clone());
                PluginCx::new(
                    id.clone(),
                    CxWorkspace::Project(self.vault.clone()),
                    self.store.plugin_dir(id),
                    table,
                    context.clone(),
                    Arc::clone(&env.sink),
                )
                .with_watch(Arc::clone(&self.hub))
            });
        for failure in failures {
            env.sink.emit(CoreEvent::PluginFailed {
                message: failure.to_string(),
            });
        }
    }
}

impl Drop for ProjectCore {
    fn drop(&mut self) {
        // explicit: plugins stop before vault + store go (reverse dependency order inside the set)
        self.plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stop_all();
    }
}
