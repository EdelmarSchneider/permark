use super::*;

/// Shared with vitest (`app/src/core/conditions/available.test.ts`): both mirrors give the same answers.
const FIXTURE: &str = include_str!("../../../../../src/core/conditions/fixture.json");

#[derive(Deserialize)]
struct Fixture {
    base: serde_json::Value,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    requirement: Requirement,
    context: serde_json::Map<String, serde_json::Value>,
    expected: Availability,
}

#[test]
fn shared_fixture_cases() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
    assert!(fixture.cases.len() >= 20);
    for case in fixture.cases {
        let mut context = fixture.base.clone();
        for (key, value) in case.context {
            context[key] = value;
        }
        let context: Context = serde_json::from_value(context).unwrap();
        assert_eq!(
            available(&case.requirement, &context),
            case.expected,
            "{}",
            case.name
        );
    }
}

fn contexts() -> Vec<Context> {
    let mut all = Vec::new();
    let workspaces = [
        WorkspaceKind::Welcome,
        WorkspaceKind::Project,
        WorkspaceKind::SingleFile,
        WorkspaceKind::Notes,
    ];
    let gits = [
        GitCondition::Unavailable,
        GitCondition::NoRepo,
        GitCondition::Repo {
            state: RepoState::Clean,
        },
        GitCondition::Repo {
            state: RepoState::Rebasing,
        },
        GitCondition::Error {
            reason: "x".to_owned(),
        },
    ];
    let agents = [
        AgentCondition::Off,
        AgentCondition::On,
        AgentCondition::Connected {
            client: "c".to_owned(),
        },
    ];
    for workspace in workspaces {
        for git in &gits {
            for permark in [
                PermarkCondition::NotInitialized,
                PermarkCondition::Initialized,
            ] {
                for agent in &agents {
                    for live in [true, false] {
                        for distribution in [Distribution::Direct, Distribution::Store] {
                            for platform in [Platform::Windows, Platform::Mac, Platform::Linux] {
                                all.push(Context {
                                    workspace,
                                    git: git.clone(),
                                    permark,
                                    agent: agent.clone(),
                                    live_updates: if live {
                                        LiveUpdates::Available
                                    } else {
                                        LiveUpdates::Unavailable {
                                            reason: "r".to_owned(),
                                        }
                                    },
                                    distribution,
                                    platform,
                                    plugins: BTreeSet::from([PluginId::of("git")]),
                                    agent_files: Vec::new(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    all
}

fn every_need() -> Vec<Need> {
    vec![
        Need::Workspace {
            kinds: Cow::Borrowed(&[WorkspaceKind::Project]),
        },
        Need::GitRepo,
        Need::GitAvailable,
        Need::Permark,
        Need::AgentOn,
        Need::AgentConnected,
        Need::LiveUpdates,
        Need::Distribution {
            distribution: Distribution::Store,
        },
        Need::Platform {
            platforms: Cow::Borrowed(&[Platform::Mac, Platform::Linux]),
        },
        Need::Plugin {
            plugin: PluginId::of("git"),
        },
        Need::Plugin {
            plugin: PluginId::of("tags"),
        },
    ]
}

/// Independent oracle: the definition of each need, written against the context fields.
fn oracle(need: &Need, c: &Context) -> bool {
    match need {
        Need::Workspace { .. } => c.workspace == WorkspaceKind::Project,
        Need::GitRepo => matches!(c.git, GitCondition::Repo { .. }),
        Need::GitAvailable => !matches!(c.git, GitCondition::Unavailable),
        Need::Permark => c.permark == PermarkCondition::Initialized,
        Need::AgentOn => !matches!(c.agent, AgentCondition::Off),
        Need::AgentConnected => matches!(c.agent, AgentCondition::Connected { .. }),
        Need::LiveUpdates => matches!(c.live_updates, LiveUpdates::Available),
        Need::Distribution { .. } => c.distribution == Distribution::Store,
        Need::Platform { .. } => c.platform != Platform::Windows,
        Need::Plugin { plugin } => plugin.as_str() == "git",
    }
}

#[test]
fn every_need_against_every_context_variant() {
    let contexts = contexts();
    assert_eq!(contexts.len(), 4 * 5 * 2 * 3 * 2 * 2 * 3);
    for need in every_need() {
        for context in &contexts {
            let as_need = Requirement {
                needs: Cow::Owned(vec![need.clone()]),
                prefers: Cow::Borrowed(&[]),
            };
            let as_prefer = Requirement {
                needs: Cow::Borrowed(&[]),
                prefers: Cow::Owned(vec![need.clone()]),
            };
            let expected = oracle(&need, context);
            match available(&as_need, context) {
                Availability::Available { degraded } => {
                    assert!(expected && degraded.is_empty(), "{need:?} {context:?}");
                }
                Availability::Unavailable { reason } => {
                    assert!(!expected, "{need:?} {context:?}");
                    assert_eq!(reason.need, need);
                    assert_eq!(reason.remedy, remedy(&need));
                }
            }
            // prefers never make it unavailable
            let Availability::Available { degraded } = available(&as_prefer, context) else {
                panic!("prefers made it unavailable");
            };
            assert_eq!(degraded.is_empty(), expected);
        }
    }
}

#[test]
fn reason_serializes_flat() {
    let reason = Reason {
        need: Need::Permark,
        remedy: Some("project.init".to_owned()),
    };
    assert_eq!(
        serde_json::to_string(&reason).unwrap(),
        r#"{"need":"permark","remedy":"project.init"}"#
    );
}

#[test]
fn malformed_requirements_are_rejected_at_the_boundary() {
    for bad in [
        r#"{"needs":[{"need":"teleport"}],"prefers":[]}"#,
        r#"{"needs":[{"need":"plugin"}],"prefers":[]}"#,
        r#"{"needs":"permark","prefers":[]}"#,
        r#"{"needs":[]}"#,
    ] {
        assert!(serde_json::from_str::<Requirement>(bad).is_err(), "{bad}");
    }
}
