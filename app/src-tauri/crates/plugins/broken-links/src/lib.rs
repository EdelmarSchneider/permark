//! Plugin `broken-links` (a checker): the Markdown links of the project whose target file is missing. A worker scans the
//! documents once and then follows the project's changes, so the list stays true without scanning again.

#![cfg_attr(test, allow(clippy::assert_is_empty))]

mod check;
#[cfg(test)]
mod tests;
mod types;

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io;
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use permark_core::conditions::Requirement;
use permark_core::document::{DocScope, Documents};
use permark_core::fileops::replacement_href;
use permark_core::format::MarkdownFeatures;
use permark_core::fsx;
use permark_core::links::{local_target, parse, Form};
use permark_core::paths::{Vault, VaultPath, WORKTREE_PREFIX};
use permark_core::plugins::{
    typed, CxWorkspace, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginId, PluginKind,
    PluginScope, RpcRegistration, Running,
};
use permark_core::tree::Explorer;
use permark_core::watch::{FsBatch, OwnWrites, Subscription};

pub use check::{candidates, root_of, Checker, MAX_DOC_BYTES};
pub use types::{
    BrokenDoc, BrokenLink, BrokenReport, BrokenRoot, BrokenSummary, RepairArgs, RepairRefusal,
    RepairResult,
};

const ID: PluginId = PluginId::of("broken-links");
/// The most links a report lists (the count is complete past it).
pub const MAX_LISTED: usize = 500;
/// Changes are gathered this long before they are applied.
const QUIET: Duration = Duration::from_millis(200);

struct BrokenLinksPlugin {
    manifest: Manifest,
}

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    Arc::new(BrokenLinksPlugin {
        manifest: Manifest {
            id: ID,
            kind: PluginKind::Checker,
            scope: PluginScope::Project,
            requires: Requirement::PROJECT,
            depends_on: Cow::Borrowed(&[]),
            default_enabled: true,
        },
    })
}

struct State {
    checker: Checker,
    ready: bool,
}

/// The Markdown features that are on for the project now: with the prompt features, links inside prompt blocks are
/// links, as reading shows them.
type Features = Arc<dyn Fn() -> MarkdownFeatures + Send + Sync>;

struct Shared {
    vault: Vault,
    scope: DocScope,
    explorer: Arc<Explorer>,
    announce: Arc<dyn Fn(&BrokenSummary) + Send + Sync>,
    features: Features,
    state: Mutex<State>,
    stop: AtomicBool,
}

enum Msg {
    Batch(FsBatch),
    Stop,
}

pub struct BrokenLinksService {
    shared: Arc<Shared>,
    tx: Sender<Msg>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// What the window is told: every root, and the opened folder alone.
fn summary_of(checker: &Checker, ready: bool) -> BrokenSummary {
    BrokenSummary {
        ready,
        total: count(checker.total()),
        current: count(checker.totals_by_root().get("").copied().unwrap_or(0)),
    }
}

/// `paths::key_of` of a file name -> project paths. The shape `Explorer::files_keyed` returns.
type FilesByKey = HashMap<String, Vec<String>>;

/// The report: the opened folder first (its documents and its count), then each worktree in the vault's order, then
/// any other root; so the list cap cuts a worktree's links before the opened folder's.
fn report_of(
    checker: &Checker,
    ready: bool,
    vault: &Vault,
    files: &dyn Fn(&HashSet<String>) -> FilesByKey,
) -> BrokenReport {
    let worktrees: Vec<String> = vault
        .worktrees()
        .iter()
        .map(|w| format!("{WORKTREE_PREFIX}{}", w.name))
        .collect();
    let rank = |prefix: &str| -> usize {
        if prefix.is_empty() {
            0
        } else {
            worktrees
                .iter()
                .position(|w| w == prefix)
                .map_or(usize::MAX, |at| at + 1)
        }
    };
    let mut broken: Vec<(&str, Vec<&check::LinkRec>)> = checker.broken().into_iter().collect();
    // stable: by root, then by path as before
    broken.sort_by_key(|(path, _)| rank(root_of(path)));
    let mut chosen = Vec::new();
    let mut listed = 0usize;
    for (path, links) in broken {
        if listed >= MAX_LISTED {
            break;
        }
        let take: Vec<&check::LinkRec> = links.into_iter().take(MAX_LISTED - listed).collect();
        listed += take.len();
        chosen.push((path, take));
    }
    let mut names = HashSet::new();
    for (path, links) in &chosen {
        for link in links {
            names.extend(keyed_names(path, &link.href));
        }
    }
    // The tree is already indexed: one lookup, not another walk of the documents.
    let by_key = files(&names);
    let mut docs = Vec::new();
    for (path, links) in chosen {
        docs.push(BrokenDoc {
            path: path.to_owned(),
            links: links
                .iter()
                .map(|link| BrokenLink {
                    line: link.line,
                    written: link.written.clone(),
                    href: link.href.clone(),
                    candidates: candidates(path, &link.href, &by_key),
                })
                .collect(),
        });
    }
    let by_root = checker.totals_by_root();
    let mut roots: Vec<BrokenRoot> = std::iter::once(String::new())
        .chain(worktrees.iter().cloned())
        .map(|prefix| BrokenRoot {
            total: count(by_root.get(prefix.as_str()).copied().unwrap_or(0)),
            prefix,
        })
        .collect();
    // a root the vault does not name (it went while its documents were still known): counted, never lost
    roots.extend(
        by_root
            .iter()
            .filter(|(prefix, _)| rank(prefix) == usize::MAX)
            .map(|(prefix, n)| BrokenRoot {
                prefix: (*prefix).to_owned(),
                total: count(*n),
            }),
    );
    let total = checker.total();
    BrokenReport {
        ready,
        docs,
        total: count(total),
        current: count(by_root.get("").copied().unwrap_or(0)),
        roots,
        truncated: total > listed,
        skipped: count(checker.skipped()),
    }
}

impl Shared {
    fn summary(&self) -> BrokenSummary {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        summary_of(&state.checker, state.ready)
    }

