use super::*;
use crate::paths::Root;
use WindowEvent as E;

fn target() -> WorkspaceRef {
    // the FSM never looks at the target: any valid ref will do
    WorkspaceRef::Project(Root::open(&std::env::temp_dir()).unwrap())
}

fn live(id: u64) -> Tab {
    Tab {
        id: TabId(id),
        state: TabState::Live {
            view: format!("v{id}"),
        },
    }
}

fn pending(id: u64) -> Tab {
    Tab {
        id: TabId(id),
        state: TabState::Pending {
            target: target(),
            document: None,
        },
    }
}

fn open(tabs: Vec<Tab>, active: Option<u64>, welcome: Option<&str>) -> WindowState {
    WindowState::Open(Strip {
        tabs,
        active: active.map(TabId),
        welcome: welcome.map(str::to_owned),
    })
}

fn spare() -> String {
    "spare".to_owned()
}

fn strip(state: &WindowState) -> &Strip {
    state.strip().unwrap()
}

fn show(id: u64, view: &str) -> Effect {
    Effect::Show {
        tab: TabId(id),
        view: view.to_owned(),
    }
}

fn hide(view: &str) -> Effect {
    Effect::Hide {
        view: view.to_owned(),
    }
}

// ---- Add -------------------------------------------------------------------------------------------------------

#[test]
fn add_live_to_welcome_adopts_the_welcome_view() {
    let welcome = open(vec![], None, Some("v1"));
    let (state, effects) = step(welcome, E::Add { tab: live(1) });
    assert_eq!(
        effects,
        [
            Effect::Attach {
                tab: TabId(1),
                view: "v1".into(),
                create: false,
                load: None
            },
            show(1, "v1")
        ]
    );
    assert_eq!(strip(&state).active, Some(TabId(1)));
    assert_eq!(strip(&state).welcome, None);
}

#[test]
fn add_live_next_to_others_creates_shows_and_hides_the_previous() {
    let state = open(vec![live(1)], Some(1), None);
    let (state, effects) = step(state, E::Add { tab: live(2) });
    assert_eq!(
        effects,
        [
            Effect::Attach {
                tab: TabId(2),
                view: "v2".into(),
                create: true,
                load: None
            },
            show(2, "v2"),
            hide("v1")
        ]
    );
    assert_eq!(strip(&state).active, Some(TabId(2)));
    assert_eq!(strip(&state).tabs.len(), 2);
}

#[test]
fn add_live_with_another_welcome_view_replaces_it() {
    let state = open(vec![], None, Some("w"));
    let (_, effects) = step(state, E::Add { tab: live(1) });
    assert!(effects.contains(&Effect::DestroyView { view: "w".into() }));
    assert!(effects.contains(&Effect::Attach {
        tab: TabId(1),
        view: "v1".into(),
        create: true,
        load: None
    }));
}

#[test]
fn add_pending_restores_without_a_view() {
    let state = open(vec![], None, None);
    let (state, effects) = step(state, E::Add { tab: pending(1) });
    assert_eq!(effects.len(), 0);
    assert_eq!(strip(&state).active, Some(TabId(1)), "first tab active");
    let (state, effects) = step(state, E::Add { tab: pending(2) });
    assert_eq!(effects.len(), 0);
    assert_eq!(strip(&state).active, Some(TabId(1)), "later ones not");
}

#[test]
fn add_twice_the_same_tab_is_refused() {
    let state = open(vec![live(1)], Some(1), None);
    let (_, effects) = step(state, E::Add { tab: live(1) });
    assert_eq!(effects, [Effect::Refused]);
}

// ---- Activate --------------------------------------------------------------------------------------------------

#[test]
fn activate_pending_creates_its_view_and_loads() {
    let state = open(vec![live(1), pending(2)], Some(1), None);
    let (state, effects) = step(
        state,
        E::Activate {
            tab: TabId(2),
            spare: spare(),
        },
    );
    assert_eq!(
        effects,
        [
            Effect::Attach {
                tab: TabId(2),
                view: "spare".into(),
                create: true,
                load: Some(Load {
                    target: target(),
                    document: None
                })
            },
            show(2, "spare"),
            hide("v1")
        ]
    );
    assert_eq!(strip(&state).active_view(), Some("spare"));
}

#[test]
fn activate_live_shows_and_hides() {
    let state = open(vec![live(1), live(2)], Some(1), None);
    let (_, effects) = step(
        state,
        E::Activate {
            tab: TabId(2),
            spare: spare(),
        },
    );
    assert_eq!(effects, [show(2, "v2"), hide("v1")]);
}

