//! Drop to open (`windows.md` Shell, `explorer.md` Drop from outside, route Open): files | folders dropped on a
//! window open there as tabs, `open_in` ignored (dropping on the window is the choice). One place: a path open
//! anywhere goes to that place. New tabs before the last one stay Pending (many paths cost one view); the last path's
//! place ends active. Registry steps only; the shell runs the outcomes in order and focuses `forward`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{ErrorKind, IpcError};
use crate::open::{route, Route};
use crate::paths::VaultPath;
use crate::settings::OpenIn;
use crate::workspace::{Outcome, Place, TabId, WorkspaceRef, WorkspaceRegistry};

/// What a drop opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DropScope {
    /// Every path: where it is open, else a new tab here.
    Tabs,
    /// Only paths open somewhere already (that place comes forward); the rest are handed back in `left` untouched,
    /// each with whether PerMark opens it as text (the Notes list those as loose files; the rest open as tabs).
    ExistingOnly,
}

/// A path `ExistingOnly` left alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LeftPath {
    pub path: String,
    /// A file PerMark opens as text (never a folder).
    pub text: bool,
}

/// Bytes read to tell a file without a known extension (`README`, `.npmrc`) from a binary one.
const SNIFF: usize = 8 * 1024;

/// Whether PerMark opens a file as text: Markdown, plain text, config and data by name; a name without a known
/// category (no extension, a dotfile) when its first bytes hold no NUL. Folders and everything else: no.
#[must_use]
pub fn opens_as_text(path: &Path) -> bool {
    use std::io::Read;

    use crate::tree::{category_of, extension_of, Category};
    if !path.is_file() {
        return false;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match category_of(&name) {
        Category::Markdown | Category::Text | Category::Config | Category::Data => true,
        Category::Other if extension_of(&name).is_none() => {
            let mut head = Vec::with_capacity(SNIFF);
            std::fs::File::open(path)
                .and_then(|file| file.take(SNIFF as u64).read_to_end(&mut head))
                .is_ok_and(|_| !head.contains(&0))
        }
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DropFailure {
    pub path: String,
    pub error: IpcError,
}

/// The answer to the view that was dropped on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DropOpened {
    /// Paths that could not be opened (one notice for the drop).
    pub failed: Vec<DropFailure>,
    /// `ExistingOnly`: paths open nowhere, left alone.
    pub left: Vec<LeftPath>,
}

/// Registry steps done; what the shell does next.
#[derive(Debug, Default)]
pub struct Dropped {
    /// View changes + loads to run, in order.
    pub outcomes: Vec<Outcome>,
    /// The window to bring forward (the last path's place).
    pub forward: Option<String>,
    pub answer: DropOpened,
}

/// The last path's place: a tab added here (Pending until shown), or a place open already.
enum Last {
    Added(TabId),
    Open(Place, Option<VaultPath>),
}

fn failure(path: &Path, error: IpcError) -> DropFailure {
    DropFailure {
        path: path.display().to_string(),
        error,
    }
}

/// Opens `paths` (absolute, in drop order) for a drop on `window`.
pub fn open_dropped(
    registry: &WorkspaceRegistry,
    window: &str,
    paths: &[PathBuf],
    scope: DropScope,
) -> Dropped {
    let mut out = Dropped::default();
    // the window shows its Welcome (no tab yet): the tab shown at the end adopts that view
    let on_welcome = registry
        .shown_view(window)
        .is_some_and(|view| registry.place_of_view(&view).is_none());
    let mut first_added: Option<TabId> = None;
    let mut last: Option<Last> = None;
    for path in paths {
        if !path.is_absolute() {
            out.answer.failed.push(failure(
                path,
                IpcError::new(ErrorKind::InvalidPath).subject(path.display().to_string()),
            ));
            continue;
        }
        let reference = match WorkspaceRef::classify(path) {
            Ok(reference) => reference,
            Err(error) => {
                out.answer.failed.push(failure(path, error.into()));
                continue;
            }
        };
        // `joined` = this window: a path open nowhere routes to a tab here, whatever `open_in` says
        match route(registry, &reference, false, OpenIn::Tab, Some(window)) {
            Route::Focus { place, document } => last = Some(Last::Open(place, document)),
            Route::Tab { .. } | Route::NewWindow if scope == DropScope::ExistingOnly => {
                out.answer.left.push(LeftPath {
                    path: path.display().to_string(),
                    text: matches!(reference, WorkspaceRef::SingleFile(_)) && opens_as_text(path),
                });
            }
            Route::Tab { .. } | Route::NewWindow => {
                match registry.restore_tab(window, &reference, None) {
                    Some(tab) => {
                        first_added.get_or_insert(tab);
                        last = Some(Last::Added(tab));
                    }
                    // open meanwhile (one place): that place
                    None => match registry.find(&reference) {
                        Some(place) => last = Some(Last::Open(place, None)),
                        None => out.answer.failed.push(failure(
                            path,
                            IpcError::new(ErrorKind::Unavailable).subject(window.to_owned()),
                        )),
                    },
                }
            }
        }
    }
    match last {
        Some(Last::Added(tab)) => {
            out.outcomes.push(registry.activate(tab));
            out.forward = Some(window.to_owned());
        }
        Some(Last::Open(place, document)) => {
            // tabs were added to a window on its Welcome: one of them must show (a window with tabs shows one)
            if let (true, Some(first)) = (on_welcome, first_added) {
                out.outcomes.push(registry.activate(first));
            }
            out.outcomes.push(registry.activate_with(&place, document));
            out.forward = Some(place.window);
        }
        None => {}
    }
    out
}

#[cfg(test)]
mod tests;
