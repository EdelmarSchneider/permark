//! Plugin `acp` (experimental, off by default; UI name "Assistant"): the person's own agent works in a copy of the
//! project's files the agent may see ([`work`]), kept in the profile, never in the project. A real file changes only
//! when the person accepts a change of the copy, hunk by hunk. The agent is spoken to over the Agent Client Protocol
//! ([`session`], [`client`]); its command comes only from the person's app settings (`[plugins.acp] command`).
//!
//! RPC (project scope): `status {}` -> [`AcpStatus`]; `prompt` [`PromptArgs`] -> `null` (the reply comes as events);
//! `cancel {}` | `reset {}` -> `null`; `answer` [`AnswerArgs`] -> `null`; `changes {}` -> [`FileChange`]`[]` (empty
//! while no copy exists); `accept | reject { path, hunks: number[] | null }` -> the fresh [`FileChange`]`[]`
//! (`null`: the whole file). Event `acp:update` -> [`AcpUpdate`].
//!
//! Off | project closed: the agent stops, then the copy is deleted (pending changes with it).

pub mod client;
pub mod dto;
pub mod session;
#[cfg(test)]
mod testing;
pub mod work;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use permark_core::conditions::Requirement;
use permark_core::plugins::{
    typed, CxWorkspace, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginId, PluginKind,
    PluginScope, RpcRegistration, Running,
};
use permark_core::{ErrorKind, IpcError};

pub use dto::{
    AcpState, AcpStatus, AcpUpdate, AnswerArgs, DecideArgs, FileChange, FileChangeKind, HunkView,
    PermissionChoice, PromptArgs, PromptSelection,
};
pub use session::{person_launch, AgentCommand, Host, Session, DEFAULT_COMMAND};
pub use work::{copy_dir, Project, WorkCopy};

const ID: PluginId = PluginId::of("acp");

/// Folder of the copies under the plugin's profile folder for the project.
const WORK: &str = "work";

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
        if let Some(place) = &self.place {
            // never follows a link: a link in its place is removed itself
            let _ = std::fs::remove_dir_all(place);
        }
    }
}

fn lock(copy: &SharedCopy) -> MutexGuard<'_, Option<WorkCopy>> {
    copy.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Accept | reject of one file's hunks (`None`: the whole file), answering the fresh list.
type Choice = fn(&mut WorkCopy, &str, Option<&[u32]>) -> Result<Vec<FileChange>, IpcError>;

/// Runs `choose` on the copy; refused while there is none.
fn decide(
    copy: &SharedCopy,
    args: &DecideArgs,
    choose: Choice,
) -> Result<Vec<FileChange>, IpcError> {
    let mut guard = lock(copy);
    let copy = guard.as_mut().ok_or_else(|| {
        IpcError::new(ErrorKind::NotFound)
            .subject(args.path.clone())
            .detail("noWorkCopy")
    })?;
    choose(copy, &args.path, args.hunks.as_deref())
}

fn register(cx: &PluginCx, copy: &SharedCopy, session: &Session) -> Vec<RpcRegistration> {
    let mut rpc = Vec::new();
    let s = session.clone();
    rpc.extend(cx.rpc("status", typed(move |_: NoArgs, _| Ok(s.status()))));
    let s = session.clone();
    rpc.extend(cx.rpc("prompt", typed(move |args: PromptArgs, _| s.prompt(args))));
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
        typed(move |_: NoArgs, _| match lock(&c).as_mut() {
            Some(copy) => copy.changes(),
            None => Ok(Vec::new()),
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
        let place = cx
            .store()
            .map(|store| store.profile_plugin_dir(&cx.id).join(WORK));
        // read once: a changed command takes effect when the plugin starts again
        let command = AgentCommand::from_settings(cx.settings.as_ref());
        let session = Session::new(Host {
            launch: person_launch(&command),
            command,
            project: Project::of(vault, services),
            place: place.clone(),
            copy: Arc::clone(&copy),
            events: cx.events().clone(),
        });
        Ok(Box::new(Started {
            _rpc: register(cx, &copy, &session),
            session,
            place,
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

    /// Turning the plugin off removes its work folder: the copy and anything an earlier run left there.
    #[test]
    fn stopping_leaves_nothing() {
        let place = tempfile::tempdir().unwrap();
        let work = place.path().join(WORK);
        std::fs::create_dir_all(work.join("old-copy")).unwrap();
        std::fs::write(work.join("old-copy").join("a.md"), "a").unwrap();
        let project = tempfile::tempdir().unwrap();
        let (vault, documents) = testing::open(project.path());
        let copy: SharedCopy = Arc::new(Mutex::new(None));
        let session = Session::new(Host {
            command: AgentCommand::default(),
            launch: Arc::new(|_: &std::path::Path| Err(std::io::Error::other("never started"))),
            project: testing::project(&vault, &documents),
            place: Some(work.clone()),
            copy: Arc::clone(&copy),
            events: testing::events(Arc::new(NoEvents)),
        });
        let started = Started {
            _rpc: Vec::new(),
            session,
            copy,
            place: Some(work.clone()),
        };
        drop(started);
        assert!(!work.exists());
        assert!(place.path().exists());
    }
}
