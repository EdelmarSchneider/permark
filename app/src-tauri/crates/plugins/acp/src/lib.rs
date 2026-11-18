//! Plugin `acp` (experimental, off by default; UI name "Assistant"): the person's own agent works in a copy of the
//! project's files the agent may see ([`work`]), kept in the profile, never in the project. A real file changes only
//! when the person accepts a change of the copy, hunk by hunk. The agent is spoken to over the Agent Client Protocol
//! ([`session`], [`client`]); which agent comes only from the person's choice in the profile ([`agents`]): a folder
//! never chooses the program the app starts.
//!
//! RPC (project scope): `status {}` -> [`AcpStatus`]; `agents {}` -> [`AgentEntry`]`[]`; `choose` [`ChooseArgs`] ->
//! [`AgentEntry`]`[]`; `prompt` [`PromptArgs`] -> `null` (the reply comes as events); `login` [`LoginArgs`] -> `null`;
//! `cancel {}` | `reset {}` -> `null`; `answer` [`AnswerArgs`] -> `null`; `changes` [`ChangesArgs`] -> the kept
//! [`ChangeList`] (computed at each turn's end | when asked to refresh; empty while no copy exists); `accept | reject`
//! [`DecideArgs`] (hunk keys; `null`: the whole file) | `undo` [`UndoArgs`] -> the updated [`ChangeList`]; `render`
//! [`RenderArgs`] -> [`RenderedReply`] (an agent's finished message as document blocks, [`reply`]). Event
//! `acp:update` -> [`AcpUpdate`].
//!
//! Off | project closed: the agent stops, then the copy is deleted (pending changes with it).

pub mod agents;
pub mod client;
pub mod dto;
pub mod reply;
pub mod session;
#[cfg(test)]
mod testing;
pub mod work;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use permark_core::conditions::Requirement;
use permark_core::plugins::{
    typed, CxWorkspace, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginId, PluginKind,
    PluginScope, ProjectServices, RpcRegistration, Running,
};
use permark_core::{ErrorKind, IpcError};

use crate::client::lock;

pub use agents::{person_launch, Agents, Choice, Plan, CHOICE_FILE};
pub use dto::{
    AcpError, AcpErrorCode, AcpOptionKind, AcpState, AcpStatus, AcpStopReason, AcpToolKind,
    AcpToolStatus, AcpUpdate, AgentEntry, AgentHow, AgentNeed, AnswerArgs, ChangeList, ChangesArgs,
    ChooseArgs, DecideArgs, FileChange, FileChangeKind, HunkView, LoginArgs, LoginKind,
    LoginMethod, PermissionChoice, PromptArgs, PromptSelection, RenderArgs, RenderedReply,
    UndoArgs,
};
pub use session::{Host, Session};
pub use work::{copy_dir, Project, WorkCopy};

const ID: PluginId = PluginId::of("acp");

/// Folder of the copies under the plugin's profile folder for the project.
const WORK: &str = "work";
/// Empty folder the agent's program starts in, beside the copies.
const RUN: &str = "run";

/// The work copy of one instance (`None` until the first prompt makes it).
pub type SharedCopy = Arc<Mutex<Option<WorkCopy>>>;

/// The plugin.
#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    Arc::new(Acp {
        manifest: Manifest {
            id: ID,
            kind: PluginKind::Feature,
            scope: PluginScope::Project,
            requires: Requirement::PROJECT,
            depends_on: Cow::Borrowed(&[]),
            // experimental: starts the person's own program, so it waits until the person turns it on
            default_enabled: false,
        },
    })
}

struct Acp {
    manifest: Manifest,
}

struct Started {
    _rpc: Vec<RpcRegistration>,
    session: Session,
    copy: SharedCopy,
    /// `<profile>/projects/<id>/plugins/acp/work/` (`None`: no state store, so no copy can exist).
    place: Option<PathBuf>,
    /// `<profile>/projects/<id>/plugins/acp/run/`, the agent's start folder.
    run: Option<PathBuf>,
}

