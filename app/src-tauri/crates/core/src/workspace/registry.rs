//! `WorkspaceRegistry`: windows -> views (tabs) -> workspaces -> shared project cores. One place per workspace: a
//! workspace is held by at most one tab of the process. Drives each window's FSM (tabs) and each view's FSM (what it
//! shows); publishes `windows-changed`, `window-changed`, `context-changed` (only what differs). Everything other
//! specs key by "window" for a workspace is keyed by the view label here.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use super::view_fsm::{self, holds, ViewEvent, ViewState};
use super::window_fsm::{self, MoveTo, Resolution, Strip, Tab, TabState, WindowEvent, WindowState};
use crate::document::{DocScope, Documents};

use super::{
    ClosingTab, CoreEnv, FileSession, Place, ProjectCore, TabId, TabSummary, WindowSummary,
    WindowView, Workspace, WorkspaceError, WorkspaceId, WorkspaceKey, WorkspaceRef,
    WorkspaceRefDto, WorkspaceView,
};
use crate::conditions::{Context, LiveUpdates, PermarkCondition, WorkspaceKind};
use crate::error::{ErrorKind, IpcError};
use crate::events::{ContextChanged, CoreEvent, WindowChanged, WindowsChanged};
use crate::paths::{FileScope, RootKey, Vault, VaultPath};
use crate::plugins::PluginId;
use crate::state::StateLocation;

#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("another open in this view superseded this one")]
    Superseded,
    #[error("the window is closing")]
    Closing,
    #[error("unknown view {0}")]
    UnknownView(String),
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
}

impl From<OpenError> for IpcError {
    fn from(error: OpenError) -> Self {
        match error {
            OpenError::Superseded => IpcError::new(ErrorKind::Superseded),
            OpenError::Closing => IpcError::new(ErrorKind::Unavailable),
            OpenError::UnknownView(label) => IpcError::new(ErrorKind::NotFound).subject(label),
            OpenError::Workspace(e) => e.into(),
        }
    }
}

/// A tab move that cannot happen (nothing changed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the tab cannot move there now")]
pub struct MoveRefused;

/// A tab move under way (`begin_move`): no second move of that tab starts until it is dropped; `move_tab` takes it.
/// Held across the shell's whole move (view reparented, then the registry step), so two moves of one tab never
/// interleave (the view would end in one window, the tab in the other).
#[must_use]
pub struct MoveTicket<'a> {
    registry: &'a WorkspaceRegistry,
    tab: TabId,
}

impl MoveTicket<'_> {
    #[must_use]
    pub fn tab(&self) -> TabId {
        self.tab
    }
}

impl Drop for MoveTicket<'_> {
    fn drop(&mut self) {
        lock(&self.registry.inner).moving.remove(&self.tab);
    }
}

/// An open in progress: proves `begin_open` reserved the workspace; consumed by `finish_open`.
#[derive(Debug)]
pub struct Ticket {
    view: String,
    number: u64,
    document: Option<VaultPath>,
}

impl Ticket {
    #[must_use]
    pub fn view(&self) -> &str {
        &self.view
    }
}

/// Result of `begin_open`: the reservation, or where the workspace already is.
#[derive(Debug)]
pub enum Begin {
    Ticket(Ticket),
    Existing(Place),
}

/// Result of `attach`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attached {
    Opened(WorkspaceId),
    Existing(Place),
}

/// A workspace built off-lock, waiting to be bound to its view.
#[derive(Debug)]
pub struct Prepared {
    reference: WorkspaceRef,
    workspace: Workspace,
}

/// What the shell does to its webviews after a registry change, in order (off the registry lock).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewChange {
    /// Make the view (hidden until `Show`).
    Create {
        window: String,
        view: String,
    },
    /// The window's view on screen (sized, focused, title + theme set).
    Show {
        window: String,
        view: String,
    },
    Hide {
        view: String,
    },
    Destroy {
        view: String,
    },
    /// The native window goes (every view of it with it).
    CloseWindow {
        window: String,
    },
    /// The close prompt: shown by `view` (the window's view on screen), listing `tabs`.
    AskClose {
        window: String,
        view: String,
        tabs: Vec<ClosingTab>,
    },
    /// A tab leaves `from` (stage B: target windows); its view (when live) goes along.
    Transfer {
        from: String,
        tab: TabId,
        view: Option<String>,
        to: MoveTo,
    },
}

/// Webview changes + the opens they start (the caller prepares + finishes each, off-lock).
#[derive(Debug, Default)]
pub struct Outcome {
    pub changes: Vec<ViewChange>,
    pub loads: Vec<(Ticket, WorkspaceRef)>,
    /// The event was not allowed (window closing | confirming its close).
    pub refused: bool,
}

/// Result of `open_tab`.
#[derive(Debug)]
pub enum TabOpen {
    /// New tab: `outcome` holds its view changes + its open (one load).
    Opening {
        tab: TabId,
        view: String,
        outcome: Outcome,
    },
    Existing(Place),
}

/// One saved work window (session).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionWindow {
    pub window: String,
    pub tabs: Vec<WorkspaceRefDto>,
    pub active: Option<u32>,
}

#[derive(Debug)]
struct Slot {
    window: String,
    state: ViewState,
    target: Option<WorkspaceRef>,
    document: Option<VaultPath>,
}

#[derive(Debug)]
struct WindowEntry {
    created: u64,
    fsm: WindowState,
    focused_ms: u64,
    /// The tabs it had when it began closing: the last window closed stays in the session as it was.
    closed: Option<Strip>,
    /// Shown on screen once (minimized still counts); `false` while being made (hidden until its first view paints).
    shown: bool,
}

#[derive(Debug)]
struct Entry {
    reference: WorkspaceRef,
    key: WorkspaceKey,
    workspace: Workspace,
}

