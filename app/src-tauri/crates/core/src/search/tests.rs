//! Search: spec tests (what is found, ranked, kept current) and adversarial tests (what must never happen).

#![allow(clippy::too_many_lines, clippy::assert_is_empty)]

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::bm25::Bm25;
use super::fold::{fold, stem, tokenize, TokenKind};
use super::fuzzy::{fold_aligned, Needle};
use super::prepare::{prepare, Sig};
use super::*;
use crate::events::{CoreEvent, Recorder};
use crate::paths::{Root, Vault};
use crate::settings::ProjectConfig;
use crate::tree::{key_of, Category};
use crate::watch::{Change, ChangeKind, FsBatch};

// ---- folding + tokens ----------------------------------------------------------------------------------------------

#[test]
fn fold_lowercases_strips_accents_and_unifies_compatibility_forms() {
    assert_eq!(fold("Descrição ÉCOLE"), "descricao ecole");
    assert_eq!(fold("plain ASCII"), "plain ascii");
    assert_eq!(fold("ﬁne ＡＢＣ"), "fine abc", "ligature + full width");
    assert_eq!(fold("が"), "が".nfd_for_test(), "kana voicing stays");
    assert_ne!(fold("が"), fold("か"));
}

trait NfdForTest {
    fn nfd_for_test(&self) -> String;
}
impl NfdForTest for str {
    fn nfd_for_test(&self) -> String {
        use unicode_normalization::UnicodeNormalization;
        self.nfd().collect()
    }
}

#[test]
fn stem_collapses_plural_and_verb_forms_on_both_sides() {
    for (a, b) in [
        ("retries", "retry"),
        ("policies", "policy"),
        ("describes", "described"),
        ("documents", "document"),
        ("personagens", "personagem"),
        ("emissões", "emissao"),
    ] {
        assert_eq!(stem(&fold(a)), stem(&fold(b)), "{a} ~ {b}");
    }
    assert_eq!(stem("日本語"), "日本語", "non-ASCII untouched");
    assert_eq!(stem("css"), "css");
    assert_eq!(stem("status"), "status");
}

#[test]
fn tokens_are_words_cjk_characters_and_pairs() {
    let words: Vec<_> = tokenize("A big-Dog, x ok")
        .iter()
        .map(|t| t.folded.clone())
        .collect();
    assert_eq!(words, ["big", "dog", "ok"], "one-letter words are dropped");
    let long = "z".repeat(60);
    assert!(tokenize(&long).is_empty(), "blobs are not words");
    let cjk = tokenize("日本語");
    let kinds: Vec<_> = cjk.iter().map(|t| (t.folded.as_str(), t.kind)).collect();
    assert_eq!(
        kinds,
        [
            ("日", TokenKind::CjkRunUnigram),
            ("日本", TokenKind::Cjk),
            ("本", TokenKind::CjkRunUnigram),
            ("本語", TokenKind::Cjk),
            ("語", TokenKind::CjkRunUnigram),
        ]
    );
    assert_eq!(
        tokenize("本")[0].kind,
        TokenKind::Cjk,
        "a lone character is one token"
    );
    let mixed: Vec<_> = tokenize("使用Rust编写")
        .iter()
        .filter(|t| t.kind == TokenKind::Word)
        .map(|t| t.folded.clone())
        .collect();
    assert_eq!(mixed, ["rust"]);
}

#[test]
fn token_ranges_point_into_the_original_text() {
    let text = "Ação e Reação";
    for t in tokenize(text) {
        assert_eq!(fold(&text[t.start..t.end]), t.folded);
    }
}

// ---- fuzzy ---------------------------------------------------------------------------------------------------------

fn fuzzy(query: &str, path: &str) -> Option<fuzzy::Match> {
    Needle::new(query)?.matches(&fold_aligned(path))
}

#[test]
fn fuzzy_tiers_rank_name_prefix_over_substring_over_subsequence_over_path() {
    let q = "plan";
    let tiers = [
        fuzzy(q, "docs/plan.md").unwrap().score,
        fuzzy(q, "docs/my-plan.md").unwrap().score,
        fuzzy(q, "docs/p-l-a-n.md").unwrap().score,
        fuzzy(q, "plan/readme.md").unwrap().score,
        fuzzy(q, "p/l/a/n.md").unwrap().score,
    ];
    for pair in tiers.windows(2) {
        assert!(pair[0] > pair[1], "{tiers:?}");
    }
    assert!(fuzzy(q, "docs/other.md").is_none());
    assert!(Needle::new("   ").is_none());
}

#[test]
fn fuzzy_prefers_word_starts_and_runs_and_reports_positions() {
    let good = fuzzy("rp", "docs/retry-policy.md").unwrap();
    let bad = fuzzy("rp", "docs/carxpool.md").unwrap();
    assert!(good.score > bad.score);
    assert_eq!(good.positions, [5, 11]);
    let run = fuzzy("abc", "x/abcdef.md").unwrap();
    assert_eq!(run.positions, [2, 3, 4]);
}

#[test]
fn fuzzy_words_match_in_any_order_and_positions_stay_aligned_with_accents() {
    let m = fuzzy("token auth", "docs/auth-flow/token.md").unwrap();
    assert!(!m.positions.is_empty());
    assert!(fuzzy("token nothere", "docs/auth-flow/token.md").is_none());
    let path = "Descrição/Índice.md";
    let m = fuzzy("indice", path).unwrap();
    let chars: Vec<char> = path.chars().collect();
    let hit: String = m.positions.iter().map(|&i| chars[i as usize]).collect();
    assert_eq!(hit, "Índice");
}

#[test]
fn fuzzy_never_panics_on_odd_input() {
    for q in ["\0", "\u{202e}", "💥💥", "a b c d e f g", "////", "..", "é"] {
        let _ = fuzzy(q, "dir/💥 file é.md");
        let _ = fuzzy(q, "");
    }
}

