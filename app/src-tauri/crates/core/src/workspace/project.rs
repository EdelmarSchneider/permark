//! `ProjectCore`: everything one open folder needs, shared by every window on it. Built in one step; Drop tears
//! down its plugins in reverse dependency order.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, RwLock};

use serde::{Deserialize, Serialize};

use crate::activity::{system_clock, ActivityLog};
use crate::conditions::{Context, Distribution};
use crate::document::{DocScope, Documents};
use crate::events::{CoreEvent, EventSink, PluginFailed, ProjectConfigChanged};
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
use crate::projects::{
    CardChange, CardDto, CardTarget, IncludeOutcome, LocateRefusal, OwnersChange, ProjectPick,
    Projects, ProjectsSubscription, ProjectsUndone, ProjectsView,
};
use crate::search::{SearchFilter, SearchIndex, SearchOptions};
use crate::settings::{
    AppSettings, ConfigRead, ConfigUnreadable, ProjectConfig, ProjectConfigFile,
    ProjectSettingChange, SettingsError,
};
use crate::state::location::DataOffer;
use crate::state::{AppDirs, ProjectId, Sharing, StateData, StateLocation, StateStore};
use crate::tree::Explorer;
use crate::watch::{
    ChangeKind, FsBatch, LiveStatus, OwnWrites, PathHold, StatusHook, Subscription, WatchHub,
    Watcher,
};

/// Process-wide services every workspace uses.
pub struct CoreEnv {
    pub dirs: AppDirs,
    pub registry: Arc<PluginRegistry>,
    pub sink: Arc<dyn EventSink>,
    pub distribution: Distribution,
    settings: Arc<RwLock<AppSettings>>,
    app_plugins: Mutex<PluginSet>,
    /// What App-scope plugins serve (the Notes tab and the welcome screen call it; no project needed).
    rpc: Arc<RpcTable>,
    /// Typed APIs App-scope plugins hand to core and to each other.
    services: Arc<Services>,
    /// Where the Notes tab's documents are (the `notes` plugin fills it).
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

    /// Where the Notes tab's documents are.
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
            self.sink
                .emit(CoreEvent::PluginFailed(PluginFailed::new(failure, None)));
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
    _projects_sub: Subscription,
    _documents_sub: Subscription,
    _search_sub: Subscription,
    _activity_sub: Subscription,
    /// Follows `.permark/config.toml` on disk ([`ProjectCore::follow_config`]).
    config_sub: Mutex<Option<Subscription>>,
    /// The journal follows where each included project keeps its state (`projects.md` §9.2).
    _activity_owners: ProjectsSubscription,
    search: Arc<SearchIndex>,
    watcher: Watcher,
    fileops: FileOps,
    /// The person's actions + the changes made outside (one journal for every window on the folder).
    activity: Arc<ActivityLog>,
    documents: Arc<Documents>,
    formats: Arc<RwLock<Arc<FormatRegistry>>>,
    /// The plugins `formats` was built from: the index reads config files again only when this set changes.
    formats_on: Mutex<BTreeSet<PluginId>>,
    explorer: Arc<Explorer>,
    /// Projects inside the folder (record, membership, owner map).
    projects: Arc<Projects>,
    hub: Arc<WatchHub>,
    id: ProjectId,
    vault: Vault,
    /// Where the project's state lives; moves when PerMark is initialized in the folder (`reload_state`).
    store: RwLock<StateStore>,
    /// What the window offers about the place of the person's data, worked out with `store` (and again on Keep): views
    /// read it, never the disk.
    data_offer: RwLock<DataOffer>,
    config: RwLock<ProjectConfig>,
    /// One change of `config` at a time (a setting written here, the file changed on disk, Initialize): each reads the
    /// file and applies what it read in one turn, so an older read never lands over a newer one.
    config_turn: Mutex<()>,
    /// A read of the team file is waiting for its turn ([`ProjectCore::follow_config`]): it reads after every report
    /// so far, so reports meanwhile start no other.
    config_pending: Arc<AtomicBool>,
    /// Tests: reads of the team file done ([`ProjectCore::reread_config`]).
    #[cfg(test)]
    config_reads: std::sync::atomic::AtomicUsize,
    enablement: Mutex<BTreeMap<PluginId, Enablement>>,
    plugins: Mutex<PluginSet>,
    // what the plugins share; they outlive the plugins (stopped first in Drop)
    own: Arc<OwnWrites>,
    facts: Arc<ProjectFacts>,
    rpc: Arc<RpcTable>,
    services: Arc<Services>,
    app_services: Arc<Services>,
    /// Agent files at the root, read once at load (`Context.agent_files`).
    agent_files: Vec<String>,
}