#[derive(Debug, Default)]
struct Published {
    windows: Vec<WindowSummary>,
    /// What each view shows + the tab it belongs to.
    views: BTreeMap<String, (WindowView, Option<TabId>)>,
    contexts: BTreeMap<String, Context>,
}

#[derive(Debug, Default)]
struct Inner {
    views: BTreeMap<String, Slot>,
    windows: BTreeMap<String, WindowEntry>,
    workspaces: BTreeMap<WorkspaceId, Entry>,
    next_window: u64,
    next_view: u64,
    next_tab: u64,
    next_ticket: u64,
    next_id: u64,
    /// Tabs with a move under way (`MoveTicket`) -> the existing window they go to (`None`: a new one).
    moving: BTreeMap<TabId, Option<String>>,
    /// Windows asked to close while a move into them was under way: asked again once it is done.
    deferred_close: BTreeSet<String>,
    published: Published,
}

impl Inner {
    fn new_view(&mut self) -> String {
        self.next_view += 1;
        format!("v-{}", self.next_view)
    }

    fn new_tab(&mut self) -> TabId {
        self.next_tab += 1;
        TabId(self.next_tab)
    }

    /// The window + tab a view belongs to (`None`: the window's Welcome view | unknown).
    fn place_of_view(&self, view: &str) -> Option<Place> {
        let window = &self.views.get(view)?.window;
        let strip = self.windows.get(window)?.fsm.strip()?;
        let tab = strip.tabs.iter().find(|t| t.view() == Some(view))?;
        Some(Place {
            window: window.clone(),
            tab: tab.id,
        })
    }

    fn window_of_tab(&self, tab: TabId) -> Option<String> {
        self.windows
            .iter()
            .find(|(_, w)| {
                w.fsm
                    .strip()
                    .is_some_and(|s| s.tabs.iter().any(|t| t.id == tab))
            })
            .map(|(label, _)| label.clone())
    }

    /// Where `key` lives: a view showing | opening it, or a pending tab.
    fn find(&self, key: &WorkspaceKey) -> Option<Place> {
        for (label, slot) in &self.views {
            let shown = holds(slot.state)
                .and_then(|id| self.workspaces.get(&id))
                .is_some_and(|e| e.key == *key);
            let opening = matches!(slot.state, ViewState::Opening { .. })
                && slot.target.as_ref().is_some_and(|t| t.key() == *key);
            if shown || opening {
                if let Some(place) = self.place_of_view(label) {
                    return Some(place);
                }
            }
        }
        self.windows.iter().find_map(|(window, entry)| {
            entry
                .fsm
                .strip()?
                .tabs
                .iter()
                .find_map(|tab| match &tab.state {
                    TabState::Pending { target, .. } if target.key() == *key => Some(Place {
                        window: window.clone(),
                        tab: tab.id,
                    }),
                    _ => None,
                })
        })
    }

    fn ordered_windows(&self) -> Vec<(&String, &WindowEntry)> {
        let mut all: Vec<(&String, &WindowEntry)> = self.windows.iter().collect();
        all.sort_by_key(|(_, w)| w.created);
        all
    }
}

type CoreSlot = Arc<Mutex<Weak<ProjectCore>>>;

pub struct WorkspaceRegistry {
    env: Arc<CoreEnv>,
    inner: Mutex<Inner>,
    /// Per-root open slot: one core per root even when two opens race.
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

    /// A registry shared by the app: projects whose live updates change recompute the views' contexts.
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

    // ---- windows -----------------------------------------------------------------------------------------------

    /// A native window was created (no tab, no view yet). Known label -> no change.
    pub fn add_window(&self, window: &str) {
        let mut inner = lock(&self.inner);
        if !inner.windows.contains_key(window) {
            inner.next_window += 1;
            let created = inner.next_window;
            inner.windows.insert(
                window.to_owned(),
                WindowEntry {
                    created,
                    fsm: WindowState::Open(Strip::default()),
                    focused_ms: 0,
                    closed: None,
                    shown: false,
                },
            );
        }
        self.publish(&mut inner);
    }

    /// A window without tabs and without a view gets its Welcome view.
    pub fn show_welcome_view(&self, window: &str) -> Outcome {
        let mut inner = lock(&self.inner);
        let spare = inner.new_view();
        let step = drive(&mut inner, window, WindowEvent::ShowWelcome { spare });
        self.settle(inner, step)
    }

    /// The native window is gone: its views closed, their workspaces released.
    pub fn forget_window(&self, window: &str) {
        let mut inner = lock(&self.inner);
        inner.windows.remove(window);
        inner.deferred_close.remove(window);
        let views: Vec<String> = inner
            .views
            .iter()
            .filter(|(_, s)| s.window == window)
            .map(|(label, _)| label.clone())
            .collect();
        let mut dropped = Vec::new();
        for view in views {
            dropped.extend(close_view(&mut inner, &view));
        }
        self.publish(&mut inner);
        drop(inner);
        drop(dropped);
        self.prune_cores();
    }

    /// The native window is on screen (shown after being made).
    pub fn window_shown(&self, window: &str) {
        if let Some(entry) = lock(&self.inner).windows.get_mut(window) {
            entry.shown = true;
        }
    }

    /// No other window is on screen: `window` closing ends the session (it is saved as this one). Windows closing
    /// and windows still being made (not shown yet) do not count; a minimized one does.
    #[must_use]
    pub fn is_last_on_screen(&self, window: &str) -> bool {
        !lock(&self.inner)
            .windows
            .iter()
            .any(|(label, w)| label != window && w.shown && !matches!(w.fsm, WindowState::Closing))
    }