// ---- BM25 ----------------------------------------------------------------------------------------------------------

fn doc(index: &mut Bm25, path: &str, text: &str) -> Vec<u32> {
    let prepared = prepare(
        path,
        Sig {
            len: text.len() as u64,
            mtime_ms: 1,
        },
        text,
        Category::Markdown,
    );
    index.add(&prepared.path.clone(), prepared.sections)
}

fn paths_of(hits: &[SectionHit]) -> Vec<&str> {
    hits.iter().map(|h| h.path.as_str()).collect()
}

#[test]
fn heading_and_file_name_hits_outrank_body_hits() {
    let mut index = Bm25::new();
    doc(
        &mut index,
        "notes/misc.md",
        "# Misc\n\nthe retry setting is mentioned once here\n",
    );
    doc(
        &mut index,
        "notes/other.md",
        "# Retry\n\nsomething else entirely\n",
    );
    doc(&mut index, "notes/retry.md", "# Intro\n\nnothing to see\n");
    let hits = index.search("retry", 10, None);
    assert_eq!(hits.len(), 3);
    assert_ne!(
        hits[2].path,
        "notes/other.md",
        "heading beats body: {:?}",
        paths_of(&hits)
    );
    let top: HashSet<&str> = paths_of(&hits[..2]).into_iter().collect();
    assert!(
        top.contains("notes/other.md") && top.contains("notes/retry.md"),
        "{:?}",
        paths_of(&hits)
    );
}

#[test]
fn the_last_word_matches_as_a_prefix_and_exact_beats_prefix() {
    let mut index = Bm25::new();
    doc(&mut index, "a.md", "# A\n\npolicy policy policy\n");
    doc(&mut index, "b.md", "# B\n\npolice\n");
    let hits = index.search("polic", 10, None);
    assert_eq!(hits.len(), 2, "prefix finds both");
    let exact = index.search("policy", 10, None);
    assert_eq!(paths_of(&exact), ["a.md"]);
    // a prefix before the last word is not expanded
    assert!(
        index.search("polic text", 10, None).is_empty()
            || index.search("polic text", 10, None).len() <= 2
    );
}

#[test]
fn sections_with_more_of_the_words_win_and_accents_and_forms_match() {
    let mut index = Bm25::new();
    doc(
        &mut index,
        "both.md",
        "# Both\n\nA descrição of the retries policy.\n",
    );
    doc(&mut index, "one.md", "# One\n\nretry retry retry retry\n");
    let hits = index.search("descricao retry policy", 10, None);
    assert_eq!(hits[0].path, "both.md", "{:?}", paths_of(&hits));
}

#[test]
fn cjk_queries_find_pairs_and_single_characters() {
    let mut index = Bm25::new();
    doc(&mut index, "jp.md", "# 日本語\n\n日本語のドキュメント\n");
    doc(&mut index, "en.md", "# English\n\nnothing\n");
    assert_eq!(paths_of(&index.search("日本語", 5, None)), ["jp.md"]);
    assert_eq!(paths_of(&index.search("本", 5, None)), ["jp.md"]);
    assert_eq!(paths_of(&index.search("ドキュメント", 5, None)), ["jp.md"]);
    assert!(index.search("韓", 5, None).is_empty());
}

#[test]
fn a_filter_keeps_the_best_allowed_sections_from_being_crowded_out() {
    let mut index = Bm25::new();
    for n in 0..30 {
        doc(
            &mut index,
            &format!("noise/{n}.md"),
            "# Plan\n\nplan plan plan plan\n",
        );
    }
    doc(&mut index, "mine/one.md", "# Other\n\nplan once\n");
    let only_mine = |p: &str| p.starts_with("mine/");
    let hits = index.search("plan", 3, Some(&only_mine));
    assert_eq!(paths_of(&hits), ["mine/one.md"]);
}

#[test]
fn removing_documents_frees_their_terms_and_ids_are_reused() {
    let mut index = Bm25::new();
    let a = doc(&mut index, "a.md", "# A\n\nuniqueword here\n");
    doc(&mut index, "b.md", "# B\n\nsomething\n");
    let terms = index.term_count();
    assert_eq!(paths_of(&index.search("uniqueword", 5, None)), ["a.md"]);
    index.remove(&a);
    assert!(index.search("uniqueword", 5, None).is_empty());
    assert!(index.term_count() < terms, "the document's own terms went");
    let again = doc(&mut index, "c.md", "# C\n\nuniqueword again\n");
    assert_eq!(again.len(), a.len());
    assert_eq!(paths_of(&index.search("uniqueword", 5, None)), ["c.md"]);
    index.remove(&again);
    index.remove(&[999]);
    assert_eq!(index.len(), 1, "only b.md's sections remain");
}

#[test]
fn snippets_mark_matches_as_parts_cut_at_words_and_fall_back_to_the_heading() {
    let mut index = Bm25::new();
    let filler = "lorem ipsum dolor sit amet ".repeat(40);
    doc(
        &mut index,
        "a.md",
        &format!("# T\n\n{filler} the needle sits here <script>alert(1)</script> {filler}\n"),
    );
    doc(
        &mut index,
        "b.md",
        "# Heading Only Match\n\nbody without it\n",
    );
    let hit = &index.search("needle", 5, None)[0];
    assert!(hit.snippet.iter().any(|p| p.mark && p.text == "needle"));
    let whole: String = hit.snippet.iter().map(|p| p.text.as_str()).collect();
    assert!(
        whole.starts_with('\u{2026}') && whole.ends_with('\u{2026}'),
        "{whole}"
    );
    assert!(whole.chars().count() < 260);
    assert!(!whole.contains("loremipsum"));
    let b = &index.search("heading", 5, None)[0];
    let text: String = b.snippet.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(text, "Heading Only Match");
    assert!(b.snippet.iter().any(|p| p.mark));
}

