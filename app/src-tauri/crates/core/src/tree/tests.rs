use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use super::*;
use crate::events::{CoreEvent, Recorder};
use crate::paths::{Root, Vault};
use crate::settings::{ProjectConfig, TreeConfig, TreeShow};
use crate::watch::{Change, ChangeKind, FsBatch};

struct Fixture {
    dir: tempfile::TempDir,
    explorer: Explorer,
    sink: Arc<Recorder>,
}

fn write(base: &Path, rel: &str) {
    let path = base.join(rel);
    if rel.ends_with('/') {
        fs::create_dir_all(&path).unwrap();
        return;
    }
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "x").unwrap();
}

fn fixture_with(files: &[&str], config: &ProjectConfig) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    for file in files {
        write(dir.path(), file);
    }
    let vault = Vault::new(Root::open(dir.path()).unwrap());
    let sink = Arc::new(Recorder::default());
    let explorer = Explorer::new(&vault, config, sink.clone(), None);
    Fixture {
        dir,
        explorer,
        sink,
    }
}

fn fixture(files: &[&str]) -> Fixture {
    fixture_with(files, &ProjectConfig::default())
}

fn everything() -> TreeFilter {
    TreeFilter {
        show: FilterShow::Everything,
        hide_empty_dirs: false,
        ..TreeFilter::default()
    }
}

fn paths(snapshot: &TreeSnapshot) -> Vec<&str> {
    snapshot.nodes.iter().map(|n| n.path.as_str()).collect()
}

fn shown(f: &Fixture, filter: TreeFilter) -> Vec<String> {
    f.explorer
        .open_view("w", Some(filter))
        .nodes
        .into_iter()
        .map(|n| n.path)
        .collect()
}

fn deltas(f: &Fixture, window: &str) -> Vec<TreeDelta> {
    f.sink
        .take()
        .into_iter()
        .filter_map(|e| match e {
            CoreEvent::TreeChanged(c) if c.window == window => Some(c.delta),
            _ => None,
        })
        .collect()
}

// ---- categories ----------------------------------------------------------------------------------------------------

#[test]
fn category_table() {
    let cases = [
        ("README.md", Category::Markdown),
        ("Notes.MD", Category::Markdown),
        ("rule.mdc", Category::Markdown),
        ("llms.txt", Category::Text),
        ("a.log", Category::Text),
        ("Cargo.toml", Category::Config),
        ("ci.YML", Category::Config),
        (".env", Category::Config),
        (".env.local", Category::Config),
        (".gitignore", Category::Config),
        (".aiignore", Category::Config),
        ("data.JSON", Category::Data),
        ("rows.csv", Category::Data),
        ("pic.png", Category::Image),
        ("logo.SVG", Category::Image),
        ("song.flac", Category::Audio),
        ("clip.mp4", Category::Video),
        ("report.docx", Category::Office),
        ("paper.pdf", Category::Office),
        ("main.rs", Category::Code),
        ("App.TSX", Category::Code),
        ("run.py", Category::Code),
        ("build.sh", Category::Code),
        ("style.css", Category::Code),
        ("page.html", Category::Code),
        ("Makefile", Category::Other),
        ("a.tar.gz", Category::Other),
        ("noext.", Category::Other),
        (".md", Category::Other),
        ("日本語.md", Category::Markdown),
    ];
    for (name, expected) in cases {
        assert_eq!(category_of(name), expected, "{name}");
    }
}

#[test]
fn default_app_opening_is_an_allow_list_with_a_deny_list_on_top() {
    for ok in [
        "a.md", "a.txt", "a.json", "a.png", "a.mp3", "a.mp4", "a.docx", "a.pdf", "A.TOML",
    ] {
        assert!(may_open_with_default_app(ok), "{ok}");
    }
    for no in [
        "run.exe",
        "x.bat",
        "x.cmd",
        "x.ps1",
        "x.sh",
        "x.js",
        "x.jar",
        "x.app",
        "x.lnk",
        "x.msi",
        "x.py",
        "x.svg",
        "x.html",
        "noext",
        "x.unknown",
        "x.vbs",
        "x.desktop",
        "x.EXE",
        "x.md.exe",
        "x.scr",
        // Office files with macros
        "book.xlsm",
        "letter.DOCM",
        "deck.pptm",
        // code is read in the app or opened in the editor, never by the system's program
        "main.rs",
        "app.ts",
        "style.css",
    ] {
        assert!(!may_open_with_default_app(no), "{no}");
    }
    assert!(is_html("a.html") && is_html("A.HTM") && !is_html("a.md"));
}

// ---- snapshot ------------------------------------------------------------------------------------------------------

#[test]
fn snapshot_is_flat_parents_first_folders_first_names_case_insensitive() {
    let f = fixture(&[
        "b.md",
        "A.md",
        "z/inner.md",
        "Y/inner.md",
        "a/b/c.md",
        "ação.md",
        "Ábaco.md",
        "10.md",
        "9.md",
    ]);
    let snap = f.explorer.open_view("w", Some(everything()));
    assert_eq!(
        paths(&snap),
        [
            "a",
            "a/b",
            "a/b/c.md",
            "Y",
            "Y/inner.md",
            "z",
            "z/inner.md",
            "10.md",
            "9.md",
            "A.md",
            "ação.md",
            "b.md",
            "Ábaco.md"
        ]
    );
    assert_eq!(snap.roots.len(), 1);
    assert_eq!((snap.total_files, snap.shown_files), (9, 9));
    // every node comes after its parent
    let mut seen = HashSet::new();
    for node in &snap.nodes {
        if let Some((parent, _)) = node.path.rsplit_once('/') {
            assert!(seen.contains(parent), "{}", node.path);
        }
        seen.insert(node.path.clone());
    }
}

#[test]
fn unicode_space_and_cjk_names_survive() {
    let f = fixture(&["My Docs/日本語 ノート.md", "Ünï/ça va.md", "emoji 😀/x.md"]);
    let snap = f.explorer.open_view("w", None);
    let names: Vec<_> = snap.nodes.iter().map(|n| n.path.as_str()).collect();
    assert!(names.contains(&"My Docs/日本語 ノート.md"));
    assert!(names.contains(&"Ünï/ça va.md"));
    assert!(names.contains(&"emoji 😀/x.md"));
}