    /// The window got the focus (`ms`: now).
    pub fn focused(&self, window: &str, ms: u64) {
        let mut inner = lock(&self.inner);
        if let Some(entry) = inner.windows.get_mut(window) {
            entry.focused_ms = ms;
        }
        self.publish(&mut inner);
    }

    /// The work window focused last (open, not asking to close): where outside opens go.
    #[must_use]
    pub fn last_focused_work(&self) -> Option<String> {
        let inner = lock(&self.inner);
        inner
            .windows
            .iter()
            .filter(|(_, w)| matches!(w.fsm, WindowState::Open(_)))
            .max_by_key(|(_, w)| (w.focused_ms, w.created))
            .map(|(label, _)| label.clone())
    }

    #[must_use]
    pub fn list_windows(&self) -> Vec<WindowSummary> {
        summaries(&lock(&self.inner))
    }

    /// The window's view on screen (active live tab's, else its Welcome view).
    #[must_use]
    pub fn shown_view(&self, window: &str) -> Option<String> {
        let inner = lock(&self.inner);
        inner
            .windows
            .get(window)?
            .fsm
            .strip()?
            .shown_view()
            .map(str::to_owned)
    }

    /// The native window a view lives in.
    #[must_use]
    pub fn window_of(&self, view: &str) -> Option<String> {
        lock(&self.inner).views.get(view).map(|s| s.window.clone())
    }

    /// The window + tab a view belongs to (`None` for a Welcome view).
    #[must_use]
    pub fn place_of_view(&self, view: &str) -> Option<Place> {
        lock(&self.inner).place_of_view(view)
    }

    /// Where a tab is now.
    #[must_use]
    pub fn window_of_tab(&self, tab: TabId) -> Option<String> {
        lock(&self.inner).window_of_tab(tab)
    }

    /// The live view of a tab.
    #[must_use]
    pub fn view_of_tab(&self, tab: TabId) -> Option<String> {
        let inner = lock(&self.inner);
        let window = inner.window_of_tab(tab)?;
        let strip = inner.windows.get(&window)?.fsm.strip()?;
        strip
            .tabs
            .iter()
            .find(|t| t.id == tab)?
            .view()
            .map(str::to_owned)
    }

    /// The view is the one its window shows now.
    #[must_use]
    pub fn is_shown(&self, view: &str) -> bool {
        let inner = lock(&self.inner);
        inner
            .views
            .get(view)
            .and_then(|slot| inner.windows.get(&slot.window))
            .and_then(|w| w.fsm.strip())
            .and_then(Strip::shown_view)
            == Some(view)
    }

    // ---- tabs --------------------------------------------------------------------------------------------------

    /// A new tab in `window` opening `reference` (shown at once), unless the workspace is open already (one place:
    /// checked + reserved under the lock). The window's Welcome view becomes the tab's view.
    ///
    /// # Errors
    /// `Closing` (window closing | asking to close), `UnknownView` (no such window).
    pub fn open_tab(
        &self,
        window: &str,
        reference: &WorkspaceRef,
        document: Option<VaultPath>,
    ) -> Result<TabOpen, OpenError> {
        let mut inner = lock(&self.inner);
        if let Some(place) = inner.find(&reference.key()) {
            return Ok(TabOpen::Existing(place));
        }
        let welcome = match inner.windows.get(window) {
            None => return Err(OpenError::UnknownView(window.to_owned())),
            Some(entry) => entry.fsm.strip().and_then(|s| s.welcome.clone()),
        };
        let view = welcome.unwrap_or_else(|| inner.new_view());
        let tab = inner.new_tab();
        let (mut outcome, dropped) = drive(
            &mut inner,
            window,
            WindowEvent::Add {
                tab: Tab {
                    id: tab,
                    state: TabState::Live { view: view.clone() },
                },
            },
        );
        if outcome.refused {
            self.settle(inner, (outcome, dropped));
            return Err(OpenError::Closing);
        }
        let ticket = begin(&mut inner, &view, reference, document);
        let outcome = match ticket {
            Ok(ticket) => {
                outcome.loads.push((ticket, reference.clone()));
                self.settle(inner, (outcome, dropped))
            }
            Err(error) => {
                self.settle(inner, (outcome, dropped));
                return Err(error);
            }
        };
        Ok(TabOpen::Opening { tab, view, outcome })
    }

    /// A restored tab (no view until activated); `None` when the workspace is open already (one place).
    pub fn restore_tab(
        &self,
        window: &str,
        reference: &WorkspaceRef,
        document: Option<VaultPath>,
    ) -> Option<TabId> {
        let mut inner = lock(&self.inner);
        if inner.find(&reference.key()).is_some() {
            return None;
        }
        let tab = inner.new_tab();
        let step = drive(
            &mut inner,
            window,
            WindowEvent::Add {
                tab: Tab {
                    id: tab,
                    state: TabState::Pending {
                        target: reference.clone(),
                        document,
                    },
                },
            },
        );
        let outcome = self.settle(inner, step);
        (!outcome.refused).then_some(tab)
    }

    /// Shows a tab (a pending one gets its view + starts loading).
    pub fn activate(&self, tab: TabId) -> Outcome {
        let mut inner = lock(&self.inner);
        let Some(window) = inner.window_of_tab(tab) else {
            return Outcome::default();
        };
        let spare = inner.new_view();
        let step = drive(&mut inner, &window, WindowEvent::Activate { tab, spare });
        self.settle(inner, step)
    }

    /// Focus on a place with a document: a pending tab opens on it; a live project tab shows it.
    pub fn activate_with(&self, place: &Place, document: Option<VaultPath>) -> Outcome {
        if let Some(document) = document {
            let mut inner = lock(&self.inner);
            let view = set_document_of_tab(&mut inner, place.tab, document);
            if let Some((view, document)) = view {
                if let Some(slot) = inner.views.get_mut(&view) {
                    slot.document = Some(document);
                }
            }
            self.publish(&mut inner);
        }
        self.activate(place.tab)
    }