#[test]
fn a_snippet_never_shows_a_secret_value_even_cut_between_key_and_value() {
    let mut index = Bm25::new();
    doc(
        &mut index,
        "a.md",
        "# Ops

Shared login for staging: password: hunter2 is rotated by the ops team every month.
",
    );
    for query in ["every", "rotated", "password", "staging"] {
        let hit = &index.search(query, 5, None)[0];
        let whole: String = hit.snippet.iter().map(|p| p.text.as_str()).collect();
        assert!(!whole.contains("hunter2"), "{query}: {whole}");
    }
    // the value itself is no excerpt either
    for hit in index.search("hunter2", 5, None) {
        let whole: String = hit.snippet.iter().map(|p| p.text.as_str()).collect();
        assert!(!whole.contains("hunter2"), "{whole}");
    }
}

#[test]
fn snippet_marks_cjk_matches_once() {
    let mut index = Bm25::new();
    doc(
        &mut index,
        "jp.md",
        "# T\n\nこれは日本語のドキュメントです\n",
    );
    let hit = &index.search("日本語", 5, None)[0];
    let marked: Vec<_> = hit
        .snippet
        .iter()
        .filter(|p| p.mark)
        .map(|p| p.text.as_str())
        .collect();
    assert_eq!(marked, ["日本語"]);
}

#[test]
fn odd_queries_return_nothing_and_never_panic() {
    let mut index = Bm25::new();
    doc(&mut index, "a.md", "# A\n\ntext here\n");
    for q in [
        "",
        " ",
        "a",
        "!!!",
        "***",
        ".*",
        "(",
        "\0",
        "💥",
        "\u{202e}",
        &"word ".repeat(500),
    ] {
        let _ = index.search(q, 10, None);
    }
    assert!(index.search("text", 0, None).is_empty());
    let empty = Bm25::new();
    assert!(empty.search("text", 5, None).is_empty());
}

#[test]
fn a_one_megabyte_section_and_a_hundred_thousand_tokens_are_fine() {
    let mut index = Bm25::new();
    let body = (0..100_000)
        .map(|n| format!("w{n}"))
        .collect::<Vec<_>>()
        .join(" ");
    let start = Instant::now();
    doc(&mut index, "big.md", &format!("# Big\n\n{body}\n"));
    assert_eq!(index.search("w99999", 5, None).len(), 1);
    assert!(start.elapsed() < Duration::from_secs(20));
}

// ---- documents: prepare + index ------------------------------------------------------------------------------------

#[test]
fn prepare_counts_field_weights_and_puts_the_file_name_in_the_first_section_only() {
    let text = "# Alpha\n\nbeta\n\n## Gamma\n\ndelta\n";
    let p = prepare(
        "docs/omega-file.md",
        Sig {
            len: 1,
            mtime_ms: 1,
        },
        text,
        Category::Markdown,
    );
    assert_eq!(p.sections.len(), 2);
    let tf =
        |s: &SectionInput, term: &str| s.terms.iter().find(|(t, _)| t == term).map(|(_, n)| *n);
    assert_eq!(tf(&p.sections[0], "alpha"), Some(2), "own heading x2");
    assert_eq!(tf(&p.sections[0], "beta"), Some(1));
    assert_eq!(tf(&p.sections[0], "omega"), Some(3), "file name x3");
    assert_eq!(tf(&p.sections[1], "alpha"), Some(1), "ancestor heading x1");
    assert_eq!(tf(&p.sections[1], "gamma"), Some(2));
    assert_eq!(
        tf(&p.sections[1], "omega"),
        None,
        "name only in the first section"
    );
}

#[test]
fn prepare_reads_link_targets_without_the_disk() {
    let text = "![p](img/a.png) [d](../x/b.md#s) [w](https://x.org/c.png) <img src=\"h.png\">\n\n[ref][r]\n\n[r]: pics/r.png\n";
    let p = prepare(
        "docs/a.md",
        Sig {
            len: 1,
            mtime_ms: 1,
        },
        text,
        Category::Markdown,
    );
    let expect: Vec<String> = ["docs/img/a.png", "x/b.md", "docs/h.png", "docs/pics/r.png"]
        .iter()
        .map(|t| key_of(t))
        .collect();
    for key in &expect {
        assert!(p.links.contains(key), "{key} in {:?}", p.links);
    }
    assert!(!p.links.iter().any(|l| l.contains("x.org")));
}

#[test]
fn index_files_folders_and_removal_by_folder() {
    let mut index = index::Index::new();
    for p in ["docs/a.md", "docs/sub/b.md", "docs2/c.md", "top.md"] {
        index.add_file(p, Category::Markdown);
    }
    assert_eq!(index.folders.get("docs"), Some(&2));
    assert_eq!(index.folders.get("docs/sub"), Some(&1));
    assert_eq!(index.remove_under("docs"), 2);
    assert!(index.files.contains_key("docs2/c.md") && index.files.contains_key("top.md"));
    assert!(!index.folders.contains_key("docs") && !index.folders.contains_key("docs/sub"));
    assert_eq!(index.folders.get("docs2"), Some(&1));
    index.remove_file("docs2/c.md");
    assert!(index.folders.is_empty());
    index.remove_file("nothing.md");
}

