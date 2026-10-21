//! Plugin `live-feed`: the list of everything that changed in the project, as it happens. The watch tells; this keeps
//! the list (coalesced saves, groups for git operations and floods) and hands it to every window that asks. Beside it,
//! the person's own actions from the project's activity journal (and the hand-off marks), for the panel's
//! mine | outside | all filter.

#![cfg_attr(test, allow(clippy::assert_is_empty))]

mod feed;

use std::borrow::Cow;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use permark_core::activity::{By, Entry, Fact, Journal, Listening};
use permark_core::conditions::Requirement;
use permark_core::plugins::{
    typed, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginId, PluginKind, PluginScope,
    RpcRegistration, Running,
};
use permark_core::watch::Subscription;
use serde_json::json;

pub use feed::{Feed, FeedEntryKind, FeedGroupReason, FeedItem, COALESCE_MS, MAX_ITEMS};

const ID: PluginId = PluginId::of("live-feed");

struct LiveFeedPlugin {
    manifest: Manifest,
}

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    Arc::new(LiveFeedPlugin {
        manifest: Manifest {
            id: ID,
            kind: PluginKind::Feature,
            scope: PluginScope::Project,
            requires: Requirement::PROJECT,
            depends_on: Cow::Borrowed(&[]),
            default_enabled: true,
        },
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

impl Plugin for LiveFeedPlugin {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let Some(hub) = cx.watch() else {
            return Err(PluginError::Start {
                id: ID,
                reason: "needs a project".to_owned(),
            });
        };
        let feed = Arc::new(Mutex::new(Feed::new()));
        let events = cx.events().clone();
        // the person's actions as the journal notes them (hand-offs included, closes not)
        let journal = cx.project().map(|p| Arc::clone(&p.activity));
        let listening = journal.as_ref().map(|journal| {
            let events = events.clone();
            journal.listen(Arc::new(move |entry: &Entry| {
                if entry.by == By::Person && entry.fact != Fact::Closed {
                    events.emit("activity", json!({ "entry": entry }));
                }
            }))
        });
        let subscription = hub.subscribe({
            let feed = Arc::clone(&feed);
            move |batch| {
                let upserts = feed
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .apply(batch, now_ms());
                if !upserts.is_empty() {
                    events.emit("items", json!({ "upsert": upserts }));
                }
            }
        });
        let mut registrations = Vec::new();
        let listing = Arc::clone(&feed);
        registrations.extend(cx.rpc(
            "items",
            typed(move |_: NoArgs, _| {
                Ok(listing
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .items())
            }),
        ));
        registrations.extend(cx.rpc(
            "activity",
            typed(move |_: NoArgs, _| {
                Ok(journal
                    .as_ref()
                    .map_or_else(Vec::new, |j| mine(&j.snapshot())))
            }),
        ));
        let clearing = Arc::clone(&feed);
        registrations.extend(cx.rpc(
            "clear",
            typed(move |_: NoArgs, _| {
                clearing
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clear();
                Ok(())
            }),
        ));
        Ok(Box::new(FeedRunning {
            _subscription: subscription,
            _registrations: registrations,
            _listening: listening,
        }))
    }
}

/// A started feed. Drop = teardown: the watch is not heard any more, nobody is answered.
struct FeedRunning {
    _subscription: Subscription,
    _registrations: Vec<RpcRegistration>,
    _listening: Option<Listening>,
}

/// The person's latest actions (and hand-off marks), newest last, at most [`MAX_ITEMS`].
fn mine(journal: &Journal) -> Vec<Entry> {
    let mut items: Vec<Entry> = journal
        .entries
        .iter()
        .rev()
        .filter(|e| e.by == By::Person && e.fact != Fact::Closed)
        .take(MAX_ITEMS)
        .cloned()
        .collect();
    items.reverse();
    items
}

impl Running for FeedRunning {}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use permark_core::conditions::Distribution;
    use permark_core::events::NoEvents;
    use permark_core::paths::Root;
    use permark_core::plugins::{PluginRegistry, RpcCall};
    use permark_core::settings::AppSettings;
    use permark_core::state::AppDirs;
    use permark_core::workspace::{CoreEnv, ProjectCore};

    use super::*;

    fn items_of(core: &ProjectCore) -> Vec<serde_json::Value> {
        core.rpc()
            .call(
                "live-feed",
                "items",
                &RpcCall {
                    window: "w".to_owned(),
                    args: serde_json::Value::Null,
                },
            )
            .map(|v| v.as_array().cloned().unwrap_or_default())
            .unwrap_or_default()
    }

    #[test]
    fn the_persons_actions_and_hand_offs_come_from_the_journal() {
        let base = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let env = CoreEnv::new(
            AppDirs::under(base.path()),
            Arc::new(PluginRegistry::new(vec![plugin()]).unwrap()),
            Arc::new(NoEvents),
            Distribution::Direct,
            AppSettings::default(),
        );
        let core = ProjectCore::open(Root::open(project.path()).unwrap(), &env);
        let journal = core.activity();
        journal.person(Fact::Tag {
            added: true,
            path: "a.md".to_owned(),
            tag: "x".to_owned(),
        });
        let _ = journal.hand_off(1, None, "rows".to_owned(), 1);
        journal.closed();
        let mine = core
            .rpc()
            .call(
                "live-feed",
                "activity",
                &RpcCall {
                    window: "w".to_owned(),
                    args: serde_json::Value::Null,
                },
            )
            .unwrap();
        let kinds: Vec<&str> = mine
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["fact"]["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["tag", "handOff"], "a close is not listed");
    }

    #[test]
    fn the_feed_follows_a_nested_repo_below_an_ignored_folder() {
        let base = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let put = |rel: &str, text: &str| {
            let path = project.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        // the master repo ignores `hubs/`; `hubs/a` is a repo of its own
        put(".git/HEAD", "ref: refs/heads/main\n");
        put(".gitignore", "hubs/\n");
        put("hubs/a/.git/HEAD", "ref: refs/heads/main\n");
        put("hubs/a/plan.md", "plan");
        let env = CoreEnv::new(
            AppDirs::under(base.path()),
            Arc::new(PluginRegistry::new(vec![plugin()]).unwrap()),
            Arc::new(NoEvents),
            Distribution::Direct,
            AppSettings::default(),
        );
        let core = ProjectCore::open(Root::open(project.path()).unwrap(), &env);
        std::thread::sleep(Duration::from_millis(250));
        put("hubs/loose.md", "the container's own file");
        put("hubs/a/plan.md", "plan written by an agent");
        let started = std::time::Instant::now();
        while !items_of(&core)
            .iter()
            // macOS may report a file the watch never read as created
            .any(|i| {
                i["path"] == "hubs/a/plan.md" && (i["kind"] == "modified" || i["kind"] == "created")
            })
        {
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "the feed heard nothing: {:?}",
                items_of(&core)
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !items_of(&core).iter().any(|i| i["path"] == "hubs/loose.md"),
            "the container's own files stay ignored"
        );
    }

    #[test]
    fn a_project_feed_hears_the_disk_answers_its_windows_and_goes_away_when_off() {
        let base = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("a.md"), "a").unwrap();
        let env = CoreEnv::new(
            AppDirs::under(base.path()),
            Arc::new(PluginRegistry::new(vec![plugin()]).unwrap()),
            Arc::new(NoEvents),
            Distribution::Direct,
            AppSettings::default(),
        );
        let core = ProjectCore::open(Root::open(project.path()).unwrap(), &env);
        std::thread::sleep(Duration::from_millis(250));
        std::fs::write(project.path().join("b ü.md"), "b").unwrap();
        let started = std::time::Instant::now();
        while !items_of(&core)
            .iter()
            .any(|i| i["path"] == "b ü.md" && i["kind"] == "created")
        {
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "the feed heard nothing"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        core.rpc()
            .call(
                "live-feed",
                "clear",
                &RpcCall {
                    window: "w".to_owned(),
                    args: serde_json::Value::Null,
                },
            )
            .unwrap();
        assert!(items_of(&core).is_empty());
        // off: nobody answers
        env.set_settings(AppSettings {
            plugins: std::collections::BTreeMap::from([(PluginId::of("live-feed"), false)]),
            ..AppSettings::default()
        });
        core.apply_plugins(&env);
        assert!(core.rpc().served().is_empty());
    }
}