#[test]
fn git_folder_and_permark_private_folders_are_never_listed() {
    let f = fixture(&[
        ".git/HEAD",
        ".permark/config.toml",
        ".permark/local/session.json",
        ".permark/cache/x",
        "a.md",
        ".a.md.123.4.part",
        "b.tmp",
    ]);
    let all = TreeFilter {
        show_hidden: true,
        ..everything()
    };
    let list = shown(&f, all);
    assert!(list.contains(&".permark/config.toml".to_owned()));
    for gone in [
        ".git",
        ".git/HEAD",
        ".permark/local",
        ".permark/cache",
        ".a.md.123.4.part",
        "b.tmp",
    ] {
        assert!(!list.iter().any(|p| p == gone), "{gone}");
    }
}

// ---- filter ---------------------------------------------------------------------------------------------------------

#[test]
fn presets_choose_categories() {
    let files = [
        "a.md", "b.txt", "c.json", "d.png", "e.rs", "f.docx", "g.toml", "h.mp3", "i.mp4",
    ];
    let f = fixture(&files);
    let docs = shown(
        &f,
        TreeFilter {
            show: FilterShow::Documents,
            ..TreeFilter::default()
        },
    );
    assert_eq!(docs, ["a.md", "b.txt"]);
    let assets = shown(&f, TreeFilter::default());
    assert_eq!(
        assets,
        ["a.md", "b.txt", "c.json", "d.png", "g.toml", "h.mp3", "i.mp4"]
    );
    let all = shown(&f, everything());
    assert_eq!(all.len(), files.len());
}

#[test]
fn custom_filter_uses_its_categories_and_switching_keeps_the_previous_preset() {
    let f = fixture(&["a.md", "c.json", "d.png"]);
    let filter = TreeFilter::default().with_show(FilterShow::Custom);
    assert_eq!(filter.categories, TreeFilter::default().categories);
    let only_images = TreeFilter {
        categories: vec![Category::Image],
        assets_near_docs_only: false,
        ..filter
    };
    assert_eq!(shown(&f, only_images), ["d.png"]);
}

#[test]
fn assets_must_be_near_documents() {
    let f = fixture(&[
        "README.md",
        "package.json",
        "docs/guide.md",
        "docs/img/a.png",
        "docs/data.json",
        "src-tauri/Cargo.toml",
        "src-tauri/icons/i.png",
        "data/x.json",
        "assets/logo.png",
        "assets/rows.csv",
        "deep/er/there/pic.png",
        "media/clip.mp4",
    ]);
    let list = shown(&f, TreeFilter::default());
    for yes in [
        "README.md",
        "package.json",    // beside the root readme
        "docs/img/a.png",  // folder above holds a document
        "docs/data.json",  // own folder holds a document
        "assets/logo.png", // asset-named folder
        "media/clip.mp4",
    ] {
        assert!(list.contains(&yes.to_owned()), "{yes} in {list:?}");
    }
    for no in [
        "src-tauri/Cargo.toml", // the root does not count for a folder below it
        "src-tauri/icons/i.png",
        "data/x.json",
        "assets/rows.csv", // data is a companion of documents only
        "deep/er/there/pic.png",
    ] {
        assert!(!list.contains(&no.to_owned()), "{no}");
    }
    let everywhere = shown(
        &f,
        TreeFilter {
            assets_near_docs_only: false,
            ..TreeFilter::default()
        },
    );
    assert!(everywhere.contains(&"data/x.json".to_owned()));
}

#[test]
fn hidden_names_need_show_hidden_and_a_hidden_folder_is_listed_for_what_it_holds() {
    let f = fixture(&[
        ".github/workflows/ci.yml",
        ".github/README.md",
        ".env",
        "a.md",
        ".cache/x.md",
    ]);
    let plain = shown(&f, TreeFilter::default());
    assert_eq!(plain, ["a.md"]);
    let with_hidden = shown(
        &f,
        TreeFilter {
            show_hidden: true,
            ..TreeFilter::default()
        },
    );
    assert!(with_hidden.contains(&".github".to_owned()));
    assert!(with_hidden.contains(&".github/README.md".to_owned()));
    assert!(with_hidden.contains(&".env".to_owned()));
    // a pattern reveals a file in a hidden folder, and the folder with it
    let revealed = shown(
        &f,
        TreeFilter {
            show: FilterShow::Custom,
            categories: vec![Category::Markdown],
            patterns: "+.github/workflows/*.yml".to_owned(),
            ..TreeFilter::default()
        },
    );
    assert!(revealed.contains(&".github".to_owned()), "{revealed:?}");
    assert!(revealed.contains(&".github/workflows/ci.yml".to_owned()));
    assert!(!revealed.contains(&".cache".to_owned()));
}

#[test]
fn code_files_the_filter_hides_are_counted_for_the_code_offer() {
    let f = fixture(&[
        "README.md",
        "docs/a.md",
        "src/main.rs",
        "src/lib.rs",
        "web/app.ts",
        "build.sh",
        "x.png",
        "Makefile",
        ".hidden/h.rs",
        "node_modules/m/index.js",
    ]);
    // the default filter shows documents and assets: four code files are hidden (a dot folder and node_modules are
    // out of the count: they are not on offer either way)
    let snapshot = f.explorer.open_view("w", None);
    assert_eq!(snapshot.code_hidden, 4);
    assert!(!paths(&snapshot).contains(&"src/main.rs"));
    // the preset Documents hides them the same
    assert_eq!(
        f.explorer
            .open_view(
                "d",
                Some(TreeFilter::default().with_show(FilterShow::Documents))
            )
            .code_hidden,
        4
    );
    // Everything shows them: none hidden
    let all = f.explorer.open_view("e", Some(everything()));
    assert_eq!(all.code_hidden, 0);
    assert!(paths(&all).contains(&"src/main.rs"));
    // Custom with the Code category on: shown, none hidden; with hidden paths on, the dot folder counts
    let custom = TreeFilter {
        show: FilterShow::Custom,
        categories: vec![Category::Markdown, Category::Code],
        ..TreeFilter::default()
    };
    let snapshot = f.explorer.open_view("c", Some(custom.clone()));
    assert_eq!(snapshot.code_hidden, 0);
    assert!(paths(&snapshot).contains(&"web/app.ts"));
    let with_hidden = TreeFilter {
        show: FilterShow::Documents,
        show_hidden: true,
        ..TreeFilter::default()
    };
    assert_eq!(f.explorer.open_view("h", Some(with_hidden)).code_hidden, 5);
    // a pattern that shows one of them takes it out of the count
    let pattern = TreeFilter {
        show: FilterShow::Custom,
        categories: vec![Category::Markdown, Category::Text, Category::Image],
        patterns: "+*.sh".to_owned(),
        ..TreeFilter::default()
    };
    assert_eq!(f.explorer.open_view("p", Some(pattern)).code_hidden, 3);
}

