//! `WorkspaceRegistry`: windows -> workspaces -> shared project cores. Drives each window's FSM; publishes
//! `workspaces-changed`, `window-changed`, `context-changed` (only what differs).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use super::fsm::{holds, step, Effect, WindowEvent, WindowState};
use crate::document::{DocScope, Documents};

use super::{
    CoreEnv, FileSession, ProjectCore, WindowView, Workspace, WorkspaceError, WorkspaceId,
    WorkspaceKey, WorkspaceRef, WorkspaceRefDto, WorkspaceSummary, WorkspaceView,
};
use crate::conditions::{Context, LiveUpdates, PermarkCondition, WorkspaceKind};
use crate::error::{ErrorKind, IpcError};
use crate::events::{ContextChanged, CoreEvent, WindowChanged, WorkspacesChanged};
use crate::paths::{FileScope, RootKey, VaultPath};
use crate::state::StateLocation;

#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("another open in this window superseded this one")]
    Superseded,
    #[error("the window is closing")]
    Closing,
    #[error("unknown window {0}")]
    UnknownWindow(String),
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
}

impl From<OpenError> for IpcError {
    fn from(error: OpenError) -> Self {
        match error {
            OpenError::Superseded => IpcError::new(ErrorKind::Superseded),
            OpenError::Closing => IpcError::new(ErrorKind::Unavailable),
            OpenError::UnknownWindow(label) => IpcError::new(ErrorKind::NotFound).subject(label),
            OpenError::Workspace(e) => e.into(),
        }
    }
}

/// An open in progress: proves `begin_open` ran; consumed by `finish_open`.
#[derive(Debug)]
pub struct Ticket {
    window: String,
    number: u64,
    document: Option<VaultPath>,
}

/// A workspace built off-lock, waiting to be bound to its window.
#[derive(Debug)]
pub struct Prepared {
    reference: WorkspaceRef,
    workspace: Workspace,
}

#[derive(Debug)]
struct Slot {
    created: u64,
    state: WindowState,
    target: Option<WorkspaceRefDto>,
    document: Option<VaultPath>,
}

#[derive(Debug)]
struct Entry {
    reference: WorkspaceRef,
    key: WorkspaceKey,
    workspace: Workspace,
}

#[derive(Debug, Default)]
struct Published {
    list: Vec<WorkspaceSummary>,
    views: BTreeMap<String, WindowView>,
    contexts: BTreeMap<String, Context>,
}

#[derive(Debug, Default)]
struct Inner {
    windows: BTreeMap<String, Slot>,
    workspaces: BTreeMap<WorkspaceId, Entry>,
    next_window: u64,
    next_ticket: u64,
    next_id: u64,
    published: Published,
}

type CoreSlot = Arc<Mutex<Weak<ProjectCore>>>;