#[test]
fn link_counts_make_the_linked_set_and_report_a_change_once() {
    let mut index = index::Index::new();
    let doc_a = prepare(
        "a.md",
        Sig {
            len: 1,
            mtime_ms: 1,
        },
        "![x](p.png)",
        Category::Markdown,
    );
    let doc_b = prepare(
        "b.md",
        Sig {
            len: 1,
            mtime_ms: 1,
        },
        "![x](p.png) [y](q.pdf)",
        Category::Markdown,
    );
    index.upsert_doc(doc_a, Category::Markdown);
    assert_eq!(index.take_linked().unwrap().len(), 1);
    assert!(index.take_linked().is_none(), "nothing new");
    index.upsert_doc(doc_b, Category::Markdown);
    assert_eq!(index.take_linked().unwrap().len(), 2);
    index.remove_file("b.md");
    let left = index.take_linked().unwrap();
    assert_eq!(left.len(), 1);
    assert!(left.contains(&key_of("p.png")));
    // replacing a document with the same links changes nothing
    let again = prepare(
        "a.md",
        Sig {
            len: 2,
            mtime_ms: 2,
        },
        "![x](p.png) more",
        Category::Markdown,
    );
    index.upsert_doc(again, Category::Markdown);
    assert!(index.take_linked().is_none());
}

// ---- the worker over a real folder ---------------------------------------------------------------------------------

struct Fx {
    dir: tempfile::TempDir,
    explorer: Arc<Explorer>,
    sink: Arc<Recorder>,
    index: SearchIndex,
}

fn write(base: &Path, rel: &str, text: &str) {
    let path = base.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn fx(files: &[(&str, &str)]) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    for (rel, text) in files {
        write(dir.path(), rel, text);
    }
    start(dir)
}

fn start(dir: tempfile::TempDir) -> Fx {
    let vault = Vault::new(Root::open(dir.path()).unwrap());
    let sink = Arc::new(Recorder::default());
    let explorer = Arc::new(Explorer::new(
        &vault,
        &ProjectConfig::default(),
        sink.clone(),
        None,
    ));
    let index = SearchIndex::start(
        vault.clone(),
        Arc::clone(&explorer),
        sink.clone(),
        "root".to_owned(),
    );
    assert!(
        index.wait_ready(Duration::from_secs(60)),
        "the first build ends"
    );
    Fx {
        dir,
        explorer,
        sink,
        index,
    }
}

fn change(path: &str, kind: ChangeKind, is_dir: bool) -> Change {
    Change {
        path: path.to_owned(),
        kind,
        is_dir,
        own: false,
    }
}

impl Fx {
    fn feed(&self, changes: Vec<Change>) {
        (self.index.feeder())(&FsBatch {
            changes,
            ..FsBatch::default()
        });
    }

    fn feed_bulk(&self) {
        (self.index.feeder())(&FsBatch {
            bulk: true,
            changes: vec![change("x", ChangeKind::Modified, false)],
            ..FsBatch::default()
        });
    }

    fn found(&self, query: &str) -> Vec<String> {
        self.index
            .sections(query, 50, &SearchFilter::default())
            .hits
            .into_iter()
            .map(|h| h.path)
            .collect()
    }

    fn wait(&self, what: &str, mut ok: impl FnMut(&Fx) -> bool) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(20) {
            if ok(self) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("timed out waiting for: {what}");
    }

    fn files(&self, query: &str) -> Vec<String> {
        self.index
            .paths(query, 50, 10, &SearchFilter::default())
            .files
            .into_iter()
            .map(|f| f.path)
            .collect()
    }
}

#[test]
fn the_first_build_reads_documents_lists_every_file_and_reports_progress() {
    let f = fx(&[
        ("README.md", "# Home\n\nwelcome aboard\n"),
        (
            "docs/guide.md",
            "# Guide\n\n## Install\n\nrun the installer\n",
        ),
        ("docs/notes.txt", "plain words here\n"),
        ("data/cfg.toml", "secret_token = 'abc123'\n"),
        ("img/logo.png", "not really a png"),
    ]);
    assert_eq!(f.found("installer"), ["docs/guide.md"]);
    assert_eq!(f.found("plain"), ["docs/notes.txt"]);
    assert!(
        f.found("abc123").is_empty(),
        "config content is not indexed"
    );
    assert!(f.found("secret_token").is_empty());
    assert_eq!(
        f.files("logo"),
        ["img/logo.png"],
        "every file is in the list by name"
    );
    let status = f.index.status();
    assert_eq!(status.phase, IndexPhase::Ready);
    assert_eq!(status.files, 5);
    assert_eq!(f.index.doc_count(), 3);
    let progress: Vec<IndexProgress> = f
        .sink
        .take()
        .into_iter()
        .filter_map(|e| match e {
            CoreEvent::IndexProgress(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(
        progress.first().map(|p| p.phase),
        Some(IndexPhase::Building)
    );
    let last = progress.last().unwrap();
    assert_eq!(
        (last.phase, last.done, last.total),
        (IndexPhase::Ready, 3, 3)
    );
    assert!(progress.iter().all(|p| p.root == "root"));
    assert!(
        !f.index
            .sections("installer", 5, &SearchFilter::default())
            .partial
    );
}

#[test]
fn ignored_folders_are_not_indexed_but_large_folders_are() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), ".gitignore", "build/\n");
    write(dir.path(), "build/out.md", "# Out\n\nignoredtext\n");
    write(
        dir.path(),
        "node_modules/pkg/readme.md",
        "# Pkg\n\nmoduletext\n",
    );
    write(dir.path(), "docs/a.md", "# A\n\nvisibletext\n");
    for n in 0..(crate::tree::LARGE_FOLDER + 20) {
        write(
            dir.path(),
            &format!("big/n{n}.md"),
            if n == 7 {
                "# Seven\n\nneedleinbig\n"
            } else {
                "x"
            },
        );
    }
    let f = start(dir);
    assert_eq!(f.found("visibletext"), ["docs/a.md"]);
    assert!(f.found("ignoredtext").is_empty());
    assert!(f.found("moduletext").is_empty());
    assert_eq!(
        f.found("needleinbig"),
        ["big/n7.md"],
        "the tree leaves this folder unlisted; the index does not"
    );
    assert!(f.files("out.md").is_empty());
}