#[test]
fn empty_folders_go_unless_asked_or_kept() {
    let f = fixture(&["empty/", "full/a.md", "only-code/x.rs"]);
    assert_eq!(shown(&f, TreeFilter::default()), ["full", "full/a.md"]);
    let show_empty = TreeFilter {
        hide_empty_dirs: false,
        ..TreeFilter::default()
    };
    let list = shown(&f, show_empty);
    assert!(list.contains(&"empty".to_owned()) && list.contains(&"only-code".to_owned()));
    f.explorer.keep_folder("empty");
    assert_eq!(
        shown(&f, TreeFilter::default()),
        ["empty", "full", "full/a.md"],
        "a folder made in the Explorer stays while empty"
    );
}

#[test]
fn patterns_show_hide_and_the_last_match_wins() {
    let f = fixture(&[
        "a.md",
        "src/main.rs",
        "src/lib.rs",
        "archive/old.md",
        "archive/keep.md",
        "Q.SQL",
    ]);
    let custom = |patterns: &str| TreeFilter {
        show: FilterShow::Custom,
        categories: vec![Category::Markdown],
        patterns: patterns.to_owned(),
        ..TreeFilter::default()
    };
    let list = shown(&f, custom("+*.rs +*.sql"));
    assert!(
        list.contains(&"src/main.rs".to_owned()) && list.contains(&"Q.SQL".to_owned()),
        "case-insensitive: {list:?}"
    );
    let list = shown(&f, custom("-archive/"));
    assert_eq!(list, ["a.md"]);
    let list = shown(&f, custom("-archive/ +archive/keep.md"));
    assert_eq!(list, ["archive", "archive/keep.md", "a.md"]);
    let list = shown(&f, custom("-archive/ +archive/keep.md -keep.md"));
    assert_eq!(list, ["a.md"]);
    // commas and braces
    let list = shown(&f, custom("+{main,lib}.rs,-a.md"));
    assert_eq!(
        list,
        [
            "archive",
            "archive/keep.md",
            "archive/old.md",
            "src",
            "src/lib.rs",
            "src/main.rs"
        ]
    );
}

#[test]
fn malformed_patterns_are_reported_and_ignored() {
    let f = fixture(&["a.md"]);
    let snap = f.explorer.open_view(
        "w",
        Some(TreeFilter {
            show: FilterShow::Custom,
            categories: vec![Category::Markdown],
            patterns: "-{x +ok* !neg #c + -".to_owned(),
            ..TreeFilter::default()
        }),
    );
    assert_eq!(snap.invalid_patterns, ["-{x", "!neg", "#c", "+", "-"]);
    assert_eq!(paths(&snap), ["a.md"]);
}

#[test]
fn project_default_comes_from_the_tree_section() {
    let config = |show, include: &[&str], exclude: &[&str]| TreeConfig {
        show,
        include: include.iter().map(|s| (*s).to_owned()).collect(),
        exclude: exclude.iter().map(|s| (*s).to_owned()).collect(),
    };
    assert_eq!(
        TreeFilter::from_config(&TreeConfig::default()),
        TreeFilter::default()
    );
    let docs = TreeFilter::from_config(&config(Some(TreeShow::Documents), &[], &[]));
    assert_eq!(docs.show, FilterShow::Documents);
    let patterned =
        TreeFilter::from_config(&config(Some(TreeShow::Documents), &["*.rs"], &["old/"]));
    assert_eq!(patterned.show, FilterShow::Custom);
    assert_eq!(patterned.categories, [Category::Markdown, Category::Text]);
    assert_eq!(patterned.patterns, "+*.rs -old/");
}

#[test]
fn the_kept_document_is_shown_even_when_the_filter_hides_it() {
    let f = fixture(&["a.md", "code/x.rs", "deep/er/y.rs"]);
    f.explorer.open_view("w", None);
    f.sink.take();
    f.explorer.keep("w", Some("deep/er/y.rs")).unwrap();
    let delta = deltas(&f, "w").remove(0);
    let added: Vec<_> = delta
        .added
        .iter()
        .map(|a| (a.node.path.as_str(), a.node.forced))
        .collect();
    assert_eq!(
        added,
        [("deep", false), ("deep/er", false), ("deep/er/y.rs", true)]
    );
    f.explorer.keep("w", None).unwrap();
    let delta = deltas(&f, "w").remove(0);
    assert_eq!(delta.removed, ["deep", "deep/er", "deep/er/y.rs"]);
    assert!(matches!(
        f.explorer.keep("nope", None),
        Err(TreeError::NoView)
    ));
}

// ---- ignore ---------------------------------------------------------------------------------------------------------

#[test]
fn gitignore_hides_and_show_ignored_reveals_without_a_repo() {
    let f = fixture(&[
        ".gitignore",
        "a.md",
        "build/out.md",
        "debug.log",
        "sub/.gitignore",
        "sub/b.md",
        "sub/gen.md",
    ]);
    fs::write(f.dir.path().join(".gitignore"), "build/\n*.log\n").unwrap();
    fs::write(f.dir.path().join("sub/.gitignore"), "gen.md\n!*.log\n").unwrap();
    f.explorer.apply_batch(&FsBatch::default());
    let plain = shown(&f, everything());
    assert!(!plain.contains(&"build".to_owned()) && !plain.contains(&"debug.log".to_owned()));
    assert!(plain.contains(&"sub/b.md".to_owned()) && !plain.contains(&"sub/gen.md".to_owned()));
    let with = shown(
        &f,
        TreeFilter {
            show_ignored: true,
            ..everything()
        },
    );
    assert!(with.contains(&"build".to_owned()) && with.contains(&"debug.log".to_owned()));
    assert!(
        !with.contains(&"build/out.md".to_owned()),
        "ignored folders stay unopened"
    );
}

