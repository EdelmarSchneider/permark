//! Plugin `broken-links` (a checker): the Markdown links of the project whose target file is missing. A worker scans the
//! documents once and then follows the project's changes, so the list stays true without scanning again.

#![cfg_attr(test, allow(clippy::assert_is_empty))]

mod check;
#[cfg(test)]
mod tests;
mod types;

use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use permark_core::conditions::Requirement;
use permark_core::document::DocScope;
use permark_core::paths::{Vault, WORKTREE_PREFIX};
use permark_core::plugins::{
    typed, CxWorkspace, Manifest, NoArgs, Plugin, PluginCx, PluginError, PluginId, PluginKind,
    PluginScope, RpcRegistration, Running,
};
use permark_core::tree::Explorer;
use permark_core::watch::{FsBatch, Subscription};

pub use check::{root_of, Checker, MAX_DOC_BYTES};
pub use types::{BrokenDoc, BrokenLink, BrokenReport, BrokenRoot, BrokenSummary};

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

struct Shared {
    vault: Vault,
    scope: DocScope,
    explorer: Arc<Explorer>,
    announce: Arc<dyn Fn(&BrokenSummary) + Send + Sync>,
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

/// The report: the opened folder first (its documents and its count), then each worktree in the vault's order, then
/// any other root; so the list cap cuts a worktree's links before the opened folder's.
fn report_of(checker: &Checker, ready: bool, vault: &Vault) -> BrokenReport {
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
    let mut docs = Vec::new();
    let mut listed = 0usize;
    for (path, links) in broken {
        if listed >= MAX_LISTED {
            break;
        }
        let links: Vec<BrokenLink> = links
            .iter()
            .take(MAX_LISTED - listed)
            .map(|l| BrokenLink {
                line: l.line,
                written: l.written.clone(),
            })
            .collect();
        listed += links.len();
        docs.push(BrokenDoc {
            path: path.to_owned(),
            links,
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
                    .read(&self.vault, path, &abs);
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
        for batch in batches {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            changed |= state
                .checker
                .apply(&self.vault, batch, &|path| self.abs_of(path));
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
    ) -> Self {
        let shared = Arc::new(Shared {
            vault,
            scope,
            explorer,
            announce,
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
        report_of(&state.checker, state.ready, &self.shared.vault)
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
        ));
        let subscription = hub.subscribe({
            let service = Arc::clone(&service);
            move |batch| service.feed(batch)
        });
        let mut registrations = Vec::new();
        let s = Arc::clone(&service);
        registrations.extend(cx.rpc("list", typed(move |_: NoArgs, _| Ok(s.report()))));
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