#[test]
fn activate_the_active_live_tab_or_an_unknown_one_does_nothing() {
    let state = open(vec![live(1)], Some(1), None);
    let (state, effects) = step(
        state,
        E::Activate {
            tab: TabId(1),
            spare: spare(),
        },
    );
    assert_eq!(effects.len(), 0);
    let (_, effects) = step(
        state,
        E::Activate {
            tab: TabId(9),
            spare: spare(),
        },
    );
    assert_eq!(effects.len(), 0);
}

#[test]
fn activate_a_pending_active_tab_loads_it_into_the_welcome_view() {
    let state = open(vec![pending(1)], Some(1), Some("w"));
    let (state, effects) = step(
        state,
        E::Activate {
            tab: TabId(1),
            spare: spare(),
        },
    );
    assert_eq!(
        effects[0],
        Effect::Attach {
            tab: TabId(1),
            view: "w".into(),
            create: false,
            load: Some(Load {
                target: target(),
                document: None
            })
        }
    );
    assert_eq!(strip(&state).welcome, None);
}

// ---- Close -----------------------------------------------------------------------------------------------------

#[test]
fn close_active_hands_over_to_the_right_neighbor_else_the_left() {
    let state = open(vec![live(1), live(2), live(3)], Some(2), None);
    let (state, effects) = step(
        state,
        E::Close {
            tab: TabId(2),
            spare: spare(),
        },
    );
    assert_eq!(
        effects,
        [show(3, "v3"), Effect::DestroyView { view: "v2".into() }]
    );
    assert_eq!(strip(&state).active, Some(TabId(3)));
    let (state, effects) = step(
        state,
        E::Close {
            tab: TabId(3),
            spare: spare(),
        },
    );
    assert_eq!(
        effects,
        [show(1, "v1"), Effect::DestroyView { view: "v3".into() }]
    );
    assert_eq!(strip(&state).active, Some(TabId(1)));
}

#[test]
fn close_active_next_to_a_pending_neighbor_loads_it() {
    let state = open(vec![live(1), pending(2)], Some(1), None);
    let (state, effects) = step(
        state,
        E::Close {
            tab: TabId(1),
            spare: spare(),
        },
    );
    assert!(matches!(
        &effects[0],
        Effect::Attach {
            create: true,
            load: Some(_),
            ..
        }
    ));
    assert_eq!(
        effects.last(),
        Some(&Effect::DestroyView { view: "v1".into() })
    );
    assert_eq!(strip(&state).active_view(), Some("spare"));
}

#[test]
fn close_another_tab_destroys_only_its_view() {
    let state = open(vec![live(1), live(2), pending(3)], Some(1), None);
    let (state, effects) = step(
        state,
        E::Close {
            tab: TabId(2),
            spare: spare(),
        },
    );
    assert_eq!(effects, [Effect::DestroyView { view: "v2".into() }]);
    let (state, effects) = step(
        state,
        E::Close {
            tab: TabId(3),
            spare: spare(),
        },
    );
    assert_eq!(effects.len(), 0, "a pending tab has nothing to destroy");
    assert_eq!(strip(&state).active, Some(TabId(1)));
}

#[test]
fn close_the_last_tab_leaves_its_view_on_the_welcome() {
    let state = open(vec![live(1)], Some(1), None);
    let (state, effects) = step(
        state,
        E::Close {
            tab: TabId(1),
            spare: spare(),
        },
    );
    assert_eq!(
        effects,
        [Effect::Welcome {
            view: "v1".into(),
            create: false
        }]
    );
    assert!(
        matches!(&state, WindowState::Open(s) if s.tabs.is_empty() && s.welcome.as_deref() == Some("v1"))
    );
    // the last pending tab (no view at all) -> a Welcome view is made
    let state = open(vec![pending(1)], Some(1), None);
    let (_, effects) = step(
        state,
        E::Close {
            tab: TabId(1),
            spare: spare(),
        },
    );
    assert_eq!(
        effects,
        [Effect::Welcome {
            view: "spare".into(),
            create: true
        }]
    );
}

// ---- Move ------------------------------------------------------------------------------------------------------