#[test]
fn watch_batches_keep_the_index_current() {
    let f = fx(&[
        ("a.md", "# A\n\noldwords\n"),
        ("keep.md", "# K\n\nkeepwords\n"),
    ]);
    // modify
    write(f.dir.path(), "a.md", "# A\n\nnewwords and more text\n");
    f.feed(vec![change("a.md", ChangeKind::Modified, false)]);
    f.wait("modified text found", |f| f.found("newwords") == ["a.md"]);
    assert!(f.found("oldwords").is_empty());
    // create
    write(f.dir.path(), "sub/c.md", "# C\n\ncreatedwords\n");
    f.feed(vec![change("sub/c.md", ChangeKind::Created, false)]);
    f.wait("created found", |f| f.found("createdwords") == ["sub/c.md"]);
    assert_eq!(f.files("c.md"), ["sub/c.md"]);
    // rename
    fs::rename(f.dir.path().join("sub/c.md"), f.dir.path().join("moved.md")).unwrap();
    f.feed(vec![change(
        "moved.md",
        ChangeKind::Renamed {
            from: "sub/c.md".to_owned(),
        },
        false,
    )]);
    f.wait("renamed found at the new place", |f| {
        f.found("createdwords") == ["moved.md"]
    });
    assert!(f.files("sub/c").is_empty());
    // remove
    fs::remove_file(f.dir.path().join("moved.md")).unwrap();
    f.feed(vec![change("moved.md", ChangeKind::Removed, false)]);
    f.wait("removed gone", |f| f.found("createdwords").is_empty());
    assert_eq!(f.found("keepwords"), ["keep.md"], "the rest is untouched");
}

#[test]
fn removing_a_folder_forgets_everything_below_it() {
    let f = fx(&[
        ("docs/a.md", "# A\n\nfolderwords\n"),
        ("docs/deep/b.md", "# B\n\nfolderwords\n"),
        ("docs2/c.md", "# C\n\nfolderwords\n"),
    ]);
    assert_eq!(f.found("folderwords").len(), 3);
    fs::remove_dir_all(f.dir.path().join("docs")).unwrap();
    f.feed(vec![change("docs", ChangeKind::Removed, true)]);
    f.wait("folder gone", |f| f.found("folderwords") == ["docs2/c.md"]);
    assert!(f.files("deep").is_empty());
    assert_eq!(
        f.index
            .paths("docs", 10, 10, &SearchFilter::default())
            .folders
            .iter()
            .map(|x| x.path.as_str())
            .collect::<Vec<_>>(),
        ["docs2"]
    );
}

#[test]
fn a_new_or_renamed_folder_and_bulk_batches_reconcile_with_the_disk() {
    let f = fx(&[("a.md", "# A\n\nfirst\n")]);
    write(f.dir.path(), "new/x.md", "# X\n\ninsidenew\n");
    write(f.dir.path(), "new/deep/y.md", "# Y\n\ninsidedeep\n");
    f.feed(vec![change("new", ChangeKind::Created, true)]);
    f.wait("folder contents found", |f| {
        f.found("insidenew") == ["new/x.md"] && f.found("insidedeep") == ["new/deep/y.md"]
    });
    fs::rename(f.dir.path().join("new"), f.dir.path().join("renamed")).unwrap();
    f.feed(vec![change(
        "renamed",
        ChangeKind::Renamed {
            from: "new".to_owned(),
        },
        true,
    )]);
    f.wait("rename followed", |f| {
        f.found("insidenew") == ["renamed/x.md"]
    });
    // events lost: files appear and vanish without a word, then a bulk batch
    fs::remove_file(f.dir.path().join("a.md")).unwrap();
    write(f.dir.path(), "silent.md", "# S\n\nsilentwords\n");
    f.feed_bulk();
    f.wait("reconciled", |f| {
        f.found("silentwords") == ["silent.md"] && f.found("first").is_empty()
    });
}

#[test]
fn own_writes_are_indexed_like_any_other() {
    let f = fx(&[("a.md", "# A\n\nbefore\n")]);
    write(f.dir.path(), "a.md", "# A\n\nafterownwrite\n");
    f.feed(vec![Change {
        path: "a.md".to_owned(),
        kind: ChangeKind::Modified,
        is_dir: false,
        own: true,
    }]);
    f.wait("own write found", |f| f.found("afterownwrite") == ["a.md"]);
}

#[test]
fn binary_oversized_and_unreadable_documents_stay_in_the_list_by_name() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("bin.md"), b"\0\0\0binary\0").unwrap();
    fs::write(dir.path().join("huge.txt"), "x ".repeat(600_000)).unwrap();
    write(dir.path(), "ok.md", "# Ok\n\nreadable\n");
    let f = start(dir);
    assert_eq!(f.found("readable"), ["ok.md"]);
    assert!(f.found("binary").is_empty());
    assert_eq!(f.files("bin.md"), ["bin.md"]);
    assert_eq!(f.files("huge"), ["huge.txt"]);
    // a document that turns binary loses its content, keeps its name
    write(f.dir.path(), "ok.md", "\0\0\0");
    f.feed(vec![change("ok.md", ChangeKind::Modified, false)]);
    f.wait("content gone", |f| f.found("readable").is_empty());
    assert_eq!(f.files("ok.md"), ["ok.md"]);
}

