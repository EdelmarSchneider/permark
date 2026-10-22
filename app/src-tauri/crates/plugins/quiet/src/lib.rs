//! Plugin `quiet`: silence a folder or a file. It stays in the tree (dimmed) and can be opened and searched, but its
//! changes stop calling for attention: no unread dot, no feed row, no toast, no follow jump, no git letter. Personal
//! (project local state, `quiet.json` in the layout of 0.5). Agents never learn the marks: a silenced path is simply
//! not shared with them.
//!
//! This side keeps the marks, answers search (`SilenceApi`) and hands the marks to the window, which draws the effects.

#![cfg_attr(test, allow(clippy::assert_is_empty))]

mod marks;
#[cfg(test)]
mod owners_tests;
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
use permark_core::projects::{Owners, OwnersChange, ProjectsSubscription};
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

/// One owner's `quiet.json` (the opened folder's, or an included project's: `projects.md` §9.2).
struct OwnerFile {
    file: RawFile,
    /// The file existed and was not marks: it is put aside before the first write.
    damaged: bool,
    /// Marks of this file that lie in a deeper owner: kept as written, unused while that one is included.
    dormant: BTreeMap<String, Interest>,
    /// What the file holds now (owner-relative), as last read | written.
    written: BTreeMap<String, Interest>,
}

/// Where the marks are kept: one file per owner (`owners` none: one file, no projects).
struct Files {
    owners: Option<Owners>,
    slots: Vec<OwnerFile>,
    /// The owners are changing: writes wait.
    paused: bool,
    /// A change waits to be written.
    pending: bool,
}

/// The marks of one project, kept in `quiet.json` of each owner (one root-relative view; the closest mark across the
/// chain's files wins, `projects.md` §5.3 D17).
pub struct QuietService {
    marks: RwLock<Marks>,
    files: Mutex<Files>,
    announce: Arc<dyn Fn(&QuietMarks) + Send + Sync>,
}

/// Reads every owner's file into one root-relative set of marks.
fn read_owners(owners: &Owners) -> (Marks, Vec<OwnerFile>) {
    let mut merged = BTreeMap::new();
    let mut slots = Vec::with_capacity(owners.slots().len());
    for (i, slot) in owners.slots().iter().enumerate() {
        let file = slot.store.raw_file(FILE);
        let (marks, damaged) = match file.read() {
            Ok(Some(bytes)) => {
                Marks::parse(&bytes).map_or((Marks::default(), true), |m| (m, false))
            }
            Ok(None) => (Marks::default(), false),
            // a file that cannot be read now is set aside like a damaged one before the first write
            Err(_) => (Marks::default(), true),
        };
        let mut dormant = BTreeMap::new();
        for (rel, interest) in marks.iter() {
            let path = owners.join(i, rel);
            if owners.owner_of(&path) == i {
                merged.insert(path, interest);
            } else {
                dormant.insert(rel.to_owned(), interest);
            }
        }
        slots.push(OwnerFile {
            file,
            damaged,
            dormant,
            written: marks.to_map(),
        });
    }
    (Marks::from_map(merged), slots)
}

impl QuietService {
    /// Reads `quiet.json` (a missing or damaged file is no marks; a file that cannot be read now is set aside like a
    /// damaged one before the first write, never written over).
    #[must_use]
    pub fn load(file: RawFile, announce: Arc<dyn Fn(&QuietMarks) + Send + Sync>) -> Arc<Self> {
        let (marks, damaged) = match file.read() {
            Ok(Some(bytes)) => {
                Marks::parse(&bytes).map_or((Marks::default(), true), |m| (m, false))
            }
            Ok(None) => (Marks::default(), false),
            Err(_) => (Marks::default(), true),
        };
        let written = marks.to_map();
        Arc::new(Self {
            marks: RwLock::new(marks),
            files: Mutex::new(Files {
                owners: None,
                slots: vec![OwnerFile {
                    file,
                    damaged,
                    dormant: BTreeMap::new(),
                    written,
                }],
                paused: false,
                pending: false,
            }),
            announce,
        })
    }

    /// The marks of every owner (the opened folder + included projects).
    #[must_use]
    pub fn load_owners(
        owners: Owners,
        announce: Arc<dyn Fn(&QuietMarks) + Send + Sync>,
    ) -> Arc<Self> {
        let (marks, slots) = read_owners(&owners);
        Arc::new(Self {
            marks: RwLock::new(marks),
            files: Mutex::new(Files {
                owners: Some(owners),
                slots,
                paused: false,
                pending: false,
            }),
            announce,
        })
    }

