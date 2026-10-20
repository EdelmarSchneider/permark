use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use permark_core::conditions::Distribution;
use permark_core::events::{CoreEvent, EventSink};
use permark_core::paths::Root;
use permark_core::plugins::{PluginId, PluginRegistry, RpcCall};
use permark_core::settings::AppSettings;
use permark_core::state::AppDirs;
use permark_core::workspace::{CoreEnv, ProjectCore};
use serde_json::{json, Value};
use std::fmt::Write as _;

use super::*;

#[derive(Default)]
struct Sink(Mutex<Vec<(String, Value)>>);

impl EventSink for Sink {
    fn emit(&self, event: CoreEvent) {
        if let CoreEvent::Plugin { name, payload } = event {
            self.0.lock().unwrap().push((name, payload));
        }
    }
}

struct Harness {
    _base: tempfile::TempDir,
    project: tempfile::TempDir,
    sink: Arc<Sink>,
    env: CoreEnv,
    core: Option<Arc<ProjectCore>>,
}

impl Harness {
    fn new() -> Self {
        let sink = Arc::new(Sink::default());
        let base = tempfile::tempdir().unwrap();
        let env = CoreEnv::new(
            AppDirs::under(base.path()),
            Arc::new(PluginRegistry::new(vec![plugin()]).unwrap()),
            sink.clone(),
            Distribution::Direct,
            AppSettings::default(),
        );
        Self {
            _base: base,
            project: tempfile::tempdir().unwrap(),
            sink,
            env,
            core: None,
        }
    }

    fn root(&self) -> PathBuf {
        dunce::canonicalize(self.project.path()).unwrap()
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.root().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn remove(&self, rel: &str) {
        let path = self.root().join(rel);
        if path.is_dir() {
            std::fs::remove_dir_all(path).unwrap();
        } else {
            std::fs::remove_file(path).unwrap();
        }
    }

    fn open(&mut self) {
        self.core = Some(ProjectCore::open(
            Root::open(&self.root()).unwrap(),
            &self.env,
        ));
        wait_until("the first scan", || self.report()["ready"] == true);
    }

    fn core(&self) -> &Arc<ProjectCore> {
        self.core.as_ref().unwrap()
    }

    fn report(&self) -> Value {
        self.core()
            .rpc()
            .call(
                "broken-links",
                "list",
                &RpcCall {
                    window: "w".to_owned(),
                    args: Value::Null,
                },
            )
            .unwrap()
    }

    /// `doc:line:written` for every broken link, in order.
    fn broken(&self) -> Vec<String> {
        let report = self.report();
        report["docs"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|d| {
                d["links"].as_array().unwrap().iter().map(move |l| {
                    format!(
                        "{}:{}:{}",
                        d["path"].as_str().unwrap(),
                        l["line"],
                        l["written"].as_str().unwrap()
                    )
                })
            })
            .collect()
    }
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(15));
    }
}

#[test]
fn what_is_missing_is_listed_with_its_line_and_what_exists_is_not() {
    let mut h = Harness::new();
    h.write(
        "docs/a.md",
        "# A\n\n[ok](b.md) [ok2](b) [folder](sub/) [asset](pic.png)\n\n[bad](missing.md)\n\n![img](nope.png)\n\n[ref][r]\n\n[r]: gone.md\n\n[web](https://example.com/x) [mail](mailto:a@b.c) [anchor](#top) [self](a.md#top)\n\n<img src=\"html-missing.png\">\n\n[spaces](my%20file.md) [back](..\\docs\\b.md) [root](/docs/b.md) [up](../../../etc/passwd)\n",
    );
    h.write("docs/b.md", "# B\n");
    h.write("docs/pic.png", "x");
    h.write("docs/sub/README.md", "# sub\n");
    h.write("docs/my file.md", "spaces\n");
    h.open();
    let broken = h.broken();
    assert_eq!(
        broken,
        [
            "docs/a.md:5:missing.md",
            "docs/a.md:7:nope.png",
            "docs/a.md:11:gone.md",
            "docs/a.md:15:html-missing.png",
            // a link that climbs out of the project leads nowhere inside it
            "docs/a.md:17:../../../etc/passwd",
        ],
        "{broken:?}"
    );
    let report = h.report();
    assert_eq!(report["total"], 5);
    assert_eq!(report["truncated"], false);
}

#[test]
fn the_list_follows_the_project_without_a_new_scan() {
    let mut h = Harness::new();
    h.write(
        "a.md",
        "# A\n\n[to b](b.md)\n\n[to dir](docs/)\n\n[no ext](notes/idea)\n",
    );
    h.open();
    assert_eq!(
        h.broken(),
        ["a.md:3:b.md", "a.md:5:docs/", "a.md:7:notes/idea"]
    );
    // the target appears
    h.write("b.md", "# B\n");
    wait_until("b appears", || {
        h.broken() == ["a.md:5:docs/", "a.md:7:notes/idea"]
    });
    // a link without the extension is fixed by the file with it; a folder link by its readme
    h.write("notes/idea.md", "idea\n");
    wait_until("idea appears", || h.broken() == ["a.md:5:docs/"]);
    h.write("docs/README.md", "# docs\n");
    wait_until("readme appears", || h.broken().is_empty());
    // and disappears
    h.remove("b.md");
    wait_until("b goes", || h.broken() == ["a.md:3:b.md"]);
    h.remove("docs");
    wait_until("docs goes", || {
        h.broken() == ["a.md:3:b.md", "a.md:5:docs/"]
    });
    // the document itself changes
    h.write("a.md", "# A\n\n[only one](still-missing.md)\n");
    wait_until("a edited", || h.broken() == ["a.md:3:still-missing.md"]);
    // the document is removed: its links go with it
    h.remove("a.md");
    wait_until("a removed", || h.broken().is_empty());
    assert!(h
        .sink
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|(n, _)| n == "broken-links:changed"));
}

