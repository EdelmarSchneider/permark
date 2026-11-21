//! Measurements of the hot paths: opt-in (`#[ignore]`), never part of the checks. Each case runs on generated input in
//! a temporary folder and prints `PERF <case> median_ms=<m> n=<runs>` on the console; nothing leaves the machine.
//!
//! Run from `app/src-tauri` in an optimized build, one case at a time:
//! `cargo test --profile perf -p permark-core --lib perf_ -- --ignored --test-threads=1`
//! (`perf` = release code with debug assertions: see the workspace manifest). `PERMARK_PERF_SCALE` (percent, default
//! 100) shrinks every input, for a quick check of the cases themselves. `perf_write_fixture` writes the large project
//! into `PERMARK_PERF_DIR` (an empty folder) for measurements of the real app.

#![allow(clippy::too_many_lines)] // each case reads top to bottom: input, then its timed runs

mod fixtures;

use std::collections::HashSet;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::activity::{system_clock, ActivityLog};
use crate::events::NoEvents;
use crate::fileops::{check_deletes, FileOps, MoveSpec};
use crate::format::{sections, Body, DocEnv, Format, MarkdownFeatures, MarkdownFormat, RenderCx};
use crate::paths::{Root, Vault, VaultPath};
use crate::search::{SearchFilter, SearchIndex};
use crate::settings::ProjectConfig;
use crate::state::{AppDirs, StateStore};
use crate::tree::{key_of, Explorer, Ignorer, TreeFilter};
use crate::watch::{
    reconcile, sig_of, Change, ChangeKind, Env, FsBatch, OwnWrites, Pending, SigStore, WatchHub,
    Watcher,
};
use fixtures::MB;

/// Runs per case unless the case is too long for that many.
const RUNS: usize = 5;
/// Longest wait for a background worker (index build, a batch) before the case fails.
const WAIT: Duration = Duration::from_mins(30);

// ---- runner --------------------------------------------------------------------------------------------------------

/// Percent of the nominal input sizes (`PERMARK_PERF_SCALE`, default 100).
fn scale() -> usize {
    std::env::var("PERMARK_PERF_SCALE")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|p| *p > 0)
        .unwrap_or(100)
}

fn scaled(n: usize) -> usize {
    (n * scale() / 100).max(1)
}

/// One line on the process's stderr: not the test's captured output, so it shows without `--nocapture`.
fn say(line: &str) {
    let _ = writeln!(std::io::stderr(), "{line}");
}

/// Runs `run` `runs` times (each returns the time of the part it measures, its setup left out) and prints the median.
fn report(case: &str, runs: usize, mut run: impl FnMut(usize) -> Duration) -> f64 {
    let mut times: Vec<Duration> = (0..runs).map(&mut run).collect();
    times.sort();
    let median = times[times.len() / 2].as_secs_f64() * 1000.0;
    let scale = scale();
    let note = if scale == 100 {
        String::new()
    } else {
        format!(" scale={scale}%")
    };
    say(&format!("PERF {case} median_ms={median:.1} n={runs}{note}"));
    median
}

fn timed<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let started = Instant::now();
    let out = f();
    (out, started.elapsed())
}