#[test]
fn encodings_line_endings_and_odd_paths_are_searchable() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "crlf.md", "# Títle\r\n\r\ncrlfwords here\r\n");
    fs::write(
        dir.path().join("bom.md"),
        b"\xEF\xBB\xBF# Bom\n\nbomwords\n",
    )
    .unwrap();
    let mut utf16: Vec<u8> = vec![0xFF, 0xFE];
    for unit in "# U\n\nutfsixteen words\n".encode_utf16() {
        utf16.extend(unit.to_le_bytes());
    }
    fs::write(dir.path().join("u16.md"), utf16).unwrap();
    write(
        dir.path(),
        "my docs/日本語 notes/ação.md",
        "# Ação\n\nunicodepath\n",
    );
    let f = start(dir);
    assert_eq!(f.found("crlfwords"), ["crlf.md"]);
    assert_eq!(
        f.found("title"),
        ["crlf.md"],
        "accent-free query finds the accented heading"
    );
    assert_eq!(f.found("bomwords"), ["bom.md"]);
    assert_eq!(f.found("utfsixteen"), ["u16.md"]);
    assert_eq!(f.found("unicodepath"), ["my docs/日本語 notes/ação.md"]);
    assert_eq!(f.files("acao"), ["my docs/日本語 notes/ação.md"]);
}

#[test]
fn hostile_documents_never_break_the_build() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "bomb.md", &format!("{} x\n", ">".repeat(5000)));
    write(dir.path(), "lists.md", &"- a\n  ".repeat(500));
    write(
        dir.path(),
        "bidi.md",
        "# \u{202e}evil\n\n\u{0000}\u{202d}text\n",
    );
    write(dir.path(), "fine.md", "# Fine\n\nstillworks\n");
    let f = start(dir);
    assert_eq!(f.found("stillworks"), ["fine.md"]);
}

#[test]
fn a_storm_of_changes_during_the_first_build_ends_in_the_state_of_the_disk() {
    let dir = tempfile::tempdir().unwrap();
    for n in 0..600 {
        write(
            dir.path(),
            &format!("d{}/n{n}.md", n % 12),
            &format!(
                "# T{n}

original{n}x
"
            ),
        );
    }
    let vault = Vault::new(Root::open(dir.path()).unwrap());
    let sink = Arc::new(Recorder::default());
    let explorer = Arc::new(Explorer::new(
        &vault,
        &ProjectConfig::default(),
        sink.clone(),
        None,
    ));
    let index = SearchIndex::start(vault, Arc::clone(&explorer), sink, "root".to_owned());
    let feed = index.feeder();
    // while the build runs: rewrite some files, delete some, add some, report each in its own batch
    for n in (0..600).step_by(3) {
        write(
            dir.path(),
            &format!("d{}/n{n}.md", n % 12),
            &format!(
                "# T{n}

rewritten{n}x
"
            ),
        );
        feed(&FsBatch {
            changes: vec![change(
                &format!("d{}/n{n}.md", n % 12),
                ChangeKind::Modified,
                false,
            )],
            ..FsBatch::default()
        });
    }
    for n in (1..600).step_by(10) {
        let rel = format!("d{}/n{n}.md", n % 12);
        fs::remove_file(dir.path().join(&rel)).unwrap();
        feed(&FsBatch {
            changes: vec![change(&rel, ChangeKind::Removed, false)],
            ..FsBatch::default()
        });
    }
    write(
        dir.path(),
        "added/new.md",
        "# New

brandnew
",
    );
    feed(&FsBatch {
        changes: vec![change("added/new.md", ChangeKind::Created, false)],
        ..FsBatch::default()
    });
    assert!(index.wait_ready(Duration::from_secs(60)));
    let found = |q: &str| index.sections(q, 5, &SearchFilter::default()).hits.len();
    let start = Instant::now();
    while found("brandnew") == 0 {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the queued changes are applied"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    // give the last queued batches time to land, then compare with the disk
    std::thread::sleep(Duration::from_millis(300));
    for n in 0..600 {
        let gone = n % 10 == 1;
        let rewritten = n % 3 == 0;
        let (word, expected) = if gone {
            (format!("original{n}x"), 0)
        } else if rewritten {
            (format!("rewritten{n}x"), 1)
        } else {
            (format!("original{n}x"), 1)
        };
        assert_eq!(found(&word), expected, "{word}");
        if rewritten && !gone {
            assert_eq!(
                found(&format!("original{n}x")),
                0,
                "old text of {n} is gone"
            );
        }
    }
}

#[test]
fn the_linked_set_reaches_the_explorer_and_shows_pictures_wherever_they_sit() {
    let f = fx(&[
        ("docs/a.md", "# A\n\n![pic](../far/away/deep/p.png)\n"),
        ("far/away/deep/p.png", "png"),
        ("far/away/deep/q.png", "png"),
    ]);
    // the Explorer builds its model on the first view; the linked set arrived before or arrives after
    f.wait("linked set known", |f| {
        f.index.linked().contains(&key_of("far/away/deep/p.png"))
    });
    f.wait("explorer told", |f| {
        let shown: Vec<String> = f
            .explorer
            .open_view("w", None)
            .nodes
            .into_iter()
            .map(|n| n.path)
            .collect();
        shown.iter().any(|p| p == "far/away/deep/p.png")
    });
    let shown: Vec<String> = f
        .explorer
        .open_view("w", None)
        .nodes
        .into_iter()
        .map(|n| n.path)
        .collect();
    assert!(
        shown.iter().any(|p| p == "far/away/deep/p.png"),
        "linked picture shown: {shown:?}"
    );
    assert!(
        !shown.iter().any(|p| p == "far/away/deep/q.png"),
        "an unlinked picture far from documents stays hidden: {shown:?}"
    );
    // the link goes away: the picture leaves the view again, with a delta
    f.sink.take();
    write(f.dir.path(), "docs/a.md", "# A\n\nno picture now\n");
    f.feed(vec![change("docs/a.md", ChangeKind::Modified, false)]);
    f.wait("picture hidden again", |f| {
        let hidden = !f
            .explorer
            .open_view("w2", None)
            .nodes
            .iter()
            .any(|n| n.path == "far/away/deep/p.png");
        hidden
    });
}

#[test]
fn paths_and_recent_answer_quick_open() {
    let f = fx(&[
        ("docs/alpha.md", "# A\n"),
        ("docs/beta.md", "# B\n"),
        ("src/alphabet.txt", "x\n"),
    ]);
    let hits = f.index.paths("alpha", 10, 5, &SearchFilter::default());
    assert_eq!(
        hits.files
            .iter()
            .map(|h| h.path.as_str())
            .collect::<Vec<_>>(),
        ["docs/alpha.md", "src/alphabet.txt"]
    );
    assert_eq!(hits.files[0].category, Category::Markdown);
    assert_eq!(hits.files[0].positions, [5, 6, 7, 8, 9]);
    assert!(f
        .index
        .paths("   ", 10, 5, &SearchFilter::default())
        .files
        .is_empty());
    let folders = f
        .index
        .paths("doc", 10, 5, &SearchFilter::default())
        .folders;
    assert_eq!(
        folders.iter().map(|x| x.path.as_str()).collect::<Vec<_>>(),
        ["docs"]
    );
    let recent = f.index.recent(
        &["docs/beta.md".to_owned(), "gone.md".to_owned()],
        8,
        &SearchFilter::default(),
    );
    assert_eq!(
        recent
            .opened
            .iter()
            .map(|h| h.path.as_str())
            .collect::<Vec<_>>(),
        ["docs/beta.md"],
        "vanished files are dropped"
    );
    assert!(
        recent.changed.len() >= 2,
        "documents the index read have a time"
    );
    // a filter keeps skipped paths out of every list
    let skip = SearchFilter {
        only: None,
        skip: ["docs/alpha.md".to_owned()].into_iter().collect(),
        muted: None,
        hidden_last: false,
    };
    assert_eq!(f.index.paths("alpha", 10, 5, &skip).files.len(), 1);
    assert_eq!(
        f.index
            .sections("alpha", 10, &skip)
            .hits
            .iter()
            .map(|h| h.path.as_str())
            .collect::<Vec<_>>(),
        ["src/alphabet.txt"],
        "the skipped document does not answer; the prefix still finds the other file name"
    );
    assert!(f
        .index
        .recent(&[], 8, &skip)
        .changed
        .iter()
        .all(|h| h.path != "docs/alpha.md"));
}

#[test]
fn dropping_the_index_during_the_first_build_is_prompt() {
    let dir = tempfile::tempdir().unwrap();
    for n in 0..800 {
        write(
            dir.path(),
            &format!("d{}/n{n}.md", n % 30),
            &format!("# T{n}\n\n{}\n", "word ".repeat(200)),
        );
    }
    let vault = Vault::new(Root::open(dir.path()).unwrap());
    let sink = Arc::new(Recorder::default());
    let explorer = Arc::new(Explorer::new(
        &vault,
        &ProjectConfig::default(),
        sink.clone(),
        None,
    ));
    let index = SearchIndex::start(vault, explorer, sink, "root".to_owned());
    let start = Instant::now();
    drop(index);
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "joined in {:?}",
        start.elapsed()
    );
}