pub struct WorkspaceRegistry {
    env: Arc<CoreEnv>,
    inner: Mutex<Inner>,
    /// Per-root open slot: one core per root even when two windows open it at once.
    cores: Mutex<HashMap<RootKey, CoreSlot>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl WorkspaceRegistry {
    #[must_use]
    pub fn new(env: Arc<CoreEnv>) -> Self {
        Self {
            env,
            inner: Mutex::new(Inner::default()),
            cores: Mutex::new(HashMap::new()),
        }
    }

    /// A registry shared by the app: projects whose live updates change recompute the windows' contexts.
    #[must_use]
    pub fn new_shared(env: &Arc<CoreEnv>) -> Arc<Self> {
        Arc::new_cyclic(|weak: &std::sync::Weak<Self>| {
            let weak = weak.clone();
            env.set_status_hook(Arc::new(move || {
                if let Some(registry) = weak.upgrade() {
                    registry.refresh();
                }
            }));
            Self::new(Arc::clone(env))
        })
    }

    #[must_use]
    pub fn env(&self) -> &Arc<CoreEnv> {
        &self.env
    }

    /// A window was created (starts on Welcome). Known label -> no change.
    pub fn add_window(&self, label: &str) {
        let mut inner = lock(&self.inner);
        if !inner.windows.contains_key(label) {
            let created = inner.next_window;
            inner.next_window += 1;
            inner.windows.insert(
                label.to_owned(),
                Slot {
                    created,
                    state: WindowState::Welcome,
                    target: None,
                    document: None,
                },
            );
        }
        self.publish(&mut inner);
    }

    /// A window closed: released + forgotten.
    pub fn forget_window(&self, label: &str) {
        let mut inner = lock(&self.inner);
        let mut dropped = Vec::new();
        if let Some(slot) = inner.windows.get_mut(label) {
            let (state, effect) = step(slot.state, WindowEvent::Close);
            slot.state = state;
            inner.windows.remove(label);
            if let Effect::Release(id) = effect {
                dropped.extend(release(&mut inner, id, label));
            }
        }
        self.publish(&mut inner);
        drop(inner);
        drop(dropped);
        self.prune_cores();
    }

    /// Step 1: the window starts opening `reference` (older opens of it become stale).
    ///
    /// # Errors
    /// `UnknownWindow`, `Closing`.
    pub fn begin_open(
        &self,
        label: &str,
        reference: &WorkspaceRef,
        document: Option<VaultPath>,
    ) -> Result<Ticket, OpenError> {
        let mut inner = lock(&self.inner);
        inner.next_ticket += 1;
        let number = inner.next_ticket;
        let slot = inner
            .windows
            .get_mut(label)
            .ok_or_else(|| OpenError::UnknownWindow(label.to_owned()))?;
        let (state, effect) = step(slot.state, WindowEvent::Open { ticket: number });
        if effect == Effect::Refused {
            return Err(OpenError::Closing);
        }
        slot.state = state;
        slot.target = Some(reference.to_dto());
        self.publish(&mut inner);
        Ok(Ticket {
            window: label.to_owned(),
            number,
            document,
        })
    }

    /// Step 2 (no registry lock held): builds | reuses the workspace. Slow part.
    ///
    /// # Errors
    /// `NotesUnavailable`; file vanished (`Vault`).
    pub fn prepare(&self, reference: &WorkspaceRef) -> Result<Prepared, WorkspaceError> {
        let workspace = match reference {
            WorkspaceRef::Project(root) => {
                let slot = Arc::clone(lock(&self.cores).entry(root.key().clone()).or_default());
                let mut weak = lock(&slot);
                let core = if let Some(core) = weak.upgrade() {
                    core
                } else {
                    let core = ProjectCore::open(root.clone(), &self.env);
                    *weak = Arc::downgrade(&core);
                    core
                };
                Workspace::Project(core)
            }
            WorkspaceRef::SingleFile(path) => {
                let scope = FileScope::open(path)?;
                let documents =
                    Documents::for_file(DocScope::File(scope.clone()), Arc::clone(&self.env.sink));
                Workspace::SingleFile(FileSession { scope, documents })
            }
            WorkspaceRef::Notes => return Err(WorkspaceError::NotesUnavailable),
        };
        Ok(Prepared {
            reference: reference.clone(),
            workspace,
        })
    }

    /// Step 3: binds the prepared workspace if the ticket is still the window's latest.
    ///
    /// # Errors
    /// `Superseded` (newer open | window closing), `UnknownWindow`, the preparation error.
    pub fn finish_open(
        &self,
        ticket: Ticket,
        prepared: Result<Prepared, WorkspaceError>,
    ) -> Result<WorkspaceId, OpenError> {
        let mut inner = lock(&self.inner);
        let mut dropped: Vec<Entry> = Vec::new();
        let Some(slot_state) = inner.windows.get(&ticket.window).map(|s| s.state) else {
            drop(inner);
            drop(prepared);
            return Err(OpenError::UnknownWindow(ticket.window));
        };
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                let (state, _) = step(
                    slot_state,
                    WindowEvent::Failed {
                        ticket: ticket.number,
                    },
                );
                if let Some(slot) = inner.windows.get_mut(&ticket.window) {
                    if state != slot_state {
                        slot.target = None;
                    }
                    slot.state = state;
                }
                self.publish(&mut inner);
                drop(inner);
                return Err(error.into());
            }
        };
        let key = prepared.reference.key();
        let existing = inner
            .workspaces
            .iter()
            .find(|(_, e)| e.key == key)
            .map(|(id, _)| *id);
        let id = existing.unwrap_or_else(|| {
            inner.next_id += 1;
            WorkspaceId(inner.next_id)
        });
        let (state, effect) = step(
            slot_state,
            WindowEvent::Opened {
                ticket: ticket.number,
                id,
            },
        );
        let result = if effect == Effect::Stale {
            Err(OpenError::Superseded)
        } else {
            if existing.is_none() {
                inner.workspaces.insert(
                    id,
                    Entry {
                        reference: prepared.reference,
                        key,
                        workspace: prepared.workspace,
                    },
                );
            }
            if let Some(slot) = inner.windows.get_mut(&ticket.window) {
                slot.state = state;
                slot.target = None;
                slot.document = ticket.document;
            }
            if let Effect::Release(old) = effect {
                dropped.extend(release(&mut inner, old, &ticket.window));
            }
            Ok(id)
        };
        self.publish(&mut inner);
        drop(inner);
        // heavy teardown (plugins) outside the lock
        drop(dropped);
        self.prune_cores();
        result
    }

    /// `begin_open` + `prepare` + `finish_open`.
    ///
    /// # Errors
    /// See the three steps.
    pub fn open(
        &self,
        label: &str,
        reference: &WorkspaceRef,
        document: Option<VaultPath>,
    ) -> Result<WorkspaceId, OpenError> {
        let ticket = self.begin_open(label, reference, document)?;
        let prepared = self.prepare(reference);
        self.finish_open(ticket, prepared)
    }

    /// Window back to Welcome (its workspace released when no other window shows it).
    pub fn show_welcome(&self, label: &str) {
        let mut inner = lock(&self.inner);
        let mut dropped = Vec::new();
        if let Some(slot) = inner.windows.get_mut(label) {
            let (state, effect) = step(slot.state, WindowEvent::ShowWelcome);
            slot.state = state;
            slot.target = None;
            slot.document = None;
            if let Effect::Release(id) = effect {
                dropped.extend(release(&mut inner, id, label));
            }
        }
        self.publish(&mut inner);
        drop(inner);
        drop(dropped);
        self.prune_cores();
    }

    /// Sets the document hint of a window showing a project (open request for a file inside it).
    pub fn set_document(&self, label: &str, document: Option<VaultPath>) {
        let mut inner = lock(&self.inner);
        if let Some(slot) = inner.windows.get_mut(label) {
            slot.document = document;
        }
        self.publish(&mut inner);
    }

    /// Workspace with the same identity as `reference`, if open.
    #[must_use]
    pub fn find(&self, reference: &WorkspaceRef) -> Option<WorkspaceId> {
        let key = reference.key();
        let inner = lock(&self.inner);
        inner
            .workspaces
            .iter()
            .find(|(_, e)| e.key == key)
            .map(|(id, _)| *id)
    }

    /// Open project containing `path` (most specific root) + the path inside it.
    #[must_use]
    pub fn find_containing(&self, path: &Path) -> Option<(WorkspaceId, VaultPath)> {
        let inner = lock(&self.inner);
        let mut best: Option<(WorkspaceId, VaultPath, usize)> = None;
        for (id, entry) in &inner.workspaces {
            if let Workspace::Project(core) = &entry.workspace {
                if let Some(rel) = core.vault().relativize(path) {
                    let depth = core.root().path().components().count();
                    if best.as_ref().is_none_or(|(_, _, d)| depth > *d) {
                        best = Some((*id, rel, depth));
                    }
                }
            }
        }
        best.map(|(id, rel, _)| (id, rel))
    }

    /// Windows showing `id`, oldest first.
    #[must_use]
    pub fn windows_of(&self, id: WorkspaceId) -> Vec<String> {
        let inner = lock(&self.inner);
        ordered(&inner)
            .into_iter()
            .filter(|(_, slot)| slot.state == WindowState::Showing { id })
            .map(|(label, _)| label.clone())
            .collect()
    }

    /// Windows on Welcome (not opening anything), oldest first.
    #[must_use]
    pub fn welcome_windows(&self) -> Vec<String> {
        let inner = lock(&self.inner);
        ordered(&inner)
            .into_iter()
            .filter(|(_, slot)| slot.state == WindowState::Welcome)
            .map(|(label, _)| label.clone())
            .collect()
    }

    #[must_use]
    pub fn window_view(&self, label: &str) -> Option<WindowView> {
        let inner = lock(&self.inner);
        inner.windows.get(label).map(|slot| view_of(&inner, slot))
    }

    #[must_use]
    pub fn context_of(&self, label: &str) -> Option<Context> {
        let inner = lock(&self.inner);
        let app_on = self.env.app_plugins_on();
        inner
            .windows
            .get(label)
            .map(|slot| context_of(&inner, slot, &self.env, &app_on))
    }

    /// Workspace a window shows (Showing only).
    #[must_use]
    pub fn workspace_of(&self, label: &str) -> Option<(WorkspaceId, Workspace)> {
        let inner = lock(&self.inner);
        let WindowState::Showing { id } = inner.windows.get(label)?.state else {
            return None;
        };
        inner.workspaces.get(&id).map(|e| (id, e.workspace.clone()))
    }

    /// A live workspace by id (the asset protocol serves for the workspace a URL names).
    #[must_use]
    pub fn workspace(&self, id: WorkspaceId) -> Option<Workspace> {
        lock(&self.inner)
            .workspaces
            .get(&id)
            .map(|e| e.workspace.clone())
    }

    /// Reference a window shows, for the session (oldest window first).
    #[must_use]
    pub fn session_windows(&self) -> Vec<(String, Option<WorkspaceRefDto>)> {
        let inner = lock(&self.inner);
        ordered(&inner)
            .into_iter()
            .filter(|(_, slot)| slot.state != WindowState::Closing)
            .map(|(label, slot)| {
                let shown = holds(slot.state)
                    .and_then(|id| inner.workspaces.get(&id))
                    .map(|e| e.reference.to_dto());
                (label.clone(), shown)
            })
            .collect()
    }

    #[must_use]
    pub fn list(&self) -> Vec<WorkspaceSummary> {
        list_of(&lock(&self.inner))
    }

    /// Open project cores (settings changes re-apply plugins).
    #[must_use]
    pub fn cores(&self) -> Vec<Arc<ProjectCore>> {
        lock(&self.inner)
            .workspaces
            .values()
            .filter_map(|e| match &e.workspace {
                Workspace::Project(core) => Some(Arc::clone(core)),
                Workspace::SingleFile(_) => None,
            })
            .collect()
    }

    /// Recomputes + emits what changed (after settings | plugin changes).
    pub fn refresh(&self) {
        let mut inner = lock(&self.inner);
        self.publish(&mut inner);
    }

    /// Emits differences against the last publish. Runs under the lock (ordered emits).
    fn publish(&self, inner: &mut Inner) {
        let list = list_of(inner);
        let app_on = self.env.app_plugins_on();
        let mut views = BTreeMap::new();
        let mut contexts = BTreeMap::new();
        for (label, slot) in &inner.windows {
            views.insert(label.clone(), view_of(inner, slot));
            contexts.insert(label.clone(), context_of(inner, slot, &self.env, &app_on));
        }
        if list != inner.published.list {
            self.env
                .sink
                .emit(CoreEvent::WorkspacesChanged(WorkspacesChanged {
                    workspaces: list.clone(),
                }));
        }
        for (label, view) in &views {
            if inner.published.views.get(label) != Some(view) {
                self.env.sink.emit(CoreEvent::WindowChanged(WindowChanged {
                    window: label.clone(),
                    view: view.clone(),
                }));
            }
        }
        for (label, context) in &contexts {
            if inner.published.contexts.get(label) != Some(context) {
                self.env
                    .sink
                    .emit(CoreEvent::ContextChanged(ContextChanged {
                        window: label.clone(),
                        context: context.clone(),
                    }));
            }
        }
        inner.published = Published {
            list,
            views,
            contexts,
        };
    }

    /// Forgets per-root slots whose core is gone (slots busy opening stay).
    fn prune_cores(&self) {
        lock(&self.cores)
            .retain(|_, slot| slot.try_lock().map_or(true, |weak| weak.strong_count() > 0));
    }
}

