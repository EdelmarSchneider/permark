//! The answers an agent gets, built from the project's files. The same functions serve the window (which has the
//! live index and the live unread markers) and `permark mcp` when the window is closed (which reads the disk), so
//! an agent sees the same shapes either way.
//!
//! Paths are the project's, relative with `/` separators. A linked worktree lives outside the project folder, so
//! nothing from one is ever listed, found or opened here.

use std::path::{Path, PathBuf};

use crate::notes;
use crate::render::{decode_text, MAX_RENDER_BYTES};
use crate::search::chunk::chunk_markdown;
use crate::search::SearchHit;
use crate::session::AppSettingsStore;
use crate::util::{canonicalize_lenient, modified_ms};
use crate::vault::{strip_root, Vault, WORKTREE_PREFIX};

use crate::comments::{CommentFilter, CommentView};

use super::protocol::{
    CommentList, ErrorKind, Hit, Opened, ProtocolError, UnreadDoc, MAX_COMMENTS_LISTED,
    MAX_SNIPPET_CHARS,
};

/// Whether a path of the vault is inside the project folder itself.
fn in_project(path: &str) -> bool {
    !path.starts_with(WORKTREE_PREFIX)
}

/// The text of a search snippet without the markup the window draws it with: the match marks are dropped and the
/// escaped characters are written out.
pub(crate) fn plain_snippet(html: &str) -> String {
    html.replace("<mark>", "")
        .replace("</mark>", "")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// `text` cut to at most `max` characters, at a word when there is one near the end, with an ellipsis where it was cut.
pub(crate) fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    // do not leave half a word, unless that would throw most of the text away
    let kept = match kept.rfind(char::is_whitespace) {
        Some(at) if at > kept.len() / 2 => &kept[..at],
        _ => &kept,
    };
    format!("{}…", kept.trim_end())
}

/// Search hits as an agent reads them: the snippet is cut to about [`MAX_SNIPPET_CHARS`] (the window around the match
/// starts a few words before it, so the match is kept).
pub(crate) fn plain_hits(hits: Vec<SearchHit>) -> Vec<Hit> {
    hits.into_iter()
        .filter(|hit| in_project(&hit.path))
        .map(|hit| Hit {
            path: hit.path,
            heading_path: hit.heading_path,
            line: hit.start_line,
            snippet: shorten(&plain_snippet(&hit.snippet), MAX_SNIPPET_CHARS),
            // the document's tags and description are put in by whoever knows them (see `DocIndex::describe`)
            tags: Vec::new(),
            description: String::new(),
        })
        .collect()
}

/// The unread documents with the time each was last written, newest first. A document that cannot be found any
/// more is left out. The list is not cut; `UnreadList::new` does that.
pub(crate) fn unread_docs(vault: &Vault, paths: &[String]) -> Vec<UnreadDoc> {
    let mut docs: Vec<UnreadDoc> = paths
        .iter()
        .filter(|path| in_project(path))
        .filter_map(|path| {
            let resolved = vault.resolve(path).ok()?;
            let meta = std::fs::metadata(&resolved.abs).ok()?;
            meta.is_file().then(|| UnreadDoc {
                path: resolved.rel,
                modified_ms: modified_ms(&meta),
            })
        })
        .collect();
    docs.sort_by(|a, b| {
        b.modified_ms
            .cmp(&a.modified_ms)
            .then_with(|| a.path.cmp(&b.path))
    });
    docs
}

/// The comments an agent is told about: the kind asked for, by document and line. Comments on a document of a
/// linked worktree are left out like everything else that lives outside the project folder.
pub(crate) fn comment_list(comments: Vec<CommentView>, which: CommentFilter) -> CommentList {
    let mut comments: Vec<CommentView> = comments
        .into_iter()
        .filter(|comment| in_project(&comment.path) && which.admits(comment))
        .collect();
    comments.sort_by(|a, b| {
        (&a.path, a.line, a.created_ms, &a.id).cmp(&(&b.path, b.line, b.created_ms, &b.id))
    });
    let total = comments.len();
    comments.truncate(MAX_COMMENTS_LISTED);
    CommentList { comments, total }
}

