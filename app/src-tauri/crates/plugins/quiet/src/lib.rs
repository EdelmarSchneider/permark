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
use permark_core::projects::{Owners, OwnersChange, OwnersGate, ProjectsSubscription};
use permark_core::state::{merge3, RawFile};
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
    /// What the file holds now (owner-relative), as last read | written: the base this core's changes are laid on
    /// when the file is written again (another tab | app may have written it meanwhile; `projects.md` §14 D51).
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
    /// Closed while the plugin started before the owners were known: marks + moves wait for them.
    gate: OwnersGate,
}

/// Reads every owner's file into one root-relative set of marks.
fn read_owners(owners: &Owners) -> (Marks, Vec<OwnerFile>) {
    let mut slots = Vec::with_capacity(owners.slots().len());
    for (i, slot) in owners.slots().iter().enumerate() {
        // written under the plugin's lock: a hung other writer never holds a change up long (N10)
        let file = slot.store.raw_file(FILE).brief_lock_wait();
        let (marks, damaged) = match file.read() {
            Ok(Some(bytes)) => {
                Marks::parse(&bytes).map_or((Marks::default(), true), |m| (m, false))
            }
            Ok(None) => (Marks::default(), false),
            // a file that cannot be read now is set aside like a damaged one before the first write
            Err(_) => (Marks::default(), true),
        };
        let written = marks.to_map();
        slots.push(OwnerFile {
            file,
            damaged,
            dormant: dormant_of(owners, i, &written),
            written,
        });
    }
    (marks_of(Some(owners), &slots), slots)
}

/// The marks of file `i` that lie in a deeper owner.
fn dormant_of(
    owners: &Owners,
    i: usize,
    written: &BTreeMap<String, Interest>,
) -> BTreeMap<String, Interest> {
    written
        .iter()
        .filter(|(rel, _)| owners.owner_of(&owners.join(i, rel)) != i)
        .map(|(rel, interest)| (rel.clone(), *interest))
        .collect()
}

/// One root-relative set of marks from what every owner's file holds (dormant marks left out).
fn marks_of(owners: Option<&Owners>, slots: &[OwnerFile]) -> Marks {
    let Some(owners) = owners else {
        return Marks::from_map(slots.first().map(|s| s.written.clone()).unwrap_or_default());
    };
    let mut merged = BTreeMap::new();
    for (i, slot) in slots.iter().enumerate() {
        for (rel, interest) in &slot.written {
            let path = owners.join(i, rel);
            if owners.owner_of(&path) == i {
                merged.insert(path, *interest);
            }
        }
    }
    Marks::from_map(merged)
}

