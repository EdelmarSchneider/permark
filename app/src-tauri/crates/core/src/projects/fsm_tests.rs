#![allow(
    clippy::many_single_char_names,
    clippy::field_reassign_with_default,
    clippy::assert_is_empty
)]

use super::*;

fn uid(n: u8) -> ProjectUid {
    let hi = char::from(b'a' + n / 26);
    let lo = char::from(b'a' + n % 26);
    ProjectUid::parse(&format!("aaaaaaaa{hi}{lo}")).unwrap()
}

fn vp(text: &str) -> VaultPath {
    VaultPath::parse(text).unwrap()
}

fn found(path: &str, marker: ProjectMarker) -> (String, Found) {
    (
        vp(path).key(),
        Found {
            path: vp(path),
            marker,
            inner_uid: None,
        },
    )
}

/// Root with: `a` (included), `b` (listed, gone), `c` (found, offered), `d` (found, declined).
fn sample() -> Inputs {
    let mut inputs = Inputs {
        answer: RootAnswer::Answered,
        ..Inputs::default()
    };
    for (n, path) in [(0, "a"), (1, "b")] {
        inputs.entries.push(RecEntry {
            uid: uid(n),
            path: vp(path),
            hidden: HiddenRule::Own,
        });
    }
    inputs
        .presence
        .insert(uid(0), Presence::Here(ProjectMarker::Git));
    inputs.presence.insert(uid(1), Presence::Gone);
    inputs.found = [
        found("a", ProjectMarker::Git),
        found("c", ProjectMarker::Permark),
        found("d", ProjectMarker::Both),
    ]
    .into_iter()
    .collect();
    inputs.declined.push("d".to_owned());
    inputs
}

fn state(inputs: &Inputs, key: &str) -> Option<Candidate> {
    inputs.derive().get(key).cloned()
}

#[test]
fn derive_gives_one_state_per_key() {
    let s = sample();
    assert_eq!(
        state(&s, "a"),
        Some(Candidate::Included {
            uid: uid(0),
            marker: ProjectMarker::Git
        })
    );
    assert_eq!(
        state(&s, "b"),
        Some(Candidate::Missing {
            uid: uid(1),
            reason: MissingReason::Gone
        })
    );
    assert_eq!(state(&s, "c"), Some(Candidate::Offered));
    assert_eq!(state(&s, "d"), Some(Candidate::Declined));
    assert_eq!(s.derive().len(), 4);
}

#[test]
fn answers_of_the_root() {
    let mut fresh = Inputs::default();
    fresh.found = [found("c", ProjectMarker::Git)].into_iter().collect();
    assert_eq!(fresh.effective_answer(), RootAnswer::Unanswered);
    // DeclineAll on the first question: every offered -> declined, profile only
    let (no, effects) = step(&fresh, &Event::DeclineAll);
    assert_eq!(no.answer, RootAnswer::DeclinedAll);
    assert_eq!(state(&no, "c"), Some(Candidate::Declined));
    assert!(effects.iter().all(|e| !matches!(e, Effect::Record(_))));
    // later found projects are declined silently
    let mut later = no.clone();
    later.found.extend([found("e", ProjectMarker::Git)]);
    assert_eq!(state(&later, "e"), Some(Candidate::Declined));
    // include from Settings answers it
    let (yes, _) = step(&later, &Event::Include(vec![("e".into(), uid(5))]));
    assert_eq!(yes.answer, RootAnswer::Answered);
    // a record with entries counts as answered (clone with a record)
    let mut cloned = Inputs::default();
    cloned.entries.push(RecEntry {
        uid: uid(0),
        path: vp("a"),
        hidden: HiddenRule::Own,
    });
    assert_eq!(cloned.effective_answer(), RootAnswer::Answered);
    // DeclineAll after an answer = decline the offered ones only
    let (d, _) = step(&sample(), &Event::DeclineAll);
    assert_eq!(d.answer, RootAnswer::Answered);
    assert_eq!(state(&d, "c"), Some(Candidate::Declined));
}

#[test]
fn include_adds_entries_and_skips_what_does_not_fit() {
    let s = sample();
    let (n, effects) = step(
        &s,
        &Event::Include(vec![
            ("c".into(), uid(2)),
            ("d".into(), uid(3)),
            ("a".into(), uid(9)),   // already included
            ("zzz".into(), uid(8)), // not found
        ]),
    );
    assert!(matches!(state(&n, "c"), Some(Candidate::Included { .. })));
    assert!(matches!(state(&n, "d"), Some(Candidate::Included { .. })));
    assert!(!n.declined.contains(&"d".to_owned()));
    let adds = effects
        .iter()
        .filter(|e| matches!(e, Effect::Record(RecordOp::Add { .. })))
        .count();
    assert_eq!(adds, 2);
    assert_eq!(
        changes(&s, &n),
        Changes {
            started: vec![uid(2), uid(3)],
            stopped: vec![]
        }
    );
    // the same uid twice never makes two entries
    let (twice, _) = step(&s, &Event::Include(vec![("c".into(), uid(0))]));
    assert_eq!(twice.entries.len(), 2, "uid of `a` refused for `c`");
}