/// Removes `id` when no window holds it any more; the entry is returned to be dropped outside the lock.
fn release(inner: &mut Inner, id: WorkspaceId, window: &str) -> Option<Entry> {
    // the window's Explorer view goes with its workspace
    if let Some(Entry {
        workspace: Workspace::Project(core),
        ..
    }) = inner.workspaces.get(&id)
    {
        core.explorer().close_view(window);
    }
    let held = inner
        .windows
        .values()
        .any(|slot| holds(slot.state) == Some(id));
    if held {
        None
    } else {
        inner.workspaces.remove(&id)
    }
}

fn ordered(inner: &Inner) -> Vec<(&String, &Slot)> {
    let mut all: Vec<(&String, &Slot)> = inner.windows.iter().collect();
    all.sort_by_key(|(_, slot)| slot.created);
    all
}

fn list_of(inner: &Inner) -> Vec<WorkspaceSummary> {
    inner
        .workspaces
        .iter()
        .map(|(id, entry)| {
            let windows = inner
                .windows
                .values()
                .filter(|slot| slot.state == WindowState::Showing { id: *id })
                .count();
            let (title, path) = match &entry.workspace {
                Workspace::Project(core) => {
                    (core.root().name(), core.root().path().display().to_string())
                }
                Workspace::SingleFile(file) => {
                    (file.scope.name(), file.scope.file().display().to_string())
                }
            };
            WorkspaceSummary {
                id: *id,
                kind: entry.reference.kind(),
                title,
                path,
                windows: u32::try_from(windows).unwrap_or(u32::MAX),
            }
        })
        .collect()
}

