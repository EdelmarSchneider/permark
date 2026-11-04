//! Store change (AC-7): choice checks, `keep` | `move` orchestration over [`Installs`]; the copy itself is
//! [`crate::relocate`].
//!
//! Choice: absolute; never inside | holding an open | recent project, the current | default store, the app's profile
//! (only the default store lives there); saved canonical (plain spelling). Change refused while a download runs; the
//! check + a "changing" mark are taken under one lock with Get's own check (no Get slips between). Installed models
//! -> `move` (background copy; old store in use until the copies are verified; then its copies + parts removed, a
//! removal that fails recorded `stale` with its store) | `keep` (old installs stay on disk, unlisted; its parts
//! removed).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError};

use permark_core::downloads::{DownloadError, Downloads};
use permark_core::fsx;
use permark_core::paths;
use permark_core::plugins::{nesting, Nesting};
use permark_core::{ErrorKind, IpcError};

use super::{Activity, InstallError, Installs, LocalAiState, Mover, StoreMode, StoreMove};
use crate::relocate::{self, Item};
use crate::state::Stale;
use crate::store::{prune, tidy};

/// Room kept free beyond a move (as the download engine).
const MOVE_MARGIN: u64 = 64 << 20;

/// A store change being decided: Get | installs refused until dropped.
struct Changing<'a>(&'a Installs);

impl Drop for Changing<'_> {
    fn drop(&mut self) {
        self.0.inner().changing = false;
    }
}