    /// The owners change: before, what waits is written and writes wait; after, the marks are read again from the new
    /// owners' files (the window is told).
    pub fn follow_owners(&self, change: &OwnersChange<'_>) {
        match change {
            OwnersChange::Pause => {
                let pending = self.lock_files().pending;
                if pending {
                    let _ = self.save();
                }
                self.lock_files().paused = true;
            }
            OwnersChange::Resume(owners) => {
                let (marks, slots) = read_owners(owners);
                {
                    let mut files = self.lock_files();
                    files.owners = Some((*owners).clone());
                    files.slots = slots;
                    files.paused = false;
                    files.pending = false;
                }
                *self.marks.write().unwrap_or_else(PoisonError::into_inner) = marks;
                (self.announce)(&self.marks());
            }
        }
    }

    fn lock_files(&self) -> std::sync::MutexGuard<'_, Files> {
        self.files.lock().unwrap_or_else(PoisonError::into_inner)
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
        // the owners are changing (a moment): the mark would land in a file about to be read again
        if self.lock_files().paused {
            return Err(IpcError::new(ErrorKind::Unavailable).subject(FILE));
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
        let owner_folders: Vec<String> = self
            .lock_files()
            .owners
            .as_ref()
            .map(|o| {
                o.slots()
                    .iter()
                    .skip(1)
                    .map(|s| s.key().to_owned())
                    .collect()
            })
            .unwrap_or_default();
        {
            let mut marks = self.marks.write().unwrap_or_else(PoisonError::into_inner);
            for change in &batch.changes {
                if let ChangeKind::Renamed { from } = &change.kind {
                    // an included project moved as a whole: its own file moves with it (or its profile folder does)
                    if owner_folders.contains(&permark_core::paths::key_of(from)) {
                        continue;
                    }
                    changed |= marks.renamed(from, &change.path);
                }
            }
        }
        if changed && self.save().is_ok() {
            (self.announce)(&self.marks());
        }
    }

    /// Writes each owner's file whose marks changed (the root-relative marks split by owner, dormant ones kept).
    fn save(&self) -> Result<(), IpcError> {
        let marks = self
            .marks
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .to_map();
        let mut files = self.lock_files();
        if files.paused {
            files.pending = true;
            return Ok(());
        }
        let parts: Vec<BTreeMap<String, Interest>> = match &files.owners {
            Some(owners) => owners
                .split(marks)
                .into_iter()
                .map(|part| part.into_iter().collect())
                .collect(),
            None => vec![marks],
        };
        let mut result = Ok(());
        for (slot, mut part) in files.slots.iter_mut().zip(parts) {
            part.extend(slot.dormant.iter().map(|(p, i)| (p.clone(), *i)));
            if part == slot.written && !slot.damaged {
                continue;
            }
            if let Err(e) = write_marks(slot, &part) {
                result = Err(e);
                continue;
            }
            slot.written = part;
        }
        files.pending = result.is_err();
        result
    }

    /// Search asks this: is the path silenced?
    #[must_use]
    pub fn lookup(self: &Arc<Self>) -> Arc<dyn SilenceLookup> {
        Arc::new(Lookup(Arc::clone(self)))
    }
}

/// Writes one owner's marks (a damaged file is put aside first, never overwritten).
fn write_marks(slot: &mut OwnerFile, marks: &BTreeMap<String, Interest>) -> Result<(), IpcError> {
    if slot.damaged {
        // what was there is not marks this version can read: kept next to the new file, never overwritten
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let aside = slot
            .file
            .path()
            .with_file_name(format!("quiet.unreadable-{ms}.json"));
        match std::fs::rename(slot.file.path(), aside) {
            Ok(()) => slot.damaged = false,
            // gone meanwhile: nothing to keep
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => slot.damaged = false,
            // still there and not ours to replace: no write
            Err(e) => return Err(IpcError::io("quiet.json", &e)),
        }
    }
    slot.file.write(&Marks::from_map(marks.clone()).to_json())?;
    Ok(())
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
        let service = QuietService::load_owners(
            project.owners(store),
            Arc::new(move |marks| {
                events.emit(
                    "changed",
                    serde_json::to_value(marks).unwrap_or(serde_json::Value::Null),
                );
            }),
        );
        // marks follow where each included project keeps its state (`projects.md` §9)
        let owners = project.follow_owners({
            let service = Arc::downgrade(&service);
            move |change| {
                if let Some(service) = service.upgrade() {
                    service.follow_owners(&change);
                }
            }
        });
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
            owners,
            registrations,
            lease: Some(lease),
        }))
    }
}

/// A started `quiet` plugin. Drop = teardown: the watch is not heard, nobody is answered, the service is withdrawn.
struct QuietRunning {
    subscription: Option<Subscription>,
    owners: Option<ProjectsSubscription>,
    registrations: Vec<RpcRegistration>,
    lease: Option<ServiceLease>,
}

impl Running for QuietRunning {}

impl Drop for QuietRunning {
    fn drop(&mut self) {
        self.subscription.take();
        self.owners.take();
        self.registrations.clear();
        self.lease.take();
    }
}
