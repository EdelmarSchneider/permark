//! Documents: reading a file into a view, telling the windows that show it when it changes.
//!
//! One [`Documents`] per workspace (project: on `ProjectCore`, fed by the file watch; single file: on `FileSession`,
//! polled). Per window it remembers the document shown; a change of that file becomes a `document-changed` event to
//! that window, which asks for the view again (blocks it already holds are not sent twice).

pub mod assets;
mod code_refs;
pub mod editor;
mod scope;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, UNIX_EPOCH};

use serde::Serialize;
use ts_rs::TS;

pub use code_refs::resolve_code_refs;
pub use scope::{DocScope, LinkTarget};

use crate::error::{ErrorKind, IpcError};
use crate::events::{CoreEvent, EventSink};
use crate::format::{Body, Editability, FormatRegistry, Notice, RenderCx};
use crate::fsx::{self, Encoding, FsxError};
use crate::watch::{ChangeKind, FsBatch};

/// Text larger than this is not rendered (`TooLarge` notice).
pub const MAX_TEXT_BYTES: u64 = 5 * 1024 * 1024;
/// How much of a file's start a format may sniff.
const HEAD_BYTES: usize = 512;
/// How often a single file's folder is looked at.
const POLL_EVERY: Duration = Duration::from_secs(1);

/// What a window shows of one file.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DocumentView {
    /// Project path as asked (single file: its name).
    pub path: String,
    pub name: String,
    #[ts(type = "string")]
    pub format: crate::format::FormatId,
    /// Role of the file (`agents`, `claude`, ...), when a role rule matches.
    pub role: Option<String>,
    pub body: Body,
    /// xxh3 hex16 of the file bytes (none for media and notices).
    pub hash: Option<String>,
    pub size: u64,
    pub mtime_ms: u64,
    pub editable: Editability,
    /// `utf16 | lossy | deepNesting`.
    pub notes: Vec<String>,
}

