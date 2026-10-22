//! Plugin `git`: read-only git awareness. Change letters per file (and a count per folder), branch and HEAD, the state
//! of the repository (merging, rebasing, ...), linked worktrees as extra roots, and the committed text of a file for
//! other plugins. Two backends with the same operations: the installed git (an external process) and the built-in one
//! (libgit2). NEVER commits, stages or creates anything.

#![cfg_attr(test, allow(clippy::assert_is_empty))] // `assert!(x.is_empty())` reads best for "nothing found"

mod builtin;
mod cheap;
mod members;
#[cfg(test)]
mod members_real_tests;
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
use std::sync::{Arc, Weak};
use std::time::Duration;

use permark_core::conditions::{GitCondition, Requirement};
use permark_core::paths::{Root, VaultPath, WORKTREE_PREFIX};
use permark_core::plugins::{
    typed, CxWorkspace, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginId, PluginKind,
    PluginScope, ProjectFacts, RepoHandle, RepoReader, RpcRegistration, Running, ServiceLease,
};
use permark_core::projects::{ProjectMap, ProjectUid, Projects, ProjectsSubscription};
use permark_core::settings::GitBackendPref;
use permark_core::watch::{is_git_signal, FsBatch, Subscription};
use serde::Deserialize;

pub use builtin::BuiltinGit;
pub use cheap::{cheap_facts, member_location, Cheap};
pub use members::{
    step, Env, MemberRef, Members, RepoEvent, RepoFsm, RootCommits, Schedule, MAX_LISTS,
    POOL_THREADS,
};
pub use ops::{Baseline, Discovery, GitError, GitOps, RawFile, RawStatus, RepoLocation, MAX_FILES};
pub use service::{
    BackendCell, BackendChoice, GitService, Prefs, ServiceArgs, Timings, Visit, Why,
};
pub use system::{find_git, SystemGit};
pub use types::{
    BackendKind, BaselineInfo, BaselineKind, CommitLine, GitFile, GitSnapshot, HeadInfo,
    InterestLevel, Letter, ProjectSnapshot, RepoError, RepoSummary, ReposChanged, WorktreeInfo,
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

/// `{ project? }`: a project's repository (uid); absent = the root repository.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ProjectArg {
    project: Option<String>,
}

#[derive(Deserialize)]
struct InterestArgs {
    ids: Vec<String>,
    level: InterestLevel,
    /// The view (tab) asking, when a window holds several: each view's interest is its own.
    #[serde(default)]
    view: Option<String>,
}

/// The included projects with their own repository in a map.
fn member_refs(map: &ProjectMap, root: &std::path::Path) -> Vec<MemberRef> {
    map.members()
        .iter()
        .filter(|m| m.marker.git())
        .map(|m| {
            let mut folder = root.to_path_buf();
            folder.extend(m.path.segments());
            MemberRef {
                uid: m.uid.as_str().to_owned(),
                path: m.path.as_str().to_owned(),
                key: m.key.clone(),
                folder,
                first_commit: m.first_commit.is_some(),
            }
        })
        .collect()
}

/// The deepest included project with its own repository holding `path` (uid, its path).
fn git_owner<'m>(
    map: &'m ProjectMap,
    path: &str,
) -> Option<&'m permark_core::projects::MemberInfo> {
    map.chain(path).into_iter().rev().find(|m| m.marker.git())
}

/// Where a batch goes (`projects.md` §8): the root repository (why), and member repositories (uid, `.git` signal).
/// Paths inside a member repository never wake the root one; a rescan wakes the root (members: [`Members::outdated`]).
#[must_use]
pub fn route(map: Option<&ProjectMap>, batch: &FsBatch) -> (Option<Why>, Vec<(String, bool)>) {
    let mut root: Option<Why> = batch.rescan.then_some(Why::Files);
    let mut members: Vec<(String, bool)> = Vec::new();
    for change in &batch.changes {
        let signal = is_git_signal(&change.path);
        match map.and_then(|m| git_owner(m, &change.path)) {
            Some(member) => {
                let uid = member.uid.as_str();
                match members.iter_mut().find(|(u, _)| u == uid) {
                    Some(entry) => entry.1 |= signal,
                    None => members.push((uid.to_owned(), signal)),
                }
            }
            None => {
                root = Some(if signal || root == Some(Why::Signal) {
                    Why::Signal
                } else {
                    Why::Files
                });
            }
        }
    }
    (root, members)
}

/// The HEAD reader other plugins use (review's "last commit"): a path inside an included project with its own
/// repository is read from that repository (path rebased), else from the root's.
struct Routed {
    root: Arc<dyn RepoReader>,
    members: Weak<Members>,
    projects: Option<Weak<Projects>>,
    backends: Arc<BackendCell>,
}