#[test]
fn move_to_another_window_transfers_and_hands_over() {
    let state = open(vec![live(1), live(2)], Some(2), None);
    let (state, effects) = step(
        state,
        E::Move {
            tab: TabId(2),
            to: MoveTo::Window("w-1".into()),
            spare: spare(),
        },
    );
    assert_eq!(
        effects,
        [
            show(1, "v1"),
            Effect::Transfer {
                tab: live(2),
                to: MoveTo::Window("w-1".into())
            }
        ]
    );
    assert_eq!(strip(&state).tabs.len(), 1);
}

#[test]
fn moving_the_last_tab_out_closes_the_window() {
    let state = open(vec![live(1)], Some(1), None);
    let (state, effects) = step(
        state,
        E::Move {
            tab: TabId(1),
            to: MoveTo::NewWindow,
            spare: spare(),
        },
    );
    assert_eq!(state, WindowState::Closing);
    assert_eq!(
        effects,
        [
            Effect::Transfer {
                tab: live(1),
                to: MoveTo::NewWindow
            },
            Effect::CloseWindow
        ]
    );
}

#[test]
fn take_in_shows_the_moved_view_without_making_one() {
    let state = open(vec![live(1)], Some(1), None);
    let (state, effects) = step(
        state,
        E::TakeIn {
            tab: live(7),
            spare: spare(),
        },
    );
    assert_eq!(
        effects,
        [
            Effect::Attach {
                tab: TabId(7),
                view: "v7".into(),
                create: false,
                load: None
            },
            show(7, "v7"),
            hide("v1")
        ]
    );
    assert_eq!(strip(&state).active, Some(TabId(7)));
    // a pending tab moves as a ref: it loads on arrival
    let (_, effects) = step(
        state,
        E::TakeIn {
            tab: pending(8),
            spare: spare(),
        },
    );
    assert!(matches!(
        &effects[0],
        Effect::Attach {
            create: true,
            load: Some(_),
            ..
        }
    ));
}

// ---- RequestClose | ConfirmResolved ----------------------------------------------------------------------------

#[test]
fn request_close_clean_closes_at_once() {
    let state = open(vec![live(1), pending(2)], Some(1), None);
    let (state, effects) = step(
        state,
        E::RequestClose {
            unsaved: vec![TabId(2), TabId(9)],
        },
    );
    // pending + unknown tabs never count as unsaved
    assert_eq!(state, WindowState::Closing);
    assert_eq!(
        effects,
        [Effect::DestroyAll {
            views: vec!["v1".into()]
        }]
    );
}

#[test]
fn request_close_unsaved_asks_once() {
    let state = open(vec![live(1), live(2)], Some(1), None);
    let (state, effects) = step(
        state,
        E::RequestClose {
            unsaved: vec![TabId(2)],
        },
    );
    assert!(matches!(state, WindowState::ConfirmingClose(_)));
    assert_eq!(
        effects,
        [Effect::AskClose {
            tabs: vec![TabId(2)]
        }]
    );
    let (state, effects) = step(
        state,
        E::RequestClose {
            unsaved: vec![TabId(2)],
        },
    );
    assert_eq!(effects.len(), 0, "second request while asking: ignored");
    // while asking: adds are refused, and so is activation (the prompt lives in the view on screen: hiding it would
    // leave the window unable to close)
    let (state, effects) = step(state, E::Add { tab: live(3) });
    assert_eq!(effects, [Effect::Refused]);
    let asking = state.clone();
    let (state, effects) = step(
        state,
        E::Activate {
            tab: TabId(2),
            spare: spare(),
        },
    );
    assert_eq!(effects, [Effect::Refused]);
    assert_eq!(state, asking, "the view on screen stays, with its prompt");
    let (state, effects) = step(state, E::ConfirmResolved(Resolution::Close));
    assert_eq!(state, WindowState::Closing);
    assert_eq!(
        effects,
        [Effect::DestroyAll {
            views: vec!["v1".into(), "v2".into()]
        }]
    );
}

#[test]
fn confirm_stay_reopens_and_shows_the_failed_tab() {
    let state = WindowState::ConfirmingClose(Strip {
        tabs: vec![live(1), live(2)],
        active: Some(TabId(1)),
        welcome: None,
    });
    let (state, effects) = step(
        state.clone(),
        E::ConfirmResolved(Resolution::Stay {
            show: Some(TabId(2)),
        }),
    );
    assert!(matches!(state, WindowState::Open(_)));
    assert_eq!(effects, [show(2, "v2"), hide("v1")]);
    let (back, effects) = step(
        WindowState::ConfirmingClose(Strip {
            tabs: vec![live(1)],
            active: Some(TabId(1)),
            welcome: None,
        }),
        E::ConfirmResolved(Resolution::Stay { show: None }),
    );
    assert!(matches!(back, WindowState::Open(_)));
    assert_eq!(effects.len(), 0);
}