#[test]
fn moving_a_target_or_a_whole_folder_is_followed() {
    let mut h = Harness::new();
    h.write("a.md", "[x](lib/x.md) [y](lib/deep/y.md)\n");
    h.write("lib/x.md", "x\n");
    h.write("lib/deep/y.md", "y\n");
    h.open();
    assert!(h.broken().is_empty());
    let ops = h.core().fileops();
    let plan = ops
        .plan_move(
            &[permark_core::fileops::MoveSpec {
                from: "lib".to_owned(),
                to: "vendor".to_owned(),
            }],
            false,
        )
        .unwrap();
    ops.apply_move(plan.plan_id, false).unwrap();
    let mut last = Vec::new();
    let started = Instant::now();
    while last != ["a.md:1:lib/x.md", "a.md:1:lib/deep/y.md"] {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "both are broken now: {last:?}"
        );
        std::thread::sleep(Duration::from_millis(15));
        last = h.broken();
    }
    // moved documents are checked at their new place
    h.write("vendor/z.md", "[up](../missing-up.md)\n");
    wait_until("z checked", || h.broken().len() == 3);
    let plan = ops
        .plan_move(
            &[permark_core::fileops::MoveSpec {
                from: "vendor".to_owned(),
                to: "third".to_owned(),
            }],
            false,
        )
        .unwrap();
    ops.apply_move(plan.plan_id, false).unwrap();
    wait_until("z follows its folder", || {
        h.broken().iter().any(|b| b.starts_with("third/z.md:1:"))
            && !h.broken().iter().any(|b| b.starts_with("vendor/"))
    });
}

#[test]
fn big_lists_are_cut_but_counted_and_odd_documents_are_survived() {
    let mut h = Harness::new();
    let mut many = String::new();
    for n in 0..700 {
        let _ = writeln!(many, "[l{n}](gone/{n}.md)\n");
    }
    h.write("many.md", &many);
    h.write(
        "nul.md",
        "[a](x\u{0}y.md) \u{202e} [b](<spaced name.md>) \r\n\r\n[c](%ZZ.md)\n",
    );
    h.write(
        "binary.md",
        "text then \u{0}\u{0}\u{0} binary [d](zzz.md)\n",
    );
    h.write("nested.md", &"> ".repeat(8000));
    h.write("deep/er/und/er/still.md", "[e](nowhere.md)\n");
    h.open();
    let report = h.report();
    assert_eq!(report["truncated"], true);
    let listed: usize = report["docs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["links"].as_array().unwrap().len())
        .sum();
    assert_eq!(listed, MAX_LISTED);
    assert!(report["total"].as_u64().unwrap() >= 700);
    assert!(
        report["skipped"].as_u64().unwrap() >= 1,
        "the binary document was left out"
    );
}

#[test]
fn a_destination_that_is_very_long_is_shortened_not_refused() {
    let mut h = Harness::new();
    h.write(
        "a.md",
        &format!("[x]({}.md)\n", "very-long-name-".repeat(100)),
    );
    h.open();
    let written = h.broken()[0].clone();
    assert!(written.chars().count() < 260, "{}", written.len());
    assert!(written.ends_with('…'));
}

#[test]
fn off_leaves_nothing_behind() {
    let mut h = Harness::new();
    h.write("a.md", "[x](gone.md)\n");
    h.open();
    assert_eq!(h.broken().len(), 1);
    let core = h.core().clone();
    h.env.set_settings(AppSettings {
        plugins: BTreeMap::from([(PluginId::of("broken-links"), false)]),
        ..AppSettings::default()
    });
    core.apply_plugins(&h.env);
    assert_eq!(core.rpc().served(), Vec::<String>::new());
    assert!(
        !core
            .store()
            .dir()
            .join("plugins")
            .join("broken-links")
            .exists(),
        "no file of its own"
    );
    h.env.set_settings(AppSettings::default());
    core.apply_plugins(&h.env);
    wait_until("on again", || {
        core.rpc()
            .call(
                "broken-links",
                "list",
                &RpcCall {
                    window: "w".to_owned(),
                    args: json!({}),
                },
            )
            .is_ok_and(|r| r["ready"] == true && r["total"] == 1)
    });
    drop(core);
}

#[test]
fn closing_the_project_during_the_first_scan_is_prompt() {
    let h = Harness::new();
    for n in 0..1500 {
        h.write(&format!("d{}/f{n}.md", n % 20), "[x](gone.md) text\n");
    }
    let started = Instant::now();
    let core = ProjectCore::open(Root::open(&h.root()).unwrap(), &h.env);
    drop(core);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "joined in {:?}",
        started.elapsed()
    );
}