fn workspace_view(id: WorkspaceId, entry: &Entry) -> WorkspaceView {
    match &entry.workspace {
        Workspace::Project(core) => WorkspaceView::Project {
            id,
            name: core.root().name(),
            root: core.root().path().display().to_string(),
            state_location: core.state_location(),
        },
        Workspace::SingleFile(file) => WorkspaceView::SingleFile {
            id,
            name: file.scope.name(),
            path: file.scope.file().display().to_string(),
            folder: file.scope.folder().path().display().to_string(),
        },
    }
}

fn view_of(inner: &Inner, slot: &Slot) -> WindowView {
    let shown = |id: WorkspaceId| inner.workspaces.get(&id).map(|e| workspace_view(id, e));
    match slot.state {
        WindowState::Welcome => WindowView::Welcome,
        WindowState::Closing => WindowView::Closing,
        WindowState::Opening { previous, .. } => WindowView::Opening {
            target: slot.target.clone().unwrap_or(WorkspaceRefDto::Notes),
            showing: previous.and_then(shown),
        },
        WindowState::Showing { id } => {
            shown(id).map_or(WindowView::Welcome, |workspace| WindowView::Showing {
                workspace,
                document: slot.document.as_ref().map(ToString::to_string),
            })
        }
    }
}

fn context_of(
    inner: &Inner,
    slot: &Slot,
    env: &CoreEnv,
    app_on: &std::collections::BTreeSet<crate::plugins::PluginId>,
) -> Context {
    let mut context = Context::welcome(env.distribution);
    context.plugins.clone_from(app_on);
    let Some(entry) = holds(slot.state).and_then(|id| inner.workspaces.get(&id)) else {
        return context;
    };
    let no_watcher = LiveUpdates::Unavailable {
        reason: "noWatcher".to_owned(),
    };
    match &entry.workspace {
        Workspace::Project(core) => {
            context.workspace = WorkspaceKind::Project;
            context.permark = if core.state_location() == StateLocation::Project {
                PermarkCondition::Initialized
            } else {
                PermarkCondition::NotInitialized
            };
            context.plugins = core.plugins_on();
            context.git = core.facts().git();
            context.live_updates = match core.live_status() {
                crate::watch::LiveStatus::Available => LiveUpdates::Available,
                crate::watch::LiveStatus::Unavailable { reason } => {
                    LiveUpdates::Unavailable { reason }
                }
            };
        }
        Workspace::SingleFile(_) => {
            context.workspace = WorkspaceKind::SingleFile;
            context.live_updates = no_watcher;
        }
    }
    context
}

#[cfg(test)]
mod tests;