#[test]
fn closing_ignores_everything_and_refuses_arrivals() {
    for event in [
        E::Add { tab: live(1) },
        E::TakeIn {
            tab: live(1),
            spare: spare(),
        },
    ] {
        let (state, effects) = step(WindowState::Closing, event);
        assert_eq!(state, WindowState::Closing);
        assert_eq!(effects, [Effect::Refused]);
    }
    for event in [
        E::Activate {
            tab: TabId(1),
            spare: spare(),
        },
        E::Close {
            tab: TabId(1),
            spare: spare(),
        },
        E::RequestClose { unsaved: vec![] },
        E::ConfirmResolved(Resolution::Close),
    ] {
        let (state, effects) = step(WindowState::Closing, event);
        assert_eq!(state, WindowState::Closing);
        assert_eq!(effects.len(), 0);
    }
}

#[test]
fn a_new_window_without_tabs_gets_one_welcome_view() {
    let (state, effects) = step(open(vec![], None, None), E::ShowWelcome { spare: spare() });
    assert_eq!(
        effects,
        [Effect::Welcome {
            view: "spare".into(),
            create: true
        }]
    );
    let (_, effects) = step(state, E::ShowWelcome { spare: spare() });
    assert_eq!(effects.len(), 0, "once");
    let (_, effects) = step(
        open(vec![pending(1)], Some(1), None),
        E::ShowWelcome { spare: spare() },
    );
    assert_eq!(effects.len(), 0, "not with tabs");
}

// ---- random walk -----------------------------------------------------------------------------------------------

/// 10 000 random events: the invariants hold after each; views created and destroyed balance out.
#[test]
fn invariants_hold_after_random_walks() {
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut state = open(vec![], None, Some("welcome"));
    let mut next_tab = 0;
    let mut next_view = 0;
    let mut views: std::collections::BTreeSet<String> = ["welcome".to_owned()].into();
    for _ in 0..10_000 {
        let roll = next();
        let known: Vec<TabId> = state
            .strip()
            .map(|s| s.tabs.iter().map(|t| t.id).collect())
            .unwrap_or_default();
        let pick = known
            .get(usize::try_from(roll >> 8).unwrap_or(0) % known.len().max(1))
            .copied()
            .unwrap_or(TabId(999));
        next_view += 1;
        let spare = format!("s{next_view}");
        let event = match roll % 7 {
            0 | 1 => {
                next_tab += 1;
                let tab = if roll % 2 == 0 {
                    Tab {
                        id: TabId(next_tab),
                        state: TabState::Live {
                            view: format!("t{next_tab}"),
                        },
                    }
                } else {
                    pending(next_tab)
                };
                E::Add { tab }
            }
            2 | 3 => E::Activate { tab: pick, spare },
            4 => E::Close { tab: pick, spare },
            5 => E::RequestClose {
                unsaved: if roll % 3 == 0 { vec![pick] } else { vec![] },
            },
            _ => E::ConfirmResolved(if roll % 2 == 0 {
                Resolution::Close
            } else {
                Resolution::Stay { show: Some(pick) }
            }),
        };
        let (after, effects) = step(state, event);
        for effect in effects {
            match effect {
                Effect::Attach { view, create, .. } => {
                    if create {
                        assert!(views.insert(view), "view made twice");
                    } else {
                        assert!(views.contains(&view), "adopted a view that does not exist");
                    }
                }
                Effect::Welcome { view, create: true } => assert!(views.insert(view)),
                Effect::DestroyView { view } => assert!(views.remove(&view), "destroyed twice"),
                Effect::DestroyAll { views: all } => {
                    for view in all {
                        assert!(views.remove(&view));
                    }
                }
                _ => {}
            }
        }
        assert!(invariants_hold(&after), "{after:?}");
        if let Some(strip) = after.strip() {
            let expected: std::collections::BTreeSet<String> = strip.views().into_iter().collect();
            assert_eq!(views, expected, "every live view is known, none leaked");
        }
        state = if after == WindowState::Closing {
            // a closed window: start a new one
            views = ["welcome".to_owned()].into();
            open(vec![], None, Some("welcome"))
        } else {
            after
        };
    }
}