/// Heading text as it is compared: no case, no spare spaces, no closing `#`s.
fn plain_heading(text: &str) -> String {
    text.trim()
        .trim_start_matches('#')
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The line of the heading `wanted` in a document: a section whose own heading is the text (without regard to
/// case), or whose heading path ends with the headings given (`Plan > Rollout`, with `>` or `›` between them).
/// The first one in the document wins. `None` when no section is called that.
pub(crate) fn locate_heading(markdown: &str, wanted: &str) -> Option<u32> {
    let wanted: Vec<String> = wanted.split(['>', '›']).map(plain_heading).collect();
    if wanted.iter().any(String::is_empty) {
        return None;
    }
    chunk_markdown(markdown)
        .iter()
        .find(|section| {
            let path: Vec<String> = section
                .heading_path
                .iter()
                .map(|heading| plain_heading(heading))
                .collect();
            path.ends_with(&wanted)
        })
        .map(|section| section.start_line)
}

/// The folder of the person's notes when it lies inside a project. The notes are private: nothing an agent connected to the
/// project asks for may reach them, and what is asked about them is answered as if they were not there (a path that does not
/// exist, a comment that is not found, a document that is not open).
#[derive(Debug, Clone, Default)]
pub(crate) struct PrivateTree {
    folder: Option<PathBuf>,
}

impl PrivateTree {
    /// The notes folder of the app whose data is in `app_data`, for the project in `root` (nothing is private when the folder is
    /// not inside it).
    pub(crate) fn of(app_data: &Path, root: &Path) -> Self {
        let chosen = AppSettingsStore::new(app_data).load().notes_folder;
        Self {
            folder: notes::folder::inside(app_data, chosen.as_deref(), root),
        }
    }

    /// A tree that is the folder `folder` (as the system spells it).
    #[cfg(test)]
    pub(crate) fn at(folder: PathBuf) -> Self {
        Self {
            folder: Some(folder),
        }
    }

    /// Whether the file at `abs` is in the private folder.
    pub(crate) fn covers(&self, abs: &Path) -> bool {
        let Some(folder) = &self.folder else {
            return false;
        };
        let canonical = canonicalize_lenient(abs).unwrap_or_else(|| abs.to_path_buf());
        strip_root(&canonical, folder).is_some()
    }

    /// Whether the document `rel` of the project (a path of the vault) is in the private folder.
    pub(crate) fn covers_path(&self, vault: &Vault, rel: &str) -> bool {
        self.folder.is_some()
            && vault
                .resolve(rel)
                .is_ok_and(|resolved| self.covers(&resolved.abs))
    }
}

/// Checks what an agent asked to be shown and works out the line: the path must be a document of the project, and
/// a heading wins over a line (a heading survives edits). A heading the document does not have is reported, and
/// the document still opens.
#[cfg(test)]
pub(crate) fn prepare_open(
    vault: &Vault,
    path: &str,
    line: Option<u32>,
    heading: Option<&str>,
) -> Result<Opened, ProtocolError> {
    prepare_open_hiding(vault, &PrivateTree::default(), path, line, heading)
}

/// [`prepare_open`] for a project whose notes folder is private: a path in it is refused exactly as a path that does not exist
/// is, before anything of the file is read.
pub(crate) fn prepare_open_hiding(
    vault: &Vault,
    private: &PrivateTree,
    path: &str,
    line: Option<u32>,
    heading: Option<&str>,
) -> Result<Opened, ProtocolError> {
    let not_found = || {
        ProtocolError::new(
            ErrorKind::NotFound,
            format!("{path} is not a document of this project"),
        )
    };
    if !in_project(path) {
        return Err(not_found());
    }
    let resolved = vault.resolve(path).map_err(|_| not_found())?;
    if resolved.root != 0 || !resolved.abs.is_file() || private.covers(&resolved.abs) {
        return Err(not_found());
    }
    let wanted = heading.map(str::trim).filter(|h| !h.is_empty());
    let found = wanted.and_then(|wanted| {
        // A document over the size the app reads is not read for a heading either; it opens at its top.
        if std::fs::metadata(&resolved.abs).ok()?.len() > MAX_RENDER_BYTES {
            return None;
        }
        let bytes = std::fs::read(&resolved.abs).ok()?;
        locate_heading(&decode_text(&bytes)?, wanted)
    });
    Ok(Opened {
        path: resolved.rel,
        line: found.or(line.filter(|n| *n >= 1)),
        heading_not_found: wanted.is_some() && found.is_none(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "# Plan\n\nIntro.\n\n## Rollout\n\nShip it.\n\n### Week 1\n\nStart small.\n\n## Risks\n\nMany.\n\n## Rollout\n\nAgain.\n";

    fn project() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("docs")).expect("mkdir");
        std::fs::write(dir.path().join("docs/plan.md"), DOC).expect("write");
        std::fs::write(dir.path().join("notes.txt"), "text").expect("write");
        let vault = Vault::open(dir.path()).expect("vault");
        (dir, vault)
    }

    #[test]
    fn comments_are_listed_by_the_kind_asked_for_without_what_lives_outside_the_project() {
        use crate::bridge::hub::testing::comment;
        let mut done = comment("c3", "docs/plan.md", 7);
        done.done = true;
        let all = vec![
            comment("c2", "docs/plan.md", 7),
            comment("c1", "docs/plan.md", 1),
            done,
            comment("c9", "@wt/feature/docs/plan.md", 1),
            comment("c0", "a.md", 40),
        ];
        let ids = |which| {
            comment_list(all.clone(), which)
                .comments
                .into_iter()
                .map(|n| n.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(CommentFilter::Open), ["c0", "c1", "c2"]);
        assert_eq!(ids(CommentFilter::Done), ["c3"]);
        assert_eq!(ids(CommentFilter::All), ["c0", "c1", "c2", "c3"]);
        assert_eq!(comment_list(all, CommentFilter::Open).total, 3);
    }

    #[test]
    fn a_long_list_of_comments_is_cut_and_still_counts_everything() {
        use crate::bridge::hub::testing::comment;
        let many: Vec<_> = (0..MAX_COMMENTS_LISTED + 7)
            .map(|n| {
                comment(
                    &format!("c{n}"),
                    "docs/plan.md",
                    u32::try_from(n).unwrap_or(0) + 1,
                )
            })
            .collect();
        let list = comment_list(many, CommentFilter::Open);
        assert_eq!(list.comments.len(), MAX_COMMENTS_LISTED);
        assert_eq!(list.total, MAX_COMMENTS_LISTED + 7);
        assert_eq!(list.comments[0].id, "c0");
    }

    #[test]
    fn snippets_lose_their_marks_and_escapes() {
        assert_eq!(
            plain_snippet("…the <mark>roll</mark>out &amp; &lt;b&gt; &quot;x&quot; it&#39;s"),
            "…the rollout & <b> \"x\" it's"
        );
        // An escaped mark in the text itself stays text.
        assert_eq!(
            plain_snippet("&lt;mark&gt;x&lt;/mark&gt;"),
            "<mark>x</mark>"
        );
    }

    #[test]
    fn a_long_text_is_cut_at_a_word_with_an_ellipsis_and_a_short_one_is_left_alone() {
        assert_eq!(shorten("short enough", 160), "short enough");
        let exact = "x".repeat(20);
        assert_eq!(shorten(&exact, 20), exact);
        let cut = shorten("alpha beta gamma delta epsilon", 18);
        assert_eq!(cut, "alpha beta gamma…");
        assert!(cut.chars().count() <= 18);
        // a long word with no space is cut where it is, and CJK text has no spaces at all
        assert_eq!(shorten("abcdefghijklmnop", 8), "abcdefg…");
        assert_eq!(shorten("日本語のテキストです", 5), "日本語の…");
        // a space near the start is not worth throwing the rest away for
        assert_eq!(shorten("ab cdefghijklmnopqrstuv", 12), "ab cdefghij…");
    }

    #[test]
    fn a_long_snippet_is_cut_in_the_hit_but_keeps_its_match_near_the_start() {
        let long = format!(
            "…the retry <mark>policy</mark> says {} and more…",
            "words ".repeat(60)
        );
        let hits = plain_hits(vec![SearchHit {
            path: "a.md".to_owned(),
            heading_path: vec![],
            snippet: long,
            start_line: 1,
            score: 1.0,
        }]);
        assert!(hits[0].snippet.chars().count() <= MAX_SNIPPET_CHARS);
        assert!(hits[0].snippet.starts_with("…the retry policy says"));
        assert!(hits[0].snippet.ends_with('…'));
    }

    #[test]
    fn hits_keep_the_path_the_headings_the_line_and_plain_text() {
        let hits = vec![
            SearchHit {
                path: "docs/plan.md".to_owned(),
                heading_path: vec!["Plan".to_owned(), "Rollout".to_owned()],
                snippet: "<mark>Ship</mark> it".to_owned(),
                start_line: 5,
                score: 1.0,
            },
            SearchHit {
                path: "@wt/feature/docs/plan.md".to_owned(),
                heading_path: vec![],
                snippet: "x".to_owned(),
                start_line: 1,
                score: 0.5,
            },
        ];
        let plain = plain_hits(hits);
        assert_eq!(plain.len(), 1, "a worktree is outside the project");
        assert_eq!(plain[0].path, "docs/plan.md");
        assert_eq!(plain[0].line, 5);
        assert_eq!(plain[0].snippet, "Ship it");
    }

    #[test]
    fn unread_documents_are_listed_newest_first_with_their_time() {
        let (dir, vault) = project();
        std::fs::write(dir.path().join("a.md"), "a").expect("write");
        std::fs::write(dir.path().join("b.md"), "b").expect("write");
        let older = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(dir.path().join("a.md"))
            .and_then(|file| file.set_modified(older))
            .expect("set the time");
        let paths = ["a.md", "b.md", "gone.md", "@wt/x/c.md"].map(str::to_owned);
        let docs = unread_docs(&vault, &paths);
        let names: Vec<_> = docs.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(names, ["b.md", "a.md"]);
        assert!(docs[0].modified_ms > docs[1].modified_ms);
    }

    #[test]
    fn a_heading_is_found_by_its_own_text_or_its_path() {
        assert_eq!(locate_heading(DOC, "Rollout"), Some(5));
        assert_eq!(locate_heading(DOC, "  rollout  "), Some(5));
        assert_eq!(locate_heading(DOC, "## Rollout"), Some(5));
        assert_eq!(locate_heading(DOC, "week 1"), Some(9));
        assert_eq!(locate_heading(DOC, "Plan > Rollout > Week 1"), Some(9));
        assert_eq!(locate_heading(DOC, "Plan › Risks"), Some(13));
        assert_eq!(locate_heading(DOC, "Plan"), Some(1));
        assert_eq!(locate_heading(DOC, "Nothing"), None);
        assert_eq!(locate_heading(DOC, ""), None);
        assert_eq!(locate_heading(DOC, "Risks > Plan"), None);
        assert_eq!(locate_heading("no headings here", "Plan"), None);
    }

    #[test]
    fn open_resolves_a_heading_to_its_line_and_prefers_it_to_a_line() {
        let (_dir, vault) = project();
        let opened = prepare_open(&vault, "docs/plan.md", Some(2), Some("Risks")).expect("open");
        assert_eq!(
            opened,
            Opened {
                path: "docs/plan.md".to_owned(),
                line: Some(13),
                heading_not_found: false
            }
        );
        let by_line = prepare_open(&vault, "docs/plan.md", Some(7), None).expect("open");
        assert_eq!(by_line.line, Some(7));
        let top = prepare_open(&vault, "docs/plan.md", None, None).expect("open");
        assert_eq!(top.line, None);
        // Zero is not a line.
        assert_eq!(
            prepare_open(&vault, "docs/plan.md", Some(0), None)
                .expect("open")
                .line,
            None
        );
    }

    #[test]
    fn a_missing_heading_is_reported_and_the_document_still_opens() {
        let (_dir, vault) = project();
        let opened =
            prepare_open(&vault, "docs/plan.md", Some(3), Some("Nonexistent")).expect("open");
        assert!(opened.heading_not_found);
        assert_eq!(opened.line, Some(3), "the line is the fallback");
        // A blank heading is no heading.
        let blank = prepare_open(&vault, "docs/plan.md", None, Some("  ")).expect("open");
        assert!(!blank.heading_not_found);
    }

    /// A heading is looked for in the document's text, so a document too big to read is not read for it.
    #[test]
    fn a_document_over_the_size_the_app_reads_is_not_searched_for_a_heading() {
        let (dir, vault) = project();
        let mut text = String::from("# Big\n\n## Rollout\n\n");
        text.push_str(&"\n".repeat(usize::try_from(MAX_RENDER_BYTES).expect("fits") + 1));
        std::fs::write(dir.path().join("docs/big.md"), &text).expect("write");
        let opened =
            prepare_open(&vault, "docs/big.md", Some(1), Some("Rollout")).expect("it still opens");
        assert!(opened.heading_not_found, "not searched");
        assert_eq!(opened.line, Some(1), "the line it came with");
        // and under the size the same heading is found
        let small = prepare_open(&vault, "docs/plan.md", None, Some("Rollout")).expect("open");
        assert!(!small.heading_not_found);
    }

    #[test]
    fn open_shows_only_documents_of_the_project() {
        let (_dir, vault) = project();
        for path in [
            "../elsewhere.md",
            "/etc/passwd",
            "C:/Windows/win.ini",
            "docs/missing.md",
            "docs",
            "@wt/feature/docs/plan.md",
            "",
        ] {
            let error = prepare_open(&vault, path, None, None).expect_err(path);
            assert_eq!(error.kind, ErrorKind::NotFound, "{path:?}");
        }
        // The reply names the path that was asked for and nothing else of this machine.
        let error = prepare_open(&vault, "docs/missing.md", None, None).expect_err("missing");
        assert_eq!(
            error.message,
            "docs/missing.md is not a document of this project"
        );
        // Any file of the project can be shown (the window decides how), a backslash spelling is the same file.
        assert!(prepare_open(&vault, "notes.txt", None, None).is_ok());
        assert_eq!(
            prepare_open(&vault, "docs\\plan.md", None, None)
                .expect("open")
                .path,
            "docs/plan.md"
        );
    }
}