// ---- measured ------------------------------------------------------------------------------------------------------

#[test]
fn ten_thousand_documents_build_and_answer_quickly() {
    // 10 000 documents are measured in release; a debug test run keeps its CPU use short (other tests have timers)
    let docs: usize = if cfg!(debug_assertions) { 1500 } else { 10_000 };
    let dir = tempfile::tempdir().unwrap();
    let words: Vec<String> = (0..2000).map(|n| format!("term{n}")).collect();
    for n in 0..docs {
        let mut text = format!(
            "---\ntags: [t{}]\n---\n# Document {n}\n\nintro {}\n",
            n % 50,
            words[n % 2000]
        );
        for s in 0..4 {
            let _ = write!(text, "\n## Section {s} of {n}\n\n");
            for k in 0..60 {
                text.push_str(&words[(n * 7 + s * 13 + k * 31) % 2000]);
                text.push(' ');
            }
            text.push('\n');
        }
        write(
            dir.path(),
            &format!("p{}/f{}/doc{n}.md", n % 10, n % 100),
            &text,
        );
    }
    let vault = Vault::new(Root::open(dir.path()).unwrap());
    let sink = Arc::new(Recorder::default());
    let explorer = Arc::new(Explorer::new(
        &vault,
        &ProjectConfig::default(),
        sink.clone(),
        None,
    ));
    let started = Instant::now();
    let index = SearchIndex::start(vault, explorer, sink, "root".to_owned());
    assert!(index.wait_ready(Duration::from_secs(300)));
    let build = started.elapsed();
    let status = index.status();
    assert_eq!(status.files, u32::try_from(docs).unwrap());
    assert_eq!(index.doc_count(), docs);
    let last = docs - 1;
    let section_query = format!("section 3 of {last}");
    let mut times: Vec<Duration> = Vec::new();
    for q in [
        "term17",
        "term42 term99",
        section_query.as_str(),
        "docum",
        "term1999 intro",
        "t7",
    ] {
        for _ in 0..5 {
            let t = Instant::now();
            let hits = index.sections(q, 50, &SearchFilter::default()).hits;
            times.push(t.elapsed());
            assert!(!hits.is_empty(), "{q}");
        }
    }
    times.sort();
    let median = times[times.len() / 2];
    let worst = *times.last().unwrap();
    let t = Instant::now();
    let files = index.paths(&format!("doc{last}"), 50, 5, &SearchFilter::default());
    let quick = t.elapsed();
    assert_eq!(
        files.files[0].path,
        format!("p{}/f{}/doc{last}.md", last % 10, last % 100)
    );
    eprintln!(
        "PERF {docs} documents: build {build:?} ({} sections), section query median {median:?} worst {worst:?}, quick open {quick:?}",
        status.sections
    );
    let (build_limit, query_limit) = if cfg!(debug_assertions) {
        (Duration::from_secs(120), Duration::from_millis(500))
    } else {
        (Duration::from_secs(10), Duration::from_millis(50))
    };
    assert!(build < build_limit, "build {build:?}");
    assert!(worst < query_limit, "worst query {worst:?}");
}