    fn abs_of(&self, path: &str) -> Option<std::path::PathBuf> {
        self.scope.resolve(path).ok().map(|(_, abs)| abs)
    }

    /// Reads every document of every root (the lock is let go between documents, so a question is answered at once).
    fn scan_all(&self) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .checker = Checker::new();
        let features = (self.features)();
        let mut prefixes = vec![String::new()];
        prefixes.extend(
            self.vault
                .worktrees()
                .iter()
                .map(|w| format!("{WORKTREE_PREFIX}{}", w.name)),
        );
        for prefix in prefixes {
            let (docs, _) = self.explorer.scan_documents(&prefix);
            for (at, path) in docs.iter().enumerate() {
                if at % 32 == 0 && self.stop.load(Ordering::Relaxed) {
                    return;
                }
                let Some(abs) = self.abs_of(path) else {
                    continue;
                };
                self.state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .checker
                    .read(&self.vault, path, &abs, &features);
            }
        }
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .ready = true;
        (self.announce)(&self.summary());
    }

    fn follow(&self, batches: &[FsBatch]) {
        if batches.iter().any(|b| b.bulk || b.rescan) {
            self.scan_all();
            return;
        }
        let mut changed = false;
        let features = (self.features)();
        for batch in batches {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            changed |=
                state
                    .checker
                    .apply(&self.vault, batch, &|path| self.abs_of(path), &features);
        }
        if changed {
            (self.announce)(&self.summary());
        }
    }
}

