//! The demo pages: sample documents shipped with the app (resource folder [`RESOURCE`]), copied once into the person's
//! Documents folder as [`FOLDER`] and opened from there as a project. The Welcome screen offers them until the person
//! used or dismissed the offer. The Windows installer may have copied the folder already: a folder found there is opened
//! as it is, never written into.

#[cfg(test)]
mod tests;

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{ErrorKind, IpcError};
use crate::fileops::copy_folder;
use crate::state::{StateData, StateFile, StateStore};

/// The folder's name in Documents (the Windows installer uses the same).
pub const FOLDER: &str = "PerMark Demo";
/// The shipped folder's name in the app's resources (`bundle.resources` in tauri.conf.json).
pub const RESOURCE: &str = "demo";
const STATE_FILE: &str = "demo-pages.json";

/// What the Welcome screen offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DemoOffer {
    /// Nothing: used or dismissed already, or this copy has no pages | no place to put them.
    Hidden,
    /// The pages are not in Documents yet: they are copied there first.
    Install,
    /// The folder is in Documents already: it opens as it is.
    Open,
}

/// The Welcome screen's offer and whether the pages can be opened at all (the lasting way in: the command palette and
/// the quick settings, whatever became of the offer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DemoStatus {
    pub offer: DemoOffer,
    /// The folder is in Documents, or this copy ships the pages and nothing else is at their place.
    pub available: bool,
}

/// Where the pages are: the folder shipped with the app and their place in Documents (`None`: unknown here).
#[derive(Debug, Clone)]
pub struct DemoPlaces {
    pub bundled: Option<PathBuf>,
    pub target: Option<PathBuf>,
}

/// Whether the offer was used or dismissed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DemoState {
    done: bool,
}

impl StateData for DemoState {
    const VERSION: u32 = 1;
}

fn state_file(store: &StateStore) -> StateFile<DemoState> {
    store.file(STATE_FILE)
}

fn mark_done(store: &StateStore) -> Result<(), IpcError> {
    state_file(store)
        .save(&DemoState { done: true })
        .map_err(|e| {
            IpcError::new(ErrorKind::Io)
                .subject(STATE_FILE)
                .detail(e.to_string())
        })
}

/// A real folder at `path` (not a link to one).
fn is_folder(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

/// What the pages' place allows, whatever became of the offer: open the folder, copy it there first, or nothing.
fn place_offer(places: &DemoPlaces) -> DemoOffer {
    let Some(target) = &places.target else {
        return DemoOffer::Hidden;
    };
    if is_folder(target) {
        DemoOffer::Open
    } else if fs::symlink_metadata(target).is_err()
        && places.bundled.as_deref().is_some_and(is_folder)
    {
        DemoOffer::Install
    } else {
        DemoOffer::Hidden
    }
}

/// What the Welcome screen offers now, and whether the pages can be opened.
#[must_use]
pub fn status(store: &StateStore, places: &DemoPlaces) -> DemoStatus {
    let here = place_offer(places);
    let done = state_file(store).load().into_value().done;
    DemoStatus {
        offer: if done { DemoOffer::Hidden } else { here },
        available: here != DemoOffer::Hidden,
    }
}

/// What the Welcome screen offers now.
#[must_use]
pub fn offer(store: &StateStore, places: &DemoPlaces) -> DemoOffer {
    status(store, places).offer
}

/// The folder to open: the one in Documents, copied there from the shipped pages first when nothing is at its place
/// (all or nothing). The offer counts as used.
///
/// # Errors
/// No Documents folder known (`Unavailable`); something other than a folder at the place (`Exists`, left alone); the
/// shipped pages missing (`NotFound`); the copy failed (nothing left in Documents).
pub fn install(store: &StateStore, places: &DemoPlaces) -> Result<PathBuf, IpcError> {
    let target = places
        .target
        .clone()
        .ok_or_else(|| IpcError::new(ErrorKind::Unavailable).subject(FOLDER))?;
    let shown = target.display().to_string();
    if !is_folder(&target) {
        if fs::symlink_metadata(&target).is_ok() {
            return Err(IpcError::new(ErrorKind::Exists).subject(shown));
        }
        let bundled = places
            .bundled
            .as_deref()
            .filter(|path| is_folder(path))
            .ok_or_else(|| IpcError::new(ErrorKind::NotFound).subject(RESOURCE))?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| IpcError::io(shown.clone(), &e))?;
        }
        match copy_folder(bundled, &target) {
            Ok(()) => {}
            // made meanwhile (by the person, by another window): opened as it is
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && is_folder(&target) => {}
            Err(e) => return Err(IpcError::io(shown, &e)),
        }
    }
    // the folder is there: a failed record only means the offer shows again (as "open")
    let _ = mark_done(store);
    Ok(target)
}

/// The person dismissed the offer: not shown again.
///
/// # Errors
/// The record cannot be written.
pub fn dismiss(store: &StateStore) -> Result<(), IpcError> {
    mark_done(store)
}