#[test]
fn ignored_folders_open_on_request_and_their_children_are_ignored() {
    let f = fixture(&[".gitignore", "build/out.md", "build/deep/x.md", "a.md"]);
    fs::write(f.dir.path().join(".gitignore"), "build/\n").unwrap();
    let shows = TreeFilter {
        show_ignored: true,
        ..everything()
    };
    f.explorer.apply_batch(&FsBatch {
        rescan: true,
        ..FsBatch::default()
    });
    let snap = f.explorer.open_view("w", Some(shows));
    let build = snap.nodes.iter().find(|n| n.path == "build").unwrap();
    assert_eq!(
        build.kind,
        NodeKind::Dir {
            unlisted: Some(Unlisted::Ignored)
        }
    );
    assert!(build.ignored);
    f.sink.take();
    f.explorer.open_folder("build").unwrap();
    let delta = deltas(&f, "w").remove(0);
    let added: Vec<_> = delta
        .added
        .iter()
        .map(|a| (a.node.path.as_str(), a.node.ignored))
        .collect();
    assert!(added.contains(&("build/out.md", true)), "{added:?}");
    assert!(added.contains(&("build/deep", true)));
    assert!(
        f.explorer.open_folder("a.md").is_err(),
        "a file is no folder"
    );
    assert!(f.explorer.open_folder("../x").is_err());
}

#[test]
fn node_modules_is_ignored_without_any_ignore_file() {
    let f = fixture(&["node_modules/pkg/index.md", "a.md", "src/node_modules/x.md"]);
    let list = shown(&f, everything());
    assert!(!list.iter().any(|p| p.contains("node_modules")), "{list:?}");
    let list = shown(
        &f,
        TreeFilter {
            show_ignored: true,
            ..everything()
        },
    );
    assert!(list.contains(&"node_modules".to_owned()));
    assert!(!list.contains(&"node_modules/pkg".to_owned()));
}

#[test]
fn a_nested_repository_is_never_hidden_by_the_enclosing_rules() {
    // a root repo that ignores `teams/`, each team folder a repo of its own
    let f = fixture(&[
        ".git/HEAD",
        ".gitignore",
        "teams/team-a/.git/HEAD",
        "teams/team-a/readme.md",
        "teams/team-a/.gitignore",
        "teams/team-a/out/x.md",
        "teams/plain/y.md",
        "root.md",
    ]);
    fs::write(f.dir.path().join(".gitignore"), "teams/\nreadme.md\n").unwrap();
    fs::write(f.dir.path().join("teams/team-a/.gitignore"), "out/\n").unwrap();
    f.explorer.apply_batch(&FsBatch {
        rescan: true,
        ..FsBatch::default()
    });
    let list = shown(&f, everything());
    assert!(list.contains(&"teams".to_owned()), "{list:?}");
    assert!(
        list.contains(&"teams/team-a/readme.md".to_owned()),
        "enclosing rules do not reach into the repo"
    );
    assert!(
        !list.contains(&"teams/team-a/out".to_owned()),
        "its own rules do"
    );
    assert!(
        !list.contains(&"teams/plain".to_owned()),
        "a folder without .git stays ignored"
    );
    assert!(list.contains(&"root.md".to_owned()));
}

#[test]
fn the_project_ignore_list_hides_like_a_gitignore() {
    let config = ProjectConfig {
        ignore: vec!["drafts/".to_owned(), "*.bak".to_owned()],
        ..ProjectConfig::default()
    };
    let f = fixture_with(&["a.md", "drafts/x.md", "a.md.bak", "keep/b.md"], &config);
    assert_eq!(shown(&f, everything()), ["keep", "keep/b.md", "a.md"]);
}

#[test]
fn a_project_inside_a_repository_honors_the_gitignore_above_it() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), ".git/HEAD");
    write(dir.path(), "docs/a.md");
    write(dir.path(), "docs/b.log");
    fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();
    let vault = Vault::new(Root::open(&dir.path().join("docs")).unwrap());
    let explorer = Explorer::new(
        &vault,
        &ProjectConfig::default(),
        Arc::new(Recorder::default()),
        None,
    );
    let snap = explorer.open_view("w", Some(everything()));
    assert_eq!(paths(&snap), ["a.md"]);
}

#[test]
fn global_excludes_apply() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.md");
    write(dir.path(), "scratch.notes");
    let mut builder = ::ignore::gitignore::GitignoreBuilder::new("");
    builder.add_line(None, "*.notes").unwrap();
    let global = builder.build().unwrap();
    let vault = Vault::new(Root::open(dir.path()).unwrap());
    let explorer = Explorer::new(
        &vault,
        &ProjectConfig::default(),
        Arc::new(Recorder::default()),
        Some(&global),
    );
    assert_eq!(
        paths(&explorer.open_view("w", Some(everything()))),
        ["a.md"]
    );
}

// ---- large folders ---------------------------------------------------------------------------------------------------

#[test]
fn a_folder_with_too_many_entries_is_listed_unopened_until_asked() {
    let f = fixture(&["a.md", "small/x.md"]);
    let big = f.dir.path().join("big");
    fs::create_dir(&big).unwrap();
    for n in 0..=LARGE_FOLDER {
        fs::write(big.join(format!("f{n}.md")), "").unwrap();
    }
    f.explorer.apply_batch(&FsBatch {
        rescan: true,
        ..FsBatch::default()
    });
    let snap = f.explorer.open_view("w", None);
    let node = snap.nodes.iter().find(|n| n.path == "big").unwrap();
    assert_eq!(
        node.kind,
        NodeKind::Dir {
            unlisted: Some(Unlisted::Large {
                entries: u32::try_from(LARGE_FOLDER + 1).unwrap()
            })
        }
    );
    assert!(!snap.nodes.iter().any(|n| n.path.starts_with("big/")));
    assert!(snap.nodes.iter().any(|n| n.path == "small/x.md"));
    f.sink.take();
    // asked for: listed whatever its size
    f.explorer.open_folder("big").unwrap();
    let delta = deltas(&f, "w").remove(0);
    assert_eq!(
        delta.removed,
        ["big"],
        "the placeholder is replaced by the listed folder"
    );
    assert_eq!(delta.added.len(), LARGE_FOLDER + 2);
    assert_eq!(delta.added[0].node.path, "big");
}

#[test]
fn exactly_the_limit_is_listed() {
    let f = fixture(&[]);
    let dir = f.dir.path().join("exact");
    fs::create_dir(&dir).unwrap();
    for n in 0..LARGE_FOLDER {
        fs::write(dir.join(format!("f{n}.md")), "").unwrap();
    }
    f.explorer.apply_batch(&FsBatch {
        rescan: true,
        ..FsBatch::default()
    });
    let snap = f.explorer.open_view("w", None);
    assert_eq!(snap.shown_files as usize, LARGE_FOLDER);
}