impl Running for Started {}

impl Drop for Started {
    fn drop(&mut self) {
        // the agent first: it may be writing into the copy
        self.session.shutdown();
        // off leaves nothing: the copy goes, and whatever an earlier run left in the plugin's own folder
        if let Some(copy) = lock(&self.copy).take() {
            let _ = copy.remove();
        }
        for folder in [&self.place, &self.run].into_iter().flatten() {
            // never follows a link: a link in its place is removed itself. Windows refuses a folder that is still the
            // working folder of an agent process the job just ended: a short retry while it exits
            let _ = permark_core::fsx::retry(&permark_core::fsx::RETRY_DELAYS, || {
                std::fs::remove_dir_all(folder)
            });
        }
    }
}

/// Accept | reject of one file's hunks by key (`None`: the whole file), answering the updated list.
type Decision = fn(&mut WorkCopy, &str, Option<&[String]>) -> Result<ChangeList, IpcError>;

/// Runs `choose` on the copy for `path`; refused while there is none.
fn on_copy(
    copy: &SharedCopy,
    path: &str,
    choose: impl FnOnce(&mut WorkCopy) -> Result<ChangeList, IpcError>,
) -> Result<ChangeList, IpcError> {
    let mut guard = lock(copy);
    let copy = guard.as_mut().ok_or_else(|| {
        IpcError::new(ErrorKind::NotFound)
            .subject(path)
            .detail("noWorkCopy")
    })?;
    choose(copy)
}

fn decide(copy: &SharedCopy, args: &DecideArgs, choose: Decision) -> Result<ChangeList, IpcError> {
    on_copy(copy, &args.path, |copy| {
        choose(copy, &args.path, args.hunks.as_deref())
    })
}

fn register(
    cx: &PluginCx,
    copy: &SharedCopy,
    session: &Session,
    services: ProjectServices,
    copy_names: Vec<PathBuf>,
) -> Vec<RpcRegistration> {
    let mut rpc = Vec::new();
    rpc.extend(cx.rpc(
        "render",
        typed(move |args: RenderArgs, _| {
            reply::render(
                &args.text,
                &services.formats(),
                services.documents.scope(),
                &copy_names,
            )
        }),
    ));
    let s = session.clone();
    rpc.extend(cx.rpc("status", typed(move |_: NoArgs, _| Ok(s.status()))));
    let s = session.clone();
    rpc.extend(cx.rpc("agents", typed(move |_: NoArgs, _| Ok(s.agents()))));
    let s = session.clone();
    rpc.extend(cx.rpc("choose", typed(move |args: ChooseArgs, _| s.choose(&args))));
    let s = session.clone();
    rpc.extend(cx.rpc("prompt", typed(move |args: PromptArgs, _| s.prompt(args))));
    let s = session.clone();
    rpc.extend(cx.rpc("login", typed(move |args: LoginArgs, _| s.login(&args))));
    let s = session.clone();
    rpc.extend(cx.rpc(
        "cancel",
        typed(move |_: NoArgs, _| {
            s.cancel();
            Ok(())
        }),
    ));
    let s = session.clone();
    rpc.extend(cx.rpc(
        "reset",
        typed(move |_: NoArgs, _| {
            s.reset();
            Ok(())
        }),
    ));
    let s = session.clone();
    rpc.extend(cx.rpc("answer", typed(move |args: AnswerArgs, _| s.answer(&args))));
    let c = Arc::clone(copy);
    rpc.extend(cx.rpc(
        "changes",
        typed(move |args: ChangesArgs, _| match lock(&c).as_mut() {
            Some(copy) if args.refresh == Some(true) => copy.refresh(),
            Some(copy) => copy.changes(),
            None => Ok(ChangeList::default()),
        }),
    ));
    let c = Arc::clone(copy);
    rpc.extend(cx.rpc(
        "accept",
        typed(move |args: DecideArgs, _| decide(&c, &args, WorkCopy::accept)),
    ));
    let c = Arc::clone(copy);
    rpc.extend(cx.rpc(
        "reject",
        typed(move |args: DecideArgs, _| decide(&c, &args, WorkCopy::reject)),
    ));
    let c = Arc::clone(copy);
    rpc.extend(cx.rpc(
        "undo",
        typed(move |args: UndoArgs, _| on_copy(&c, &args.path, |copy| copy.undo(&args.path))),
    ));
    rpc
}

