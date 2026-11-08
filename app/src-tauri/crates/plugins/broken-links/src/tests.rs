use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use permark_core::conditions::Distribution;
use permark_core::events::{CoreEvent, EventSink};
use permark_core::paths::{key_of, Root, CASE_INSENSITIVE};
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
        Self::with_plugins(vec![plugin()])
    }

    /// With a plugin that turns the prompt features on, as the prompt blocks plugin does.
    fn with_prompts() -> Self {
        use permark_core::format::{
            Contributions, MarkdownFeature, PromptBlocks, DEFAULT_PROMPT_TAGS,
        };
        use permark_core::plugins::StaticPlugin;
        let prompts = StaticPlugin::app(
            "prompt-blocks",
            PluginKind::Embed,
            Contributions::default().markdown(vec![MarkdownFeature::PromptBlocks(PromptBlocks {
                tags: DEFAULT_PROMPT_TAGS,
            })]),
        );
        Self::with_plugins(vec![plugin(), prompts])
    }

    fn with_plugins(plugins: Vec<Arc<dyn Plugin>>) -> Self {
        let sink = Arc::new(Sink::default());
        let base = tempfile::tempdir().unwrap();
        let env = CoreEnv::new(
            AppDirs::under(base.path()),
            Arc::new(PluginRegistry::new(plugins).unwrap()),
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

    fn repair(&self, path: &str, line: u32, href: &str, target: &str) -> Value {
        self.core()
            .rpc()
            .call(
                "broken-links",
                "repair",
                &RpcCall {
                    window: "w".to_owned(),
                    args: json!({
                        "path": path,
                        "line": line,
                        "href": href,
                        "target": target,
                    }),
                },
            )
            .unwrap()
    }

    fn bytes(&self, rel: &str) -> Vec<u8> {
        std::fs::read(self.root().join(rel)).unwrap()
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
fn a_listed_row_names_the_same_root_file_and_keeps_the_full_href() {
    let mut h = Harness::new();
    let long = format!("{}gone.md", "sub/".repeat(80));
    h.write("docs/a.md", &format!("[one](missing.md)\n[two]({long})\n"));
    h.write("notes/missing.md", "# here\n");
    h.write("@wt/other/missing.md", "# elsewhere\n");
    h.open();
    let report = h.report();
    let docs = report["docs"].as_array().unwrap();
    assert_eq!(docs.len(), 1, "{report}");
    assert_eq!(docs[0]["path"], "docs/a.md");
    let links = docs[0]["links"].as_array().unwrap();
    assert_eq!(links[0]["line"], 1);
    assert_eq!(links[0]["written"], "missing.md");
    assert_eq!(links[0]["href"], "missing.md");
    assert_eq!(
        links[0]["candidates"],
        json!(["notes/missing.md"]),
        "the other root is known to the tree and is not a candidate"
    );
    assert_eq!(links[1]["href"], long);
    assert_ne!(
        links[1]["written"], long,
        "a shortened label is not the href"
    );
    assert!(links[1]["written"].as_str().unwrap().ends_with('…'));
    assert!(links[1]["candidates"].as_array().unwrap().is_empty());
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
    // what the watch reports, shown when the wait below fails
    let heard: Arc<Mutex<Vec<String>>> = Arc::default();
    let _listening = h.core().watcher().hub().subscribe({
        let heard = Arc::clone(&heard);
        move |batch| heard.lock().unwrap().push(format!("{batch:?}"))
    });
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
            "both are broken now: {last:?}; the watch said {:?}",
            heard.lock().unwrap()
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
fn links_inside_prompt_blocks_are_checked_and_repaired_while_the_prompt_features_are_on() {
    // no blank line after the tag: without the prompt features the block is raw HTML and holds no link
    const NOTE: &str = "<context>\r\n- [plan](old/plan.md) `[code](old/plan.md)`\r\n</context>\r\n";
    let mut h = Harness::with_prompts();
    h.write("a/note.md", NOTE);
    h.write("b/plan.md", "# plan\n");
    h.open();
    assert_eq!(h.broken(), ["a/note.md:2:old/plan.md"]);
    assert_eq!(
        h.repair("a/note.md", 2, "old/plan.md", "b/plan.md"),
        json!({"outcome": "wrote", "href": "../b/plan.md"})
    );
    assert_eq!(
        h.bytes("a/note.md"),
        NOTE.replacen("(old/plan.md)", "(../b/plan.md)", 1)
            .as_bytes(),
        "the code span, the tags and CRLF stay"
    );

    let mut off = Harness::new();
    off.write("a/note.md", NOTE);
    off.open();
    assert_eq!(off.broken(), Vec::<String>::new());
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

/// An opened folder and linked worktrees beside it (siblings, as `git worktree add` makes them).
struct Roots {
    _dirs: Vec<tempfile::TempDir>,
    main: PathBuf,
    worktrees: Vec<(String, PathBuf)>,
}

impl Roots {
    fn new(worktrees: &[&str]) -> Self {
        let mut dirs = vec![tempfile::tempdir().unwrap()];
        let main = dunce::canonicalize(dirs[0].path()).unwrap();
        let mut found = Vec::new();
        for name in worktrees {
            let dir = tempfile::tempdir().unwrap();
            found.push(((*name).to_owned(), dunce::canonicalize(dir.path()).unwrap()));
            dirs.push(dir);
        }
        Self {
            _dirs: dirs,
            main,
            worktrees: found,
        }
    }

    fn vault(&self) -> Vault {
        Vault::new(Root::open(&self.main).unwrap()).with_worktrees(self.worktrees.clone())
    }

    /// Writes a document (`@wt/<name>/...` lands in that worktree) and has the checker read it.
    fn add(&self, checker: &mut Checker, path: &str, text: &str) {
        let abs = match path.strip_prefix(WORKTREE_PREFIX) {
            Some(rest) => {
                let (name, rel) = rest.split_once('/').unwrap();
                let (_, root) = self.worktrees.iter().find(|(n, _)| n == name).unwrap();
                root.join(rel)
            }
            None => self.main.join(path),
        };
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(&abs, text).unwrap();
        checker.read(&self.vault(), path, &abs, &MarkdownFeatures::default());
    }
}

fn root(prefix: &str, total: u32) -> BrokenRoot {
    BrokenRoot {
        prefix: prefix.to_owned(),
        total,
    }
}

fn no_files(_: &HashSet<String>) -> HashMap<String, Vec<String>> {
    HashMap::new()
}

#[test]
fn the_opened_folder_is_counted_apart_from_its_linked_worktrees() {
    // issue #62: the panel showed opened root + every worktree as the folder's count
    let roots = Roots::new(&["feature", "clean"]);
    let mut checker = Checker::new();
    roots.add(&mut checker, "docs/a.md", "[x](gone.md)\n");
    roots.add(
        &mut checker,
        "@wt/feature/docs/a.md",
        "[x](gone.md)\n\n[y](also-gone.md)\n",
    );
    roots.add(&mut checker, "@wt/feature/docs/b.md", "[z](nope.md)\n");
    let vault = roots.vault();
    let report = report_of(&checker, true, &vault, &no_files);
    assert_eq!(report.current, 1, "only the opened folder");
    assert_eq!(report.total, 4, "every root, as before");
    assert_eq!(
        report.roots,
        [root("", 1), root("@wt/feature", 3), root("@wt/clean", 0)]
    );
    let paths: Vec<&str> = report.docs.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "docs/a.md",
            "@wt/feature/docs/a.md",
            "@wt/feature/docs/b.md"
        ],
        "the opened folder first"
    );
    assert!(!report.truncated);
    let summary = summary_of(&checker, true);
    assert_eq!((summary.current, summary.total), (1, 4));
}

#[test]
fn a_folder_without_worktrees_reports_one_root_and_nothing_before_the_first_scan_is_hidden() {
    let roots = Roots::new(&[]);
    let mut checker = Checker::new();
    assert_eq!(
        report_of(&checker, false, &roots.vault(), &no_files).roots,
        [root("", 0)]
    );
    roots.add(&mut checker, "a.md", "[x](gone.md)\n");
    let report = report_of(&checker, false, &roots.vault(), &no_files);
    assert!(!report.ready);
    assert_eq!((report.current, report.total), (1, 1));
    assert_eq!(report.roots, [root("", 1)]);
}

#[test]
fn the_cap_keeps_the_opened_folders_links_before_a_worktrees() {
    let roots = Roots::new(&["big"]);
    let mut checker = Checker::new();
    let mut many = String::new();
    for n in 0..(MAX_LISTED + 100) {
        let _ = writeln!(many, "[l{n}](gone/{n}.md)\n");
    }
    // `@` sorts before letters: by path alone the worktree would fill the list
    roots.add(&mut checker, "@wt/big/aaa.md", &many);
    roots.add(&mut checker, "zzz.md", "[a](gone-a.md) [b](gone-b.md)\n");
    let report = report_of(&checker, true, &roots.vault(), &no_files);
    assert_eq!(report.docs[0].path, "zzz.md");
    assert_eq!(report.docs[0].links.len(), 2);
    let listed: usize = report.docs.iter().map(|d| d.links.len()).sum();
    assert_eq!(listed, MAX_LISTED);
    assert!(report.truncated);
    assert_eq!(report.current, 2);
    assert_eq!(report.roots, [root("", 2), root("@wt/big", 600)]);
}

/// `key_of(file name)` -> project paths, the shape `files_keyed` returns.
fn by_name(pairs: &[(&str, &[&str])]) -> HashMap<String, Vec<String>> {
    pairs
        .iter()
        .map(|(name, paths)| {
            (
                key_of(name),
                paths.iter().copied().map(str::to_owned).collect(),
            )
        })
        .collect()
}

#[test]
#[allow(clippy::too_many_lines)] // one story: one, several, zero, other root, .md only when omitted
fn candidates_are_same_root_files_of_that_name_and_md_only_when_it_was_left_out() {
    assert_eq!(
        candidates(
            "a/note.md",
            "old/plan.md",
            &by_name(&[("plan.md", &["notes/plan.md", "@wt/other/plan.md"])])
        ),
        ["notes/plan.md"],
        "one same-root match; the other root is not a candidate"
    );
    assert_eq!(
        candidates(
            "a/note.md",
            "old/plan.md",
            &by_name(&[(
                "plan.md",
                &["z/plan.md", "m/plan.md", "@wt/other/x/plan.md"]
            )])
        ),
        ["m/plan.md", "z/plan.md"],
        "several, in path order"
    );
    assert!(
        candidates(
            "a/note.md",
            "old/plan.md",
            &by_name(&[("other.md", &["notes/other.md"])])
        )
        .is_empty(),
        "zero"
    );
    assert!(
        candidates(
            "a/note.md",
            "old/plan.md",
            &by_name(&[("plan.md", &["@wt/other/plan.md"])])
        )
        .is_empty(),
        "the only match is another root"
    );
    assert_eq!(
        candidates(
            "@wt/feature/a/note.md",
            "old/plan.md",
            &by_name(&[(
                "plan.md",
                &[
                    "notes/plan.md",
                    "@wt/feature/c/plan.md",
                    "@wt/other/plan.md"
                ]
            )])
        ),
        ["@wt/feature/c/plan.md"],
        "a worktree matches only itself"
    );
    assert_eq!(
        candidates(
            "a/note.md",
            "old/plan",
            &by_name(&[
                ("plan", &["notes/plan"]),
                ("plan.md", &["notes/plan.md", "c/plan.md"]),
                ("plan.md.md", &["notes/plan.md.md"]),
            ])
        ),
        ["c/plan.md", "notes/plan", "notes/plan.md"],
        "a basename without .md also asks for that name plus .md"
    );
    assert_eq!(
        candidates(
            "a/note.md",
            "old/plan.md",
            &by_name(&[
                ("plan", &["notes/plan"]),
                ("plan.md", &["notes/plan.md"]),
                ("plan.md.md", &["notes/plan.md.md"]),
            ])
        ),
        ["notes/plan.md"],
        ".md is not added when the basename already ends in it"
    );
    assert_eq!(
        candidates(
            "a/note.md",
            "pics/shot.png",
            &by_name(&[
                ("shot.png", &["img/shot.png"]),
                ("shot.png.md", &["img/shot.png.md"]),
                ("shot.md", &["img/shot.md"]),
            ])
        ),
        ["img/shot.png", "img/shot.png.md"],
        "another extension is not stripped; .md is added only as a suffix"
    );
    assert_eq!(
        candidates(
            "a/note.md",
            "old/docs/",
            &by_name(&[
                ("README.md", &["docs/README.md"]),
                ("index.md", &["docs/index.md"]),
                ("docs.md", &["ref/docs.md"]),
            ])
        ),
        ["ref/docs.md"],
        "a folder link is not another folder's readme"
    );
    assert!(
        candidates("a/note.md", "#top", &HashMap::new()).is_empty(),
        "a pure anchor names no file"
    );
    assert!(
        candidates(
            "a/note.md",
            "https://example.com/plan.md",
            &by_name(&[("plan.md", &["notes/plan.md"])])
        )
        .is_empty(),
        "a scheme names no file"
    );
    assert_eq!(
        candidates(
            "a/note.md",
            "old/plan.md?q=1#section",
            &by_name(&[("plan.md", &["notes/plan.md"])])
        ),
        ["notes/plan.md"],
        "query and anchor do not change the name"
    );
}

#[test]
#[allow(clippy::too_many_lines)] // one story: write, suffix, changed bytes, unsaved, other root
fn repair_writes_the_one_destination_and_skips_changed_unsaved_and_other_root() {
    const HREF: &str = "old/plan.md?q=1#section";
    let mut h = Harness::new();
    let note = format!("[one]({HREF})\r\n[two]({HREF})\r\n");
    let plain = format!("[x]({HREF})\n");
    h.write("a/note.md", &note);
    h.write("b/plan.md", "# plan\n");
    h.write("c/note.md", &plain);
    h.write("d/note.md", &plain);
    h.write("e/note.md", &plain);
    h.write("g/note.md", &plain);
    h.write("same.md", "[a](old/plan.md) [b](old/plan.md)\n");
    // a path `root_of` treats as another root; the opened folder's repair must not take it
    h.write("@wt/other/plan.md", "# other\n");
    let bad = b"[x](old/plan.md?q=1#section)\n\xff";
    std::fs::write(h.root().join("bad.md"), bad).unwrap();
    h.open();
    assert_eq!(
        h.broken(),
        [
            "a/note.md:1:old/plan.md?q=1#section",
            "a/note.md:2:old/plan.md?q=1#section",
            "bad.md:1:old/plan.md?q=1#section",
            "c/note.md:1:old/plan.md?q=1#section",
            "d/note.md:1:old/plan.md?q=1#section",
            "e/note.md:1:old/plan.md?q=1#section",
            "g/note.md:1:old/plan.md?q=1#section",
            "same.md:1:old/plan.md",
            "same.md:1:old/plan.md",
        ],
        "list behavior before a repair"
    );

    let wrote = h.repair("a/note.md", 1, HREF, "b/plan.md");
    assert_eq!(
        wrote,
        json!({"outcome": "wrote", "href": "../b/plan.md?q=1#section"})
    );
    let expected = format!("[one](../b/plan.md?q=1#section)\r\n[two]({HREF})\r\n");
    assert_eq!(
        h.bytes("a/note.md"),
        expected.as_bytes(),
        "one destination, suffix kept, the other copy and CRLF stay"
    );

    let edited = b"edited while the plan was open\n";
    std::fs::write(h.root().join("c/note.md"), edited).unwrap();
    assert_eq!(
        h.repair("c/note.md", 1, HREF, "b/plan.md"),
        json!({"outcome": "refused", "reason": "mismatch"}),
        "the href is gone: the file changed under the plan"
    );
    assert_eq!(h.bytes("c/note.md"), edited);

    let changed = h.root().join("g/note.md");
    before_second_read({
        let changed = changed.clone();
        move || {
            std::fs::write(&changed, b"changed by someone else\n").unwrap();
        }
    });
    assert_eq!(
        h.repair("g/note.md", 1, HREF, "b/plan.md"),
        json!({"outcome": "refused", "reason": "changed"}),
        "bytes changed after they were read"
    );
    assert_eq!(h.bytes("g/note.md"), b"changed by someone else\n");

    h.core()
        .documents()
        .set_unsaved("w", Some("d/note.md".to_owned()));
    assert_eq!(
        h.repair("d/note.md", 1, HREF, "b/plan.md"),
        json!({"outcome": "refused", "reason": "unsaved"})
    );
    assert_eq!(h.bytes("d/note.md"), plain.as_bytes());

    let names: HashSet<String> = ["plan.md".to_owned()].into();
    let known = h.core().explorer().files_keyed(&names);
    let mut plans = known.get(&key_of("plan.md")).cloned().unwrap_or_default();
    plans.sort();
    assert!(
        plans.iter().any(|path| path == "@wt/other/plan.md")
            && plans.iter().any(|path| path == "b/plan.md"),
        "the other root is known, and so is the same-root file: {plans:?}"
    );
    assert_eq!(
        h.repair("e/note.md", 1, HREF, "@wt/other/plan.md"),
        json!({"outcome": "refused", "reason": "notCandidate"}),
        "a target in another root is not a candidate"
    );
    assert_eq!(h.bytes("e/note.md"), plain.as_bytes());
    assert_eq!(h.bytes("@wt/other/plan.md"), b"# other\n");

    let same = h.bytes("same.md");
    assert_eq!(
        h.repair("same.md", 1, "old/plan.md", "b/plan.md"),
        json!({"outcome": "refused", "reason": "mismatch"}),
        "two copies on the line: neither is rewritten"
    );
    assert_eq!(h.bytes("same.md"), same);

    assert_eq!(
        h.repair("bad.md", 1, HREF, "b/plan.md"),
        json!({"outcome": "refused", "reason": "notUtf8"})
    );
    assert_eq!(h.bytes("bad.md"), bad);

    wait_until("the checker follows the write", || {
        h.broken()
            == [
                "a/note.md:2:old/plan.md?q=1#section",
                "bad.md:1:old/plan.md?q=1#section",
                "d/note.md:1:old/plan.md?q=1#section",
                "e/note.md:1:old/plan.md?q=1#section",
                "same.md:1:old/plan.md",
                "same.md:1:old/plan.md",
            ]
    });
}

fn line_of(text: &str, marker: &str) -> u32 {
    let at = text
        .find(marker)
        .unwrap_or_else(|| panic!("marker not in the note: {marker}"));
    u32::try_from(text[..at].bytes().filter(|b| *b == b'\n').count() + 1).unwrap()
}

fn candidates_at(report: &Value, path: &str, line: u32, href: &str) -> Vec<String> {
    let doc = report["docs"]
        .as_array()
        .and_then(|docs| docs.iter().find(|doc| doc["path"] == path))
        .unwrap_or_else(|| panic!("no document {path}"));
    let row = doc["links"]
        .as_array()
        .and_then(|links| {
            links
                .iter()
                .find(|row| row["line"] == line && row["href"] == href)
        })
        .unwrap_or_else(|| panic!("no {href} on line {line} of {path}: {doc}"));
    let mut paths = row["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

#[test]
#[allow(clippy::too_many_lines)] // one note: the near misses refuse, then each chosen span is the only bytes that move
fn adversarial_repair_rejects_near_misses_and_rewrites_only_the_chosen_span() {
    let note = "\u{feff}---\n\
link: old/front.md\n\
---\n\
\n\
pré [bom](old/plan.md#sec?x=1) pós\n\
\n\
[short](old/plan) [long](old/plan.md)\n\
\n\
[title](old/plan.md \"keep me\")\n\
\n\
[cased](old/Plan.MD)\n\
\n\
[angle](<old/My Notes (1).md>)\n\
\n\
[pct](old/my%20plan.md)\n\
\n\
[png](pics/shot.png)\n\
\n\
![pic](pics/shot.png)\n\
\n\
[miss](gone/missing)\n\
\n\
[near](old/planx)\n\
\n\
[mark](old/plan.markdown)\n\
\n\
[dot](./plan.md)\n\
\n\
[win](sub\\plan.md)\n\
\n\
[root](/old/plan.md)\n\
\n\
<a href=\"old/plan.md\">html</a>\n\
\n\
[ref]: old/plan.md\n\
\n\
[stay](old/plan.md)\n\
\n\
`[code](old/inlined.md)`\n\
\n\
```\n\
[fence](old/fenced.md)\n\
```\n\
\n\
$[math](old/math.md)$\n\
\n\
[web](https://example.com/plan.md) [mail](mailto:a@b.md) [hash](#plan.md)\n\
"
    .replace('\n', "\r\n");
    let mut h = Harness::new();
    h.write(".gitignore", "hidden/\nsecret/plan.md\n");
    for (path, text) in [
        ("notes/Plan.md", "# plan\n"),
        ("other/plan.md", "# other\n"),
        ("hidden/plan.md", "# hidden\n"),
        ("secret/plan.md", "# secret\n"),
        ("notes/plan.md.md", "# double\n"),
        ("notes/planning.md", "# fuzzy\n"),
        ("notes/plan.md.txt", "nope\n"),
        ("notes/xplan.md", "# x\n"),
        ("notes/plan.markdown", "# markdown\n"),
        ("img/shot.png", "png\n"),
        ("img/shot.md", "# not the png\n"),
        ("img/shot.png.md", "# suffix\n"),
        ("other/shot.PNG", "PNG\n"),
        ("docs/missing/README.md", "# readme\n"),
        ("docs/missing/index.md", "# index\n"),
        ("b/my plan.md", "# space\n"),
        ("decoy/my%20plan.md", "# encoded name\n"),
        ("weird/My Notes (1).md", "# weird\n"),
        ("real/fenced.md", "# fence\n"),
        ("real/inlined.md", "# inline\n"),
        ("real/math.md", "# math\n"),
        ("real/front.md", "# front\n"),
        ("a/note.md", note.as_str()),
    ] {
        h.write(path, text);
    }
    std::fs::create_dir_all(h.root().join("folder").join("plan.md")).unwrap();
    h.open();
    let report = h.report();
    // `key_of` folds case only where the OS ignores it. `notes/Plan.md` is a hit on Windows and macOS.
    let plans = if CASE_INSENSITIVE {
        vec!["notes/Plan.md".to_owned(), "other/plan.md".to_owned()]
    } else {
        vec!["other/plan.md".to_owned()]
    };
    for (marker, href) in [
        ("[bom]", "old/plan.md#sec?x=1"),
        ("[short]", "old/plan"),
        ("[long]", "old/plan.md"),
        ("[title]", "old/plan.md"),
        ("[dot]", "./plan.md"),
        ("[win]", "sub\\plan.md"),
        ("[root]", "/old/plan.md"),
        ("<a href", "old/plan.md"),
        ("[ref]", "old/plan.md"),
        ("[stay]", "old/plan.md"),
    ] {
        assert_eq!(
            candidates_at(&report, "a/note.md", line_of(&note, marker), href),
            plans,
            "{marker} must not take a near miss, an ignored file, or a directory"
        );
    }
    let cased = if CASE_INSENSITIVE {
        plans.clone()
    } else {
        Vec::new()
    };
    assert_eq!(
        candidates_at(
            &report,
            "a/note.md",
            line_of(&note, "[cased]"),
            "old/Plan.MD"
        ),
        cased,
        "Plan.MD matches plan.md only where the OS ignores case"
    );
    let mut shots = vec!["img/shot.png".to_owned(), "img/shot.png.md".to_owned()];
    if CASE_INSENSITIVE {
        shots.push("other/shot.PNG".to_owned());
    }
    shots.sort();
    assert_eq!(
        candidates_at(
            &report,
            "a/note.md",
            line_of(&note, "[png]"),
            "pics/shot.png"
        ),
        shots,
        "shot.png does not match shot.md; the other case counts only where the OS ignores case"
    );
    assert_eq!(
        candidates_at(
            &report,
            "a/note.md",
            line_of(&note, "![pic]"),
            "pics/shot.png"
        ),
        shots
    );
    assert_eq!(
        candidates_at(
            &report,
            "a/note.md",
            line_of(&note, "[angle]"),
            "old/My Notes (1).md"
        ),
        ["weird/My Notes (1).md"]
    );
    assert_eq!(
        candidates_at(
            &report,
            "a/note.md",
            line_of(&note, "[pct]"),
            "old/my%20plan.md"
        ),
        ["b/my plan.md"],
        "the encoded href names the decoded file, not a file whose name is the escape"
    );
    assert_eq!(
        candidates_at(
            &report,
            "a/note.md",
            line_of(&note, "[mark]"),
            "old/plan.markdown"
        ),
        ["notes/plan.markdown"],
        "a different extension is not a stem match"
    );
    assert!(
        candidates_at(
            &report,
            "a/note.md",
            line_of(&note, "[miss]"),
            "gone/missing"
        )
        .is_empty(),
        "a folder's README or index is not a candidate"
    );
    assert!(
        candidates_at(&report, "a/note.md", line_of(&note, "[near]"), "old/planx").is_empty(),
        "planx is not plan"
    );
    for href in [
        "old/front.md",
        "old/inlined.md",
        "old/fenced.md",
        "old/math.md",
        "https://example.com/plan.md",
        "mailto:a@b.md",
        "#plan.md",
    ] {
        assert!(
            !report["docs"].as_array().unwrap().iter().any(|doc| {
                doc["links"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|link| link["href"] == href)
            }),
            "{href} is not a broken link of this note"
        );
    }

    let original = h.bytes("a/note.md");
    assert_eq!(original, note.as_bytes());
    let refuse = |line: u32, href: &str, target: &str, reason: &str| {
        assert_eq!(
            h.repair("a/note.md", line, href, target),
            json!({"outcome": "refused", "reason": reason}),
            "line {line} {href} -> {target}"
        );
        assert_eq!(h.bytes("a/note.md"), original, "a refusal wrote");
    };
    let plan_line = line_of(&note, "[bom]");
    for target in [
        "hidden/plan.md",
        "secret/plan.md",
        "folder/plan.md",
        "notes/plan.md.md",
        "notes/planning.md",
        "notes/xplan.md",
        "notes/plan.md.txt",
        "docs/missing/README.md",
        "docs/missing/index.md",
        "decoy/my%20plan.md",
        "../notes/Plan.md",
        "",
    ] {
        refuse(plan_line, "old/plan.md#sec?x=1", target, "notCandidate");
    }
    refuse(
        line_of(&note, "[png]"),
        "pics/shot.png",
        "img/shot.md",
        "notCandidate",
    );
    refuse(
        line_of(&note, "[miss]"),
        "gone/missing",
        "docs/missing/README.md",
        "notCandidate",
    );
    refuse(0, "old/plan.md#sec?x=1", "notes/Plan.md", "mismatch");
    refuse(999, "old/plan.md#sec?x=1", "notes/Plan.md", "mismatch");
    refuse(
        line_of(&note, "[fence]"),
        "old/fenced.md",
        "real/fenced.md",
        "mismatch",
    );
    refuse(
        line_of(&note, "[code]"),
        "old/inlined.md",
        "real/inlined.md",
        "mismatch",
    );
    refuse(
        line_of(&note, "[math]"),
        "old/math.md",
        "real/math.md",
        "mismatch",
    );
    refuse(
        line_of(&note, "link:"),
        "old/front.md",
        "real/front.md",
        "mismatch",
    );
    let schemes = line_of(&note, "[web]");
    refuse(
        schemes,
        "https://example.com/plan.md",
        "notes/Plan.md",
        "notCandidate",
    );
    refuse(schemes, "mailto:a@b.md", "notes/Plan.md", "notCandidate");
    refuse(schemes, "#plan.md", "notes/Plan.md", "notCandidate");
    if !CASE_INSENSITIVE {
        refuse(
            plan_line,
            "old/plan.md#sec?x=1",
            "notes/Plan.md",
            "notCandidate",
        );
        refuse(
            line_of(&note, "[cased]"),
            "old/Plan.MD",
            "other/plan.md",
            "notCandidate",
        );
        refuse(
            line_of(&note, "![pic]"),
            "pics/shot.png",
            "other/shot.PNG",
            "notCandidate",
        );
    }

    let mut expect = note.clone();
    let rewrite = |expect: &mut String,
                   marker: &str,
                   href: &str,
                   target: &str,
                   new_href: &str,
                   old_span: &str,
                   new_span: &str| {
        assert_eq!(
            h.repair("a/note.md", line_of(expect, marker), href, target),
            json!({"outcome": "wrote", "href": new_href}),
            "{marker}"
        );
        assert_eq!(expect.matches(old_span).count(), 1, "{old_span}");
        *expect = expect.replacen(old_span, new_span, 1);
        assert_eq!(h.bytes("a/note.md"), expect.as_bytes(), "{marker}");
    };
    let (bom_target, bom_href) = if CASE_INSENSITIVE {
        ("notes/Plan.md", "../notes/Plan.md#sec?x=1")
    } else {
        ("other/plan.md", "../other/plan.md#sec?x=1")
    };
    rewrite(
        &mut expect,
        "[bom]",
        "old/plan.md#sec?x=1",
        bom_target,
        bom_href,
        "pré [bom](old/plan.md#sec?x=1) pós",
        &format!("pré [bom]({bom_href}) pós"),
    );
    rewrite(
        &mut expect,
        "[short]",
        "old/plan",
        "other/plan.md",
        "../other/plan",
        "[short](old/plan)",
        "[short](../other/plan)",
    );
    rewrite(
        &mut expect,
        "[title]",
        "old/plan.md",
        "other/plan.md",
        "../other/plan.md",
        "[title](old/plan.md \"keep me\")",
        "[title](../other/plan.md \"keep me\")",
    );
    rewrite(
        &mut expect,
        "[long]",
        "old/plan.md",
        "other/plan.md",
        "../other/plan.md",
        "[long](old/plan.md)",
        "[long](../other/plan.md)",
    );
    if CASE_INSENSITIVE {
        rewrite(
            &mut expect,
            "[cased]",
            "old/Plan.MD",
            "other/plan.md",
            "../other/plan.md",
            "[cased](old/Plan.MD)",
            "[cased](../other/plan.md)",
        );
    }
    rewrite(
        &mut expect,
        "[angle]",
        "old/My Notes (1).md",
        "weird/My Notes (1).md",
        "<../weird/My Notes (1).md>",
        "[angle](<old/My Notes (1).md>)",
        "[angle](<../weird/My Notes (1).md>)",
    );
    rewrite(
        &mut expect,
        "[pct]",
        "old/my%20plan.md",
        "b/my plan.md",
        "../b/my%20plan.md",
        "[pct](old/my%20plan.md)",
        "[pct](../b/my%20plan.md)",
    );
    rewrite(
        &mut expect,
        "[png]",
        "pics/shot.png",
        "img/shot.png",
        "../img/shot.png",
        "[png](pics/shot.png)",
        "[png](../img/shot.png)",
    );
    if CASE_INSENSITIVE {
        rewrite(
            &mut expect,
            "![pic]",
            "pics/shot.png",
            "other/shot.PNG",
            "../other/shot.PNG",
            "![pic](pics/shot.png)",
            "![pic](../other/shot.PNG)",
        );
    }
    rewrite(
        &mut expect,
        "[mark]",
        "old/plan.markdown",
        "notes/plan.markdown",
        "../notes/plan.markdown",
        "[mark](old/plan.markdown)",
        "[mark](../notes/plan.markdown)",
    );
    rewrite(
        &mut expect,
        "[dot]",
        "./plan.md",
        "other/plan.md",
        "../other/plan.md",
        "[dot](./plan.md)",
        "[dot](../other/plan.md)",
    );
    rewrite(
        &mut expect,
        "[win]",
        "sub\\plan.md",
        "other/plan.md",
        "../other/plan.md",
        "[win](sub\\plan.md)",
        "[win](../other/plan.md)",
    );
    rewrite(
        &mut expect,
        "[root]",
        "/old/plan.md",
        "other/plan.md",
        "/other/plan.md",
        "[root](/old/plan.md)",
        "[root](/other/plan.md)",
    );
    rewrite(
        &mut expect,
        "<a href",
        "old/plan.md",
        "other/plan.md",
        "../other/plan.md",
        "<a href=\"old/plan.md\">",
        "<a href=\"../other/plan.md\">",
    );
    rewrite(
        &mut expect,
        "[ref]",
        "old/plan.md",
        "other/plan.md",
        "../other/plan.md",
        "[ref]: old/plan.md",
        "[ref]: ../other/plan.md",
    );
    assert_eq!(
        h.repair(
            "a/note.md",
            line_of(&note, "[bom]"),
            "old/plan.md#sec?x=1",
            "notes/Plan.md"
        ),
        json!({"outcome": "refused", "reason": "mismatch"}),
        "the same call again does not write a second time"
    );
    assert_eq!(
        h.repair(
            "a/note.md",
            line_of(&note, "[stay]"),
            "old/plan.md",
            "hidden/plan.md"
        ),
        json!({"outcome": "refused", "reason": "notCandidate"}),
        "a later refusal still does not take the ignored file"
    );
    assert_eq!(h.bytes("a/note.md"), expect.as_bytes());
    assert!(
        expect.contains("[stay](old/plan.md)"),
        "the copy that was not chosen is still the old href"
    );
    let mut still_broken = Vec::new();
    if !CASE_INSENSITIVE {
        still_broken.push(format!(
            "a/note.md:{}:old/Plan.MD",
            line_of(&note, "[cased]")
        ));
        still_broken.push(format!(
            "a/note.md:{}:pics/shot.png",
            line_of(&note, "![pic]")
        ));
    }
    still_broken.push(format!(
        "a/note.md:{}:gone/missing",
        line_of(&note, "[miss]")
    ));
    still_broken.push(format!("a/note.md:{}:old/planx", line_of(&note, "[near]")));
    still_broken.push(format!(
        "a/note.md:{}:old/plan.md",
        line_of(&note, "[stay]")
    ));
    wait_until("the checker drops every link that now resolves", || {
        h.broken() == still_broken
    });
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

/// Another program holds `path` without delete sharing (a rename over it waits), writes `text` through its handle as
/// soon as the temp of an atomic write of `path` shows up, and lets go a moment later.
#[cfg(windows)]
fn slip_in_save(path: &std::path::Path, text: &'static str) -> std::thread::JoinHandle<()> {
    use std::io::Write as _;
    use std::os::windows::fs::OpenOptionsExt;
    const SHARE_READ_WRITE: u32 = 0x1 | 0x2;
    let mut held = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(SHARE_READ_WRITE)
        .open(path)
        .unwrap();
    let dir = path.parent().unwrap().to_path_buf();
    let prefix = format!(".{}.", path.file_name().unwrap().to_string_lossy());
    std::thread::spawn(move || {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            let temp = std::fs::read_dir(&dir).unwrap().flatten().any(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.starts_with(&prefix) && fsx::is_scratch_name(&name)
            });
            if temp {
                break;
            }
            std::thread::yield_now();
        }
        held.set_len(0).unwrap();
        held.write_all(text.as_bytes()).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        drop(held);
    })
}

#[cfg(windows)]
#[test]
fn a_repair_never_replaces_a_save_that_slips_in_before_its_write() {
    let mut h = Harness::new();
    h.write("a/note.md", "[x](old/plan.md)\n");
    h.write("b/plan.md", "# plan\n");
    h.open();
    let writer = slip_in_save(&h.root().join("a/note.md"), "their save\n");
    let result = h.repair("a/note.md", 1, "old/plan.md", "b/plan.md");
    writer.join().unwrap();
    assert_eq!(result, json!({"outcome": "refused", "reason": "changed"}));
    assert_eq!(h.bytes("a/note.md"), b"their save\n");
}