/// What happened to the document a window shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "change", rename_all = "camelCase")]
#[ts(export)]
pub enum DocChange {
    /// The file has other content (or is back): ask for the view again.
    Changed { version: String },
    /// The file is gone; `moved_to` when the watch saw it renamed.
    Deleted { moved_to: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DocumentChanged {
    pub window: String,
    pub path: String,
    #[serde(flatten)]
    pub change: DocChange,
}

/// How a version of a file is told from another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Hash of the bytes (text the view was made from).
    Hash,
    /// Length + modification time (media, notices: files not read).
    Signature,
}

#[derive(Debug, Clone)]
struct Shown {
    path: String,
    /// Folded key of the project path (matching watch changes).
    key: String,
    abs: PathBuf,
    mode: Mode,
    /// Last version sent to the window (`None`: the file is gone).
    version: Option<String>,
}

struct Inner {
    scope: DocScope,
    sink: Arc<dyn EventSink>,
    shown: Mutex<HashMap<String, Shown>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn mtime_ms(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn signature(meta: &std::fs::Metadata) -> String {
    format!("{}-{}", meta.len(), mtime_ms(meta))
}

/// The version of the file now (`None`: gone | unreadable).
fn version_of(shown: &Shown) -> Option<String> {
    match shown.mode {
        Mode::Signature => std::fs::metadata(&shown.abs)
            .ok()
            .filter(std::fs::Metadata::is_file)
            .map(|m| signature(&m)),
        Mode::Hash => fsx::read_bytes(&shown.abs, MAX_TEXT_BYTES)
            .ok()
            .map(|bytes| fsx::hash(&bytes).hex())
            .or_else(|| {
                // grew past the limit | locked: still there, so told apart by size and time
                std::fs::metadata(&shown.abs)
                    .ok()
                    .filter(std::fs::Metadata::is_file)
                    .map(|m| signature(&m))
            }),
    }
}

impl Inner {
    fn emit(&self, window: &str, shown: &Shown, change: DocChange) {
        self.sink.emit(CoreEvent::DocumentChanged(DocumentChanged {
            window: window.to_owned(),
            path: shown.path.clone(),
            change,
        }));
    }

    /// Looks at the documents of `windows` (labels) and tells the ones whose file is not as last told. `moved_to`:
    /// where the file went, when it is gone because of a rename.
    fn check(&self, windows: &[String], moved_to: Option<&str>) {
        let work: Vec<(String, Shown)> = {
            let shown = lock(&self.shown);
            windows
                .iter()
                .filter_map(|w| shown.get(w).map(|s| (w.clone(), s.clone())))
                .collect()
        };
        for (window, doc) in work {
            // the disk is looked at without holding the lock
            let now = version_of(&doc);
            let mut shown = lock(&self.shown);
            let Some(entry) = shown.get_mut(&window) else {
                continue;
            };
            if entry.path != doc.path || entry.version == now {
                continue;
            }
            entry.version.clone_from(&now);
            let change = match now {
                Some(version) => DocChange::Changed { version },
                None => DocChange::Deleted {
                    moved_to: moved_to.map(str::to_owned),
                },
            };
            let snapshot = entry.clone();
            drop(shown);
            self.emit(&window, &snapshot, change);
        }
    }

    fn windows_showing(&self, key: &str) -> Vec<String> {
        lock(&self.shown)
            .iter()
            .filter(|(_, s)| s.key == key)
            .map(|(w, _)| w.clone())
            .collect()
    }

    fn all_windows(&self) -> Vec<String> {
        lock(&self.shown).keys().cloned().collect()
    }
}

/// A thread that looks at the shown files of a single-file workspace; stopped and joined on drop.
struct Poller {
    stop: Option<mpsc::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl Poller {
    fn start(inner: Arc<Inner>) -> Self {
        let (stop, wake) = mpsc::channel::<()>();
        let handle = std::thread::Builder::new()
            .name("permark-doc-poll".to_owned())
            .spawn(move || loop {
                match wake.recv_timeout(POLL_EVERY) {
                    Err(RecvTimeoutError::Timeout) => {
                        let windows = inner.all_windows();
                        if !windows.is_empty() {
                            inner.check(&windows, None);
                        }
                    }
                    // a message or a dropped sender: stop
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
                }
            })
            .ok();
        Self {
            stop: Some(stop),
            handle,
        }
    }
}

impl Drop for Poller {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub struct Documents {
    inner: Arc<Inner>,
    _poller: Option<Poller>,
}

impl std::fmt::Debug for Documents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Documents").finish_non_exhaustive()
    }
}

impl Documents {
    /// For a project: changes arrive through [`Documents::on_batch`].
    #[must_use]
    pub fn for_project(scope: DocScope, sink: Arc<dyn EventSink>) -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(Inner {
                scope,
                sink,
                shown: Mutex::new(HashMap::new()),
            }),
            _poller: None,
        })
    }

    /// For a single file: its changes are polled while the service lives.
    #[must_use]
    pub fn for_file(scope: DocScope, sink: Arc<dyn EventSink>) -> Arc<Self> {
        let inner = Arc::new(Inner {
            scope,
            sink,
            shown: Mutex::new(HashMap::new()),
        });
        let poller = Poller::start(Arc::clone(&inner));
        Arc::new(Self {
            inner,
            _poller: Some(poller),
        })
    }

    #[must_use]
    pub fn scope(&self) -> &DocScope {
        &self.inner.scope
    }