impl Plugin for Acp {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let (Some(services), CxWorkspace::Project(vault)) = (cx.project(), &cx.workspace) else {
            return Err(PluginError::Start {
                id: ID,
                reason: "needs a project".to_owned(),
            });
        };
        let copy: SharedCopy = Arc::new(Mutex::new(None));
        let own = cx.store().map(|store| store.profile_plugin_dir(&cx.id));
        let place = own.as_ref().map(|dir| dir.join(WORK));
        let run = own.map(|dir| dir.join(RUN));
        let copy_names = reply::copy_spellings(place.as_deref(), vault.main());
        // the profile folder itself, whatever the project holds: read at every agent start
        let choice = cx
            .store()
            .map(|store| store.app_file::<Choice>(CHOICE_FILE));
        let session = Session::new(Host {
            agents: Agents::new(choice),
            launch: person_launch(),
            project: Project::of(vault, services),
            place: place.clone(),
            run: run.clone(),
            copy: Arc::clone(&copy),
            events: cx.events().clone(),
        });
        Ok(Box::new(Started {
            _rpc: register(cx, &copy, &session, services.clone(), copy_names),
            session,
            place,
            run,
            copy,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use permark_core::conditions::{Context, Distribution};
    use permark_core::events::NoEvents;

    use super::*;

    #[test]
    fn the_plugin_is_off_until_the_person_turns_it_on() {
        let plugin = plugin();
        let manifest = plugin.manifest();
        assert_eq!(manifest.id.as_str(), "acp");
        assert_eq!(manifest.scope, PluginScope::Project);
        assert_eq!(manifest.requires, Requirement::PROJECT);
        assert!(manifest.depends_on.is_empty());
        assert!(!manifest.default_enabled);
    }

    #[test]
    fn it_needs_a_project() {
        let dir = tempfile::tempdir().unwrap();
        let cx = PluginCx::new(
            ID,
            CxWorkspace::App,
            dir.path().to_path_buf(),
            None,
            Context::welcome(Distribution::Direct),
            Arc::new(NoEvents),
        );
        assert!(plugin().start(&cx).is_err());
    }

    /// Turning the plugin off removes its work and start folders: the copy and anything an earlier run left there.
    #[test]
    fn stopping_leaves_nothing() {
        let place = tempfile::tempdir().unwrap();
        let work = place.path().join(WORK);
        let run = place.path().join(RUN);
        std::fs::create_dir_all(work.join("old-copy")).unwrap();
        std::fs::write(work.join("old-copy").join("a.md"), "a").unwrap();
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(run.join(".npmrc"), "x").unwrap();
        let project = tempfile::tempdir().unwrap();
        let (vault, documents) = testing::open(project.path());
        let copy: SharedCopy = Arc::new(Mutex::new(None));
        let session = Session::new(Host {
            agents: Agents::new(None),
            launch: Arc::new(|_: &Plan, _: &std::path::Path| {
                Err(std::io::Error::other("never started"))
            }),
            project: testing::project(&vault, &documents),
            place: Some(work.clone()),
            run: Some(run.clone()),
            copy: Arc::clone(&copy),
            events: testing::events(Arc::new(NoEvents)),
        });
        let started = Started {
            _rpc: Vec::new(),
            session,
            copy,
            place: Some(work.clone()),
            run: Some(run.clone()),
        };
        drop(started);
        assert!(!work.exists());
        assert!(!run.exists());
        assert!(place.path().exists());
    }
}