#[test]
fn listing_ten_thousand_files_is_fast() {
    let dir = tempfile::tempdir().unwrap();
    for d in 0..100 {
        let folder = dir.path().join(format!("dir{d:03}"));
        fs::create_dir_all(folder.join("sub")).unwrap();
        for n in 0..50 {
            fs::write(folder.join(format!("n{n}.md")), "").unwrap();
            fs::write(folder.join("sub").join(format!("p{n}.png")), "").unwrap();
        }
    }
    let vault = Vault::new(Root::open(dir.path()).unwrap());
    let explorer = Explorer::new(
        &vault,
        &ProjectConfig::default(),
        Arc::new(Recorder::default()),
        None,
    );
    let started = std::time::Instant::now();
    let snap = explorer.open_view("w", None);
    let elapsed = started.elapsed();
    assert_eq!(snap.total_files, 10_000);
    eprintln!("listing 10 000 files: {elapsed:?}");
    // 300 ms in a release build; a debug build runs several times slower
    let limit = if cfg!(debug_assertions) { 3_000 } else { 300 };
    assert!(elapsed.as_millis() < limit, "{elapsed:?}");
}

// ---- links are leaves ----------------------------------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn symbolic_links_are_leaves_never_followed() {
    let f = fixture(&["real/inner.md", "a.md"]);
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "secret/s.md");
    std::os::unix::fs::symlink(outside.path(), f.dir.path().join("escape")).unwrap();
    std::os::unix::fs::symlink(f.dir.path().join("real"), f.dir.path().join("alias")).unwrap();
    f.explorer.apply_batch(&FsBatch {
        rescan: true,
        ..FsBatch::default()
    });
    let list = shown(&f, everything());
    assert!(list.contains(&"escape".to_owned()) && list.contains(&"alias".to_owned()));
    assert!(
        !list
            .iter()
            .any(|p| p.starts_with("escape/") || p.starts_with("alias/")),
        "{list:?}"
    );
}

#[cfg(windows)]
#[test]
fn junctions_are_leaves_never_followed() {
    let f = fixture(&["real/inner.md", "a.md"]);
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "secret/s.md");
    let link = f.dir.path().join("escape");
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(outside.path())
        .output()
        .is_ok_and(|o| o.status.success());
    if !made {
        eprintln!("junction could not be made here; skipped");
        return;
    }
    f.explorer.apply_batch(&FsBatch {
        rescan: true,
        ..FsBatch::default()
    });
    let list = shown(&f, everything());
    assert!(list.contains(&"escape".to_owned()), "{list:?}");
    assert!(!list.iter().any(|p| p.starts_with("escape/")), "{list:?}");
}

// ---- views and deltas -----------------------------------------------------------------------------------------------

/// What a window does with a delta (the frontend mirrors this).
fn apply(list: &mut Vec<TreeNode>, delta: &TreeDelta) {
    let gone: HashSet<&str> = delta.removed.iter().map(String::as_str).collect();
    list.retain(|n| !gone.contains(n.path.as_str()));
    for added in &delta.added {
        let at = match &added.after {
            None => 0,
            Some(path) => list
                .iter()
                .position(|n| &n.path == path)
                .map(|i| i + 1)
                .unwrap(),
        };
        list.insert(at, added.node.clone());
    }
}

#[test]
fn a_new_file_arrives_as_a_small_delta_after_its_predecessor() {
    let f = fixture(&["a.md", "b.md", "docs/x.md"]);
    let snap = f.explorer.open_view("w", None);
    let mut list = snap.nodes.clone();
    fs::write(f.dir.path().join("ab.md"), "x").unwrap();
    f.explorer.apply_batch(&FsBatch {
        changes: vec![Change {
            path: "ab.md".to_owned(),
            kind: ChangeKind::Created,
            is_dir: false,
            own: false,
        }],
        ..FsBatch::default()
    });
    let mut got = deltas(&f, "w");
    assert_eq!(got.len(), 1);
    let delta = got.remove(0);
    assert_eq!(delta.seq, snap.seq + 1);
    assert!(delta.removed.is_empty(), "nothing is dropped");
    assert_eq!(delta.added.len(), 1);
    assert_eq!(delta.added[0].after.as_deref(), Some("a.md"));
    assert_eq!((delta.total_files, delta.shown_files), (4, 4));
    apply(&mut list, &delta);
    assert_eq!(list, f.explorer.open_view("check", None).nodes);
}

#[test]
fn a_change_that_shows_nothing_sends_nothing_but_counts_change_when_a_file_is_hidden_by_the_filter()
{
    let f = fixture(&["a.md"]);
    f.explorer.open_view("w", None);
    fs::write(f.dir.path().join("code.rs"), "x").unwrap();
    f.explorer.sync_paths(&["code.rs".to_owned()], &[]);
    let delta = deltas(&f, "w").remove(0);
    assert!(delta.is_empty(), "only the counts changed");
    assert_eq!((delta.total_files, delta.shown_files), (2, 1));
    f.explorer.sync_paths(&["code.rs".to_owned()], &[]);
    assert!(deltas(&f, "w").is_empty(), "nothing changed, nothing sent");
}

#[test]
fn every_window_has_its_own_filter_and_gets_its_own_delta() {
    let f = fixture(&["a.md", "code.rs"]);
    f.explorer.open_view("docs", None);
    f.explorer.open_view("all", Some(everything()));
    fs::write(f.dir.path().join("b.rs"), "x").unwrap();
    f.explorer.sync_paths(&["b.rs".to_owned()], &[]);
    let events = f.sink.take();
    let windows: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            CoreEvent::TreeChanged(c) => Some((c.window.clone(), c.delta.added.len())),
            _ => None,
        })
        .collect();
    assert!(windows.contains(&("all".to_owned(), 1)), "{windows:?}");
    let docs = windows.iter().find(|(w, _)| w == "docs").map(|(_, n)| *n);
    assert_eq!(docs, Some(0), "counts only");
    f.explorer.close_view("all");
    fs::write(f.dir.path().join("c.rs"), "x").unwrap();
    f.explorer.sync_paths(&["c.rs".to_owned()], &[]);
    assert!(f
        .sink
        .take()
        .iter()
        .all(|e| !matches!(e, CoreEvent::TreeChanged(c) if c.window == "all")));
}

#[test]
fn set_filter_returns_a_snapshot_with_a_newer_seq() {
    let f = fixture(&["a.md", "b.rs"]);
    let first = f.explorer.open_view("w", None);
    let second = f.explorer.set_filter("w", everything()).unwrap();
    assert!(second.seq > first.seq);
    assert_eq!(second.nodes.len(), 2);
    assert!(matches!(
        f.explorer.set_filter("nobody", everything()),
        Err(TreeError::NoView)
    ));
}

