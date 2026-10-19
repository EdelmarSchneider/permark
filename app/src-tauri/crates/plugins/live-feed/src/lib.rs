//! Plugin `live-feed`: the list of everything that changed in the project, as it happens. The watch tells; this keeps
//! the list (coalesced saves, groups for git operations and floods) and hands it to every window that asks.

#![cfg_attr(test, allow(clippy::assert_is_empty))]

mod feed;

use std::borrow::Cow;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

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
        }))
    }
}

/// A started feed. Drop = teardown: the watch is not heard any more, nobody is answered.
struct FeedRunning {
    _subscription: Subscription,
    _registrations: Vec<RpcRegistration>,
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