fn run(shared: &Shared, rx: &std::sync::mpsc::Receiver<Msg>) {
    shared.scan_all();
    let mut batches: Vec<FsBatch> = Vec::new();
    let mut deadline: Option<Instant> = None;
    loop {
        let message = match deadline {
            Some(at) => rx.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match message {
            Ok(Msg::Batch(batch)) => {
                batches.push(batch);
                deadline.get_or_insert_with(|| Instant::now() + QUIET);
            }
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {
                deadline = None;
                shared.follow(&std::mem::take(&mut batches));
            }
        }
    }
}

impl BrokenLinksService {
    /// Starts the worker: it scans the documents at once.
    #[must_use]
    pub fn start(
        vault: Vault,
        scope: DocScope,
        explorer: Arc<Explorer>,
        announce: Arc<dyn Fn(&BrokenSummary) + Send + Sync>,
        features: Features,
    ) -> Self {
        let shared = Arc::new(Shared {
            vault,
            scope,
            explorer,
            announce,
            features,
            state: Mutex::new(State {
                checker: Checker::new(),
                ready: false,
            }),
            stop: AtomicBool::new(false),
        });
        let (tx, rx) = channel();
        let worker = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("permark-broken-links".to_owned())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || run(&worker, &rx))
            .ok();
        Self {
            shared,
            tx,
            thread: Mutex::new(thread),
        }
    }

    /// The project's watch reported a batch (called on the watch thread: quick).
    pub fn feed(&self, batch: &FsBatch) {
        if !batch.is_empty() {
            let _ = self.tx.send(Msg::Batch(batch.clone()));
        }
    }

    /// The broken links, by document and line.
    #[must_use]
    pub fn report(&self) -> BrokenReport {
        let state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        report_of(&state.checker, state.ready, &self.shared.vault, &|names| {
            self.shared.explorer.files_keyed(names)
        })
    }

    /// Waits until the first scan is done (tests).
    #[must_use]
    pub fn wait_ready(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        while !self
            .shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .ready
        {
            if start.elapsed() > timeout {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
        true
    }
}

impl Drop for BrokenLinksService {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        let _ = self.tx.send(Msg::Stop);
        if let Some(thread) = self
            .thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
thread_local! {
    static BEFORE_SECOND_READ: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        std::cell::RefCell::new(None);
}

/// A test changes the file after the repair plan is made and before the bytes are read again.
#[cfg(test)]
pub(crate) fn before_second_read(hook: impl FnOnce() + 'static) {
    BEFORE_SECOND_READ.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

#[cfg(test)]
fn take_before_second_read() -> Option<Box<dyn FnOnce()>> {
    BEFORE_SECOND_READ.with(|slot| slot.borrow_mut().take())
}

/// Drops a hook the call never reached, so the next call on this thread does not run it.
#[cfg(test)]
struct ClearBeforeSecondRead;

#[cfg(test)]
impl Drop for ClearBeforeSecondRead {
    fn drop(&mut self) {
        let _unused = take_before_second_read();
    }
}

#[cfg(test)]
fn run_before_second_read() {
    if let Some(hook) = take_before_second_read() {
        hook();
    }
}

fn refused(reason: RepairRefusal) -> RepairResult {
    RepairResult::Refused { reason }
}

fn document_is_unsaved(documents: &Documents, path: &str) -> bool {
    VaultPath::parse(path).is_ok_and(|parsed| documents.unsaved_keys().contains(&parsed.key()))
}

/// Names `candidates` looks up. Asking for `name.md` as well is a superset when the basename already ends in `.md`.
fn keyed_names(doc: &str, href: &str) -> HashSet<String> {
    let Some(target) = local_target(doc, href) else {
        return HashSet::new();
    };
    let Some(name) = target.rsplit('/').next().filter(|name| !name.is_empty()) else {
        return HashSet::new();
    };
    let mut names = HashSet::with_capacity(2);
    names.insert(name.to_owned());
    names.insert(format!("{name}.md"));
    names
}

fn one_destination(
    text: &str,
    line: u32,
    href: &str,
    features: &MarkdownFeatures,
) -> Result<(Range<usize>, Form), RepairRefusal> {
    let mut matched = parse(text, features)
        .into_iter()
        .filter(|dest| u32::try_from(dest.line).ok() == Some(line) && dest.inner(text) == href);
    let Some(dest) = matched.next() else {
        return Err(RepairRefusal::Mismatch);
    };
    if matched.next().is_some() {
        return Err(RepairRefusal::Mismatch);
    }
    Ok((dest.range, dest.form))
}

fn target_is_candidate(explorer: &Explorer, doc: &str, href: &str, target: &str) -> bool {
    let by_key = explorer.files_keyed(&keyed_names(doc, href));
    candidates(doc, href, &by_key)
        .iter()
        .any(|path| path == target)
}

/// Raw bytes, not a text decode (newlines stay as stored). Larger than the checker reads: left alone.
fn read_raw(abs: &Path) -> Result<Vec<u8>, RepairRefusal> {
    let meta = match std::fs::metadata(abs) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(RepairRefusal::Missing);
        }
        Err(_) => return Err(RepairRefusal::Unreadable),
    };
    if !meta.is_file() || meta.len() > MAX_DOC_BYTES {
        return Err(RepairRefusal::Unreadable);
    }
    match std::fs::read(abs) {
        Ok(bytes) if u64::try_from(bytes.len()).unwrap_or(u64::MAX) <= MAX_DOC_BYTES => Ok(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(RepairRefusal::Missing),
        Ok(_) | Err(_) => Err(RepairRefusal::Unreadable),
    }
}

fn splice(text: &str, range: Range<usize>, token: &str) -> Option<String> {
    let head = text.get(..range.start)?;
    let tail = text.get(range.end..)?;
    let mut out = String::with_capacity(head.len() + token.len() + tail.len());
    out.push_str(head);
    out.push_str(token);
    out.push_str(tail);
    Some(out)
}

/// Rewrites the one destination. A refusal writes nothing. No move journal and no second scan: the watch sees the
/// write.
fn repair(
    documents: &Documents,
    explorer: &Explorer,
    own: &OwnWrites,
    features: &MarkdownFeatures,
    args: &RepairArgs,
) -> RepairResult {
    #[cfg(test)]
    let _clear_hook = ClearBeforeSecondRead;
    if document_is_unsaved(documents, &args.path) {
        return refused(RepairRefusal::Unsaved);
    }
    let Ok((_, abs)) = documents.scope().resolve(&args.path) else {
        return refused(RepairRefusal::Unreadable);
    };
    let bytes = match read_raw(&abs) {
        Ok(bytes) => bytes,
        Err(reason) => return refused(reason),
    };
    let seen = fsx::hash(&bytes);
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return refused(RepairRefusal::NotUtf8);
    };
    let (range, form) = match one_destination(text, args.line, &args.href, features) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    if !target_is_candidate(explorer, &args.path, &args.href, &args.target) {
        return refused(RepairRefusal::NotCandidate);
    }
    let Some(token) = replacement_href(&args.path, &args.href, form, &args.target) else {
        return refused(RepairRefusal::NoReplacement);
    };
    #[cfg(test)]
    run_before_second_read();
    let again = match read_raw(&abs) {
        Ok(bytes) => bytes,
        Err(reason) => return refused(reason),
    };
    if fsx::hash(&again) != seen {
        return refused(RepairRefusal::Changed);
    }
    let Some(new_text) = splice(text, range, &token) else {
        return refused(RepairRefusal::Mismatch);
    };
    let new_bytes = new_text.into_bytes();
    let new_hash = fsx::hash(&new_bytes);
    own.expect_write(&args.path, new_hash);
    // a save that lands after the read above is never replaced: the rename waits for the bytes the edit is for
    match fsx::write_atomic_if_unchanged(&abs, &new_bytes, seen) {
        Ok(true) => RepairResult::Wrote { href: token },
        written => {
            own.retract_write(&args.path, new_hash);
            refused(if written.is_ok() {
                RepairRefusal::Changed
            } else {
                RepairRefusal::WriteFailed
            })
        }
    }
}

impl Plugin for BrokenLinksPlugin {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        let (Some(project), CxWorkspace::Project(vault), Some(hub)) =
            (cx.project(), &cx.workspace, cx.watch())
        else {
            return Err(PluginError::Start {
                id: ID,
                reason: "needs a project".to_owned(),
            });
        };
        let events = cx.events().clone();
        let features: Features = {
            let project = project.clone();
            Arc::new(move || *project.formats().features())
        };
        let service = Arc::new(BrokenLinksService::start(
            vault.clone(),
            project.documents.scope().clone(),
            Arc::clone(&project.explorer),
            Arc::new(move |summary| {
                events.emit(
                    "changed",
                    serde_json::to_value(summary).unwrap_or(serde_json::Value::Null),
                );
            }),
            Arc::clone(&features),
        ));
        let subscription = hub.subscribe({
            let service = Arc::clone(&service);
            move |batch| service.feed(batch)
        });
        let mut registrations = Vec::new();
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc("list", typed(move |_: NoArgs, _| Ok(s.report()))));
        let documents = Arc::clone(&project.documents);
        let explorer = Arc::clone(&project.explorer);
        let own = Arc::clone(&project.own);
        registrations.extend(cx.rpc(
            "repair",
            typed(move |args: RepairArgs, _| {
                Ok(repair(&documents, &explorer, &own, &features(), &args))
            }),
        ));
        Ok(Box::new(BrokenLinksRunning {
            subscription: Some(subscription),
            registrations,
            service: Some(service),
        }))
    }
}

/// A started `broken-links` plugin. Drop = teardown: the watch is not heard, nobody is answered, the worker joins.
struct BrokenLinksRunning {
    subscription: Option<Subscription>,
    registrations: Vec<RpcRegistration>,
    service: Option<Arc<BrokenLinksService>>,
}

impl Running for BrokenLinksRunning {}

impl Drop for BrokenLinksRunning {
    fn drop(&mut self) {
        self.subscription.take();
        self.registrations.clear();
        self.service.take();
    }
}
