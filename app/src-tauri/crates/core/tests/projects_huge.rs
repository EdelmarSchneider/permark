//! Spec `projects.md` §13 round 2 (huge root): 2000 found projects + a record of 2000 entries. Times the record parse
//! and the map build after the walk; measures the heap the projects service holds (own test binary: a counting
//! allocator sees only this test). Numbers printed (`--nocapture`); bounds are loose (debug builds, CI machines).

use std::alloc::{GlobalAlloc, Layout, System};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use permark_core::conditions::Distribution;
use permark_core::paths::Root;
use permark_core::plugins::PluginRegistry;
use permark_core::projects::{record, MemberState, RecordRead};
use permark_core::settings::AppSettings;
use permark_core::state::AppDirs;
use permark_core::workspace::{CoreEnv, ProjectCore};

struct Counting;

static LIVE: AtomicIsize = AtomicIsize::new(0);

// test-only heap counter: a global allocator is an unsafe trait by definition
// SAFETY: forwards every call to the system allocator unchanged; only counts the bytes.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size().cast_signed(), Ordering::Relaxed);
        // SAFETY: same layout contract as the caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size().cast_signed(), Ordering::Relaxed);
        // SAFETY: `ptr` came from `alloc` with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(
            new_size.cast_signed() - layout.size().cast_signed(),
            Ordering::Relaxed,
        );
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

const N: usize = 2000;

struct Opened {
    core: Arc<ProjectCore>,
    /// Open -> walk ended.
    walk_ms: f64,
    /// Walk ended -> map ready (discovery + presence + map build).
    map_ms: f64,
    /// Heap the open core holds once idle.
    heap: isize,
}

fn open(env: &CoreEnv, root: Root) -> Opened {
    let before = LIVE.load(Ordering::Relaxed);
    let start = Instant::now();
    let core = ProjectCore::open(root, env);
    // blocks until the walk ended (never walks twice)
    let _ = core.explorer().markers();
    let walked = Instant::now();
    while !core.projects().snapshot().ready {
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "map never ready"
        );
        std::thread::sleep(Duration::from_micros(200));
    }
    let map_ms = walked.elapsed().as_secs_f64() * 1000.0;
    let walk_ms = (walked - start).as_secs_f64() * 1000.0;
    // the index may still build: let it settle so the heap number is the idle one
    std::thread::sleep(Duration::from_millis(1500));
    Opened {
        core,
        walk_ms,
        map_ms,
        heap: LIVE.load(Ordering::Relaxed) - before,
    }
}

#[test]
fn a_root_with_2000_projects_and_2000_entries() {
    let base = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut text = String::from("# many\nversion = 1\n");
    for i in 0..N {
        let rel = format!("p{:02}/repo-{i}", i % 50);
        std::fs::create_dir_all(root.join(&rel).join(".git")).unwrap();
        std::fs::write(root.join(&rel).join("README.md"), "# r\n").unwrap();
        let _ = write!(text, "\n[[project]]\npath = \"{rel}\"\n");
    }
    // parse alone
    let start = Instant::now();
    let read = record::parse(text.as_bytes());
    let parse_ms = start.elapsed().as_secs_f64() * 1000.0;
    let RecordRead::Read(parsed) = read else {
        panic!("record not read");
    };
    assert_eq!(parsed.entries.len(), N);
    let env = CoreEnv::new(
        AppDirs::under(base.path()),
        Arc::new(PluginRegistry::new(Vec::new()).unwrap()),
        Arc::new(permark_core::events::NoEvents),
        Distribution::Direct,
        AppSettings::default(),
    );
    // all found, none listed (2000 offered)
    let offered = open(&env, Root::open(root).unwrap());
    assert_eq!(
        offered.core.projects_view().offered.count,
        u32::try_from(N).unwrap()
    );
    let (offered_walk, offered_map, offered_heap) = (offered.walk_ms, offered.map_ms, offered.heap);
    drop(offered);
    // listed: 2000 included members
    std::fs::create_dir_all(root.join(".permark")).unwrap();
    std::fs::write(root.join(".permark/projects.toml"), &text).unwrap();
    let listed = open(&env, Root::open(root).unwrap());
    let view = listed.core.projects_view();
    assert_eq!(
        view.rows
            .iter()
            .filter(|r| r.state == MemberState::Included)
            .count(),
        N
    );
    let n = isize::try_from(N).unwrap();
    println!(
        "huge root ({N} projects): record parse {parse_ms:.1} ms ({} KiB); offered: walk {offered_walk:.0} ms, map after walk {offered_map:.1} ms, core heap {} KiB; listed: walk {:.0} ms, map after walk {:.1} ms, core heap {} KiB ({} B per included member over offered)",
        text.len() / 1024,
        offered_heap / 1024,
        listed.walk_ms,
        listed.map_ms,
        listed.heap / 1024,
        (listed.heap - offered_heap) / n,
    );
    assert!(parse_ms < 2000.0, "parse {parse_ms} ms");
    assert!(listed.map_ms < 30_000.0, "map {} ms", listed.map_ms);
}