#[test]
fn renamed_folders_arrive_as_removed_and_added_with_a_rename_hint() {
    let f = fixture(&["old/a.md", "old/sub/b.md", "z.md"]);
    let snap = f.explorer.open_view("w", None);
    let mut list = snap.nodes;
    fs::rename(f.dir.path().join("old"), f.dir.path().join("new")).unwrap();
    f.explorer.sync_paths(
        &["old".to_owned(), "new".to_owned()],
        &[Renamed {
            from: "old".to_owned(),
            to: "new".to_owned(),
        }],
    );
    let delta = deltas(&f, "w").remove(0);
    assert_eq!(
        delta.renamed,
        [Renamed {
            from: "old".to_owned(),
            to: "new".to_owned()
        }]
    );
    assert_eq!(
        delta.removed,
        ["old", "old/sub", "old/sub/b.md", "old/a.md"]
    );
    apply(&mut list, &delta);
    assert_eq!(list, f.explorer.open_view("check", None).nodes);
}

/// Every path below `root` (folders and files), as project paths.
fn list_all(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let name = entry.file_name().into_string().unwrap();
            let path = if rel.is_empty() {
                name
            } else {
                format!("{rel}/{name}")
            };
            out.push(path.clone());
            if entry.file_type().unwrap().is_dir() {
                walk(&entry.path(), &path, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, "", &mut out);
    out
}

#[test]
#[allow(clippy::too_many_lines)] // one long random walk, easier to read in one piece
fn deltas_equal_the_difference_of_snapshots_under_random_changes() {
    // a small deterministic generator: the same sequence every run
    let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next = move |bound: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        usize::try_from(seed % bound as u64).unwrap()
    };
    let f = fixture(&["start.md", "docs/a.md", "docs/img/p.png", "src/lib.rs"]);
    let filters = [
        TreeFilter::default(),
        everything(),
        TreeFilter {
            show_hidden: true,
            ..TreeFilter::default()
        },
    ];
    let mut lists: Vec<Vec<TreeNode>> = Vec::new();
    for (n, filter) in filters.iter().enumerate() {
        lists.push(
            f.explorer
                .open_view(&format!("w{n}"), Some(filter.clone()))
                .nodes,
        );
    }
    f.sink.take();
    let names = [
        "a", "b", "c.md", "d.png", "E.md", "e.md", ".h.md", "docs", "Docs", "x.rs", "m.json",
    ];
    let root = f.dir.path();
    for step in 0..250 {
        let existing = list_all(root);
        let dirs: Vec<String> = std::iter::once(String::new())
            .chain(existing.iter().filter(|p| root.join(p).is_dir()).cloned())
            .collect();
        let name = names[next(names.len())];
        let mut touched: Vec<String> = Vec::new();
        let mut renames: Vec<Renamed> = Vec::new();
        match next(5) {
            0 | 1 => {
                let dir = &dirs[next(dirs.len())];
                let path = if dir.is_empty() {
                    name.to_owned()
                } else {
                    format!("{dir}/{name}")
                };
                if !root.join(&path).exists() {
                    if next(3) == 0 {
                        fs::create_dir(root.join(&path)).unwrap();
                    } else {
                        fs::write(root.join(&path), "x").unwrap();
                    }
                    touched.push(path);
                }
            }
            2 if !existing.is_empty() => {
                let path = existing[next(existing.len())].clone();
                let target = root.join(&path);
                if target.is_dir() {
                    fs::remove_dir_all(&target).unwrap();
                } else {
                    fs::remove_file(&target).unwrap();
                }
                touched.push(path);
            }
            3 | 4 if !existing.is_empty() => {
                let from = existing[next(existing.len())].clone();
                let dir = &dirs[next(dirs.len())];
                let to = if dir.is_empty() {
                    name.to_owned()
                } else {
                    format!("{dir}/{name}")
                };
                let inside = to == from || to.starts_with(&format!("{from}/"));
                if !inside
                    && !root.join(&to).exists()
                    && fs::rename(root.join(&from), root.join(&to)).is_ok()
                {
                    renames.push(Renamed {
                        from: from.clone(),
                        to: to.clone(),
                    });
                    touched.push(from);
                    touched.push(to);
                }
            }
            _ => {}
        }
        if touched.is_empty() {
            continue;
        }
        f.explorer.sync_paths(&touched, &renames);
        let mut by_window: std::collections::HashMap<String, Vec<TreeDelta>> =
            std::collections::HashMap::new();
        for event in f.sink.take() {
            if let CoreEvent::TreeChanged(c) = event {
                by_window.entry(c.window).or_default().push(c.delta);
            }
        }
        for (n, filter) in filters.iter().enumerate() {
            for delta in by_window.remove(&format!("w{n}")).unwrap_or_default() {
                apply(&mut lists[n], &delta);
            }
            let fresh = f.explorer.open_view("check", Some(filter.clone())).nodes;
            assert_eq!(lists[n], fresh, "step {step}, filter {n}");
        }
    }
}

// ---- watch batches ---------------------------------------------------------------------------------------------------

#[test]
fn a_changed_gitignore_rebuilds_the_picture() {
    let f = fixture(&["a.md", "b.md"]);
    f.explorer.open_view("w", None);
    f.sink.take();
    fs::write(f.dir.path().join(".gitignore"), "b.md\n").unwrap();
    f.explorer.apply_batch(&FsBatch {
        changes: vec![Change {
            path: ".gitignore".to_owned(),
            kind: ChangeKind::Created,
            is_dir: false,
            own: false,
        }],
        ..FsBatch::default()
    });
    let delta = deltas(&f, "w").remove(0);
    assert_eq!(delta.removed, ["b.md"]);
}

#[test]
fn batches_before_the_first_walk_are_ignored_and_a_vanished_file_leaves() {
    let f = fixture(&["a.md"]);
    // no view yet, the model is not built: nothing to update, nothing breaks
    f.explorer.apply_batch(&FsBatch {
        changes: vec![Change {
            path: "ghost.md".to_owned(),
            kind: ChangeKind::Removed,
            is_dir: false,
            own: false,
        }],
        ..FsBatch::default()
    });
    assert_eq!(paths(&f.explorer.open_view("w", None)), ["a.md"]);
    fs::remove_file(f.dir.path().join("a.md")).unwrap();
    f.sink.take();
    f.explorer.apply_batch(&FsBatch {
        changes: vec![Change {
            path: "a.md".to_owned(),
            kind: ChangeKind::Removed,
            is_dir: false,
            own: false,
        }],
        ..FsBatch::default()
    });
    assert_eq!(deltas(&f, "w").remove(0).removed, ["a.md"]);
}