#[test]
fn leave_out_remove_hidden_agent_and_paths() {
    let s = sample();
    let (n, effects) = step(&s, &Event::LeaveOut(vec![uid(0)]));
    assert_eq!(state(&n, "a"), Some(Candidate::Declined));
    assert!(effects.contains(&Effect::Record(RecordOp::Remove { uid: uid(0) })));
    assert_eq!(changes(&s, &n).stopped, vec![uid(0)]);
    // Remove: only Missing entries
    let (r, effects) = step(&s, &Event::Remove(vec![uid(0), uid(1)]));
    assert_eq!(effects.len(), 3, "one remove + save + rules: {effects:?}");
    assert_eq!(state(&r, "b"), None);
    assert!(state(&r, "a").is_some());
    // hidden rule
    let (h, effects) = step(&s, &Event::SetHidden(uid(0), HiddenRule::OwnRoot));
    assert_eq!(h.entries[0].hidden, HiddenRule::OwnRoot);
    assert_eq!(effects.len(), 2);
    let (_, none) = step(&h, &Event::SetHidden(uid(0), HiddenRule::OwnRoot));
    assert!(none.is_empty(), "no change, no write");
    // agent
    let (a, _) = step(&s, &Event::SetAgent(uid(0), AgentChoice::Off));
    assert!(a.agent_off.contains(&uid(0)));
    let (back, _) = step(&a, &Event::SetAgent(uid(0), AgentChoice::Same));
    assert!(back.agent_off.is_empty());
    let (_, none) = step(&s, &Event::SetAgent(uid(7), AgentChoice::Off));
    assert!(none.is_empty(), "unknown uid");
    // path: refused onto another entry's key
    let (p, effects) = step(&s, &Event::SetPath(uid(1), vp("a")));
    assert!(effects.is_empty());
    assert_eq!(p, s);
    let (p, effects) = step(&s, &Event::SetPath(uid(1), vp("moved/b")));
    assert_eq!(p.entries[1].path.as_str(), "moved/b");
    assert_eq!(effects.len(), 2);
}

#[test]
fn decline_only_takes_offered_keys_and_caps_the_list() {
    let s = sample();
    let (n, effects) = step(
        &s,
        &Event::Decline(vec!["c".into(), "a".into(), "d".into()]),
    );
    assert_eq!(n.declined, vec!["d".to_owned(), "c".to_owned()]);
    assert!(!effects.is_empty());
    let mut many = Inputs::default();
    for i in 0..(MAX_ANSWERS + 5) {
        many.found
            .extend([found(&format!("p{i}"), ProjectMarker::Git)]);
    }
    many.answer = RootAnswer::Answered;
    let keys: Vec<String> = many.found.keys().cloned().collect();
    let (n, _) = step(&many, &Event::Decline(keys));
    assert_eq!(n.declined.len(), MAX_ANSWERS);
}

/// xorshift (no dependency).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % (n.max(1) as u64)).unwrap()
    }
}

fn check(inputs: &Inputs) {
    let derived = inputs.derive();
    // record entries == Included ∪ Missing
    let mut keys = BTreeSet::new();
    for e in &inputs.entries {
        assert!(keys.insert(e.path.key()), "one entry per key");
        assert!(matches!(
            derived.get(&e.path.key()),
            Some(Candidate::Included { .. } | Candidate::Missing { .. })
        ));
    }
    let uids: BTreeSet<_> = inputs.entries.iter().map(|e| e.uid.clone()).collect();
    assert_eq!(uids.len(), inputs.entries.len(), "one entry per uid");
    // no Offered key in the record or the declined list
    for (key, state) in &derived {
        if *state == Candidate::Offered {
            assert!(!keys.contains(key));
            assert!(!inputs.declined.contains(key));
        }
    }
    assert!(
        inputs.agent_off.is_subset(&uids),
        "agent choices only for entries"
    );
}

#[test]
fn random_walks_keep_the_invariants() {
    let paths = ["a", "b", "a/b", "c", "Hub", "hub", "x/y/z", "ü", "e\u{301}"];
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let mut inputs = Inputs::default();
    let mut next_uid = 0u8;
    for _ in 0..10_000 {
        let path = paths[rng.below(paths.len())];
        let key = vp(path).key();
        let event = match rng.below(9) {
            0 => {
                // disk: found | lost
                if rng.below(2) == 0 {
                    inputs.found.extend([found(path, ProjectMarker::Git)]);
                } else {
                    inputs.found.remove(&key);
                }
                for e in &inputs.entries {
                    let here = inputs.found.contains_key(&e.path.key());
                    inputs.presence.insert(
                        e.uid.clone(),
                        if here {
                            Presence::Here(ProjectMarker::Git)
                        } else {
                            Presence::Gone
                        },
                    );
                }
                check(&inputs);
                continue;
            }
            1 | 2 => {
                next_uid = next_uid.wrapping_add(1);
                Event::Include(vec![(key, uid(next_uid))])
            }
            3 => Event::Decline(vec![key]),
            4 => Event::DeclineAll,
            5 => match inputs.entries.get(rng.below(inputs.entries.len())) {
                Some(e) => Event::LeaveOut(vec![e.uid.clone()]),
                None => continue,
            },
            6 => match inputs.entries.get(rng.below(inputs.entries.len())) {
                Some(e) => Event::Remove(vec![e.uid.clone()]),
                None => continue,
            },
            7 => match inputs.entries.get(rng.below(inputs.entries.len())) {
                Some(e) => Event::SetAgent(e.uid.clone(), AgentChoice::Off),
                None => continue,
            },
            _ => match inputs.entries.get(rng.below(inputs.entries.len())) {
                Some(e) => Event::SetPath(e.uid.clone(), vp(path)),
                None => continue,
            },
        };
        let (next, _) = step(&inputs, &event);
        check(&next);
        inputs = next;
    }
}

#[test]
fn membership_file_keeps_unknown_fields() {
    let json = r#"{"answer":"declinedAll","declined":["a"],"future":{"x":1}}"#;
    let m: Membership = serde_json::from_str(json).unwrap();
    assert_eq!(m.answer, RootAnswer::DeclinedAll);
    assert!(m.migrated.is_empty() && m.agent_off.is_empty());
    let back = serde_json::to_string(&m).unwrap();
    assert!(back.contains("\"future\":{\"x\":1}"));
}