    /// Removes a tab (its unsaved prompt already answered).
    pub fn close_tab(&self, tab: TabId) -> Outcome {
        let mut inner = lock(&self.inner);
        let Some(window) = inner.window_of_tab(tab) else {
            return Outcome::default();
        };
        let spare = inner.new_view();
        let step = drive(&mut inner, &window, WindowEvent::Close { tab, spare });
        self.settle(inner, step)
    }

    /// Starts a move of `tab` (held until the ticket drops). `NewWindow`: the window's only tab never moves out
    /// (it would leave the window empty); tabs being moved out already do not count as staying. `Window(to)`: the
    /// only tab may go (the window it leaves closes); `to` stays open while the ticket lives (its close asked
    /// meanwhile waits: [`Self::take_deferred_close`]).
    ///
    /// # Errors
    /// `MoveRefused`: unknown tab, source not open (closing | asking), a move of it under way, only tab (`NewWindow`),
    /// `to` unknown | the source | not open (closing | asking).
    pub fn begin_move(&self, tab: TabId, to: &MoveTo) -> Result<MoveTicket<'_>, MoveRefused> {
        let mut inner = lock(&self.inner);
        let from = inner.window_of_tab(tab).ok_or(MoveRefused)?;
        let Some(WindowState::Open(strip)) = inner.windows.get(&from).map(|w| &w.fsm) else {
            return Err(MoveRefused);
        };
        let staying = strip
            .tabs
            .iter()
            .filter(|t| t.id != tab && !inner.moving.contains_key(&t.id))
            .count();
        let target = match to {
            MoveTo::NewWindow if staying == 0 => return Err(MoveRefused),
            MoveTo::NewWindow => None,
            MoveTo::Window(window) => {
                let open = inner
                    .windows
                    .get(window)
                    .is_some_and(|w| matches!(w.fsm, WindowState::Open(_)));
                // a close waiting for the moves into it takes no new one (a stream of moves would hold it for ever)
                if *window == from || !open || inner.deferred_close.contains(window) {
                    return Err(MoveRefused);
                }
                Some(window.clone())
            }
        };
        if inner.moving.contains_key(&tab) {
            return Err(MoveRefused);
        }
        inner.moving.insert(tab, target);
        Ok(MoveTicket {
            registry: self,
            tab,
        })
    }

    /// Moves the ticket's tab into `to` (another work window) in one step under the lock: the workspace is never
    /// without its place (one place holds throughout). `view`: the tab's view as the shell moved it (`None`:
    /// pending); the tab's view changed meanwhile -> refused. Changes: the source's (`Transfer` included), then the
    /// target's.
    ///
    /// # Errors
    /// `MoveRefused`: unknown tab | target, same window, target not open, source not open (closing | asking).
    pub fn move_tab(
        &self,
        ticket: &MoveTicket<'_>,
        to: &str,
        view: Option<&str>,
    ) -> Result<Outcome, MoveRefused> {
        debug_assert!(
            std::ptr::eq(ticket.registry, self),
            "a ticket of this registry"
        );
        let tab = ticket.tab;
        let mut inner = lock(&self.inner);
        let from = inner.window_of_tab(tab).ok_or(MoveRefused)?;
        let source = inner.windows.get(&from).ok_or(MoveRefused)?;
        let target_open = inner
            .windows
            .get(to)
            .is_some_and(|w| matches!(w.fsm, WindowState::Open(_)));
        let WindowState::Open(strip) = &source.fsm else {
            return Err(MoveRefused);
        };
        let moved = strip
            .tabs
            .iter()
            .find(|t| t.id == tab)
            .cloned()
            .ok_or(MoveRefused)?;
        if from == to || !target_open || moved.view() != view {
            return Err(MoveRefused);
        }
        let spare = inner.new_view();
        let (mut outcome, mut dropped) = drive(
            &mut inner,
            &from,
            WindowEvent::Move {
                tab,
                to: MoveTo::Window(to.to_owned()),
                spare,
            },
        );
        if let Some(view) = moved.view() {
            if let Some(slot) = inner.views.get_mut(view) {
                to.clone_into(&mut slot.window);
            }
        }
        let spare = inner.new_view();
        let (arrived, gone) = drive(&mut inner, to, WindowEvent::TakeIn { tab: moved, spare });
        outcome.changes.extend(arrived.changes);
        outcome.loads.extend(arrived.loads);
        dropped.extend(gone);
        Ok(self.settle(inner, (outcome, dropped)))
    }

    /// The window was asked to close: closes at once, or asks about the tabs with unsaved edits (`AskClose`). A tab
    /// moving into it -> nothing yet (the native close is held back): asked again once the move is done
    /// ([`Self::take_deferred_close`]), the moved tab then counted.
    pub fn request_close(&self, window: &str) -> Outcome {
        let mut inner = lock(&self.inner);
        if inner
            .moving
            .values()
            .any(|to| to.as_deref() == Some(window))
        {
            inner.deferred_close.insert(window.to_owned());
            self.publish(&mut inner);
            return Outcome::default();
        }
        let unsaved: Vec<TabId> = inner
            .windows
            .get(window)
            .and_then(|w| w.fsm.strip())
            .map(|strip| {
                strip
                    .tabs
                    .iter()
                    .filter(|t| t.view().is_some_and(|v| unsaved_of(&inner, v).is_some()))
                    .map(|t| t.id)
                    .collect()
            })
            .unwrap_or_default();
        let step = drive(&mut inner, window, WindowEvent::RequestClose { unsaved });
        self.settle(inner, step)
    }

    /// The window's close was asked during a move into it and no move into it is under way now: true once (the
    /// shell asks the close again).
    #[must_use]
    pub fn take_deferred_close(&self, window: &str) -> bool {
        let mut inner = lock(&self.inner);
        if inner
            .moving
            .values()
            .any(|to| to.as_deref() == Some(window))
        {
            return false;
        }
        inner.deferred_close.remove(window)
    }

    /// The close prompt was answered.
    pub fn resolve_close(&self, window: &str, resolution: Resolution) -> Outcome {
        let mut inner = lock(&self.inner);
        let step = drive(&mut inner, window, WindowEvent::ConfirmResolved(resolution));
        self.settle(inner, step)
    }

    /// The window is asking about its close (prompt up).
    #[must_use]
    pub fn is_confirming_close(&self, window: &str) -> bool {
        matches!(
            lock(&self.inner).windows.get(window).map(|w| &w.fsm),
            Some(WindowState::ConfirmingClose(_))
        )
    }

    /// The window is closing for real (its native close goes through).
    #[must_use]
    pub fn is_closing(&self, window: &str) -> bool {
        matches!(
            lock(&self.inner).windows.get(window).map(|w| &w.fsm),
            None | Some(WindowState::Closing)
        )
    }

    /// Publishes, lets go of the lock, then drops the workspaces let go (plugins stop outside the lock).
    fn settle(
        &self,
        mut inner: MutexGuard<'_, Inner>,
        (outcome, dropped): (Outcome, Vec<Entry>),
    ) -> Outcome {
        self.publish(&mut inner);
        drop(inner);
        if !dropped.is_empty() {
            drop(dropped);
            self.prune_cores();
        }
        outcome
    }

    // ---- opening in a view -------------------------------------------------------------------------------------

    /// Step 1: the view starts opening `reference` (an older open of it becomes stale) unless the workspace is
    /// open already somewhere (one place; reserved under the lock: a second `begin_open` of it gets `Existing` even
    /// while the first prepares).
    ///
    /// # Errors
    /// `UnknownView` (no such view, or not a tab's view), `Closing`.
    pub fn begin_open(
        &self,
        view: &str,
        reference: &WorkspaceRef,
        document: Option<VaultPath>,
    ) -> Result<Begin, OpenError> {
        let mut inner = lock(&self.inner);
        if let Some(place) = inner.find(&reference.key()) {
            return Ok(Begin::Existing(place));
        }
        if inner.place_of_view(view).is_none() {
            return Err(OpenError::UnknownView(view.to_owned()));
        }
        let ticket = begin(&mut inner, view, reference, document)?;
        self.publish(&mut inner);
        Ok(Begin::Ticket(ticket))
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
                    Documents::polled(DocScope::File(scope.clone()), Arc::clone(&self.env.sink));
                Workspace::SingleFile(FileSession { scope, documents })
            }
            WorkspaceRef::Notes => {
                // the Notes need the `notes` plugin: it sets the folder and serves the list
                if !self.env.app_plugins_on().contains(&PluginId::of("notes")) {
                    return Err(WorkspaceError::NotesUnavailable);
                }
                Workspace::Notes(self.env.notes())
            }
        };
        Ok(Prepared {
            reference: reference.clone(),
            workspace,
        })
    }

    /// Step 3: binds the prepared workspace if the ticket is still the view's latest.
    ///
    /// # Errors
    /// `Superseded` (newer open | view closed meanwhile), the preparation error.
    pub fn finish_open(
        &self,
        ticket: Ticket,
        prepared: Result<Prepared, WorkspaceError>,
    ) -> Result<WorkspaceId, OpenError> {
        let mut inner = lock(&self.inner);
        let mut dropped: Vec<Entry> = Vec::new();
        let Some(slot_state) = inner.views.get(&ticket.view).map(|s| s.state) else {
            // the tab closed while it was loading: nothing comes back
            drop(inner);
            drop(prepared);
            self.prune_cores();
            return Err(OpenError::Superseded);
        };
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                let (state, _) = view_fsm::step(
                    slot_state,
                    ViewEvent::Failed {
                        ticket: ticket.number,
                    },
                );
                if let Some(slot) = inner.views.get_mut(&ticket.view) {
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
        let (state, effect) = view_fsm::step(
            slot_state,
            ViewEvent::Opened {
                ticket: ticket.number,
                id,
            },
        );
        let result = if effect == view_fsm::Effect::Stale {
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
            if let Some(slot) = inner.views.get_mut(&ticket.view) {
                slot.state = state;
                slot.target = None;
                slot.document = ticket.document;
            }
            if let view_fsm::Effect::Release(old) = effect {
                dropped.extend(release(&mut inner, old, &ticket.view));
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
    pub fn attach(
        &self,
        view: &str,
        reference: &WorkspaceRef,
        document: Option<VaultPath>,
    ) -> Result<Attached, OpenError> {
        match self.begin_open(view, reference, document)? {
            Begin::Existing(place) => Ok(Attached::Existing(place)),
            Begin::Ticket(ticket) => self.load(ticket, reference).map(Attached::Opened),
        }
    }

    /// `prepare` + `finish_open` of a reserved open.
    ///
    /// # Errors
    /// See the two steps.
    pub fn load(&self, ticket: Ticket, reference: &WorkspaceRef) -> Result<WorkspaceId, OpenError> {
        let prepared = self.prepare(reference);
        self.finish_open(ticket, prepared)
    }

    /// Sets the document hint of a view showing a project (open request for a file inside it).
    pub fn set_document(&self, view: &str, document: Option<VaultPath>) {
        let mut inner = lock(&self.inner);
        if let Some(slot) = inner.views.get_mut(view) {
            slot.document = document;
        }
        self.publish(&mut inner);
    }

    // ---- lookups -----------------------------------------------------------------------------------------------

    /// Where the workspace of `reference` is (shown, loading or pending).
    #[must_use]
    pub fn find(&self, reference: &WorkspaceRef) -> Option<Place> {
        lock(&self.inner).find(&reference.key())
    }

    /// Open project tab (live | pending) containing `path` (deepest root wins) + the path inside it.
    #[must_use]
    pub fn find_containing(&self, path: &Path) -> Option<(Place, VaultPath)> {
        let inner = lock(&self.inner);
        let mut best: Option<(Place, VaultPath, usize)> = None;
        let mut consider = |place: Option<Place>, vault: &Vault| {
            let (Some(place), Some(rel)) = (place, vault.relativize(path)) else {
                return;
            };
            let depth = vault.main().path().components().count();
            if best.as_ref().is_none_or(|(_, _, d)| depth > *d) {
                best = Some((place, rel, depth));
            }
        };
        for (label, slot) in &inner.views {
            if let Some(Entry {
                workspace: Workspace::Project(core),
                ..
            }) = holds(slot.state).and_then(|id| inner.workspaces.get(&id))
            {
                consider(inner.place_of_view(label), core.vault());
            }
            if let (ViewState::Opening { .. }, Some(WorkspaceRef::Project(root))) =
                (slot.state, &slot.target)
            {
                consider(inner.place_of_view(label), &Vault::new(root.clone()));
            }
        }
        for (window, entry) in &inner.windows {
            for tab in entry
                .fsm
                .strip()
                .map(|s| s.tabs.as_slice())
                .unwrap_or_default()
            {
                if let TabState::Pending {
                    target: WorkspaceRef::Project(root),
                    ..
                } = &tab.state
                {
                    let place = Place {
                        window: window.clone(),
                        tab: tab.id,
                    };
                    consider(Some(place), &Vault::new(root.clone()));
                }
            }
        }
        best.map(|(place, rel, _)| (place, rel))
    }

    /// The place of a live workspace.
    #[must_use]
    pub fn place_of(&self, id: WorkspaceId) -> Option<Place> {
        let inner = lock(&self.inner);
        inner
            .views
            .iter()
            .filter(|(_, slot)| holds(slot.state) == Some(id))
            .find_map(|(label, _)| inner.place_of_view(label))
    }

    #[must_use]
    pub fn window_view(&self, view: &str) -> Option<WindowView> {
        let inner = lock(&self.inner);
        inner.views.get(view).map(|slot| view_of(&inner, slot))
    }

    #[must_use]
    pub fn context_of(&self, view: &str) -> Option<Context> {
        let inner = lock(&self.inner);
        let app_on = self.env.app_plugins_on();
        inner
            .views
            .get(view)
            .map(|slot| context_of(&inner, slot, &self.env, &app_on))
    }

    /// Workspace a view shows (Showing only).
    #[must_use]
    pub fn workspace_of(&self, view: &str) -> Option<(WorkspaceId, Workspace)> {
        let inner = lock(&self.inner);
        let ViewState::Showing { id } = inner.views.get(view)?.state else {
            return None;
        };
        inner.workspaces.get(&id).map(|e| (id, e.workspace.clone()))
    }

    /// Every view label (live tabs + Welcome views).
    #[must_use]
    pub fn views(&self) -> Vec<String> {
        lock(&self.inner).views.keys().cloned().collect()
    }

    /// A live workspace by id (the asset protocol serves for the workspace a URL names).
    #[must_use]
    pub fn workspace(&self, id: WorkspaceId) -> Option<Workspace> {
        lock(&self.inner)
            .workspaces
            .get(&id)
            .map(|e| e.workspace.clone())
    }

    /// Work windows for the session (creation order): tabs (pending as their ref, Notes left out) + active index.
    #[must_use]
    pub fn session_windows(&self) -> Vec<SessionWindow> {
        let inner = lock(&self.inner);
        inner
            .ordered_windows()
            .into_iter()
            .filter_map(|(label, w)| {
                let strip = w.fsm.strip().or(w.closed.as_ref())?;
                let refs: Vec<(TabId, WorkspaceRef)> = strip
                    .tabs
                    .iter()
                    .filter_map(|t| tab_target(&inner, t).map(|r| (t.id, r)))
                    .filter(|(_, r)| !matches!(r, WorkspaceRef::Notes))
                    .collect();
                let active = strip
                    .active
                    .and_then(|a| refs.iter().position(|(id, _)| *id == a))
                    .or((!refs.is_empty()).then_some(0))
                    .and_then(|a| u32::try_from(a).ok());
                Some(SessionWindow {
                    window: label.clone(),
                    tabs: refs.iter().map(|(_, r)| r.to_dto()).collect(),
                    active,
                })
            })
            .collect()
    }

    /// A Notes tab exists (stage A: the Notes are a tab; the session keeps them as `notes.open`).
    #[must_use]
    pub fn notes_open(&self) -> bool {
        lock(&self.inner).find(&WorkspaceKey::Notes).is_some()
    }

    /// Open project cores (settings changes re-apply plugins).
    #[must_use]
    pub fn cores(&self) -> Vec<Arc<ProjectCore>> {
        lock(&self.inner)
            .workspaces
            .values()
            .filter_map(|e| match &e.workspace {
                Workspace::Project(core) => Some(Arc::clone(core)),
                Workspace::SingleFile(_) | Workspace::Notes(_) => None,
            })
            .collect()
    }

    /// Recomputes + emits what changed (after settings | plugin | unsaved changes).
    pub fn refresh(&self) {
        let mut inner = lock(&self.inner);
        self.publish(&mut inner);
    }

    /// Emits differences against the last publish. Runs under the lock (ordered emits).
    fn publish(&self, inner: &mut Inner) {
        let windows = summaries(inner);
        let app_on = self.env.app_plugins_on();
        let mut views = BTreeMap::new();
        let mut contexts = BTreeMap::new();
        for (label, slot) in &inner.views {
            let tab = inner.place_of_view(label).map(|p| p.tab);
            views.insert(label.clone(), (view_of(inner, slot), tab));
            contexts.insert(label.clone(), context_of(inner, slot, &self.env, &app_on));
        }
        if windows != inner.published.windows {
            self.env
                .sink
                .emit(CoreEvent::WindowsChanged(WindowsChanged {
                    windows: windows.clone(),
                }));
        }
        for (label, shown) in &views {
            if inner.published.views.get(label) != Some(shown) {
                let (view, tab) = shown.clone();
                self.env.sink.emit(CoreEvent::WindowChanged(WindowChanged {
                    window: label.clone(),
                    view,
                    tab,
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
            windows,
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

/// One window event under the lock; effects -> view changes + loads. Returns the workspaces let go, to be
/// dropped by the caller once the lock is released (plugins stop outside it).
fn drive(inner: &mut Inner, window: &str, event: WindowEvent) -> (Outcome, Vec<Entry>) {
    let mut outcome = Outcome::default();
    let mut dropped = Vec::new();
    let Some(entry) = inner.windows.get_mut(window) else {
        outcome.refused = true;
        return (outcome, dropped);
    };
    let state = std::mem::replace(&mut entry.fsm, WindowState::Closing);
    // a window a move emptied keeps nothing for the session (its tab is saved where it went, once)
    let before = match event {
        WindowEvent::Move { .. } => None,
        _ => state.strip().cloned(),
    };
    let (state, effects) = window_fsm::step(state, event);
    if state == WindowState::Closing && entry.closed.is_none() {
        entry.closed = before;
    }
    entry.fsm = state;
    for effect in effects {
        apply(inner, window, effect, &mut outcome, &mut dropped);
    }
    (outcome, dropped)
}

/// Reserves `reference` in `view` (view FSM `Open`).
fn begin(
    inner: &mut Inner,
    view: &str,
    reference: &WorkspaceRef,
    document: Option<VaultPath>,
) -> Result<Ticket, OpenError> {
    inner.next_ticket += 1;
    let number = inner.next_ticket;
    let slot = inner
        .views
        .get_mut(view)
        .ok_or_else(|| OpenError::UnknownView(view.to_owned()))?;
    let (state, effect) = view_fsm::step(slot.state, ViewEvent::Open { ticket: number });
    if effect == view_fsm::Effect::Refused {
        return Err(OpenError::Closing);
    }
    slot.state = state;
    slot.target = Some(reference.clone());
    Ok(Ticket {
        view: view.to_owned(),
        number,
        document,
    })
}

/// Runs one window FSM effect on the views (under the lock).
fn apply(
    inner: &mut Inner,
    window: &str,
    effect: window_fsm::Effect,
    outcome: &mut Outcome,
    dropped: &mut Vec<Entry>,
) {
    use window_fsm::Effect as W;
    let new_slot = |window: &str| Slot {
        window: window.to_owned(),
        state: ViewState::Welcome,
        target: None,
        document: None,
    };
    match effect {
        W::Attach {
            view, create, load, ..
        } => {
            if create {
                inner.views.insert(view.clone(), new_slot(window));
                outcome.changes.push(ViewChange::Create {
                    window: window.to_owned(),
                    view: view.clone(),
                });
            }
            if let Some(load) = load {
                if let Ok(ticket) = begin(inner, &view, &load.target, load.document) {
                    outcome.loads.push((ticket, load.target));
                }
            }
        }
        W::Show { view, .. } => outcome.changes.push(ViewChange::Show {
            window: window.to_owned(),
            view,
        }),
        W::Hide { view } => outcome.changes.push(ViewChange::Hide { view }),
        W::DestroyView { view } => {
            dropped.extend(close_view(inner, &view));
            outcome.changes.push(ViewChange::Destroy { view });
        }
        W::Welcome { view, create } => {
            if create {
                inner.views.insert(view.clone(), new_slot(window));
                outcome.changes.push(ViewChange::Create {
                    window: window.to_owned(),
                    view: view.clone(),
                });
            } else if let Some(slot) = inner.views.get_mut(&view) {
                let (state, effect) = view_fsm::step(slot.state, ViewEvent::ShowWelcome);
                slot.state = state;
                slot.target = None;
                slot.document = None;
                if let view_fsm::Effect::Release(id) = effect {
                    dropped.extend(release(inner, id, &view));
                }
            }
            outcome.changes.push(ViewChange::Show {
                window: window.to_owned(),
                view,
            });
        }
        W::Transfer { tab, to } => outcome.changes.push(ViewChange::Transfer {
            from: window.to_owned(),
            tab: tab.id,
            view: tab.view().map(str::to_owned),
            to,
        }),
        W::CloseWindow | W::DestroyAll { .. } => outcome.changes.push(ViewChange::CloseWindow {
            window: window.to_owned(),
        }),
        W::AskClose { tabs } => {
            let shown = inner
                .windows
                .get(window)
                .and_then(|w| w.fsm.strip())
                .and_then(Strip::shown_view)
                .map(str::to_owned);
            let listed = tabs
                .iter()
                .filter_map(|tab| closing_tab(inner, window, *tab))
                .collect();
            if let Some(view) = shown {
                outcome.changes.push(ViewChange::AskClose {
                    window: window.to_owned(),
                    view,
                    tabs: listed,
                });
            }
        }
        W::Refused => outcome.refused = true,
    }
}

/// A tab in the close prompt: its name + the document with unsaved edits.
fn closing_tab(inner: &Inner, window: &str, tab: TabId) -> Option<ClosingTab> {
    let strip = inner.windows.get(window)?.fsm.strip()?;
    let found = strip.tabs.iter().find(|t| t.id == tab)?;
    let view = found.view()?;
    let summary = tab_summary(inner, found);
    Some(ClosingTab {
        tab,
        title: summary.title,
        document: unsaved_of(inner, view).unwrap_or_default(),
    })
}

/// The document with unsaved edits in `view`, if any.
fn unsaved_of(inner: &Inner, view: &str) -> Option<String> {
    let id = holds(inner.views.get(view)?.state)?;
    inner
        .workspaces
        .get(&id)?
        .workspace
        .documents()
        .unsaved(view)
}

/// Sets the document a pending tab opens on | a live project tab shows; returns the live view to update.
fn set_document_of_tab(
    inner: &mut Inner,
    tab: TabId,
    document: VaultPath,
) -> Option<(String, VaultPath)> {
    let window = inner.window_of_tab(tab)?;
    let entry = inner.windows.get_mut(&window)?;
    // while the close prompt is up nothing changes under it (no document switched in a hidden tab)
    let WindowState::Open(strip) = &mut entry.fsm else {
        return None;
    };
    let found = strip.tabs.iter_mut().find(|t| t.id == tab)?;
    match &mut found.state {
        TabState::Pending { document: hint, .. } => {
            *hint = Some(document);
            None
        }
        TabState::Live { view } => Some((view.clone(), document)),
    }
}

/// Closes + forgets a view; its workspace released when nothing else holds it.
fn close_view(inner: &mut Inner, view: &str) -> Option<Entry> {
    let slot = inner.views.get_mut(view)?;
    let (_, effect) = view_fsm::step(slot.state, ViewEvent::Close);
    inner.views.remove(view);
    match effect {
        view_fsm::Effect::Release(id) => release(inner, id, view),
        _ => None,
    }
}

/// Removes `id` when no view holds it any more; the entry is returned to be dropped outside the lock.
fn release(inner: &mut Inner, id: WorkspaceId, view: &str) -> Option<Entry> {
    if let Some(entry) = inner.workspaces.get(&id) {
        // the view's unsaved guard + Explorer view go with its workspace
        entry.workspace.documents().set_unsaved(view, None);
        if let Workspace::Project(core) = &entry.workspace {
            core.explorer().close_view(view);
        }
    }
    let held = inner
        .views
        .iter()
        .any(|(label, slot)| label != view && holds(slot.state) == Some(id));
    if held {
        None
    } else {
        inner.workspaces.remove(&id)
    }
}

/// What a tab holds | opens | waits for.
fn tab_target(inner: &Inner, tab: &Tab) -> Option<WorkspaceRef> {
    match &tab.state {
        TabState::Pending { target, .. } => Some(target.clone()),
        TabState::Live { view } => {
            let slot = inner.views.get(view)?;
            match slot.state {
                ViewState::Opening { .. } => slot.target.clone(),
                _ => holds(slot.state)
                    .and_then(|id| inner.workspaces.get(&id))
                    .map(|e| e.reference.clone()),
            }
        }
    }
}

fn name_of(reference: &WorkspaceRef) -> (WorkspaceKind, String, String) {
    match reference {
        WorkspaceRef::Project(root) => (
            WorkspaceKind::Project,
            root.name(),
            root.path().display().to_string(),
        ),
        WorkspaceRef::SingleFile(path) => (
            WorkspaceKind::SingleFile,
            path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            ),
            path.display().to_string(),
        ),
        WorkspaceRef::Notes => (WorkspaceKind::Notes, "Notes".to_owned(), String::new()),
    }
}

fn tab_summary(inner: &Inner, tab: &Tab) -> TabSummary {
    let (kind, title, path) = tab_target(inner, tab).map_or_else(
        || (WorkspaceKind::Welcome, String::new(), String::new()),
        |r| name_of(&r),
    );
    let (loaded, unsaved) = match tab.view() {
        Some(view) => (true, unsaved_of(inner, view).is_some()),
        None => (false, false),
    };
    TabSummary {
        tab: tab.id,
        kind,
        title,
        path,
        loaded,
        unsaved,
    }
}

fn summaries(inner: &Inner) -> Vec<WindowSummary> {
    inner
        .ordered_windows()
        .into_iter()
        .filter_map(|(label, entry)| {
            let strip = entry.fsm.strip()?;
            Some(WindowSummary {
                window: label.clone(),
                closing: matches!(entry.fsm, WindowState::ConfirmingClose(_))
                    || inner.deferred_close.contains(label),
                tabs: strip.tabs.iter().map(|t| tab_summary(inner, t)).collect(),
                active: strip.active,
                focused_ms: entry.focused_ms,
            })
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
        Workspace::Notes(core) => WorkspaceView::Notes {
            id,
            name: "Notes".to_owned(),
            folder: core
                .scope()
                .folder()
                .map(|root| root.path().display().to_string()),
        },
    }
}

fn view_of(inner: &Inner, slot: &Slot) -> WindowView {
    let shown = |id: WorkspaceId| inner.workspaces.get(&id).map(|e| workspace_view(id, e));
    match slot.state {
        ViewState::Welcome => WindowView::Welcome,
        ViewState::Closing => WindowView::Closing,
        ViewState::Opening { previous, .. } => WindowView::Opening {
            target: slot
                .target
                .as_ref()
                .map_or(WorkspaceRefDto::Notes, WorkspaceRef::to_dto),
            showing: previous.and_then(shown),
        },
        ViewState::Showing { id } => {
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
            context.agent = core.facts().agent();
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
        Workspace::Notes(_) => {
            context.workspace = WorkspaceKind::Notes;
            context.live_updates = no_watcher;
        }
    }
    context
}

#[cfg(test)]
mod tests;