    /// Reads `path`, renders it and remembers it as the window's document.
    ///
    /// # Errors
    /// Path outside the workspace | not found | unreadable.
    pub fn open(
        &self,
        window: &str,
        path: &str,
        known: &[String],
        formats: &FormatRegistry,
    ) -> Result<DocumentView, IpcError> {
        let scope = &self.inner.scope;
        let (key, abs) = scope.resolve(path)?;
        let meta = std::fs::metadata(&abs).map_err(|e| IpcError::io(path, &e))?;
        if !meta.is_file() {
            return Err(IpcError::new(ErrorKind::NotFound).subject(path));
        }
        let known: HashSet<String> = known.iter().cloned().collect();
        let name = crate::format::name_of(path).to_owned();
        let cx = |path| RenderCx {
            path,
            env: scope,
            features: formats.features(),
            known: &known,
        };
        let mut view = DocumentView {
            path: path.to_owned(),
            name,
            format: crate::format::FormatId::TEXT,
            role: None,
            body: Body::Notice {
                notice: Notice::NoViewer,
            },
            hash: None,
            size: meta.len(),
            mtime_ms: mtime_ms(&meta),
            editable: read_only("noViewer"),
            notes: Vec::new(),
        };
        let mut mode = Mode::Signature;
        let mut version = Some(signature(&meta));
        match formats.pick(path, &[]) {
            Some(format) if !format.reads_text() => {
                let rendered = format.render("", &cx(path));
                view.format = format.id();
                view.body = rendered.body;
                view.editable = read_only("media");
            }
            _ => match fsx::read_bytes(&abs, MAX_TEXT_BYTES) {
                Err(FsxError::TooLarge { .. }) => {
                    view.body = Body::Notice {
                        notice: Notice::TooLarge {
                            limit: MAX_TEXT_BYTES,
                        },
                    };
                    view.editable = read_only("tooLarge");
                }
                Err(error) => return Err(error.into()),
                Ok(bytes) => {
                    mode = Mode::Hash;
                    let digest = fsx::hash(&bytes).hex();
                    version = Some(digest.clone());
                    view.hash = Some(digest);
                    match fsx::decode(&bytes) {
                        None => {
                            view.body = Body::Notice {
                                notice: Notice::Binary,
                            };
                            view.editable = read_only("binary");
                        }
                        Some(decoded) => {
                            let head = &bytes[..bytes.len().min(HEAD_BYTES)];
                            if let Some(format) = formats.pick(path, head) {
                                let rendered = format.render(&decoded.text, &cx(path));
                                view.format = format.id();
                                view.role = (format.id() == crate::format::FormatId::MARKDOWN)
                                    .then(|| formats.role_of(path))
                                    .flatten()
                                    .map(str::to_owned);
                                view.body = rendered.body;
                                view.notes
                                    .extend(rendered.notes.iter().map(|n| (*n).to_owned()));
                                view.editable = Editability::Surgical;
                            } else {
                                view.editable = read_only("noViewer");
                            }
                            if matches!(decoded.encoding, Encoding::Utf16Le | Encoding::Utf16Be) {
                                view.notes.push("utf16".to_owned());
                            }
                            if decoded.lossy {
                                view.notes.push("lossy".to_owned());
                            }
                        }
                    }
                }
            },
        }
        lock(&self.inner.shown).insert(
            window.to_owned(),
            Shown {
                path: path.to_owned(),
                key,
                abs,
                mode,
                version,
            },
        );
        Ok(view)
    }

    /// The window shows no document any more.
    pub fn close(&self, window: &str) {
        lock(&self.inner.shown).remove(window);
    }

    /// The text of a document as read (copy document). Same limit as the view.
    ///
    /// # Errors
    /// Outside the workspace | not found | too large | binary.
    pub fn text(&self, path: &str) -> Result<String, IpcError> {
        let (_, abs) = self.inner.scope.resolve(path)?;
        Ok(fsx::read_text(&abs, MAX_TEXT_BYTES)?.text)
    }

    /// File watch batch (project): tells the windows whose document it touched.
    pub fn on_batch(&self, batch: &FsBatch) {
        if lock(&self.inner.shown).is_empty() {
            return;
        }
        if batch.rescan {
            self.inner.check(&self.inner.all_windows(), None);
        }
        for change in &batch.changes {
            let key = crate::paths::VaultPath::parse(&change.path).map(|p| p.key());
            let Ok(key) = key else {
                continue;
            };
            match &change.kind {
                ChangeKind::Created | ChangeKind::Modified | ChangeKind::Removed => {
                    self.inner.check(&self.inner.windows_showing(&key), None);
                }
                ChangeKind::Renamed { from } => {
                    if let Ok(old) = crate::paths::VaultPath::parse(from) {
                        let windows = self.inner.windows_showing(&old.key());
                        self.inner.check(&windows, Some(&change.path));
                    }
                    self.inner.check(&self.inner.windows_showing(&key), None);
                }
            }
        }
    }

    /// Where each `href` of the document `doc` leads.
    #[must_use]
    pub fn resolve_links(&self, doc: &str, hrefs: &[String]) -> Vec<LinkTarget> {
        self.inner.scope.resolve_links(doc, hrefs)
    }
}

fn read_only(reason: &str) -> Editability {
    Editability::ReadOnly {
        reason: reason.to_owned(),
    }
}
