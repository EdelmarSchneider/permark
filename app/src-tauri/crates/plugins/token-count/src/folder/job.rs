//! The two phases of a folder count, run on the job's thread: the scan (look at the listing, ask or refuse when the
//! folder is big) and the count (read each file once, or take it from the cache).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use permark_core::fsx::{self, FsxError};
use permark_core::plugins::{ChangeListener, WindowFilesApi};
use permark_core::tree::ScanFile;
use permark_core::watch::FsBatch;
use xxhash_rust::xxh3::xxh3_64;

use super::cache::{Entry, FileCache, Verdict as Known};
use super::guard::{seconds, skip_before_reading, FolderSkip, Found, Verdict, MAX_FILE_BYTES};
use super::summary::Tally;
use super::{failure_of, lock, FolderJob, Folders, Held, Project, Run};
use crate::bpe::Bpe;
use crate::estimate::estimate;
use crate::tokenizer::Tokenizer;
use crate::vocab::VocabStore;

/// The least time between two progress announcements.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// What a window asked for.
#[derive(Clone)]
pub(super) struct Request {
    pub window: String,
    pub folder: String,
    pub tokenizer: Tokenizer,
    pub auto: bool,
}

/// The files of a scan, sorted into those that will be read and those that are left out by name or size.
pub(super) struct Plan {
    pub read: Vec<ScanFile>,
    pub skipped: Vec<(String, FolderSkip)>,
    pub found: Found,
}

impl Plan {
    fn of(files: Vec<ScanFile>) -> Self {
        let mut plan = Self {
            read: Vec::new(),
            skipped: Vec::new(),
            found: Found::default(),
        };
        for file in files {
            plan.found.files += 1;
            if let Some(reason) = skip_before_reading(&file) {
                plan.skipped.push((file.path, reason));
            } else {
                plan.found.bytes += file.size;
                plan.read.push(file);
            }
        }
        plan
    }

    fn total(&self) -> u32 {
        u32::try_from(self.found.files).unwrap_or(u32::MAX)
    }
}

/// How the tokens are counted for the whole job.
enum Method {
    Exact(Arc<Bpe>),
    Estimate(Tokenizer),
}

struct Mode {
    method: Method,
    /// The number is an estimate (a family, or the stand-in for an exact count whose vocabulary is not here).
    approx: bool,
    /// Key of the cache entries: the tokenizer, and `~` for the stand-in.
    basis: String,
    tokenizer: Tokenizer,
}

impl Mode {
    fn choose(tokenizer: Tokenizer, vocab: &VocabStore) -> Self {
        let id = tokenizer.id();
        match tokenizer.encoding() {
            Some(encoding) => match vocab.load(encoding) {
                Ok(bpe) => Self {
                    method: Method::Exact(bpe),
                    approx: false,
                    basis: id.to_owned(),
                    tokenizer,
                },
                Err(_) => Self {
                    method: Method::Estimate(tokenizer),
                    approx: true,
                    basis: format!("{id}~"),
                    tokenizer,
                },
            },
            None => Self {
                method: Method::Estimate(tokenizer),
                approx: true,
                basis: id.to_owned(),
                tokenizer,
            },
        }
    }

    fn tokens(&self, text: &str) -> u64 {
        match &self.method {
            Method::Exact(bpe) => bpe.count(text),
            Method::Estimate(tokenizer) => estimate(*tokenizer, text),
        }
    }
}

/// How the job's thread ends: whatever way it leaves (a panic included), the slot is not left working.
struct Ending<'a> {
    folders: &'a Arc<Folders>,
    window: &'a str,
    folder: &'a str,
    id: u64,
    ended: bool,
}

impl Ending<'_> {
    fn end(&mut self, job: FolderJob) {
        self.ended = true;
        self.folders.set(self.window, self.id, job);
    }

    /// The thread is done without a state to set (it left the slot waiting for the person).
    fn rest(&mut self) {
        self.ended = true;
    }
}

impl Drop for Ending<'_> {
    fn drop(&mut self) {
        if !self.ended {
            self.folders.set(
                self.window,
                self.id,
                FolderJob::Failed {
                    folder: self.folder.to_owned(),
                    reason: super::FolderFailure::Io,
                },
            );
        }
    }
}

/// The state a job is in when it starts counting: what the window sees until the thread has chosen its method.
pub(super) fn counting_state(request: &Request, plan: &Plan, vocab: &VocabStore) -> FolderJob {
    FolderJob::Counting {
        folder: request.folder.clone(),
        done: 0,
        total: plan.total(),
        tokenizer: request.tokenizer.id().to_owned(),
        approx: !exact_ready(request.tokenizer, vocab),
    }
}

/// An exact tokenizer whose vocabulary is on this computer.
fn exact_ready(tokenizer: Tokenizer, vocab: &VocabStore) -> bool {
    tokenizer
        .encoding()
        .is_some_and(|e| vocab.states()[e.index()] == crate::vocab::VocabState::Ready)
}

