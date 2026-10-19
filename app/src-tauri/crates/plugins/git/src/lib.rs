//! Plugin `git`: read-only git awareness. Change letters per file (and a count per folder), branch and HEAD, the state
//! of the repository (merging, rebasing, ...), linked worktrees as extra roots, and the committed text of a file for
//! other plugins. Two backends with the same operations: the installed git (an external process) and the built-in one
//! (libgit2). NEVER commits, stages or creates anything.

#![cfg_attr(test, allow(clippy::assert_is_empty))] // `assert!(x.is_empty())` reads best for "nothing found"

mod builtin;
mod ops;
mod porcelain;
mod service;
mod system;
#[cfg(test)]
mod tests;
mod types;
mod worktrees;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use permark_core::conditions::{GitCondition, Requirement};
use permark_core::paths::Root;
use permark_core::plugins::{
    typed, CxWorkspace, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginId, PluginKind,
    PluginScope, ProjectFacts, RepoHandle, RpcRegistration, Running, ServiceLease,
};
use permark_core::settings::GitBackendPref;
use permark_core::watch::{is_git_signal, Subscription};
use serde::Deserialize;

pub use builtin::BuiltinGit;
pub use ops::{Baseline, Discovery, GitError, GitOps, RawFile, RawStatus, RepoLocation, MAX_FILES};
pub use service::{BackendChoice, GitService, Prefs, ServiceArgs, Timings, Visit, Why};
pub use system::{find_git, SystemGit};
pub use types::{
    BackendKind, BaselineInfo, BaselineKind, GitFile, GitSnapshot, HeadInfo, Letter, WorktreeInfo,
};
pub use worktrees::{branch_of, linked_worktrees, Linked};

const ID: PluginId = PluginId::of("git");

fn manifest() -> Manifest {
    Manifest {
        id: ID,
        kind: PluginKind::Feature,
        scope: PluginScope::Project,
        requires: Requirement::PROJECT,
        depends_on: Cow::Borrowed(&[]),
        default_enabled: true,
    }
}

struct GitPlugin {
    manifest: Manifest,
}

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    Arc::new(GitPlugin {
        manifest: manifest(),
    })
}

/// The backend setting in the plugin's table (`backend = "system"`).
fn backend_pref(settings: Option<&toml_edit::Table>) -> GitBackendPref {
    settings
        .and_then(|table| table.get("backend"))
        .and_then(|item| item.as_str())
        .and_then(GitBackendPref::parse)
        .unwrap_or_default()
}

#[derive(Deserialize)]
struct BaselineArgs {
    kind: BaselineKind,
}

impl Plugin for GitPlugin {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn roots(&self, root: &Root) -> Vec<(String, PathBuf)> {
        let home = dirs::home_dir();
        linked_worktrees(root.path(), home.as_deref())
            .into_iter()
            .map(|linked| (linked.name, linked.folder))
            .collect()
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let (Some(project), CxWorkspace::Project(vault)) = (cx.project(), &cx.workspace) else {
            return Err(PluginError::Start {
                id: ID,
                reason: "needs a project".to_owned(),
            });
        };
        let events = cx.events().clone();
        let announce = Arc::new(move |snapshot: Option<&GitSnapshot>| {
            events.emit(
                "snapshot",
                serde_json::to_value(snapshot).unwrap_or(serde_json::Value::Null),
            );
        });
        let service = Arc::new(GitService::start(ServiceArgs {
            root: vault.main().path().to_path_buf(),
            worktrees: vault
                .worktrees()
                .iter()
                .map(|w| (w.name.clone(), w.root.path().to_path_buf()))
                .collect(),
            choice: BackendChoice::Pref(backend_pref(cx.settings.as_ref())),
            home: dirs::home_dir(),
            timings: Timings::default(),
            facts: Arc::clone(&project.facts),
            announce,
            prefs: cx.state_file("prefs.json"),
            visit: cx.state_file("visit.json"),
        }));
        let trigger = service.trigger_fn();
        let subscription = cx.watch().map(|hub| {
            hub.subscribe(move |batch| {
                let signal = batch.changes.iter().any(|c| is_git_signal(&c.path));
                trigger(if signal { Why::Signal } else { Why::Files });
            })
        });
        let mut registrations = Vec::new();
        let snapshot = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "snapshot",
            typed(move |_: NoArgs, _| Ok(snapshot.snapshot().map(|s| (*s).clone()))),
        ));
        let refresher = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "refresh",
            typed(move |_: NoArgs, _| {
                refresher.trigger(Why::Focus);
                Ok(())
            }),
        ));
        let baseline = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "setBaseline",
            typed(move |args: BaselineArgs, _| {
                baseline.set_baseline(args.kind);
                Ok(())
            }),
        ));
        let lease = project
            .services
            .provide(Arc::new(RepoHandle(service.reader())));
        Ok(Box::new(GitRunning {
            facts: Arc::clone(&project.facts),
            subscription: Some(subscription).flatten(),
            registrations,
            lease: Some(lease),
            service: Some(service),
        }))
    }
}

/// A started `git` plugin. Drop = teardown: stops hearing the watch, stops answering, withdraws the reader, joins
/// the worker (which remembers where HEAD is), and the project has no git condition any more.
struct GitRunning {
    facts: Arc<ProjectFacts>,
    subscription: Option<Subscription>,
    registrations: Vec<RpcRegistration>,
    lease: Option<ServiceLease>,
    service: Option<Arc<GitService>>,
}

impl Running for GitRunning {}

impl Drop for GitRunning {
    fn drop(&mut self) {
        self.subscription.take();
        self.registrations.clear();
        self.lease.take();
        self.service.take();
        self.facts.set_git(GitCondition::NoRepo);
    }
}