/// Polls `ok` until it holds; fails the case after [`WAIT`].
fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let started = Instant::now();
    while !ok() {
        assert!(started.elapsed() < WAIT, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A word no generated text holds (letters only: the tokenizer keeps it whole).
fn marker(n: usize) -> String {
    let letters: String = n
        .to_string()
        .bytes()
        .map(|digit| char::from(b'a' + (digit - b'0')))
        .collect();
    format!("zqx{letters}")
}

fn change(path: &str, kind: ChangeKind, is_dir: bool) -> Change {
    Change {
        path: path.to_owned(),
        kind,
        is_dir,
        own: false,
    }
}

/// `rel` (with `/`) below `root`, with the platform's separators.
fn abs_of(root: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

fn vault_of(dir: &Path) -> Vault {
    Vault::new(Root::open(dir).unwrap())
}

fn explorer_of(vault: &Vault) -> Explorer {
    Explorer::new(vault, &ProjectConfig::default(), Arc::new(NoEvents), None)
}

// ---- Markdown ------------------------------------------------------------------------------------------------------

struct NoFiles;

impl DocEnv for NoFiles {
    fn file_exists(&self, _: &str) -> bool {
        false
    }
}

/// Reading a document: the whole pipeline (parse, HTML, sanitizer, blocks, outline) and the section split.
#[test]
#[ignore = "measurement: run with --ignored in an optimized build"]
fn perf_markdown_render_and_sections() {
    let inputs = [
        ("1mb", fixtures::typical_markdown(scaled(MB), 1)),
        ("5mb", fixtures::typical_markdown(scaled(5 * MB), 2)),
        (
            "5mb_headings",
            fixtures::heading_dense_markdown(scaled(5 * MB)),
        ),
    ];
    let known = HashSet::new();
    let features = MarkdownFeatures::default();
    let cx = RenderCx {
        path: "docs/a.md",
        env: &NoFiles,
        features: &features,
        known: &known,
    };
    for (name, text) in &inputs {
        let rendered = MarkdownFormat.render(text, &cx);
        let Body::Markdown { outline, .. } = &rendered.body else {
            panic!("{name}: not read as Markdown");
        };
        assert!(rendered.notes.is_empty(), "{name}: {:?}", rendered.notes);
        say(&format!(
            "PERF markdown_{name} bytes={} headings={}",
            text.len(),
            outline.len()
        ));
        report(&format!("markdown_render_{name}"), RUNS, |_| {
            timed(|| MarkdownFormat.render(text, &cx)).1
        });
        report(&format!("markdown_sections_{name}"), RUNS, |_| {
            timed(|| sections(text)).1
        });
    }
}

// ---- search --------------------------------------------------------------------------------------------------------

/// Snippets of hits in large sections (files without headings are one section).
#[test]
#[ignore = "measurement: run with --ignored in an optimized build"]
fn perf_search_large_sections() {
    let dir = tempfile::tempdir().unwrap();
    let count = scaled(100);
    for at in 0..count {
        let text = fixtures::prose(scaled(MB), u64::try_from(at).unwrap());
        fixtures::write(dir.path(), &format!("big/s{at}.md"), text.as_bytes());
    }
    let vault = vault_of(dir.path());
    let explorer = Arc::new(explorer_of(&vault));
    let mut built = None;
    report("index_build_100_docs_1mb", 1, |_| {
        let (index, took) = timed(|| {
            let index = SearchIndex::start(
                vault.clone(),
                Arc::clone(&explorer),
                Arc::new(NoEvents),
                "root".to_owned(),
            );
            assert!(index.wait_ready(WAIT));
            index
        });
        built = Some(index);
        took
    });
    let index = built.unwrap();
    report("search_100_hits_in_1mb_sections", RUNS, |_| {
        let (hits, took) = timed(|| index.sections("harbor", 100, &SearchFilter::default()).hits);
        assert_eq!(hits.len(), count);
        took
    });
}

// ---- large project: tree + index -----------------------------------------------------------------------------------

/// A project of 100k files (+ 50k ignored): the tree's first list, a one-file change per window, the index's first
/// build, queries, a new folder and a bulk batch reaching the index.
#[test]
#[ignore = "measurement: run with --ignored in an optimized build"]
fn perf_large_project_tree_and_index() {
    let dir = tempfile::tempdir().unwrap();
    let (made, took) =
        timed(|| fixtures::large_folder(dir.path(), scaled(100_000), scaled(50_000)));
    say(&format!(
        "PERF large_project written in {:.1} s: documents={} media={} ignored={}",
        took.as_secs_f64(),
        made.documents,
        made.media,
        made.ignored
    ));
    let listed = made.documents + made.media;
    let vault = vault_of(dir.path());

    report("tree_snapshot_100k", 3, |_| {
        let explorer = explorer_of(&vault);
        let (snapshot, took) = timed(|| explorer.open_view("w", Some(TreeFilter::default())));
        assert_eq!(usize::try_from(snapshot.total_files).unwrap(), listed);
        took
    });

    let explorer = Arc::new(explorer_of(&vault));
    explorer.open_view("w0", None);
    let mut next = 0;
    let mut one_new_file = |explorer: &Explorer| {
        next += 1;
        let rel = format!("fresh/new{next}.md");
        fixtures::write(dir.path(), &rel, b"# New\n");
        timed(|| explorer.sync_paths(&[rel], &[])).1
    };
    report("tree_delta_100k_1_window", 21, |_| one_new_file(&explorer));
    explorer.open_view("w1", None);
    explorer.open_view("w2", None);
    report("tree_delta_100k_3_windows", 21, |_| one_new_file(&explorer));

    let mut built = None;
    report("index_build_100k", 1, |_| {
        let (index, took) = timed(|| {
            let index = SearchIndex::start(
                vault.clone(),
                Arc::clone(&explorer),
                Arc::new(NoEvents),
                "root".to_owned(),
            );
            assert!(index.wait_ready(WAIT));
            index
        });
        built = Some(index);
        took
    });
    let index = built.unwrap();
    assert!(index.doc_count() >= made.documents);
    say(&format!(
        "PERF index_text_100k bytes={} documents={} sections={}",
        index.text_bytes(),
        index.doc_count(),
        index.status().sections
    ));
    let all = SearchFilter::default();
    for (case, query) in [
        ("search_prefix_1_char_100k", "t"),
        ("search_common_term_100k", "plan"),
        ("search_three_terms_100k", "plan review harbor"),
    ] {
        let hits = index.sections(query, 100, &all).hits.len();
        say(&format!("PERF {case} hits={hits}"));
        report(case, 11, |_| timed(|| index.sections(query, 100, &all)).1);
    }
    report("search_quick_open_100k", 11, |_| {
        timed(|| index.paths("note9999", 50, 5, &all)).1
    });

    let feed = index.feeder();
    report("index_new_folder_100k", RUNS, |run| {
        let word = marker(run);
        let folder = format!("added{run}");
        let mut changes = vec![change(&folder, ChangeKind::Created, true)];
        for k in 0..3 {
            let rel = format!("{folder}/doc{k}.md");
            fixtures::write(dir.path(), &rel, format!("# Added\n\n{word}\n").as_bytes());
            changes.push(change(&rel, ChangeKind::Created, false));
        }
        let started = Instant::now();
        feed(&FsBatch {
            changes,
            ..FsBatch::default()
        });
        wait_until("the new folder's documents", || {
            index.sections(&word, 10, &all).hits.len() == 3
        });
        started.elapsed()
    });
    report("index_bulk_batch_100k", 3, |run| {
        let word = marker(1000 + run);
        let fence = format!("fence{run}.md");
        fixtures::write(
            dir.path(),
            &fence,
            format!("# Fence\n\n{word}\n").as_bytes(),
        );
        let started = Instant::now();
        feed(&FsBatch {
            bulk: true,
            changes: vec![change("area0/topic0/note0.md", ChangeKind::Modified, false)],
            ..FsBatch::default()
        });
        // the worker takes batches in order: the fence is found once the bulk pass is done
        feed(&FsBatch {
            changes: vec![change(&fence, ChangeKind::Created, false)],
            ..FsBatch::default()
        });
        wait_until("the fence after a bulk batch", || {
            index.sections(&word, 10, &all).hits.len() == 1
        });
        started.elapsed()
    });
}

// ---- watch: the reconcile pass -------------------------------------------------------------------------------------

/// A project folder and its ignore rules, for the reconcile pass called directly (no OS events, no debounce).
struct WatchFx {
    dir: tempfile::TempDir,
    vault: Vault,
    ignorers: Vec<(String, Arc<Ignorer>)>,
}

impl WatchFx {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let vault = vault_of(dir.path());
        let ignorers = explorer_of(&vault).ignorers();
        Self {
            dir,
            vault,
            ignorers,
        }
    }

    fn abs(&self, rel: &str) -> PathBuf {
        abs_of(self.vault.main().path(), rel)
    }

    fn write(&self, rel: &str, bytes: &[u8]) {
        fixtures::write(self.dir.path(), rel, bytes);
    }

    /// A store holding the signatures of `rels` as they are on disk now (what the watch saw before).
    fn seen(&self, rels: &[String]) -> SigStore {
        let sigs = SigStore::default();
        for rel in rels {
            let abs = self.abs(rel);
            let meta = fs::symlink_metadata(&abs).unwrap();
            sigs.set(key_of(rel), rel, sig_of(&abs, &meta).unwrap());
        }
        sigs
    }

    fn move_all(&self, from: &[String], to: &[String]) {
        for (a, b) in from.iter().zip(to) {
            fs::rename(self.abs(a), self.abs(b)).unwrap();
        }
    }

    /// The hints of moves `from` -> `to` as the OS reports them: the removal, then the creation (naming where it came
    /// from when `hinted`).
    fn moved(&self, from: &[String], to: &[String], hinted: bool) -> Vec<Pending> {
        from.iter()
            .zip(to)
            .flat_map(|(a, b)| {
                [
                    Pending {
                        removed: true,
                        ..hint(self.abs(a))
                    },
                    Pending {
                        created: true,
                        rename_from: hinted.then(|| self.abs(a)),
                        ..hint(self.abs(b))
                    },
                ]
            })
            .collect()
    }

    fn run(
        &self,
        sigs: &SigStore,
        known: &dyn Fn(&str) -> Option<bool>,
        pending: Vec<Pending>,
    ) -> (Vec<Change>, Duration) {
        let env = Env {
            vault: &self.vault,
            sigs,
            ignorers: &self.ignorers,
            known,
            opened_ignored: &|_| false,
            wanted: &|_| false,
        };
        let (done, took) = timed(|| reconcile(&env, pending));
        (done.changes, took)
    }
}

fn hint(abs: PathBuf) -> Pending {
    Pending {
        abs,
        created: false,
        removed: false,
        modified: false,
        rename_from: None,
        after_removal: None,
        deferred: false,
    }
}

fn unknown(_: &str) -> Option<bool> {
    None
}

fn renamed(changes: &[Change]) -> usize {
    changes
        .iter()
        .filter(|c| matches!(c.kind, ChangeKind::Renamed { .. }))
        .count()
}

/// New files: each is read and hashed (pairing a move, marking the app's own write).
#[test]
#[ignore = "measurement: run with --ignored in an optimized build"]
fn perf_watch_created_files() {
    let fx = WatchFx::new();
    let n = scaled(1000);
    report("watch_created_1000_md", RUNS, |run| {
        let rels: Vec<String> = (0..n).map(|i| format!("new{run}/doc{i}.md")).collect();
        for (i, rel) in rels.iter().enumerate() {
            fx.write(
                rel,
                format!("# Doc {i}\n\nText of document {i}.\n").as_bytes(),
            );
        }
        let pending = rels
            .iter()
            .map(|rel| Pending {
                created: true,
                ..hint(fx.abs(rel))
            })
            .collect();
        let (changes, took) = fx.run(&SigStore::default(), &unknown, pending);
        assert_eq!(changes.len(), n);
        took
    });
    let big = scaled(300);
    let mut bytes = vec![b'x'; 7 * MB];
    let rels: Vec<String> = (0..big).map(|i| format!("big/picture{i}.bin")).collect();
    for (i, rel) in rels.iter().enumerate() {
        bytes[..8].copy_from_slice(&u64::try_from(i).unwrap().to_le_bytes());
        fx.write(rel, &bytes);
    }
    report("watch_created_300_files_7mb", 3, |_| {
        let pending = rels
            .iter()
            .map(|rel| Pending {
                created: true,
                ..hint(fx.abs(rel))
            })
            .collect();
        let (changes, took) = fx.run(&SigStore::default(), &unknown, pending);
        assert_eq!(changes.len(), big);
        took
    });
}

/// A storm in an ignored folder (`rm -rf node_modules`): every hint is dropped.
#[test]
#[ignore = "measurement: run with --ignored in an optimized build"]
fn perf_watch_ignored_storm() {
    let fx = WatchFx::new();
    let n = scaled(50_000);
    report("watch_removed_50k_ignored", RUNS, |_| {
        let pending = (0..n)
            .map(|i| Pending {
                removed: true,
                ..hint(fx.abs(&format!("node_modules/pkg{}/lib/file{i}.js", i / 100)))
            })
            .collect();
        let (changes, took) = fx.run(&SigStore::default(), &unknown, pending);
        assert_eq!(changes.len(), 0);
        took
    });
}

/// A checkout: files the watch knew change content in place.
#[test]
#[ignore = "measurement: run with --ignored in an optimized build"]
fn perf_watch_checkout() {
    let fx = WatchFx::new();
    let n = scaled(5000);
    let rels: Vec<String> = (0..n).map(|i| format!("co/d{}/f{i}.md", i / 100)).collect();
    for rel in &rels {
        fx.write(rel, format!("# {rel}\n\nfirst\n").as_bytes());
    }
    report("watch_checkout_5000", RUNS, |run| {
        let sigs = fx.seen(&rels);
        for rel in &rels {
            fx.write(
                rel,
                format!("# {rel}\n\nversion {run} of the text\n").as_bytes(),
            );
        }
        let pending = rels
            .iter()
            .map(|rel| Pending {
                modified: true,
                ..hint(fx.abs(rel))
            })
            .collect();
        let (changes, took) = fx.run(&sigs, &unknown, pending);
        assert_eq!(changes.len(), n);
        took
    });
}

/// Moves: renames the OS reported (hinted), removal + creation pairs found by content, moves the app announced.
#[test]
#[ignore = "measurement: run with --ignored in an optimized build"]
fn perf_watch_moves() {
    let fx = WatchFx::new();
    let n = scaled(5000);
    let left: Vec<String> = (0..n).map(|i| format!("mv/a/f{i}.md")).collect();
    let right: Vec<String> = (0..n).map(|i| format!("mv/b/f{i}.md")).collect();
    for (i, rel) in left.iter().enumerate() {
        fx.write(rel, format!("# Moved {i}\n\nbody {i}\n").as_bytes());
    }
    fs::create_dir_all(fx.abs("mv/b")).unwrap();
    // each run moves every file to the other side
    let mut at_left = true;
    for (case, hinted) in [
        ("watch_renames_5000_hinted", true),
        ("watch_moves_5000_by_content", false),
    ] {
        report(case, RUNS, |_| {
            let (from, to) = if at_left {
                (&left, &right)
            } else {
                (&right, &left)
            };
            at_left = !at_left;
            let sigs = fx.seen(from);
            fx.move_all(from, to);
            let (changes, took) = fx.run(&sigs, &unknown, fx.moved(from, to, hinted));
            assert_eq!(renamed(&changes), n, "{case}");
            took
        });
    }
    // announcements expire (a few seconds): a pass slower than that reports the moves as removals + creations
    let mut paired = n;
    report("watch_moves_5000_announced", RUNS, |_| {
        let (from, to) = if at_left {
            (&left, &right)
        } else {
            (&right, &left)
        };
        at_left = !at_left;
        let own = OwnWrites::new();
        let pairs: Vec<(String, String)> = from.iter().cloned().zip(to.iter().cloned()).collect();
        own.expect_moves(&pairs);
        fx.move_all(from, to);
        let known = |path: &str| own.moved_from(path);
        let (mut changes, reconciled) =
            fx.run(&SigStore::default(), &known, fx.moved(from, to, false));
        let ((), pairing) = timed(|| own.pair_moves(&mut changes));
        paired = paired.min(renamed(&changes));
        reconciled + pairing
    });
    say(&format!(
        "PERF watch_moves_5000_announced renamed={paired}/{n} (fewest of the runs)"
    ));
}

/// Saves while git's merge marker stays in the repository (the real watch: events, debounce, flush).
#[test]
#[ignore = "measurement: run with --ignored in an optimized build"]
fn perf_watch_saves_during_merge() {
    let dir = tempfile::tempdir().unwrap();
    let docs = scaled(1000);
    for i in 0..docs {
        fixtures::write(
            dir.path(),
            &format!("docs/d{i}.md"),
            format!("# D{i}\n").as_bytes(),
        );
    }
    fixtures::write(dir.path(), ".git/HEAD", b"ref: refs/heads/main\n");
    fixtures::write(
        dir.path(),
        ".git/MERGE_HEAD",
        b"0123456789abcdef0123456789abcdef01234567\n",
    );
    let vault = vault_of(dir.path());
    let hub = WatchHub::new();
    let seen: Arc<std::sync::Mutex<Vec<FsBatch>>> = Arc::default();
    let _sub = hub.subscribe({
        let seen = Arc::clone(&seen);
        move |batch| seen.lock().unwrap().push(batch.clone())
    });
    let _watcher = Watcher::start(
        &vault,
        explorer_of(&vault).ignorers(),
        hub,
        Arc::new(OwnWrites::new()),
        None,
    );
    std::thread::sleep(Duration::from_millis(500));
    seen.lock().unwrap().clear();
    let saves = 10;
    let mut bulk = 0;
    report("watch_save_during_merge", saves, |run| {
        let rel = format!("docs/d{run}.md");
        let started = Instant::now();
        fixtures::write(dir.path(), &rel, format!("# D{run}\n\nsaved\n").as_bytes());
        let mut batch = None;
        wait_until("the save's batch", || {
            let mut seen = seen.lock().unwrap();
            batch = seen
                .iter()
                .position(|b| b.changes.iter().any(|c| c.path == rel))
                .map(|at| seen.remove(at));
            batch.is_some()
        });
        let took = started.elapsed();
        bulk += usize::from(batch.is_some_and(|b| b.bulk));
        took
    });
    say(&format!(
        "PERF watch_save_during_merge bulk_batches={bulk}/{saves}"
    ));
}

// ---- activity journal ----------------------------------------------------------------------------------------------

/// One large batch of outside changes noted in the journal (a checkout, an unzip).
#[test]
#[ignore = "measurement: run with --ignored in an optimized build"]
fn perf_activity_outside() {
    for count in [20_000, 50_000] {
        let n = scaled(count);
        let changes = (0..n)
            .map(|i| {
                change(
                    &format!("d{}/f{i}.md", i / 100),
                    ChangeKind::Modified,
                    false,
                )
            })
            .collect();
        let batch = FsBatch {
            changes,
            ..FsBatch::default()
        };
        report(&format!("activity_outside_{}k", count / 1000), 3, |_| {
            let dir = tempfile::tempdir().unwrap();
            let project = dir.path().join("p");
            fs::create_dir_all(&project).unwrap();
            let root = Root::open(&project).unwrap();
            let store = StateStore::for_project(&AppDirs::under(&dir.path().join("state")), &root);
            let log = ActivityLog::open(&store, system_clock());
            timed(|| log.outside(&batch)).1
        });
    }
}

// ---- file operations -----------------------------------------------------------------------------------------------

/// Checks of many-item moves and deletes, and the link plan of a move preview, over 2000 linked documents.
#[test]
#[ignore = "measurement: run with --ignored in an optimized build"]
fn perf_fileops_checks_and_plan() {
    let dir = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let docs = scaled(2000);
    let others = scaled(3000);
    let mut names = Vec::with_capacity(docs + others);
    for i in 0..docs {
        let text = format!(
            "# Doc {i}\n\nSee [next](d{}.md), [far](d{}.md) and ![picture](f{}.png).\n",
            (i + 1) % docs,
            (i * 7) % docs,
            i % others
        );
        fixtures::write(dir.path(), &format!("src/d{i}.md"), text.as_bytes());
        names.push(format!("d{i}.md"));
    }
    for i in 0..others {
        fixtures::write(dir.path(), &format!("src/f{i}.png"), b"\x89PNG\r\n\x1a\n");
        names.push(format!("f{i}.png"));
    }
    fs::create_dir_all(dir.path().join("dst")).unwrap();
    let root = Root::open(dir.path()).unwrap();
    let vault = Vault::new(root.clone());
    let store = StateStore::for_project(&AppDirs::under(state.path()), &root);
    let explorer = Arc::new(explorer_of(&vault));
    explorer.open_view("w", Some(TreeFilter::default()));
    let ops = FileOps::new(vault.clone(), explorer, Arc::new(OwnWrites::new()), &store);
    let specs = |count: usize| -> Vec<MoveSpec> {
        names[..scaled(count).min(names.len())]
            .iter()
            .map(|name| MoveSpec {
                from: format!("src/{name}"),
                to: format!("dst/{name}"),
            })
            .collect()
    };
    for count in [2000, 5000] {
        let moves = specs(count);
        report(&format!("fileops_check_moves_{count}"), 3, |_| {
            timed(|| ops.plan_move(&moves, false).unwrap()).1
        });
    }
    for count in [2000, 5000] {
        let moves = specs(count);
        report(
            &format!("fileops_plan_links_{count}_moves_2000_docs"),
            3,
            |_| timed(|| ops.plan_move(&moves, true).unwrap()).1,
        );
    }
    for count in [2000, 5000] {
        let paths: Vec<VaultPath> = names[..scaled(count).min(names.len())]
            .iter()
            .map(|name| VaultPath::parse(&format!("src/{name}")).unwrap())
            .collect();
        report(&format!("fileops_check_deletes_{count}"), 3, |_| {
            let (kept, took) = timed(|| check_deletes(&vault, &paths).unwrap());
            assert_eq!(kept.len(), paths.len());
            took
        });
    }
}

// ---- input for the real app ----------------------------------------------------------------------------------------

/// Writes the large project (+ a 5 MB document and a 1 MB document with footnotes) into `PERMARK_PERF_DIR`.
#[test]
#[ignore = "writes a large folder: run with --ignored and PERMARK_PERF_DIR set"]
fn perf_write_fixture() {
    let Some(target) = std::env::var_os("PERMARK_PERF_DIR").map(PathBuf::from) else {
        say("PERF write_fixture skipped: set PERMARK_PERF_DIR to an empty folder");
        return;
    };
    let empty = fs::read_dir(&target).map_or(true, |mut entries| entries.next().is_none());
    assert!(
        empty,
        "PERMARK_PERF_DIR must be empty or absent: {}",
        target.display()
    );
    let made = fixtures::large_folder(&target, scaled(100_000), scaled(50_000));
    fixtures::write(
        &target,
        "docs/typical-5mb.md",
        fixtures::typical_markdown(scaled(5 * MB), 2).as_bytes(),
    );
    fixtures::write(
        &target,
        "docs/footnotes-1mb.md",
        fixtures::footnoted_markdown(scaled(MB)).as_bytes(),
    );
    say(&format!(
        "PERF write_fixture dir={} documents={} media={} ignored={} (+ docs/typical-5mb.md, docs/footnotes-1mb.md)",
        target.display(),
        made.documents,
        made.media,
        made.ignored
    ));
}
