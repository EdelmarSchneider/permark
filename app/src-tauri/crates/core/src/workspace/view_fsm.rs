//! View lifecycle FSM (one per tab view): `(State, Event) -> (State, Effect)`. Pure; the registry drives it under its lock.

use super::WorkspaceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewState {
    Welcome,
    /// Opening `ticket`; `previous` stays on screen until the new workspace is ready (failure -> back to it).
    Opening {
        ticket: u64,
        previous: Option<WorkspaceId>,
    },
    Showing {
        id: WorkspaceId,
    },
    Closing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewEvent {
    Open { ticket: u64 },
    Opened { ticket: u64, id: WorkspaceId },
    Failed { ticket: u64 },
    ShowWelcome,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Nothing,
    /// The window no longer shows this workspace.
    Release(WorkspaceId),
    /// Result of an older ticket: drop it (caller answers `Superseded`).
    Stale,
    /// Event not allowed now (open while closing).
    Refused,
}

/// The one transition function.
#[must_use]
pub fn step(state: ViewState, event: ViewEvent) -> (ViewState, Effect) {
    use ViewEvent as E;
    use ViewState as S;
    match (state, event) {
        (S::Closing, E::Open { .. }) => (S::Closing, Effect::Refused),
        (S::Closing, E::Opened { .. }) => (S::Closing, Effect::Stale),

        (S::Welcome, E::Open { ticket }) => (
            S::Opening {
                ticket,
                previous: None,
            },
            Effect::Nothing,
        ),
        (S::Showing { id }, E::Open { ticket }) => (
            S::Opening {
                ticket,
                previous: Some(id),
            },
            Effect::Nothing,
        ),
        (S::Opening { previous, .. }, E::Open { ticket }) => {
            (S::Opening { ticket, previous }, Effect::Nothing)
        }

        (S::Opening { ticket, previous }, E::Opened { ticket: done, id }) if done == ticket => {
            let effect = match previous {
                Some(old) if old != id => Effect::Release(old),
                _ => Effect::Nothing,
            };
            (S::Showing { id }, effect)
        }
        (S::Opening { ticket, previous }, E::Failed { ticket: done }) if done == ticket => {
            let back = previous.map_or(S::Welcome, |id| S::Showing { id });
            (back, Effect::Nothing)
        }
        (state, E::Opened { .. }) => (state, Effect::Stale),
        (state, E::Failed { .. }) => (state, Effect::Nothing),

        (S::Welcome, E::ShowWelcome) => (S::Welcome, Effect::Nothing),
        (S::Showing { id }, E::ShowWelcome) => (S::Welcome, Effect::Release(id)),
        (S::Opening { previous, .. }, E::ShowWelcome) => (
            S::Welcome,
            previous.map_or(Effect::Nothing, Effect::Release),
        ),

        (S::Closing, E::ShowWelcome | E::Close) | (S::Welcome, E::Close) => {
            (S::Closing, Effect::Nothing)
        }
        (S::Showing { id }, E::Close) => (S::Closing, Effect::Release(id)),
        (S::Opening { previous, .. }, E::Close) => (
            S::Closing,
            previous.map_or(Effect::Nothing, Effect::Release),
        ),
    }
}

/// Workspace a state keeps alive (shown | still on screen while opening).
#[must_use]
pub fn holds(state: ViewState) -> Option<WorkspaceId> {
    match state {
        ViewState::Showing { id }
        | ViewState::Opening {
            previous: Some(id), ..
        } => Some(id),
        ViewState::Opening { previous: None, .. } | ViewState::Welcome | ViewState::Closing => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ViewEvent as E;
    use ViewState as S;

    const A: WorkspaceId = WorkspaceId(1);
    const B: WorkspaceId = WorkspaceId(2);

    fn run(events: &[ViewEvent]) -> (ViewState, Vec<Effect>) {
        let mut state = S::Welcome;
        let mut effects = Vec::new();
        for &event in events {
            let (next, effect) = step(state, event);
            state = next;
            effects.push(effect);
        }
        (state, effects)
    }

    #[test]
    fn open_show_switch_and_close() {
        let (state, effects) = run(&[
            E::Open { ticket: 1 },
            E::Opened { ticket: 1, id: A },
            E::Open { ticket: 2 },
            E::Opened { ticket: 2, id: B },
            E::Close,
        ]);
        assert_eq!(state, S::Closing);
        assert_eq!(
            effects,
            [
                Effect::Nothing,
                Effect::Nothing,
                Effect::Nothing,
                Effect::Release(A),
                Effect::Release(B)
            ]
        );
    }

    #[test]
    fn newer_open_supersedes_older_result_in_any_order() {
        // newer finishes first
        let (state, effects) = run(&[
            E::Open { ticket: 1 },
            E::Open { ticket: 2 },
            E::Opened { ticket: 2, id: B },
            E::Opened { ticket: 1, id: A },
        ]);
        assert_eq!(state, S::Showing { id: B });
        assert_eq!(effects[3], Effect::Stale);
        // older finishes first
        let (state, effects) = run(&[
            E::Open { ticket: 1 },
            E::Open { ticket: 2 },
            E::Opened { ticket: 1, id: A },
            E::Opened { ticket: 2, id: B },
        ]);
        assert_eq!(state, S::Showing { id: B });
        assert_eq!(effects[2], Effect::Stale);
    }

    #[test]
    fn failure_returns_to_what_was_shown() {
        let (state, _) = run(&[
            E::Open { ticket: 1 },
            E::Opened { ticket: 1, id: A },
            E::Open { ticket: 2 },
            E::Failed { ticket: 2 },
        ]);
        assert_eq!(state, S::Showing { id: A });
        let (state, _) = run(&[E::Open { ticket: 1 }, E::Failed { ticket: 1 }]);
        assert_eq!(state, S::Welcome);
        // stale failure changes nothing
        let (state, _) = run(&[
            E::Open { ticket: 1 },
            E::Open { ticket: 2 },
            E::Failed { ticket: 1 },
        ]);
        assert_eq!(
            state,
            S::Opening {
                ticket: 2,
                previous: None
            }
        );
    }

    #[test]
    fn reopening_the_same_workspace_releases_nothing() {
        let (_, effects) = run(&[
            E::Open { ticket: 1 },
            E::Opened { ticket: 1, id: A },
            E::Open { ticket: 2 },
            E::Opened { ticket: 2, id: A },
        ]);
        assert_eq!(effects[3], Effect::Nothing);
    }

    #[test]
    fn closing_absorbs_everything_and_late_results_are_stale() {
        let (state, effects) = run(&[
            E::Open { ticket: 1 },
            E::Close,
            E::Opened { ticket: 1, id: A },
            E::Open { ticket: 2 },
            E::ShowWelcome,
        ]);
        assert_eq!(state, S::Closing);
        assert_eq!(effects[2], Effect::Stale);
        assert_eq!(effects[3], Effect::Refused);
    }

    #[test]
    fn rapid_random_transitions_never_hold_two_workspaces_or_leak() {
        // deterministic pseudo-random walk; invariant: every Release names what the state held before
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut state = S::Welcome;
        let mut next_ticket = 0;
        for _ in 0..10_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let id = WorkspaceId(seed % 3);
            let ticket = seed % (next_ticket + 1);
            let event = match seed % 5 {
                0 => {
                    next_ticket += 1;
                    E::Open {
                        ticket: next_ticket,
                    }
                }
                1 => E::Opened { ticket, id },
                2 => E::Failed { ticket },
                3 => E::ShowWelcome,
                _ => E::Open {
                    ticket: next_ticket,
                },
            };
            let held = holds(state);
            let (next, effect) = step(state, event);
            if let Effect::Release(released) = effect {
                assert_eq!(Some(released), held, "{state:?} {event:?}");
            }
            if effect == Effect::Stale {
                assert_eq!(next, state);
            }
            state = next;
        }
    }
}