impl std::fmt::Debug for ProjectCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectCore")
            .field("root", &self.vault.main().path())
            .finish_non_exhaustive()
    }
}

/// The project's watch, told what the tree holds; the tree is built at once in the background.
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
    watcher.knows_tree(Box::new({
        let explorer = Arc::clone(explorer);
        move |path| {
            // `holds` never walks. It is true for every directory, including a container. Only an opened ignored
            // dir lets a new ignored child through.
            let folder = explorer.holds(path)?;
            if !folder {
                return Some(PathHold::File);
            }
            Some(if explorer.opened_ignored(path) {
                PathHold::OpenedIgnored
            } else {
                PathHold::Dir
            })
        }
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
    #[allow(clippy::too_many_lines)] // the composition root: every service built and wired in one place
    pub fn open(root: Root, env: &CoreEnv) -> Arc<Self> {
        let id = ProjectId::of(&root);
        // where the person's data sits is settled before anything reads or writes it: a first open records it, so a
        // `.permark/` that arrives later never moves it
        let store = crate::state::location::settle(&env.dirs, &root);
        // the person's data 0.5 kept in its own profile folder, taken over once
        crate::state::older::take_over_project(&env.dirs, &root, &store);
        // a context file for scripts that came with the folder goes before a plugin starts, whatever the agent
        // connection's state: scripts would take it for the person's live context
        crate::state::context::remove_foreign(&store);
        let data_offer = crate::state::location::offer(&store);
        let config = ProjectConfig::load(&root);
        // plugins that are on may offer extra roots (linked worktrees) before the project is built
        let (enablement, on) = project_enablement(env, &config, &store);
        let extra_roots = env.registry.roots(&on, &root);
        let vault = Vault::new(root).with_worktrees(extra_roots);
        // a few name checks at the root, no walk (Live's default + the agent offer read them)
        let agent_files = super::agent_files(&vault);
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
        // the included projects shape the walk: read before it starts, never here (open costs the same for any record)
        let projects = Projects::open(
            vault.main(),
            &env.dirs,
            Arc::clone(&explorer),
            Arc::clone(&own),
            Arc::clone(&hub),
            Arc::clone(&env.sink),
            &config.ignore,
        );
        let projects_sub = hub.subscribe({
            let projects = Arc::clone(&projects);
            move |batch| projects.watch(batch)
        });
        let documents =
            Documents::for_project(DocScope::Project(vault.clone()), Arc::clone(&env.sink));
        let documents_sub = hub.subscribe({
            let documents = Arc::clone(&documents);
            move |batch| documents.on_batch(batch)
        });
        // the index walks the project at once and follows the watch from the first batch on; it starts with the
        // formats of the plugins that are on, so config keys are in the first build (no window where the index says
        // ready and config files are still unread)
        let formats = Arc::new(RwLock::new(Arc::new(FormatRegistry::build(
            &env.registry.contributions(&on),
        ))));
        let search = Arc::new(SearchIndex::start_with(
            vault.clone(),
            Arc::clone(&explorer),
            Arc::clone(&env.sink),
            vault.main().path().display().to_string(),
            Arc::clone(&formats),
        ));
        // front matter tags from the index pass: cached per owning project, shown at once next open
        {
            let weak = Arc::downgrade(&projects);
            let fallback = store.clone();
            search.harvest().use_cache(Some(Arc::new(move || {
                weak.upgrade().map_or_else(
                    || crate::projects::Owners::root_only(fallback.clone()),
                    |p| p.owners(),
                )
            })));
        }
        let search_sub = hub.subscribe(search.feeder());
        let watcher = start_watcher(&vault, &explorer, &hub, &own, env);
        projects.start();
        let activity = Arc::new(ActivityLog::open(&store, system_clock()));
        let activity_sub = hub.subscribe({
            let activity = Arc::clone(&activity);
            move |batch| activity.outside(batch)
        });
        // the journal waits (facts kept) until the worker tells the owners; told already -> now (subscribed first:
        // a `Resume` between the check and the subscription is never missed; a second one changes nothing)
        let activity_owners = projects.follow_owners({
            let activity = Arc::downgrade(&activity);
            move |change| {
                if let Some(activity) = activity.upgrade() {
                    activity.follow_owners(&change);
                }
            }
        });
        if projects.owners_known() {
            activity.follow_owners(&OwnersChange::Resume(&projects.owners()));
        }
        let fileops = FileOps::new(
            vault.clone(),
            Arc::clone(&explorer),
            Arc::clone(&own),
            &store,
        )
        .with_activity(Arc::clone(&activity))
        .with_formats(Arc::clone(&formats));
        let core = Arc::new(Self {
            _explorer_sub: explorer_sub,
            _projects_sub: projects_sub,
            _documents_sub: documents_sub,
            _search_sub: search_sub,
            _activity_sub: activity_sub,
            config_sub: Mutex::new(None),
            _activity_owners: activity_owners,
            search,
            watcher,
            fileops,
            activity,
            documents,
            formats,
            formats_on: Mutex::new(on.clone()),
            explorer,
            projects,
            hub,
            id,
            store: RwLock::new(store),
            data_offer: RwLock::new(data_offer),
            config: RwLock::new(config),
            config_turn: Mutex::new(()),
            config_pending: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            config_reads: std::sync::atomic::AtomicUsize::new(0),
            vault,
            enablement: Mutex::new(BTreeMap::new()),
            plugins: Mutex::new(PluginSet::new(PluginScope::Project)),
            own,
            facts: ProjectFacts::new(env.status_hook()),
            rpc: RpcTable::new(),
            services: Services::new(),
            app_services: Arc::clone(env.services()),
            agent_files,
        });
        {
            let mut plugins = core.plugins.lock().unwrap_or_else(PoisonError::into_inner);
            core.start_plugins(env, &mut plugins, enablement, &on);
        }
        core
    }

    /// The agent files found at the root when the project was opened (`AGENT_FILES` order).
    #[must_use]
    pub fn agent_files(&self) -> &[String] {
        &self.agent_files
    }

    /// Tests: the watch hub of the project (a test subscriber hears each batch after the project's own parts).
    #[cfg(test)]
    pub(crate) fn hub(&self) -> &Arc<WatchHub> {
        &self.hub
    }

    /// Tests: how many reads of the team file are done.
    #[cfg(test)]
    pub(crate) fn config_reads(&self) -> usize {
        self.config_reads.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Tests: the config turn, held until the guard goes (a read of the team file waits meanwhile).
    #[cfg(test)]
    pub(crate) fn hold_config_turn(&self) -> MutexGuard<'_, ()> {
        self.config_turn()
    }

    /// Tests: the plugin set, held until the guard goes (starting | stopping plugins waits meanwhile).
    #[cfg(test)]
    pub(crate) fn hold_plugins(&self) -> MutexGuard<'_, PluginSet> {
        self.plugins.lock().unwrap_or_else(PoisonError::into_inner)
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
        .with_projects(Arc::clone(&self.projects))
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
        if let Some(uid) = &options.project {
            // "This project": the member's subtree, projects inside it included
            self.projects.load();
            let map = self.projects.snapshot();
            let member = map.member(uid).ok_or_else(|| {
                crate::IpcError::new(crate::ErrorKind::NotFound).subject(uid.as_str().to_owned())
            })?;
            filter.within = Some(member.key.clone());
        }
        Ok(filter)
    }

    /// The project tree service.
    #[must_use]
    pub fn explorer(&self) -> &Arc<Explorer> {
        &self.explorer
    }

    /// Projects inside the folder.
    #[must_use]
    pub fn projects(&self) -> &Arc<Projects> {
        &self.projects
    }

    /// Settings > Projects + the strip.
    #[must_use]
    pub fn projects_view(&self) -> ProjectsView {
        self.projects.view()
    }

    /// Includes projects; a folder without `.permark/` is initialized first (`projects.md` §2.4: `.permark/` alone
    /// would move the state). Returns whether PerMark was initialized (the caller tells the windows).
    ///
    /// # Errors
    /// `Unavailable` (record unreadable | newer), init failure, `Conflict`, IO.
    pub fn projects_include(
        &self,
        env: &CoreEnv,
        pick: &ProjectPick,
    ) -> Result<(IncludeOutcome, bool), crate::IpcError> {
        self.projects.include(pick, &|| self.initialize(env))
    }

    /// Initializes PerMark in this folder ([`crate::state::init_project`]): the plugins stop first, so what they write
    /// last lands in the place the person's data is copied from; then everything reads the state again from the new
    /// place ([`ProjectCore::reload_state`]). A refusal starts the plugins again on the place they had. The caller
    /// tells the windows.
    ///
    /// # Errors
    /// What `init_project` refuses: both places hold data (`Conflict`), a link or a file in the way, IO.
    pub fn initialize(&self, env: &CoreEnv) -> Result<(), crate::IpcError> {
        // one config turn from the stop to the reload: a read of the team file meanwhile would start the plugins again
        // on the place being copied from
        let turn = self.config_turn();
        self.plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stop_all();
        if let Err(error) = crate::state::init_project(&env.dirs, self.vault.main()) {
            self.apply_plugins(env);
            return Err(error.into());
        }
        self.reload_state_in(&turn, env);
        Ok(())
    }

    /// Takes back the last include (`token`); a `.permark/` it made goes when untouched (the state goes back to the
    /// profile). Returns the outcome + whether the state moved (the caller tells the windows).
    ///
    /// # Errors
    /// Record | profile write failures.
    pub fn projects_undo(
        &self,
        env: &CoreEnv,
        token: u64,
    ) -> Result<(ProjectsUndone, bool), crate::IpcError> {
        let plan = self.projects.undo(token)?;
        if !plan.undone || !plan.created_permark {
            return Ok((
                ProjectsUndone {
                    undone: plan.undone,
                    permark_kept: false,
                },
                false,
            ));
        }
        // plugins flush their files into `.permark/local/` first; the state then goes back to the profile (one config
        // turn throughout: a read of the team file meanwhile would start them again on the place being removed)
        let turn = self.config_turn();
        self.plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stop_all();
        let removed = crate::state::uninit_project(&env.dirs, self.vault.main()).unwrap_or(false);
        if removed {
            self.reload_state_in(&turn, env);
        } else {
            self.apply_plugins(env);
        }
        drop(turn);
        Ok((
            ProjectsUndone {
                undone: true,
                permark_kept: !removed,
            },
            removed,
        ))
    }

    /// Locate… for a missing project (`abs`: the folder the person picked).
    ///
    /// # Errors
    /// Record write failures.
    pub fn projects_locate(
        &self,
        uid: &crate::projects::ProjectUid,
        abs: &std::path::Path,
    ) -> Result<Option<LocateRefusal>, crate::IpcError> {
        self.projects.locate(uid, abs)
    }

    /// One field of a card (the folder's own | a member's).
    ///
    /// # Errors
    /// Card write failures.
    pub fn projects_card_set(
        &self,
        target: &CardTarget,
        change: &CardChange,
    ) -> Result<CardDto, crate::IpcError> {
        self.projects.card_set(target, change)
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
        self.store
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .location()
    }

    /// Whether the folder had a team folder when its state was last read (open, Initialize, a reload): a `.permark/`
    /// that arrives meanwhile counts from the next read.
    #[must_use]
    pub fn has_team_folder(&self) -> bool {
        self.store
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .has_team_folder()
    }

    /// What the window offers about the place of the person's data ([`crate::state::location::offer`]), as worked out
    /// when the state was last read | the data was kept.
    #[must_use]
    pub fn data_offer(&self) -> DataOffer {
        *self
            .data_offer
            .read()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The person keeps the folder's data on this computer ([`crate::state::location::keep`]): the place and the plugins
    /// stay as they are (the store already reads the profile), only the offer is worked out again. The caller tells
    /// the windows.
    ///
    /// # Errors
    /// The store reads the folder's own `.permark/local/` (a conflict, nothing written); the profile's row file cannot
    /// be written.
    pub fn keep_data(&self) -> Result<(), crate::IpcError> {
        let store = self.store();
        crate::state::location::keep(&store)?;
        *self
            .data_offer
            .write()
            .unwrap_or_else(PoisonError::into_inner) = crate::state::location::offer(&store);
        Ok(())
    }

    /// PerMark was initialized in this folder: the state folder and the config are read again, the file operations'
    /// journal moves, the project plugins start over on the new state, the tree is walked again. The caller tells the
    /// windows (`WorkspaceRegistry::refresh`).
    pub fn reload_state(&self, env: &CoreEnv) {
        let turn = self.config_turn();
        self.reload_state_in(&turn, env);
    }

    /// The config turn, held until the guard goes: one change of the config at a time.
    fn config_turn(&self) -> MutexGuard<'_, ()> {
        self.config_turn
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// [`ProjectCore::reload_state`] inside the config turn the caller holds (`_turn`: proof of it).
    fn reload_state_in(&self, _turn: &MutexGuard<'_, ()>, env: &CoreEnv) {
        let store = StateStore::for_project(&env.dirs, self.vault.main());
        let config = ProjectConfig::load(self.vault.main());
        *self
            .data_offer
            .write()
            .unwrap_or_else(PoisonError::into_inner) = crate::state::location::offer(&store);
        *self.store.write().unwrap_or_else(PoisonError::into_inner) = store.clone();
        *self.config.write().unwrap_or_else(PoisonError::into_inner) = config.clone();
        self.fileops.use_store(&store);
        self.activity.use_store(&store);
        self.explorer.reconfigure(&config);
        self.plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stop_all();
        // plugins keeping state per owner start on the folder's new place, not the one the owners had
        self.projects.reload_owners();
        self.apply_plugins(env);
        self.rescan();
    }

    /// One setting of `.permark/config.toml` changed: written surgically, then live (the Explorer's default filter, the
    /// ignore rules, the plugins turned off for everyone; the agent reads the file at each call).
    ///
    /// # Errors
    /// `NotInitialized`, an invalid value, the file is not TOML (refused, untouched), an IO failure.
    pub fn change_config(
        &self,
        env: &CoreEnv,
        change: &ProjectSettingChange,
    ) -> Result<ProjectConfig, SettingsError> {
        let _turn = self.config_turn();
        let before = self.config();
        let config = ProjectConfigFile::of(self.vault.main()).apply(change)?;
        self.use_config(env, &before, &config);
        Ok(config)
    }

    /// Follows `.permark/config.toml` on disk while the folder is open: a change made outside (a git pull, a hand edit,
    /// the config sheet) applies as a setting written here does. A batch that names the file or its folder, or asks to
    /// rebuild (lost reports, Refresh), has it read on a thread of its own: starting | stopping plugins is no work for
    /// the watch thread, and a read never waits there on a write of this core in progress. One read waiting for its
    /// turn serves every report until it starts. Once listening, the file is read again: a change between the open's
    /// read and the subscription applies now, not at the next report.
    pub fn follow_config(self: &Arc<Self>, env: &Arc<CoreEnv>) {
        let core = Arc::downgrade(self);
        let weak_env = Arc::downgrade(env);
        let pending = Arc::clone(&self.config_pending);
        let keys = [
            crate::paths::key_of(&format!("{}/config.toml", crate::state::PERMARK_DIR)),
            crate::paths::key_of(crate::state::PERMARK_DIR),
        ];
        let team_file = move |path: &str| keys.contains(&crate::paths::key_of(path));
        let subscription = self.hub.subscribe(move |batch| {
            let touched = batch.rescan
                || batch.changes.iter().any(|change| {
                    team_file(&change.path)
                        || matches!(&change.kind, ChangeKind::Renamed { from } if team_file(from))
                });
            if !touched || pending.swap(true, Ordering::SeqCst) {
                return;
            }
            let (core, env) = (core.clone(), weak_env.clone());
            let spawned = std::thread::Builder::new()
                .name("permark-config".to_owned())
                .spawn(move || {
                    if let (Some(core), Some(env)) = (core.upgrade(), env.upgrade()) {
                        core.reread_config(&env);
                    }
                });
            if spawned.is_err() {
                pending.store(false, Ordering::SeqCst);
            }
        });
        *self
            .config_sub
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(subscription);
        self.reread_config(env);
    }

    /// Reads `.permark/config.toml` again and applies what differs from what applied so far, by the rules of an open
    /// (the app's own write and the same bytes again differ in nothing: no work). A file that is not TOML keeps what
    /// applied, so a broken file never lifts what the team narrowed, and says where it stops being TOML; one that
    /// cannot be read now (locked, too large, not text, a linked `.permark`) changes nothing.
    pub(crate) fn reread_config(&self, env: &CoreEnv) {
        let _turn = self.config_turn();
        // a report from now on needs a read after this one: what changes during it may be missed by it
        self.config_pending.store(false, Ordering::SeqCst);
        let before = self.config();
        let read = ProjectConfig::load_checked(self.vault.main());
        #[cfg(test)]
        self.config_reads
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let after = match read {
            ConfigRead::Read(_, config) => config,
            ConfigRead::Absent => ProjectConfig::default(),
            ConfigRead::Unreadable(ConfigUnreadable::NotToml { line }) => ProjectConfig {
                unreadable_line: Some(line),
                ..before.clone()
            },
            ConfigRead::Unreadable(_) => return,
        };
        self.use_config(env, &before, &after);
    }

    /// `after` becomes the config (`before`: the one that applied until now) and what follows it is brought along: the
    /// plugins turned off for everyone, the Explorer's default filter + ignore rules (the folder read again); windows
    /// are told. Called inside the config turn.
    fn use_config(&self, env: &CoreEnv, before: &ProjectConfig, after: &ProjectConfig) {
        if before == after {
            return;
        }
        *self.config.write().unwrap_or_else(PoisonError::into_inner) = after.clone();
        if before.plugins_off != after.plugins_off {
            self.apply_plugins(env);
            env.sink.emit(CoreEvent::PluginsChanged);
            // the windows' contexts name the plugins that are on
            if let Some(hook) = env.status_hook() {
                hook();
            }
        }
        if before.tree != after.tree || before.ignore != after.ignore {
            self.explorer.reconfigure(after);
            self.rescan();
        }
        env.sink
            .emit(CoreEvent::ProjectConfigChanged(ProjectConfigChanged {
                root: self.root().path().display().to_string(),
            }));
    }

    /// What git may see of the folder's `.permark/` ([`crate::state::set_sharing`]): a rewritten ignore file reaches the
    /// tree at once (its rules decide what is listed as ignored), not only when the watcher reports it.
    ///
    /// # Errors
    /// What `set_sharing` refuses: no real team folder, someone else's ignore file, IO.
    pub fn set_sharing(&self, to: Sharing) -> Result<Sharing, crate::state::StateError> {
        let before = crate::state::sharing(self.vault.main());
        let now = crate::state::set_sharing(self.vault.main(), to)?;
        if now != before {
            self.explorer
                .sync_paths(&[format!("{}/.gitignore", crate::state::PERMARK_DIR)], &[]);
        }
        Ok(now)
    }

    /// The person asked to read the folder again: the tree is walked, and the index, git and every plugin that keeps a
    /// picture of the disk look at it again, as after lost watch reports. Returns at once; the windows are told as each
    /// part is done.
    pub fn refresh(&self) {
        self.watcher.rescan();
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
        let mut plugins = self.plugins.lock().unwrap_or_else(PoisonError::into_inner);
        self.apply_plugins_to(env, &mut plugins);
    }

    /// Stops plugin `id` and its dependents, then starts them again (a setting of `id` changed).
    pub fn restart_plugin(&self, env: &CoreEnv, id: &PluginId) {
        let mut plugins = self.plugins.lock().unwrap_or_else(PoisonError::into_inner);
        plugins.stop_with_dependents(&env.registry, id);
        self.apply_plugins_to(env, &mut plugins);
    }

    /// Enablement worked out and applied in one hold of the plugin set: an apply that read older settings | config
    /// never lands after one that read newer ones (a plugin the person just turned off never starts again).
    fn apply_plugins_to(&self, env: &CoreEnv, plugins: &mut PluginSet) {
        let (enablement, on) = project_enablement(env, &self.config(), &self.store());
        self.start_plugins(env, plugins, enablement, &on);
    }

    fn start_plugins(
        &self,
        env: &CoreEnv,
        plugins: &mut PluginSet,
        enablement: BTreeMap<PluginId, Enablement>,
        on: &BTreeSet<PluginId>,
    ) {
        let settings = env.settings();
        let store = self.store();
        *self
            .enablement
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = enablement;
        // config files are searched by their keys while a config format is on: read again only when the plugins change
        let formats_changed = {
            let mut formats_on = self
                .formats_on
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let changed = *formats_on != *on;
            if changed {
                *self.formats.write().unwrap_or_else(PoisonError::into_inner) =
                    Arc::new(FormatRegistry::build(&env.registry.contributions(on)));
                formats_on.clone_from(on);
                self.search.formats_changed();
            }
            changed
        };
        let mut context = Context::welcome(env.distribution);
        context.workspace = crate::conditions::WorkspaceKind::Project;
        context.plugins.clone_from(on);
        let failures = plugins.apply(&env.registry, on, |id| {
            // the person's own settings only: a shared project file never sets a plugin's keys for everyone
            PluginCx::new(
                id.clone(),
                CxWorkspace::Project(self.vault.clone()),
                store.plugin_dir(id),
                settings.plugin_tables.get(id).map(|t| t.0.clone()),
                context.clone(),
                Arc::clone(&env.sink),
            )
            .with_watch(Arc::clone(&self.hub))
            .with_project(self.project_services())
            .with_store(store.clone())
        });
        // plugins that kept running are told, so what they read with the old formats (links of documents) is read again
        if formats_changed {
            plugins.formats_changed();
        }
        // named as the project's views name it (`WorkspaceView::Project.root`)
        let root = self.root().path().display().to_string();
        for failure in failures {
            env.sink.emit(CoreEvent::PluginFailed(PluginFailed::new(
                failure,
                Some(root.clone()),
            )));
        }
    }
}

/// Enablement of every plugin for a project (app settings, the project's config, the person's choices) and the
/// ids that are on.
fn project_enablement(
    env: &CoreEnv,
    config: &ProjectConfig,
    store: &StateStore,
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
    let on = crate::plugins::on_ids(&enablement);
    (enablement, on)
}

impl Drop for ProjectCore {
    fn drop(&mut self) {
        // the projects worker joins first: every member released
        self.projects.stop();
        // the next action starts a new session
        self.activity.closed();
        // explicit: plugins stop before vault + store go (reverse dependency order inside the set)
        self.plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stop_all();
    }
}