impl QuietService {
    /// Reads `quiet.json` (a missing or damaged file is no marks; a file that cannot be read now is set aside like a
    /// damaged one before the first write, never written over).
    #[must_use]
    pub fn load(file: RawFile, announce: Arc<dyn Fn(&QuietMarks) + Send + Sync>) -> Arc<Self> {
        let file = file.brief_lock_wait();
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
            gate: OwnersGate::default(),
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
            gate: OwnersGate::default(),
        })
    }

    /// The owners change: before, what waits is written and writes wait; after, the marks are read again from the new
    /// owners' files (the window is told).
    pub fn follow_owners(&self, change: &OwnersChange<'_>) {
        match change {
            OwnersChange::Pause => {
                let mut files = self.lock_files();
                if files.pending {
                    let _ = self.save(&mut files);
                }
                files.paused = true;
            }
            OwnersChange::Resume(owners) => {
                // already running on these owners (the `Resume` told when the owners became known): nothing to do
                {
                    let files = self.lock_files();
                    if !files.paused && files.owners.as_ref().is_some_and(|o| o.same(owners)) {
                        drop(files);
                        self.gate.open();
                        return;
                    }
                }
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
                self.gate.open();
            }
        }
    }

    /// Started before the owners were known: marks + moves wait for the first `Resume`.
    pub fn start_paused(&self) {
        self.gate.close();
        self.follow_owners(&OwnersChange::Pause);
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
        self.gate.wait();
        // one change at a time: the marks, their files and what the files were read as stay together
        let mut files = self.lock_files();
        // the owners are changing (a moment): the mark would land in a file about to be read again
        if files.paused {
            return Err(IpcError::new(ErrorKind::Unavailable).subject(FILE));
        }
        let changed = {
            let mut marks = self.marks.write().unwrap_or_else(PoisonError::into_inner);
            let before = marks.clone();
            marks.set(&checked, interest);
            drop_idle(&mut marks, &checked);
            *marks != before
        };
        if changed {
            self.save(&mut files)?;
            drop(files);
            (self.announce)(&self.marks());
        }
        Ok(self.marks())
    }

    /// Moved files take their marks along.
    fn on_batch(&self, batch: &FsBatch) {
        // a move before the owners are known: applied once they are (never into a file about to be read again)
        self.gate.wait();
        let mut changed = false;
        let mut files = self.lock_files();
        let owner_folders: Vec<String> = files
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
        if changed && self.save(&mut files).is_ok() {
            drop(files);
            (self.announce)(&self.marks());
        }
    }

    /// Writes each owner's file whose marks changed (the root-relative marks split by owner, dormant ones kept): this
    /// core's changes since it last read | wrote a file go on top of the file as it is now; then the marks are what
    /// the files hold (another tab's | app's marks included). Called with the files locked.
    fn save(&self, files: &mut Files) -> Result<(), IpcError> {
        let marks = self
            .marks
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .to_map();
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
        let mut wrote = false;
        let owners = files.owners.clone();
        for (i, (slot, mut part)) in files.slots.iter_mut().zip(parts).enumerate() {
            part.extend(slot.dormant.iter().map(|(p, i)| (p.clone(), *i)));
            if part == slot.written && !slot.damaged {
                continue;
            }
            match write_marks(slot, &part) {
                Ok(now) => {
                    if let Some(owners) = &owners {
                        slot.dormant = dormant_of(owners, i, &now);
                    }
                    slot.written = now;
                    wrote = true;
                }
                Err(e) => result = Err(e),
            }
        }
        if wrote {
            *self.marks.write().unwrap_or_else(PoisonError::into_inner) =
                marks_of(owners.as_ref(), &files.slots);
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

/// Writes one owner's marks over the file as it is now (`marks` = this core's, `slot.written` = what it last saw; a
/// damaged file is put aside first, never overwritten). Returns what the file holds afterwards.
fn write_marks(
    slot: &mut OwnerFile,
    marks: &BTreeMap<String, Interest>,
) -> Result<BTreeMap<String, Interest>, IpcError> {
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
    let base = &slot.written;
    let mut merged = None;
    let mut foreign = false;
    slot.file.update(|now| {
        let fresh = match now {
            None => BTreeMap::new(),
            Some(bytes) => {
                let Some(fresh) = Marks::parse(bytes) else {
                    foreign = true;
                    return None;
                };
                fresh.to_map()
            }
        };
        let next = merge3(base, marks, fresh);
        let bytes = Marks::from_map(next.clone()).to_json();
        merged = Some(next);
        Some(bytes)
    })?;
    if foreign {
        // another program wrote something that is not marks meanwhile: set aside at the next write
        slot.damaged = true;
        return Err(IpcError::new(ErrorKind::Unavailable).subject(FILE));
    }
    Ok(merged.unwrap_or_else(|| marks.clone()))
}

/// A `speaking` mark with no silenced folder above it changes nothing: dropped at | below the paths just `set` (marks
/// elsewhere stay as written: in a project opened alone, a mark that only matters under the outer folder's silence is
/// the outer tab's, `projects.md` §14 D51).
fn drop_idle(marks: &mut Marks, set: &[String]) {
    let keys: Vec<String> = set.iter().map(|p| permark_core::paths::key_of(p)).collect();
    let touched = |path: &str| {
        let key = permark_core::paths::key_of(path);
        keys.iter().any(|k| {
            k.is_empty()
                || key == *k
                || key
                    .strip_prefix(k.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    };
    let idle: Vec<String> = marks
        .iter()
        .filter(|(path, interest)| *interest == Interest::Speaking && touched(path))
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
        let started = project.owners(store);
        // S3: before the worker knows the owners, nothing is written (it could land in the outer folder's file)
        let early = !project.owners_known();
        let service = QuietService::load_owners(
            started.clone(),
            Arc::new(move |marks| {
                events.emit(
                    "changed",
                    serde_json::to_value(marks).unwrap_or(serde_json::Value::Null),
                );
            }),
        );
        if early {
            service.start_paused();
        }
        // marks follow where each included project keeps its state (`projects.md` §9)
        let owners = project.follow_owners({
            let service = Arc::downgrade(&service);
            move |change| {
                if let Some(service) = service.upgrade() {
                    service.follow_owners(&change);
                }
            }
        });
        // an owners change between the read above and the subscription (or owners known meanwhile): caught up here
        let now = project.owners(store);
        if !now.same(&started) || (early && project.owners_known()) {
            service.follow_owners(&OwnersChange::Resume(&now));
        }
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