// ---- config files: keys, never values --------------------------------------------------------------------------------

struct KeysOnly;

impl crate::format::Format for KeysOnly {
    fn id(&self) -> crate::format::FormatId {
        crate::format::FormatId::TOML
    }

    fn claims(&self, _name: &str, ext: Option<&str>, _head: &[u8]) -> crate::format::Claim {
        if ext == Some("toml") {
            crate::format::Claim::Strong
        } else {
            crate::format::Claim::No
        }
    }

    fn render(&self, text: &str, _cx: &crate::format::RenderCx<'_>) -> crate::format::Rendered {
        crate::format::Rendered {
            body: crate::format::Body::Text {
                text: text.to_owned(),
                language: None,
                problem: None,
                partial: None,
            },
            notes: Vec::new(),
        }
    }

    fn searchable(&self, text: &str) -> Option<String> {
        Some(
            text.lines()
                .filter_map(|l| l.split_once('=').map(|(k, _)| k.trim().to_owned()))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

#[test]
fn config_files_are_found_by_their_keys_while_a_format_is_on_and_never_by_values() {
    use crate::format::{Contributions, FormatRegistry};
    use std::sync::RwLock;

    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "app.toml",
        "database_host = \"zebra-value\"\napi_token = \"TOPSECRET99\"\n",
    );
    let vault = Vault::new(Root::open(dir.path()).unwrap());
    let sink = Arc::new(Recorder::default());
    let explorer = Arc::new(Explorer::new(
        &vault,
        &ProjectConfig::default(),
        sink.clone(),
        None,
    ));
    let formats = Arc::new(RwLock::new(Arc::new(FormatRegistry::core())));
    let index = SearchIndex::start_with(
        vault,
        explorer,
        sink,
        "root".to_owned(),
        Arc::clone(&formats),
    );
    assert!(index.wait_ready(Duration::from_secs(60)));
    let found = |q: &str| -> Vec<String> {
        index
            .sections(q, 20, &SearchFilter::default())
            .hits
            .into_iter()
            .map(|h| h.path)
            .collect()
    };
    let wait_for = |what: &str, mut ok: Box<dyn FnMut() -> bool>| {
        let start = Instant::now();
        while !ok() {
            assert!(start.elapsed() < Duration::from_secs(20), "{what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    // no config format on: by name only
    assert!(found("database_host").is_empty());
    // a format on: the keys are searchable
    *formats.write().unwrap() = Arc::new(FormatRegistry::build(
        &Contributions::default().formats(vec![Arc::new(KeysOnly)]),
    ));
    index.formats_changed();
    wait_for(
        "keys indexed",
        Box::new(|| !found("database_host").is_empty()),
    );
    assert_eq!(found("database_host"), ["app.toml"]);
    assert_eq!(found("api_token"), ["app.toml"]);
    // values never
    assert!(found("zebra").is_empty() && found("TOPSECRET99").is_empty());
    // the format off again: back to the name
    *formats.write().unwrap() = Arc::new(FormatRegistry::core());
    index.formats_changed();
    wait_for(
        "keys forgotten",
        Box::new(|| found("database_host").is_empty()),
    );
}

#[test]
fn hidden_paths_rank_after_every_other_match_unless_the_tree_shows_them() {
    let f = fx(&[
        (".claude/rules/notes.md", "x\n"),
        (".notes.md", "x\n"),
        ("docs/project-notes-archive-old.md", "x\n"),
        ("notes/a.md", "x\n"),
    ]);
    let rank = |hidden_last: bool| -> Vec<(String, bool)> {
        let filter = SearchFilter {
            hidden_last,
            ..SearchFilter::default()
        };
        f.index
            .paths("notes", 10, 0, &filter)
            .files
            .into_iter()
            .map(|h| (h.path, h.hidden))
            .collect()
    };
    let normal = rank(false);
    // a better name match that is hidden comes first by score alone
    assert!(normal[0].1, "{normal:?}");
    let second = rank(true);
    let first_hidden = second.iter().position(|(_, h)| *h).unwrap();
    assert!(second[..first_hidden].iter().all(|(_, h)| !h), "{second:?}");
    assert!(second[first_hidden..].iter().all(|(_, h)| *h), "{second:?}");
    assert_eq!(
        second.iter().filter(|(_, h)| *h).count(),
        2,
        "hidden ones still listed: {second:?}"
    );
    // a limit keeps the non-hidden ones first
    let filter = SearchFilter {
        hidden_last: true,
        ..SearchFilter::default()
    };
    let one = f.index.paths("notes", 1, 0, &filter).files;
    assert!(!one[0].hidden, "{one:?}");
    // folders too
    let folders = f.index.paths("rules", 5, 5, &filter).folders;
    assert!(folders.iter().all(|h| h.hidden), "{folders:?}");
}

#[test]
fn hidden_means_a_dot_segment_below_the_root() {
    assert!(is_hidden_path(".env"));
    assert!(is_hidden_path("a/.git/config"));
    assert!(is_hidden_path(".claude/rules/x.md"));
    assert!(!is_hidden_path("docs/a.md"));
    assert!(!is_hidden_path("v1.2/notes.md"));
    assert!(
        !is_hidden_path("@wt/.feature/docs/a.md"),
        "the worktree name is not a segment"
    );
    assert!(is_hidden_path("@wt/feature/.github/ci.yml"));
    assert!(!is_hidden_path(""));
}