#[test]
fn hostile_paths_in_a_batch_change_nothing() {
    let f = fixture(&["a.md"]);
    let before = f.explorer.open_view("w", None);
    f.sink.take();
    let hostile = [
        "../outside.md",
        "/etc/passwd",
        "C:/Windows/x",
        "a.md/../../x",
        "",
        "@wt/ghost/x.md",
        ".git/HEAD",
        ".permark/local/x",
        "nul",
        "a\0b",
    ];
    f.explorer.apply_batch(&FsBatch {
        changes: hostile
            .iter()
            .map(|p| Change {
                path: (*p).to_owned(),
                kind: ChangeKind::Created,
                is_dir: false,
                own: false,
            })
            .collect(),
        ..FsBatch::default()
    });
    assert!(deltas(&f, "w").is_empty(), "nothing sent");
    assert_eq!(f.explorer.open_view("w", None).nodes, before.nodes);
}

#[test]
fn case_variants_are_one_entry_where_the_file_system_ignores_case() {
    let f = fixture(&["Readme.md"]);
    f.explorer.open_view("w", None);
    f.sink.take();
    // a stale event with another spelling must not duplicate the entry
    f.explorer.sync_paths(&["README.md".to_owned()], &[]);
    let list = shown(&f, everything());
    if crate::paths::CASE_INSENSITIVE {
        assert_eq!(list.len(), 1, "{list:?}");
    }
}

#[test]
fn deep_nesting_is_listed_without_overflowing() {
    let dir = tempfile::tempdir().unwrap();
    let mut path = dir.path().to_path_buf();
    for _ in 0..60 {
        path.push("d");
    }
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("deep.md"), "x").unwrap();
    let vault = Vault::new(Root::open(dir.path()).unwrap());
    let explorer = Explorer::new(
        &vault,
        &ProjectConfig::default(),
        Arc::new(Recorder::default()),
        None,
    );
    let snap = explorer.open_view("w", None);
    assert_eq!(snap.shown_files, 1);
    assert_eq!(snap.nodes.len(), 61);
}

#[test]
fn documents_lists_markdown_outside_ignored_folders() {
    let f = fixture(&[
        "a.md",
        "docs/b.md",
        "docs/c.txt",
        "node_modules/p/x.md",
        ".hidden/h.md",
    ]);
    f.explorer.open_view("w", None);
    assert_eq!(
        f.explorer.documents(""),
        [".hidden/h.md", "a.md", "docs/b.md"]
    );
    assert_eq!(
        f.explorer.listed("docs/b.md"),
        Some(NodeKind::File {
            category: Category::Markdown
        })
    );
    assert_eq!(f.explorer.listed("nope.md"), None);
}

// ---- scan + linked files --------------------------------------------------------------------------------------------

fn scanned(f: &Fixture) -> (Vec<String>, Vec<String>) {
    let never = std::sync::atomic::AtomicBool::new(false);
    let mut files = Vec::new();
    let mut unchecked = Vec::new();
    for (_, scan) in f.explorer.scan(&never) {
        files.extend(scan.files.into_iter().map(|f| f.path));
        unchecked.extend(scan.unchecked);
    }
    files.sort();
    (files, unchecked)
}

#[test]
fn scan_lists_what_the_tree_lists_without_the_size_guard_and_names_ignored_folders() {
    let f = fixture(&[
        ".gitignore",
        "docs/a.md",
        "docs/img/p.png",
        "build/out.md",
        "node_modules/x/y.md",
    ]);
    fs::write(f.dir.path().join(".gitignore"), "build/\n").unwrap();
    let (files, unchecked) = scanned(&f);
    assert_eq!(files, [".gitignore", "docs/a.md", "docs/img/p.png"]);
    assert_eq!(unchecked, ["build", "node_modules"]);
    // Markdown and Text files carry their place on disk; the others do not
    let never = std::sync::atomic::AtomicBool::new(false);
    let scan = &f.explorer.scan(&never)[0].1;
    assert!(scan
        .files
        .iter()
        .find(|x| x.path == "docs/a.md")
        .unwrap()
        .abs
        .is_some());
    assert!(scan
        .files
        .iter()
        .find(|x| x.path == "docs/img/p.png")
        .unwrap()
        .abs
        .is_none());
}

#[test]
fn scan_documents_names_a_root_and_knows_when_there_is_none() {
    // (a folder over the size limit is walked: the search and plan tests make one)
    let f = fixture(&["docs/a.md", "docs/b.txt", "top.md"]);
    let (docs, unchecked) = f.explorer.scan_documents("");
    assert_eq!(docs, ["docs/a.md", "top.md"]);
    assert_eq!(unchecked.len(), 0);
    assert_eq!(f.explorer.scan_documents("@wt/none").0.len(), 0);
}

#[test]
fn scan_stops_when_cancelled_and_never_follows_links_or_enters_git() {
    let f = fixture(&["a.md", "sub/b.md", ".git/config"]);
    let cancelled = std::sync::atomic::AtomicBool::new(true);
    let scan = &f.explorer.scan(&cancelled)[0].1;
    assert!(scan.cut_off);
    let (files, _) = scanned(&f);
    assert_eq!(files, ["a.md", "sub/b.md"]);
}

fn folder_scan(
    f: &Fixture,
    folder: &str,
    stop: &mut dyn FnMut(&ScanFile) -> bool,
) -> Result<Scan, TreeError> {
    let never = std::sync::atomic::AtomicBool::new(false);
    let path = crate::paths::VaultPath::parse(folder).unwrap();
    let abs = f.dir.path().join(folder);
    f.explorer.scan_folder(&path, &abs, &never, stop)
}

fn folder_paths(scan: &Scan) -> Vec<&str> {
    let mut paths: Vec<&str> = scan.files.iter().map(|x| x.path.as_str()).collect();
    paths.sort_unstable();
    paths
}