/// Looks at the folder: ends the job (nothing to ask, too big), parks it waiting for the person, or counts.
pub(super) fn scan_phase(me: &Arc<Folders>, id: u64, request: &Request, cancel: &AtomicBool) {
    let folder = request.folder.as_str();
    let mut ending = Ending {
        folders: me,
        window: &request.window,
        folder,
        id,
        ended: false,
    };
    let Some(files) = me.app.services.get::<WindowFilesApi>() else {
        ending.end(FolderJob::Failed {
            folder: folder.to_owned(),
            reason: super::FolderFailure::Unavailable,
        });
        return;
    };
    let limits = if request.auto {
        me.thresholds.auto_limits()
    } else {
        me.thresholds.refuse_limits()
    };
    let mut seen = Found::default();
    let scanned = files
        .0
        .scan_folder(&request.window, folder, cancel, &mut |file| {
            seen.add(file, limits)
        });
    let scan = match scanned {
        Ok(scan) => scan,
        Err(error) => {
            ending.end(FolderJob::Failed {
                folder: folder.to_owned(),
                reason: failure_of(&error),
            });
            return;
        }
    };
    if cancel.load(Ordering::Relaxed) {
        ending.end(FolderJob::Cancelled {
            folder: folder.to_owned(),
        });
        return;
    }
    if scan.scan.cut_off {
        // the walk stopped at a limit (or at its own: too deep, too many entries)
        if request.auto {
            ending.end(FolderJob::Idle);
        } else {
            ending.end(FolderJob::Refused {
                folder: folder.to_owned(),
                files: u32::try_from(seen.files).unwrap_or(u32::MAX),
                bytes: seen.bytes,
                reason: seen.passed(limits).unwrap_or(super::FolderRefusal::Files),
            });
        }
        return;
    }
    let plan = Plan::of(scan.scan.files);
    let held = Held {
        request: request.clone(),
        plan,
        project: scan.project,
    };
    let found = held.plan.found;
    // an automatic count was held to its small limits by the walk itself: it never asks
    let verdict = if request.auto {
        Verdict::Go
    } else {
        me.thresholds.verdict(found.files, found.bytes)
    };
    match verdict {
        Verdict::Go => {
            ending.rest();
            count_phase(me, id, held, cancel);
        }
        Verdict::Confirm => {
            let exact = exact_ready(request.tokenizer, &me.vocab);
            let job = FolderJob::Confirm {
                folder: folder.to_owned(),
                files: held.plan.total(),
                bytes: found.bytes,
                seconds: seconds(found.files, found.bytes, exact),
            };
            ending.rest();
            me.hold(&request.window, id, held, job);
        }
        Verdict::Refuse(reason) => ending.end(FolderJob::Refused {
            folder: folder.to_owned(),
            files: held.plan.total(),
            bytes: found.bytes,
            reason,
        }),
    }
}

/// Reads and counts the files of a plan; stores the result unless the job was cancelled.
pub(super) fn count_phase(me: &Arc<Folders>, id: u64, held: Held, cancel: &AtomicBool) {
    let Held {
        request,
        plan,
        project,
    } = held;
    let folder = request.folder.as_str();
    let window = request.window.as_str();
    let mut ending = Ending {
        folders: me,
        window,
        folder,
        id,
        ended: false,
    };
    let mode = Mode::choose(request.tokenizer, &me.vocab);
    let total = plan.total();
    let counting = |done: u32| FolderJob::Counting {
        folder: folder.to_owned(),
        done,
        total,
        tokenizer: mode.tokenizer.id().to_owned(),
        approx: mode.approx,
    };
    let mut tally = Tally::default();
    for (path, reason) in &plan.skipped {
        tally.skipped(path, *reason);
    }
    let mut done = u32::try_from(plan.skipped.len()).unwrap_or(u32::MAX);
    if !me.set(window, id, counting(done)) {
        ending.rest();
        return;
    }
    let cache = me.attach(&project, &request, id);
    let started = Instant::now();
    let mut last = Instant::now();
    let mut cancelled = false;
    for file in &plan.read {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        #[cfg(test)]
        std::thread::sleep(Duration::from_micros(me.pace.load(Ordering::Relaxed)));
        match count_file(&mode, &cache, file) {
            Outcome::Counted { tokens, cached } => tally.counted(&file.path, tokens, cached),
            Outcome::Skipped(reason) => tally.skipped(&file.path, reason),
        }
        done += 1;
        if last.elapsed() >= PROGRESS_EVERY {
            last = Instant::now();
            if !me.set(window, id, counting(done)) {
                // the plugin is stopping
                cancelled = true;
                break;
            }
        }
    }
    let touched = me.detach(&project, id);
    // a cache that cannot be written only makes the next count slower
    let pending = lock(&cache).pending();
    if let Some((file, bytes)) = pending {
        let _ = write_cache(&file, &bytes);
    }
    if cancelled || cancel.load(Ordering::Relaxed) {
        ending.end(FolderJob::Cancelled {
            folder: folder.to_owned(),
        });
        return;
    }
    let millis = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let summary = tally.finish(folder, mode.tokenizer.id(), mode.approx, millis);
    me.store(&project, summary, touched);
    ending.end(FolderJob::Done {
        folder: folder.to_owned(),
    });
}

