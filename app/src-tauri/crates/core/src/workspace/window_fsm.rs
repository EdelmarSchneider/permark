//! Window FSM: the tabs of one native window. `(State, Event) -> (State, Vec<Effect>)`, pure; the registry drives it
//! under its lock and turns the effects into view changes (webviews created | shown | hidden | destroyed).
//!
//! A tab is `Pending` (restored, never shown: no view, no workspace) or `Live` (has its view). A window with no tab
//! shows its Welcome view; the first tab adopts that view, the last tab closed leaves its view as the Welcome.

use super::{TabId, WorkspaceRef};
use crate::paths::VaultPath;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabState {
    /// Restored, not shown yet: opens `target` (on `document`) once activated.
    Pending {
        target: WorkspaceRef,
        document: Option<VaultPath>,
    },
    /// Has its view (`ViewLabel`).
    Live { view: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    pub id: TabId,
    pub state: TabState,
}

impl Tab {
    #[must_use]
    pub fn view(&self) -> Option<&str> {
        match &self.state {
            TabState::Live { view } => Some(view),
            TabState::Pending { .. } => None,
        }
    }
}

/// The tab strip of a window: tabs in strip order, the active one, the Welcome view while no tab is live.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Strip {
    pub tabs: Vec<Tab>,
    /// `Some` iff `tabs` is not empty; names one of them.
    pub active: Option<TabId>,
    /// The view showing the Welcome (only while no tab is live; `None` before the window first shows anything).
    pub welcome: Option<String>,
}

