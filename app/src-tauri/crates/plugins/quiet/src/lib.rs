//! Plugin `quiet`: silence a folder or a file. It stays in the tree (dimmed) and can be opened and searched, but its
//! changes stop calling for attention: no unread dot, no feed row, no toast, no follow jump, no git letter. Personal
//! (project local state, `quiet.json` in the layout of 0.5). Agents never learn the marks: a silenced path is simply
//! not shared with them.
//!
//! This side keeps the marks, answers search (`SilenceApi`) and hands the marks to the window, which draws the effects.

#![cfg_attr(test, allow(clippy::assert_is_empty))]

mod marks;
#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use permark_core::conditions::Requirement;
use permark_core::error::{ErrorKind, IpcError};
use permark_core::organizing::{SilenceApi, SilenceLookup};
use permark_core::paths::VaultPath;
use permark_core::plugins::{
    typed, CxWorkspace, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginId, PluginKind,
    PluginScope, RpcRegistration, Running, ServiceLease,
};
use permark_core::state::RawFile;
use permark_core::watch::{ChangeKind, FsBatch, Subscription};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use marks::{waking, Interest, Marks};

const ID: PluginId = PluginId::of("quiet");
/// The file in the project's local state.
pub const FILE: &str = "quiet.json";

/// The marks of a project, as the window gets them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QuietMarks {
    pub marks: BTreeMap<String, Interest>,
}

#[derive(Deserialize)]
struct SetArgs {
    paths: Vec<String>,
    interest: String,
}

struct QuietPlugin {
    manifest: Manifest,
}

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    Arc::new(QuietPlugin {
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

/// The marks of one project, kept in `quiet.json`.
pub struct QuietService {
    marks: RwLock<Marks>,
    file: RawFile,
    /// The file existed and was not marks: it is put aside before the first write.
    damaged: Mutex<bool>,
    announce: Arc<dyn Fn(&QuietMarks) + Send + Sync>,
}

impl QuietService {
    /// Reads `quiet.json` (a missing or damaged file is no marks).
    #[must_use]
    pub fn load(file: RawFile, announce: Arc<dyn Fn(&QuietMarks) + Send + Sync>) -> Arc<Self> {
        let (marks, damaged) = match file.read() {
            Ok(Some(bytes)) => {
                Marks::parse(&bytes).map_or((Marks::default(), true), |m| (m, false))
            }
            _ => (Marks::default(), false),
        };
        Arc::new(Self {
            marks: RwLock::new(marks),
            file,
            damaged: Mutex::new(damaged),
            announce,
        })
    }

    #[must_use]
    pub fn marks(&self) -> QuietMarks {
        QuietMarks {
            marks: self
                .marks
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .to_map(),
        }
    }

    /// Marks `paths` (project paths) `quiet` or `speaking`. A `speaking` mark that changes nothing (no silenced folder
    /// above it) is not kept: the path just follows its parent again.
    ///
    /// # Errors
    /// A path that is no project path, an interest that is neither, or the file cannot be written.
    pub fn set(&self, paths: &[String], interest: &str) -> Result<QuietMarks, IpcError> {
        let interest = Interest::parse(interest)
            .ok_or_else(|| IpcError::new(ErrorKind::InvalidData).subject("interest"))?;
        let mut checked = Vec::with_capacity(paths.len());
        for path in paths {
            checked.push(if path.is_empty() {
                String::new()
            } else {
                VaultPath::parse(path)?.to_string()
            });
        }
        let changed = {
            let mut marks = self.marks.write().unwrap_or_else(PoisonError::into_inner);
            let before = marks.clone();
            marks.set(&checked, interest);
            drop_idle(&mut marks);
            *marks != before
        };
        if changed {
            self.save()?;
            (self.announce)(&self.marks());
        }
        Ok(self.marks())
    }

    /// Moved files take their marks along.
    fn on_batch(&self, batch: &FsBatch) {
        let mut changed = false;
        {
            let mut marks = self.marks.write().unwrap_or_else(PoisonError::into_inner);
            for change in &batch.changes {
                if let ChangeKind::Renamed { from } = &change.kind {
                    changed |= marks.renamed(from, &change.path);
                }
            }
        }
        if changed && self.save().is_ok() {
            (self.announce)(&self.marks());
        }
    }

    fn save(&self) -> Result<(), IpcError> {
        let bytes = self
            .marks
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .to_json();
        let mut damaged = self.damaged.lock().unwrap_or_else(PoisonError::into_inner);
        if *damaged {
            // what was there is not marks this version can read: kept next to the new file, never overwritten
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis());
            let aside = self
                .file
                .path()
                .with_file_name(format!("quiet.unreadable-{ms}.json"));
            if std::fs::rename(self.file.path(), aside).is_ok() {
                *damaged = false;
            }
        }
        self.file.write(&bytes)?;
        Ok(())
    }

    /// Search asks this: is the path silenced?
    #[must_use]
    pub fn lookup(self: &Arc<Self>) -> Arc<dyn SilenceLookup> {
        Arc::new(Lookup(Arc::clone(self)))
    }
}

/// A `speaking` mark with no silenced folder above it changes nothing.
fn drop_idle(marks: &mut Marks) {
    let idle: Vec<String> = marks
        .iter()
        .filter(|(_, interest)| *interest == Interest::Speaking)
        .map(|(path, _)| path.to_owned())
        .filter(|path| {
            // without its own mark the path speaks already (no silenced folder above it)
            let mut without = marks.clone();
            without.clear(std::slice::from_ref(path));
            !without.is_quiet(path)
        })
        .collect();
    marks.clear(&idle);
}

struct Lookup(Arc<QuietService>);

impl SilenceLookup for Lookup {
    fn is_silenced(&self, path: &str) -> bool {
        self.0
            .marks
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .is_quiet(path)
    }
}

impl Plugin for QuietPlugin {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let (Some(project), CxWorkspace::Project(_), Some(hub), Some(store)) =
            (cx.project(), &cx.workspace, cx.watch(), cx.store())
        else {
            return Err(PluginError::Start {
                id: ID,
                reason: "needs a project".to_owned(),
            });
        };
        let events = cx.events().clone();
        let service = QuietService::load(
            store.raw_file(FILE),
            Arc::new(move |marks| {
                events.emit(
                    "changed",
                    serde_json::to_value(marks).unwrap_or(serde_json::Value::Null),
                );
            }),
        );
        let subscription = hub.subscribe({
            let service = Arc::clone(&service);
            move |batch| service.on_batch(batch)
        });
        let mut registrations = Vec::new();
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc("rules", typed(move |_: NoArgs, _| Ok(s.marks()))));
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc(
            "set",
            typed(move |args: SetArgs, _| s.set(&args.paths, &args.interest)),
        ));
        let lease = project
            .services
            .provide(Arc::new(SilenceApi(service.lookup())));
        Ok(Box::new(QuietRunning {
            subscription: Some(subscription),
            registrations,
            lease: Some(lease),
        }))
    }
}

/// A started `quiet` plugin. Drop = teardown: the watch is not heard, nobody is answered, the service is withdrawn.
struct QuietRunning {
    subscription: Option<Subscription>,
    registrations: Vec<RpcRegistration>,
    lease: Option<ServiceLease>,
}

impl Running for QuietRunning {}

impl Drop for QuietRunning {
    fn drop(&mut self) {
        self.subscription.take();
        self.registrations.clear();
        self.lease.take();
    }
}