/// Writes the bytes of a cache file (serialized under the cache's lock, written outside it).
fn write_cache(file: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    fsx::write_atomic(file, bytes)
}

enum Outcome {
    Counted { tokens: u64, cached: bool },
    Skipped(FolderSkip),
}

fn known(entry: Entry) -> Outcome {
    match entry.verdict {
        Known::Tokens(tokens) => Outcome::Counted {
            tokens,
            cached: true,
        },
        Known::Binary => Outcome::Skipped(FolderSkip::Binary),
    }
}

/// One file: from the cache when its size and time are those it was counted at, else read once; the same content
/// under another time is taken from the cache too.
fn count_file(mode: &Mode, cache: &Mutex<FileCache>, file: &ScanFile) -> Outcome {
    let Some(abs) = &file.abs else {
        return Outcome::Skipped(FolderSkip::Unreadable);
    };
    let before = lock(cache).get(&mode.basis, &file.path);
    if let Some(entry) = before {
        if entry.size == file.size && entry.modified_ms == file.modified_ms {
            return known(entry);
        }
    }
    // a file swapped for a link since the listing is not followed
    if !std::fs::symlink_metadata(abs).is_ok_and(|m| m.file_type().is_file()) {
        return Outcome::Skipped(FolderSkip::Unreadable);
    }
    let bytes = match fsx::read_bytes(abs, MAX_FILE_BYTES) {
        Ok(bytes) => bytes,
        Err(FsxError::TooLarge { .. }) => return Outcome::Skipped(FolderSkip::TooLarge),
        Err(_) => return Outcome::Skipped(FolderSkip::Unreadable),
    };
    let hash = xxh3_64(&bytes);
    if let Some(entry) = before.filter(|e| e.hash == hash) {
        let fresh = Entry {
            size: file.size,
            modified_ms: file.modified_ms,
            ..entry
        };
        lock(cache).put(&mode.basis, &file.path, fresh);
        return known(entry);
    }
    let (verdict, outcome) = match fsx::decode(&bytes) {
        Some(decoded) => {
            let tokens = mode.tokens(&decoded.text);
            (
                Known::Tokens(tokens),
                Outcome::Counted {
                    tokens,
                    cached: false,
                },
            )
        }
        None => (Known::Binary, Outcome::Skipped(FolderSkip::Binary)),
    };
    lock(cache).put(
        &mode.basis,
        &file.path,
        Entry {
            size: file.size,
            modified_ms: file.modified_ms,
            hash,
            verdict,
        },
    );
    outcome
}

impl Folders {
    fn cache_file(&self, project: &str) -> std::path::PathBuf {
        self.dir
            .join(format!("{:016x}.json", xxh3_64(project.as_bytes())))
    }

    /// The project's cache, with the job registered as working below its folder, and the project's file watch on.
    fn attach(
        self: &Arc<Self>,
        project: &str,
        request: &Request,
        job: u64,
    ) -> Arc<Mutex<FileCache>> {
        // the cache file is read before the lock: results are asked for while a big one is parsed
        let known = lock(&self.projects)
            .by_root
            .get(project)
            .map(|p| Arc::clone(&p.cache));
        let cache = known.unwrap_or_else(|| {
            Arc::new(Mutex::new(FileCache::open(Some(self.cache_file(project)))))
        });
        let (cache, watch) = {
            let mut projects = lock(&self.projects);
            let entry = projects.project(project, || Project {
                cache: Arc::clone(&cache),
                results: std::collections::BTreeMap::new(),
                sub: None,
                runs: Vec::new(),
            });
            entry.runs.push(Run {
                job,
                folder: request.folder.clone(),
                touched: false,
            });
            (Arc::clone(&entry.cache), entry.sub.is_none())
        };
        if watch {
            let me = self.me.clone();
            let root = project.to_owned();
            let listener: ChangeListener = Arc::new(move |batch: &FsBatch| {
                if let Some(me) = me.upgrade() {
                    me.changed(&root, batch);
                }
            });
            let sub = self
                .app
                .services
                .get::<WindowFilesApi>()
                .and_then(|files| files.0.watch(&request.window, listener));
            if let Some(sub) = sub {
                let mut projects = lock(&self.projects);
                if let Some(entry) = projects.by_root.get_mut(project) {
                    if entry.sub.is_none() {
                        entry.sub = Some(sub);
                    }
                }
            }
        }
        cache
    }

    /// The job stops working below its folder; whether something there changed meanwhile.
    fn detach(&self, project: &str, job: u64) -> bool {
        let mut projects = lock(&self.projects);
        let Some(entry) = projects.by_root.get_mut(project) else {
            return false;
        };
        let at = entry.runs.iter().position(|r| r.job == job);
        at.is_some_and(|at| entry.runs.remove(at).touched)
    }

    fn store(&self, project: &str, summary: super::FolderSummary, stale: bool) {
        {
            let mut projects = lock(&self.projects);
            let Some(entry) = projects.by_root.get_mut(project) else {
                return;
            };
            entry.results.insert(
                summary.folder.clone(),
                super::Stored {
                    summary: Arc::new(summary),
                    stale,
                },
            );
        }
        self.announce_badges(project);
    }
}