impl Strip {
    fn index(&self, tab: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == tab)
    }

    /// The view of the active tab, when it is live.
    #[must_use]
    pub fn active_view(&self) -> Option<&str> {
        let active = self.active?;
        self.tabs.iter().find(|t| t.id == active)?.view()
    }

    /// Every view of the window (live tabs + Welcome).
    #[must_use]
    pub fn views(&self) -> Vec<String> {
        self.tabs
            .iter()
            .filter_map(|t| t.view().map(str::to_owned))
            .chain(self.welcome.clone())
            .collect()
    }

    /// The view on screen: the active live tab's, else the Welcome.
    #[must_use]
    pub fn shown_view(&self) -> Option<&str> {
        self.active_view().or(self.welcome.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowState {
    Open(Strip),
    /// The close prompt is up (tabs with unsaved edits listed).
    ConfirmingClose(Strip),
    Closing,
}

impl WindowState {
    #[must_use]
    pub fn strip(&self) -> Option<&Strip> {
        match self {
            Self::Open(strip) | Self::ConfirmingClose(strip) => Some(strip),
            Self::Closing => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveTo {
    Window(String),
    NewWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Close,
    /// Stay open; `show`: activate that tab (a save failed there).
    Stay {
        show: Option<TabId>,
    },
}

/// `spare`: a fresh view label, used only when the event needs a new view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowEvent {
    /// `Live` tab -> appended, active, shown; `Pending` tab (restore) -> appended, active only when the strip was empty.
    Add {
        tab: Tab,
    },
    /// A tab moved here from another window (its view comes along when live); appended + activated.
    TakeIn {
        tab: Tab,
        spare: String,
    },
    Activate {
        tab: TabId,
        spare: String,
    },
    /// After the tab's unsaved prompt.
    Close {
        tab: TabId,
        spare: String,
    },
    Move {
        tab: TabId,
        to: MoveTo,
        spare: String,
    },
    /// `unsaved`: the live tabs holding unsaved edits.
    RequestClose {
        unsaved: Vec<TabId>,
    },
    ConfirmResolved(Resolution),
    /// A window with no tab and no view yet: its Welcome view is made.
    ShowWelcome {
        spare: String,
    },
}

/// What opening a pending tab's view loads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Load {
    pub target: WorkspaceRef,
    pub document: Option<VaultPath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// `view` belongs to `tab` from now on: `create` -> make the view (else it exists: the Welcome view adopted);
    /// `load` -> open that workspace in it (a pending tab).
    Attach {
        tab: TabId,
        view: String,
        create: bool,
        load: Option<Load>,
    },
    Show {
        tab: TabId,
        view: String,
    },
    Hide {
        view: String,
    },
    DestroyView {
        view: String,
    },
    /// `view` shows the Welcome now (its workspace let go); `create` -> make it first.
    Welcome {
        view: String,
        create: bool,
    },
    /// The tab leaves for another window (workspace + view kept alive).
    Transfer {
        tab: Tab,
        to: MoveTo,
    },
    /// The window emptied by a move closes.
    CloseWindow,
    DestroyAll {
        views: Vec<String>,
    },
    AskClose {
        tabs: Vec<TabId>,
    },
    /// Not allowed in this state (the caller picks another window | keeps the tab where it was).
    Refused,
}

type Step = (WindowState, Vec<Effect>);

/// The one transition function.
#[must_use]
pub fn step(state: WindowState, event: WindowEvent) -> Step {
    use WindowEvent as E;
    match (state, event) {
        (WindowState::Closing, E::Add { .. } | E::TakeIn { .. } | E::Move { .. }) => {
            (WindowState::Closing, vec![Effect::Refused])
        }
        (WindowState::Closing, _) => (WindowState::Closing, Vec::new()),

        (WindowState::Open(strip), E::Add { tab }) => {
            let (strip, effects) = add(strip, tab, None);
            (WindowState::Open(strip), effects)
        }
        (WindowState::Open(strip), E::TakeIn { tab, spare }) => {
            let (strip, effects) = add(strip, tab, Some(spare));
            (WindowState::Open(strip), effects)
        }
        (WindowState::Open(strip), E::Activate { tab, spare }) => {
            let (strip, effects) = activate(strip, tab, spare);
            (WindowState::Open(strip), effects)
        }
        (WindowState::ConfirmingClose(strip), E::Activate { tab, spare }) => {
            let (strip, effects) = activate(strip, tab, spare);
            (WindowState::ConfirmingClose(strip), effects)
        }
        (WindowState::Open(strip), E::Close { tab, spare }) => {
            let (strip, effects) = close(strip, tab, spare);
            (WindowState::Open(strip), effects)
        }
        (WindowState::Open(strip), E::Move { tab, to, spare }) => move_out(strip, tab, to, spare),
        (WindowState::Open(strip), E::RequestClose { unsaved }) => {
            let unsaved: Vec<TabId> = strip
                .tabs
                .iter()
                .filter(|t| t.view().is_some() && unsaved.contains(&t.id))
                .map(|t| t.id)
                .collect();
            if unsaved.is_empty() {
                let views = strip.views();
                (WindowState::Closing, vec![Effect::DestroyAll { views }])
            } else {
                (
                    WindowState::ConfirmingClose(strip),
                    vec![Effect::AskClose { tabs: unsaved }],
                )
            }
        }
        (WindowState::ConfirmingClose(strip), E::ConfirmResolved(Resolution::Close)) => {
            let views = strip.views();
            (WindowState::Closing, vec![Effect::DestroyAll { views }])
        }
        (WindowState::ConfirmingClose(strip), E::ConfirmResolved(Resolution::Stay { show })) => {
            match show {
                Some(tab) if strip.tabs.iter().any(|t| t.id == tab && t.view().is_some()) => {
                    // a live tab: no new view is needed
                    let (strip, effects) = activate(strip, tab, String::new());
                    (WindowState::Open(strip), effects)
                }
                _ => (WindowState::Open(strip), Vec::new()),
            }
        }
        (
            WindowState::ConfirmingClose(strip),
            E::Add { .. } | E::TakeIn { .. } | E::Close { .. } | E::Move { .. },
        ) => (WindowState::ConfirmingClose(strip), vec![Effect::Refused]),
        (WindowState::ConfirmingClose(strip), E::RequestClose { .. }) => {
            // one prompt at a time
            (WindowState::ConfirmingClose(strip), Vec::new())
        }
        (WindowState::Open(strip), E::ConfirmResolved(_)) => (WindowState::Open(strip), Vec::new()),
        (WindowState::Open(mut strip), E::ShowWelcome { spare }) => {
            if strip.tabs.is_empty() && strip.welcome.is_none() {
                strip.welcome = Some(spare.clone());
                (
                    WindowState::Open(strip),
                    vec![Effect::Welcome {
                        view: spare,
                        create: true,
                    }],
                )
            } else {
                (WindowState::Open(strip), Vec::new())
            }
        }
        (WindowState::ConfirmingClose(strip), E::ShowWelcome { .. }) => {
            (WindowState::ConfirmingClose(strip), Vec::new())
        }
    }
}

/// Appends `tab`. Live -> shown + active (adopts the Welcome view when it is that view; replaces it otherwise);
/// pending -> listed (activated when moved in: `spare` given; else active only on an empty strip).
fn add(mut strip: Strip, tab: Tab, moved_in: Option<String>) -> (Strip, Vec<Effect>) {
    if strip.index(tab.id).is_some() {
        return (strip, vec![Effect::Refused]);
    }
    let id = tab.id;
    match tab.state.clone() {
        TabState::Live { view } => {
            let mut effects = Vec::new();
            let welcome = strip.welcome.take();
            effects.push(Effect::Attach {
                tab: id,
                view: view.clone(),
                // a moved-in view exists already; the Welcome view is adopted
                create: moved_in.is_none() && welcome.as_deref() != Some(view.as_str()),
                load: None,
            });
            effects.push(Effect::Show {
                tab: id,
                view: view.clone(),
            });
            if let Some(previous) = strip.active_view() {
                effects.push(Effect::Hide {
                    view: previous.to_owned(),
                });
            }
            if let Some(welcome) = welcome.filter(|w| *w != view) {
                effects.push(Effect::DestroyView { view: welcome });
            }
            strip.tabs.push(tab);
            strip.active = Some(id);
            (strip, effects)
        }
        TabState::Pending { .. } => {
            strip.tabs.push(tab);
            if let Some(spare) = moved_in {
                return activate(strip, id, spare);
            }
            if strip.active.is_none() {
                strip.active = Some(id);
            }
            (strip, Vec::new())
        }
    }
}

/// Shows `tab` (a pending one gets its view first: the Welcome view when there is one, else `spare`).
fn activate(mut strip: Strip, tab: TabId, spare: String) -> (Strip, Vec<Effect>) {
    let Some(at) = strip.index(tab) else {
        return (strip, Vec::new());
    };
    let live = strip.tabs[at].view().is_some();
    if strip.active == Some(tab) && live {
        return (strip, Vec::new());
    }
    let previous = strip
        .active_view()
        .filter(|_| strip.active != Some(tab))
        .map(str::to_owned);
    let mut effects = Vec::new();
    let view = match strip.tabs[at].state.clone() {
        TabState::Live { view } => view,
        TabState::Pending { target, document } => {
            let (view, create) = match strip.welcome.take() {
                Some(welcome) => (welcome, false),
                None => (spare, true),
            };
            strip.tabs[at].state = TabState::Live { view: view.clone() };
            effects.push(Effect::Attach {
                tab,
                view: view.clone(),
                create,
                load: Some(Load { target, document }),
            });
            view
        }
    };
    effects.push(Effect::Show { tab, view });
    if let Some(previous) = previous {
        effects.push(Effect::Hide { view: previous });
    }
    strip.active = Some(tab);
    (strip, effects)
}

/// Takes `tab` out of the strip; the active one hands over to its right neighbor, else its left one. Returns the
/// removed tab, or `None` (unknown tab).
fn take_out(strip: &mut Strip, tab: TabId, spare: String) -> Option<(Tab, Vec<Effect>)> {
    let at = strip.index(tab)?;
    let was_active = strip.active == Some(tab);
    let removed = strip.tabs.remove(at);
    let mut effects = Vec::new();
    if strip.tabs.is_empty() {
        strip.active = None;
    } else if was_active {
        let next = strip.tabs[at.min(strip.tabs.len() - 1)].id;
        // the removed tab is no longer the active one: nothing of it to hide
        strip.active = None;
        let (next_strip, shown) = activate(std::mem::take(strip), next, spare);
        *strip = next_strip;
        effects = shown;
    }
    Some((removed, effects))
}

fn close(mut strip: Strip, tab: TabId, spare: String) -> (Strip, Vec<Effect>) {
    let Some((removed, mut effects)) = take_out(&mut strip, tab, spare.clone()) else {
        return (strip, Vec::new());
    };
    match (strip.tabs.is_empty(), removed.state) {
        // the last tab's view stays, on the Welcome
        (true, TabState::Live { view }) => {
            strip.welcome = Some(view.clone());
            effects.push(Effect::Welcome {
                view,
                create: false,
            });
        }
        (true, TabState::Pending { .. }) => {
            if strip.welcome.is_none() {
                strip.welcome = Some(spare.clone());
                effects.push(Effect::Welcome {
                    view: spare,
                    create: true,
                });
            }
        }
        (false, TabState::Live { view }) => effects.push(Effect::DestroyView { view }),
        (false, TabState::Pending { .. }) => {}
    }
    (strip, effects)
}

fn move_out(mut strip: Strip, tab: TabId, to: MoveTo, spare: String) -> Step {
    let Some((removed, mut effects)) = take_out(&mut strip, tab, spare) else {
        return (WindowState::Open(strip), Vec::new());
    };
    effects.push(Effect::Transfer { tab: removed, to });
    if strip.tabs.is_empty() {
        // a window the move emptied closes (its Welcome view, if any, with it)
        if let Some(welcome) = strip.welcome.take() {
            effects.push(Effect::DestroyView { view: welcome });
        }
        effects.push(Effect::CloseWindow);
        return (WindowState::Closing, effects);
    }
    (WindowState::Open(strip), effects)
}

/// The invariants every reachable state keeps (tests): `active` set iff tabs exist and names one; the Welcome view
/// only while no tab is live; tab ids and views unique.
#[must_use]
pub fn invariants_hold(state: &WindowState) -> bool {
    let Some(strip) = state.strip() else {
        return true;
    };
    let active_ok = match strip.active {
        Some(active) => strip.tabs.iter().any(|t| t.id == active),
        None => strip.tabs.is_empty(),
    };
    let live = strip.tabs.iter().any(|t| t.view().is_some());
    let welcome_ok = strip.welcome.is_none() || !live;
    let mut ids: Vec<u64> = strip.tabs.iter().map(|t| t.id.0).collect();
    ids.sort_unstable();
    ids.dedup();
    let mut views = strip.views();
    let count = views.len();
    views.sort();
    views.dedup();
    active_ok && welcome_ok && ids.len() == strip.tabs.len() && views.len() == count
}

#[cfg(test)]
mod tests;