impl RepoReader for Routed {
    fn blob_at_head(&self, path: &str) -> Option<Vec<u8>> {
        if !path.starts_with(WORKTREE_PREFIX) {
            if let Some(projects) = self.projects.as_ref().and_then(Weak::upgrade) {
                let map = projects.snapshot();
                if let Some(member) = git_owner(&map, path) {
                    let members = self.members.upgrade()?;
                    let location = members.location(member.uid.as_str())?;
                    let depth = member.path.segments().count();
                    let rel = path.split('/').skip(depth).collect::<Vec<_>>().join("/");
                    let backend = self.backends.get()?;
                    // one of the pool's places: member git work stays bounded (`projects.md` §8)
                    return members
                        .with_permit(|| backend.blob_at_head(&location, &rel).ok().flatten())
                        .flatten();
                }
            }
        }
        self.root.blob_at_head(path)
    }

    fn head(&self) -> Option<String> {
        self.root.head()
    }

    fn commits_between(&self, old: &str, new: &str) -> Option<u32> {
        self.root.commits_between(old, new)
    }
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

    #[allow(clippy::too_many_lines)] // one place wires the root worker, the member pool and every method
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
        let root_path = vault.main().path().to_path_buf();
        let projects: Option<Arc<Projects>> = project.projects().cloned();
        // members known at open leave the root status from its first look (verifier N2)
        let excluded: Vec<String> = projects
            .as_ref()
            .map(|p| {
                member_refs(&p.snapshot(), &root_path)
                    .into_iter()
                    .map(|m| m.key)
                    .collect()
            })
            .unwrap_or_default();
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
            excluded,
        }));
        let weak_projects = projects.as_ref().map(Arc::downgrade);
        let members = Arc::new(Members::new(members_env(
            cx,
            &service,
            weak_projects.clone(),
        )));
        // members follow the projects map: started, stopped, moved
        let (map_lease, identity) = match &projects {
            Some(projects) => {
                let weak = Arc::downgrade(&members);
                let root = root_path.clone();
                let lease = projects.subscribe_map(move |map| {
                    if let Some(members) = weak.upgrade() {
                        members.sync(&member_refs(map, &root));
                    }
                });
                members.sync(&member_refs(&projects.snapshot(), &root_path));
                // root commits for re-attach: looked up in the member pool, answered later
                let asking = Arc::downgrade(&members);
                let identity = projects.provide_root_commits(Arc::new(move |folders| {
                    if let Some(members) = asking.upgrade() {
                        members.look_up_roots(folders);
                    }
                }));
                (Some(lease), Some(identity))
            }
            None => (None, None),
        };
        let trigger = service.trigger_fn();
        let routed_members = Arc::downgrade(&members);
        let routed_projects = weak_projects.clone();
        let subscription = cx.watch().map(|hub| {
            hub.subscribe(move |batch| {
                let map = routed_projects
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .map(|p| p.snapshot());
                let (root, changed) = route(map.as_deref(), batch);
                if let Some(why) = root {
                    trigger(why);
                }
                if let Some(members) = routed_members.upgrade() {
                    if batch.rescan {
                        members.outdated();
                    }
                    members.changed(&changed);
                }
            })
        });
        let mut registrations = Vec::new();
        let snapshot = Arc::clone(&service);
        let snap_members = Arc::downgrade(&members);
        registrations.extend(cx.rpc(
            "snapshot",
            typed(move |args: ProjectArg, _| {
                Ok(match args.project {
                    None => snapshot.snapshot().map(|s| (*s).clone()),
                    Some(uid) => snap_members.upgrade().and_then(|m| m.snapshot(&uid)),
                })
            }),
        ));
        let refresher = Arc::clone(&service);
        let refresh_members = Arc::downgrade(&members);
        registrations.extend(cx.rpc(
            "refresh",
            typed(move |args: ProjectArg, _| {
                if args.project.is_none() {
                    refresher.trigger(Why::Focus);
                }
                if let Some(members) = refresh_members.upgrade() {
                    members.focus(args.project.as_deref());
                }
                Ok(())
            }),
        ));
        let baseline = Arc::clone(&service);
        let baseline_members = Arc::downgrade(&members);
        registrations.extend(cx.rpc(
            "setBaseline",
            typed(move |args: BaselineArgs, _| {
                let before = baseline.baseline();
                baseline.set_baseline(args.kind);
                if before != args.kind {
                    if let Some(members) = baseline_members.upgrade() {
                        members.baseline_changed();
                    }
                }
                Ok(())
            }),
        ));
        let repos_members = Arc::downgrade(&members);
        registrations.extend(cx.rpc(
            "repos",
            typed(move |_: NoArgs, _| {
                Ok(repos_members
                    .upgrade()
                    .map(|m| m.summaries())
                    .unwrap_or_default())
            }),
        ));
        // a view that lets go of the project takes its interest with it (`projects.md` §8 RAII)
        let gone_members = Arc::downgrade(&members);
        registrations.extend(cx.on_caller_gone(Arc::new(move |caller: &str| {
            if let Some(members) = gone_members.upgrade() {
                members.forget_caller(caller);
            }
        })));
        let interest_members = Arc::downgrade(&members);
        registrations.extend(cx.rpc(
            "interest",
            typed(move |args: InterestArgs, call| {
                if let Some(members) = interest_members.upgrade() {
                    let who = match &args.view {
                        Some(view) => format!("{}/{view}", call.window),
                        None => call.window.clone(),
                    };
                    members.interest(&who, args.level, &args.ids);
                }
                Ok(())
            }),
        ));
        let lease = project
            .services
            .provide(Arc::new(RepoHandle(Arc::new(Routed {
                root: service.reader(),
                members: Arc::downgrade(&members),
                projects: weak_projects,
                backends: service.backends(),
            }))));
        Ok(Box::new(GitRunning {
            facts: Arc::clone(&project.facts),
            subscription: Some(subscription).flatten(),
            map_lease,
            identity,
            registrations,
            lease: Some(lease),
            members: Some(members),
            service: Some(service),
        }))
    }
}