impl Installs {
    /// Marks a store change under the lock Get decides under: refused while moving | another change | a download is
    /// queued | running.
    fn begin_change(&self) -> Result<Changing<'_>, InstallError> {
        let mut inner = self.inner();
        if inner.moving.is_some() || inner.changing {
            return Err(InstallError::Moving);
        }
        let active = inner
            .live
            .iter()
            .find(|(_, l)| matches!(l.activity, Activity::Queued | Activity::Running(_)));
        if let Some((id, _)) = active {
            return Err(InstallError::Downloading(id.clone()));
        }
        inner.changing = true;
        Ok(Changing(self))
    }

    /// Installed versions listed in the current store (none when it is unavailable | no download service).
    fn installed_here(&self) -> Vec<Item> {
        let Ok(engine) = self.engine() else {
            return Vec::new();
        };
        let state = self.inner().state.clone();
        let mut items: Vec<Item> = Vec::new();
        for model in &self.catalog.models {
            if items.iter().any(|i| i.lineage == model.lineage) {
                continue;
            }
            if let Some(installed) = Self::usable(Some(&engine), &state, model) {
                items.push(Item {
                    lineage: model.lineage.clone(),
                    installed,
                });
            }
        }
        items
    }

    /// Refuses a chosen folder overlapping a store, the app's profile | a project.
    fn check_choice(&self, chosen: &Path, current: &Path) -> Result<(), InstallError> {
        let shown = chosen.display().to_string();
        if [current, self.default_store.as_path()]
            .iter()
            .any(|store| nesting(chosen, store) != Nesting::Apart)
        {
            return Err(InstallError::InsideStore(shown));
        }
        if self
            .links
            .profile
            .as_ref()
            .is_some_and(|profile| nesting(chosen, profile) != Nesting::Apart)
        {
            return Err(InstallError::InsideAppData(shown));
        }
        if self
            .links
            .places
            .as_ref()
            .is_some_and(|places| places.0.overlaps(chosen))
        {
            return Err(InstallError::InsideProject(shown));
        }
        Ok(())
    }

    /// Store choice (AC-7): `path` = a folder the person picked (`None` | the default folder: the default). Same
    /// folder: nothing. Installed models need `mode` ([`StoreMode::Move`]: background copy, [`crate::relocate`]).
    ///
    /// # Errors
    /// [`InstallError::Moving`], [`InstallError::NotAbsolute`], [`InstallError::InsideStore`] (inside | holding the
    /// current | default store), [`InstallError::InsideAppData`], [`InstallError::InsideProject`],
    /// [`InstallError::Downloading`] (one queued | running), [`InstallError::ModeRequired`], `NoSpace` (move),
    /// [`InstallError::Folder`] (cannot be made), state write.
    pub fn set_store(
        &self,
        path: Option<&str>,
        mode: Option<StoreMode>,
    ) -> Result<LocalAiState, InstallError> {
        self.not_moving()?;
        let current = self.store_of(&self.inner().state);
        let mut target: Option<PathBuf> = match path {
            None => None,
            Some(text) => {
                let target = PathBuf::from(text);
                if !target.is_absolute() {
                    return Err(InstallError::NotAbsolute(text.to_owned()));
                }
                (nesting(&target, &self.default_store) != Nesting::Same).then_some(target)
            }
        };
        let mut to = target.clone().unwrap_or_else(|| self.default_store.clone());
        if nesting(&to, &current) == Nesting::Same {
            return Ok(self.state());
        }
        if let Some(chosen) = &target {
            self.check_choice(chosen, &current)?;
        }
        let _changing = self.begin_change()?;
        let items = self.installed_here();
        let mode = match (items.is_empty(), mode) {
            (true, _) => StoreMode::Keep,
            (false, Some(mode)) => mode,
            (false, None) => return Err(InstallError::ModeRequired(to.display().to_string())),
        };
        if mode == StoreMode::Move {
            let total: u64 = items.iter().map(Item::bytes).sum();
            let needed = total.saturating_add(MOVE_MARGIN.max(total / 100));
            let free = fsx::free_space(&to).map_err(|e| DownloadError::disk(&e))?;
            if free < needed {
                return Err(DownloadError::NoSpace { needed, free }.into());
            }
        }
        if target.is_some() {
            std::fs::create_dir_all(&to)
                .map_err(|e| InstallError::Folder(to.display().to_string(), e))?;
            // saved as the disk names it (links resolved, plain spelling); checked again under that name
            to = paths::canonicalize(&to)
                .map_err(|e| InstallError::Folder(to.display().to_string(), e))?;
            self.check_choice(&to, &current)?;
            target = Some(to.clone());
        }
        match mode {
            StoreMode::Keep => {
                let old = self.engine().ok();
                self.switch(target)?;
                if let Some(old) = old {
                    self.drop_parts(&old);
                }
            }
            StoreMode::Move => self.start_move(current, target, to, items)?,
        }
        self.announce();
        Ok(self.state())
    }

    /// Uses store `target` from now on (state written first); activity of the old store forgotten. Stale records of
    /// `target` naming a version recorded installed: dropped (that version is in use there again).
    fn switch(&self, target: Option<PathBuf>) -> Result<(), InstallError> {
        let old = {
            let mut inner = self.inner();
            let mut next = inner.state.clone();
            let to = target.clone().unwrap_or_else(|| self.default_store.clone());
            let installed = next.installed.clone();
            next.stale.retain(|s| {
                s.store.as_deref() != Some(to.as_path())
                    || installed
                        .get(&s.lineage)
                        .is_none_or(|i| i.catalog_version != s.installed.catalog_version)
            });
            next.store = target;
            self.save(&next)?;
            inner.state = next;
            inner.live.clear();
            inner.move_failed = None;
            inner.engine.take()
        };
        // dropped with no lock held (it joins its worker, if any)
        drop(old);
        Ok(())
    }

    /// Parts of every catalog version in store `old` (no longer in use: unreachable) removed. Best effort.
    fn drop_parts(&self, old: &Downloads) {
        for model in &self.catalog.models {
            if let Ok(unit) = model.unit() {
                let _ = old.delete_part(&unit);
            }
            prune(old.store(), &model.lineage, model.catalog_version);
        }
    }

    fn start_move(
        &self,
        from: PathBuf,
        target: Option<PathBuf>,
        to: PathBuf,
        items: Vec<Item>,
    ) -> Result<(), InstallError> {
        let kit = self.kit.as_ref().ok_or(InstallError::NoDownloads)?;
        let installs = self.me.upgrade().ok_or(InstallError::NoDownloads)?;
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut inner = self.inner();
            if inner.moving.is_some() {
                return Err(InstallError::Moving);
            }
            inner.moving = Some(Mover {
                to: to.clone(),
                cancel: Arc::clone(&cancel),
                done: 0,
                total: items.iter().map(Item::bytes).sum(),
            });
            inner.move_failed = None;
        }
        // copies are told by the move itself: the target engine's listener hears nothing
        let copier = kit.downloads(to, Arc::new(|_, _| {}));
        let mut slot = self.mover.lock().unwrap_or_else(PoisonError::into_inner);
        // the last move ended (it cleared `moving` before this one could start)
        if let Some(previous) = slot.take() {
            let _ = previous.join();
        }
        let spawned = std::thread::Builder::new()
            .name("local-ai-move".into())
            .spawn(move || installs.run_move(&from, target, &copier, &items, &cancel));
        match spawned {
            Ok(handle) => {
                *slot = Some(handle);
                Ok(())
            }
            Err(error) => {
                drop(slot);
                self.inner().moving = None;
                Err(DownloadError::disk(&error).into())
            }
        }
    }

    /// The move thread: target tidied (leftovers of a stopped move), copy + check, then the new store in use and the
    /// old copies + parts removed (a copy that cannot be removed: `stale`, tried again later). Failure | cancel: what
    /// the move made removed, old store stays.
    fn run_move(
        &self,
        from: &Path,
        target: Option<PathBuf>,
        copier: &Downloads,
        items: &[Item],
        cancel: &AtomicBool,
    ) {
        let mut report = |done: u64, total: u64| {
            let to = {
                let mut inner = self.inner();
                let Some(mover) = inner.moving.as_mut() else {
                    return;
                };
                mover.done = done;
                mover.to.display().to_string()
            };
            if let Ok(payload) = serde_json::to_value(StoreMove::Moving { to, done, total }) {
                self.events.emit("move", payload);
            }
        };
        let state = self.inner().state.clone();
        let _ = copier.while_idle(|| {
            tidy(copier.store(), &self.lineages(), |lineage, version| {
                self.known_version(&state, lineage, version)
            });
        });
        let outcome = relocate::copy_all(from, items, copier, cancel, &mut report);
        let switched = outcome.and_then(|made| {
            let old = self.engine().ok();
            match self.switch(target) {
                Ok(()) => Ok(old),
                Err(error) => {
                    relocate::discard(&made);
                    Err(DownloadError::Disk {
                        detail: error.to_string(),
                    })
                }
            }
        });
        match switched {
            Ok(old) => {
                if let Some(old) = old {
                    self.drop_old_copies(&old, items);
                    self.drop_parts(&old);
                }
                self.inner().moving = None;
            }
            Err(error) => {
                let mut inner = self.inner();
                let to = inner.moving.take().map(|m| m.to);
                if error != DownloadError::Cancelled {
                    inner.move_failed = to.map(|to| (to, error));
                }
            }
        }
        self.announce();
    }

    /// The moved versions removed from the old store; one that cannot be (file held open) recorded `stale`.
    fn drop_old_copies(&self, old: &Downloads, items: &[Item]) {
        let mut left = Vec::new();
        for item in items {
            let gone = item
                .installed
                .unit(&item.lineage)
                .is_ok_and(|unit| old.remove(&unit).is_ok());
            if gone {
                prune(old.store(), &item.lineage, item.installed.catalog_version);
            } else {
                left.push(Stale {
                    lineage: item.lineage.clone(),
                    installed: item.installed.clone(),
                    store: Some(old.store().to_path_buf()),
                });
            }
        }
        if !left.is_empty() {
            // best effort: a failed write only loses the record (the folder stays, unlisted)
            let _ = self.change(|s| s.stale.extend(left));
        }
    }

    /// Stops a running store move (copies removed, old store kept). None running: nothing.
    #[must_use]
    pub fn cancel_move(&self) -> LocalAiState {
        if let Some(mover) = &self.inner().moving {
            mover.cancel.store(true, Ordering::Relaxed);
        }
        self.snapshot(self.current_engine().as_deref())
    }

    /// Shows the current store folder in the file manager (only the plugin's own store).
    ///
    /// # Errors
    /// [`InstallError::StoreUnavailable`] (gone | not made yet), no reveal service, the system refused.
    pub fn reveal_store(&self) -> Result<(), IpcError> {
        let store = self.store_of(&self.inner().state);
        if !store.is_dir() {
            return Err(InstallError::StoreUnavailable.into());
        }
        let reveal = self
            .links
            .reveal
            .as_ref()
            .ok_or_else(|| IpcError::new(ErrorKind::Unavailable).subject("reveal"))?;
        reveal.0.reveal(&store)
    }
}