#[test]
fn a_folder_scan_starts_inside_with_the_rules_of_the_folders_above_and_measures_files() {
    let f = fixture(&[
        ".gitignore",
        "top.md",
        "docs/a.md",
        "docs/noise.log",
        "docs/build/z.md",
        "docs/sub/b.md",
        "docs/img/p.png",
        "node_modules/x/y.md",
        "docs/node_modules/x/y.md",
    ]);
    fs::write(
        f.dir.path().join(".gitignore"),
        "*.log
build/
",
    )
    .unwrap();
    fs::write(f.dir.path().join("docs/a.md"), "twelve bytes").unwrap();
    let scan = folder_scan(&f, "docs", &mut |_| false).unwrap();
    assert!(!scan.cut_off);
    assert_eq!(
        folder_paths(&scan),
        ["docs/a.md", "docs/img/p.png", "docs/sub/b.md"]
    );
    let a = scan.files.iter().find(|x| x.path == "docs/a.md").unwrap();
    assert_eq!(a.size, 12);
    assert!(a.modified_ms > 1_500_000_000_000, "{}", a.modified_ms);
    // every file knows where it is, whatever its kind
    assert!(scan.files.iter().all(|x| x.abs.is_some()));
    assert_eq!(
        a.abs.as_deref(),
        Some(f.dir.path().join("docs/a.md").as_path())
    );
    // the project folder itself: everything the tree would list
    let all = folder_scan(&f, "", &mut |_| false).unwrap();
    assert!(folder_paths(&all).contains(&"top.md"));
    assert!(!folder_paths(&all).contains(&"node_modules/x/y.md"));
}

#[cfg(unix)]
#[test]
fn a_folder_scan_leaves_out_links_to_files() {
    let f = fixture(&["docs/a.md"]);
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.md"), "outside").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("secret.md"),
        f.dir.path().join("docs/link.md"),
    )
    .unwrap();
    let scan = folder_scan(&f, "docs", &mut |_| false).unwrap();
    assert_eq!(folder_paths(&scan), ["docs/a.md"]);
}

#[test]
fn a_folder_scan_ends_when_the_caller_says_so_and_when_cancelled() {
    let files: Vec<String> = (0..20).map(|n| format!("d{}/f{n}.md", n % 4)).collect();
    let names: Vec<&str> = files.iter().map(String::as_str).collect();
    let f = fixture(&names);
    let mut seen = 0;
    let scan = folder_scan(&f, "", &mut |_| {
        seen += 1;
        seen == 7
    })
    .unwrap();
    assert!(scan.cut_off);
    assert_eq!(scan.files.len(), 7, "no file after the one that ended it");
    assert_eq!(seen, 7);
    let cancelled = std::sync::atomic::AtomicBool::new(true);
    let path = crate::paths::VaultPath::parse("d1").unwrap();
    let abs = f.dir.path().join("d1");
    let scan = f
        .explorer
        .scan_folder(&path, &abs, &cancelled, &mut |_| false)
        .unwrap();
    assert!(scan.cut_off);
    assert_eq!(scan.files.len(), 0);
}

#[test]
fn a_folder_scan_of_an_ignored_folder_gives_only_the_repositories_nested_in_it() {
    let f = fixture(&[
        ".gitignore",
        "build/out.md",
        "build/inner/.git/config",
        "build/inner/x.md",
        "build/plain/y.md",
    ]);
    fs::write(
        f.dir.path().join(".gitignore"),
        "build/
",
    )
    .unwrap();
    let scan = folder_scan(&f, "build", &mut |_| false).unwrap();
    assert_eq!(folder_paths(&scan), ["build/inner/x.md"]);
}

#[test]
fn a_folder_scan_refuses_what_is_no_folder_or_belongs_to_no_root() {
    let f = fixture(&["a.md"]);
    assert!(matches!(
        folder_scan(&f, "a.md", &mut |_| false),
        Err(TreeError::NotAFolder(_))
    ));
    assert!(matches!(
        folder_scan(&f, "missing", &mut |_| false),
        Err(TreeError::NotAFolder(_))
    ));
    let never = std::sync::atomic::AtomicBool::new(false);
    let path = crate::paths::VaultPath::parse("@wt/none/x").unwrap();
    assert!(matches!(
        f.explorer
            .scan_folder(&path, f.dir.path(), &never, &mut |_| false),
        Err(TreeError::NotAFolder(_))
    ));
}

#[test]
fn linked_assets_are_shown_wherever_they_sit_and_leave_when_the_link_goes() {
    let f = fixture(&[
        "docs/a.md",
        "far/deep/er/p.png",
        "far/deep/er/q.png",
        "docs/near.png",
    ]);
    let shown = |name: &str| -> Vec<String> {
        f.explorer
            .open_view(name, None)
            .nodes
            .into_iter()
            .map(|n| n.path)
            .collect()
    };
    let before = shown("w1");
    assert!(before.contains(&"docs/near.png".to_owned()));
    assert!(
        !before.contains(&"far/deep/er/p.png".to_owned()),
        "far from every document"
    );
    f.sink.take();
    f.explorer.set_linked(Arc::new(
        [key_of("far/deep/er/p.png")].into_iter().collect(),
    ));
    let after = shown("w2");
    assert!(after.contains(&"far/deep/er/p.png".to_owned()));
    assert!(
        !after.contains(&"far/deep/er/q.png".to_owned()),
        "an unlinked sibling stays out"
    );
    let sent = deltas(&f, "w1");
    assert_eq!(sent.len(), 1, "the open view heard it");
    assert!(sent[0]
        .added
        .iter()
        .any(|a| a.node.path == "far/deep/er/p.png"));
    // the same set again says nothing; an empty set takes the picture away
    f.explorer.set_linked(Arc::new(
        [key_of("far/deep/er/p.png")].into_iter().collect(),
    ));
    assert_eq!(deltas(&f, "w1").len(), 0);
    f.explorer.set_linked(Arc::new(HashSet::new()));
    let gone = deltas(&f, "w1");
    assert!(
        gone[0].removed.contains(&"far/deep/er/p.png".to_owned()),
        "{gone:?}"
    );
    // a filter that shows everything is unaffected; documents-only never shows assets
    let docs_only = TreeFilter {
        show: FilterShow::Documents,
        ..TreeFilter::default()
    };
    f.explorer.set_linked(Arc::new(
        [key_of("far/deep/er/p.png")].into_iter().collect(),
    ));
    let only: Vec<String> = f
        .explorer
        .open_view("w3", Some(docs_only))
        .nodes
        .into_iter()
        .map(|n| n.path)
        .collect();
    assert!(!only.contains(&"far/deep/er/p.png".to_owned()));
}