/// What the member pool needs from the plugin's surroundings.
fn members_env(cx: &PluginCx, service: &Arc<GitService>, projects: Option<Weak<Projects>>) -> Env {
    let repos_events = cx.events().clone();
    let snapshot_events = cx.events().clone();
    let baseline_of = Arc::downgrade(service);
    let excluding = Arc::downgrade(service);
    let visit_projects = projects.clone();
    let root_projects = projects.clone();
    let files_only = cx.files_only();
    Env {
        backends: service.backends(),
        timings: Timings::default(),
        on_repos: Arc::new(move |changed: &ReposChanged| {
            repos_events.emit(
                "repos",
                serde_json::to_value(changed).unwrap_or(serde_json::Value::Null),
            );
        }),
        on_snapshot: Arc::new(move |snapshot: &ProjectSnapshot| {
            snapshot_events.emit(
                "projectSnapshot",
                serde_json::to_value(snapshot).unwrap_or(serde_json::Value::Null),
            );
        }),
        baseline: Arc::new(move || {
            baseline_of
                .upgrade()
                .map_or(BaselineKind::LastVisit, |s| s.baseline())
        }),
        exclude: Arc::new(move |keys| {
            if let Some(service) = excluding.upgrade() {
                service.set_excluded(keys);
            }
        }),
        // the file the project uses when opened alone (`projects.md` §8: per-repo `visit.json`)
        visit_file: Arc::new(move |path: &str| {
            if files_only {
                return None;
            }
            let projects = visit_projects.as_ref()?.upgrade()?;
            let store = projects.store_of(&VaultPath::parse(path).ok()?)?;
            Some(store.plugin_file(&ID, "visit.json"))
        }),
        first_commits: Arc::new(move |learned: &[(String, String)]| {
            let Some(projects) = projects.as_ref().and_then(Weak::upgrade) else {
                return;
            };
            let learned: Vec<(ProjectUid, String)> = learned
                .iter()
                .filter_map(|(uid, oid)| Some((ProjectUid::parse(uid)?, oid.clone())))
                .collect();
            let _ = projects.set_first_commits(&learned);
        }),
        root_commits: Arc::new(move |answers: &[(String, Option<String>)]| {
            if let Some(projects) = root_projects.as_ref().and_then(Weak::upgrade) {
                projects.root_commits_learned(answers);
            }
        }),
        announce_after: Duration::from_millis(150),
        first_commit_after: Duration::from_secs(2),
    }
}

/// A started `git` plugin. Drop = teardown: stops hearing the watch and the projects map, stops answering,
/// withdraws the reader, stops the member repositories (work cancelled, pool joined, visits remembered), joins the
/// root worker (which remembers where HEAD is), and the project has no git condition any more.
struct GitRunning {
    facts: Arc<ProjectFacts>,
    subscription: Option<Subscription>,
    map_lease: Option<ProjectsSubscription>,
    identity: Option<ProjectsSubscription>,
    registrations: Vec<RpcRegistration>,
    lease: Option<ServiceLease>,
    members: Option<Arc<Members>>,
    service: Option<Arc<GitService>>,
}

impl Running for GitRunning {}

impl Drop for GitRunning {
    fn drop(&mut self) {
        self.subscription.take();
        self.map_lease.take();
        self.identity.take();
        self.registrations.clear();
        self.lease.take();
        self.members.take();
        self.service.take();
        self.facts.set_git(GitCondition::NoRepo);
    }
}
