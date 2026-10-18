//! Links in the project whose file does not exist, and the files a repair can point them at.
//!
//! The scan is the one a move uses ([`super::scan::find_destinations`]) plus the same resolver the
//! reader uses when a link is clicked. A repair is offered when another file in the same root has
//! the name the link asked for (an extensionless Markdown link also matches `name.md`, which is how
//! the resolver already looks). Several files with that name are all offered; nothing is chosen for
//! the person.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use super::plan::{destination_for, split_root, LinkEdit, World};
use super::scan::{find_destinations, split_suffix, unescape, Dest, Form};
use super::world::VaultWorld;
use super::BufferText;
use crate::links::{self, scheme_of, LinkTarget};
use crate::render::MAX_RENDER_BYTES;
use crate::vault::{key, NodeKind, Vault};

/// How many broken links the list carries. The count ([`BrokenReport::total`]) is complete past this.
const MAX_LISTED: usize = 2_000;

/// One link, image or reference definition whose file is not there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokenLink {
    /// The document it is written in.
    pub path: String,
    /// 1-based line of the destination.
    pub line: usize,
    /// Byte range of the destination token in that document's text.
    pub start: usize,
    pub end: usize,
    /// The destination as written, shortened when it is very long.
    pub written: String,
    /// Files in the same root with the name this link asks for, nearest path first.
    pub repairs: Vec<String>,
}

/// Every broken link of the open folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokenReport {
    pub links: Vec<BrokenLink>,
    /// How many there are, including the ones past [`MAX_LISTED`].
    pub total: usize,
    pub truncated: bool,
    /// Documents that were too large, not text, or unreadable, and were left out.
    pub skipped: usize,
}

/// Why a repair did not change the link. `repaired` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RepairStatus {
    Repaired,
    /// The destination is no longer where it was, or the link is not broken any more.
    Stale,
    /// `target` is not a file this link can be pointed at.
    NotAMatch,
}

/// What a repair did. A document open with unsaved text is not written: `buffer` is the change for
/// the editor. A file on disk comes back with `op_id` so the write can be undone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairOutcome {
    pub status: RepairStatus,
    pub op_id: Option<u64>,
    pub buffer: Option<super::BufferEdits>,
}

impl RepairOutcome {
    pub(crate) fn status(status: RepairStatus) -> Self {
        Self {
            status,
            op_id: None,
            buffer: None,
        }
    }
}

/// What rewriting one destination takes, when the text still holds it and `target` is a real choice.
pub(crate) enum Prepared {
    Edit(LinkEdit),
    Stale,
    NotAMatch,
}

/// The broken links of `vault`. `buffers` are documents open with unsaved text: those are read from
/// the editor instead of the disk.
pub(crate) fn scan(vault: &Vault, buffers: &[BufferText]) -> BrokenReport {
    let world = VaultWorld::new(vault);
    let docs = world.documents();
    let index = name_index(vault);
    let held: HashMap<String, &str> = buffers
        .iter()
        .map(|b| (key(&b.path), b.text.as_str()))
        .collect();

    let mut links = Vec::new();
    let mut total = 0;
    let mut skipped = 0;
    for doc in docs {
        let disk = if held.contains_key(&key(&doc)) {
            None
        } else {
            let Some(read) = read_doc(&world, &doc) else {
                skipped += 1;
                continue;
            };
            Some(read)
        };
        let text = held.get(&key(&doc)).copied().unwrap_or_else(|| {
            disk.as_deref()
                .expect("the document was read when it is not open in an editor")
        });
        for dest in find_destinations(text) {
            let Some(missing) = local_missing(vault, &doc, text, &dest) else {
                continue;
            };
            total += 1;
            if links.len() >= MAX_LISTED {
                continue;
            }
            links.push(BrokenLink {
                path: doc.clone(),
                line: line_of(text, dest.start),
                start: dest.start,
                end: dest.end,
                written: shorten(dest.inner(text), 300),
                repairs: candidates(&index, &doc, &missing),
            });
        }
    }
    BrokenReport {
        truncated: total > links.len(),
        total,
        links,
        skipped,
    }
}

/// The edit that points the destination `start..end` of `text` (the document `doc`) at `target`.
pub(crate) fn prepare(
    vault: &Vault,
    doc: &str,
    text: &str,
    start: usize,
    end: usize,
    target: &str,
) -> Prepared {
    let Some(dest) = find_destinations(text)
        .into_iter()
        .find(|d| d.start == start && d.end == end)
    else {
        return Prepared::Stale;
    };
    let Some(missing) = local_missing(vault, doc, text, &dest) else {
        return Prepared::Stale;
    };
    let Ok(resolved) = vault.resolve(target) else {
        return Prepared::NotAMatch;
    };
    if !resolved.abs.is_file() {
        return Prepared::NotAMatch;
    }
    let choices = candidates(&name_index(vault), doc, &missing);
    if !choices.iter().any(|path| key(path) == key(&resolved.rel)) {
        return Prepared::NotAMatch;
    }
    let Some(new) = destination_for(doc, text, &dest, &resolved.rel) else {
        return Prepared::NotAMatch;
    };
    Prepared::Edit(LinkEdit {
        start: dest.start,
        end: dest.end,
        old: dest.raw(text).to_owned(),
        new,
        line: line_of(text, dest.start),
        causes: Vec::new(),
    })
}

/// File name (the last segment) to the vault paths that have it.
fn name_index(vault: &Vault) -> HashMap<String, Vec<String>> {
    let mut index: HashMap<String, Vec<String>> = HashMap::new();
    for item in vault.walk() {
        if item.kind != NodeKind::File {
            continue;
        }
        index.entry(key(&item.name)).or_default().push(item.rel);
    }
    for paths in index.values_mut() {
        paths.sort();
        paths.dedup();
    }
    index
}

/// Files in the same root as `doc` whose name is the one `missing` asks for.
fn candidates(index: &HashMap<String, Vec<String>>, doc: &str, missing: &str) -> Vec<String> {
    let root = split_root(doc).0;
    let missing_key = key(missing);
    let mut out = Vec::new();
    for name in wanted_names(missing) {
        let Some(paths) = index.get(&name) else {
            continue;
        };
        for path in paths {
            if split_root(path).0 != root || key(path) == missing_key {
                continue;
            }
            if out.iter().any(|have: &String| key(have) == key(path)) {
                continue;
            }
            out.push(path.clone());
        }
    }
    out.sort();
    out
}

/// The file names a missing path can be repaired to: its own name, and `name.md` when it names no
/// extension (the resolver tries that before it calls a link missing).
fn wanted_names(missing: &str) -> Vec<String> {
    let base = base_name(missing);
    let base = base.split(['?', '#']).next().unwrap_or(base);
    if base.is_empty() || base == "." || base == ".." {
        return Vec::new();
    }
    let mut names = vec![key(base)];
    if Path::new(base).extension().is_none() {
        names.push(key(&format!("{base}.md")));
    }
    names
}

fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The vault path a local destination points at when no file is there. External links, anchors and
/// anything that is not a path are `None` (they are not broken files).
fn local_missing(vault: &Vault, doc: &str, text: &str, dest: &Dest) -> Option<String> {
    let raw = dest.inner(text);
    if dest.form == Form::Html && (raw.contains('&') || raw.trim() != raw) {
        return None;
    }
    let (path_raw, suffix) = split_suffix(raw);
    let path = unescape(path_raw);
    if path.is_empty() || path.starts_with("//") || path.starts_with("\\\\") {
        return None;
    }
    if let Some(scheme) = scheme_of(&path) {
        if scheme != "file" && scheme.len() != 1 {
            return None;
        }
    }
    let href = format!("{path}{suffix}");
    match links::resolve(vault, doc, &href) {
        LinkTarget::Missing { path } => Some(path),
        _ => None,
    }
}

fn read_doc(world: &VaultWorld<'_>, doc: &str) -> Option<String> {
    let bytes = world.read(doc).ok()?;
    if bytes.len() as u64 > MAX_RENDER_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn line_of(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    text[..offset].bytes().filter(|b| *b == b'\n').count() + 1
}

fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::fileops::plan::destination_for;
    use crate::fileops::scan::{Dest, Form, Kind};

    fn vault(files: &[(&str, &str)]) -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().expect("tempdir");
        for (rel, text) in files {
            let path = dir.path().join(rel);
            fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            fs::write(path, text).expect("write");
        }
        let vault = Vault::open(dir.path()).expect("open");
        (dir, vault)
    }

    #[test]
    fn a_missing_file_is_listed_and_a_link_that_exists_is_not() {
        let (_dir, vault) = vault(&[
            (
                "readme.md",
                "See [gone](old/guide.md) and [ok](docs/guide.md) and [web](https://example.com) and [here](#top).\n",
            ),
            ("docs/guide.md", "# Guide\n"),
        ]);
        let report = scan(&vault, &[]);
        assert_eq!(report.total, 1);
        assert_eq!(report.skipped, 0);
        assert!(!report.truncated);
        let link = &report.links[0];
        assert_eq!(link.path, "readme.md");
        assert_eq!(link.line, 1);
        assert_eq!(link.written, "old/guide.md");
        assert_eq!(link.repairs, vec!["docs/guide.md".to_owned()]);
    }

    #[test]
    fn an_image_and_an_extensionless_link_match_a_file_of_that_name() {
        let (_dir, vault) = vault(&[
            ("readme.md", "![pic](old/logo.png)\n\nSee [g](old/guide).\n"),
            ("img/logo.png", "png"),
            ("docs/guide.md", "guide"),
        ]);
        let report = scan(&vault, &[]);
        assert_eq!(report.total, 2);
        assert_eq!(report.links[0].repairs, vec!["img/logo.png".to_owned()]);
        assert_eq!(report.links[1].repairs, vec!["docs/guide.md".to_owned()]);
    }

    #[test]
    fn two_files_with_the_same_name_are_both_offered_and_a_code_span_is_not() {
        let (_dir, vault) = vault(&[
            (
                "readme.md",
                "```\n[x](nope.md)\n```\n\nSee [g](old/guide.md).\n",
            ),
            ("docs/guide.md", "a"),
            ("notes/guide.md", "b"),
        ]);
        let report = scan(&vault, &[]);
        assert_eq!(report.total, 1);
        assert_eq!(
            report.links[0].repairs,
            vec!["docs/guide.md".to_owned(), "notes/guide.md".to_owned()]
        );
    }

    #[test]
    fn unsaved_text_is_what_gets_scanned() {
        let (_dir, vault) = vault(&[
            ("readme.md", "See [ok](docs/guide.md).\n"),
            ("docs/guide.md", "a"),
        ]);
        let report = scan(
            &vault,
            &[BufferText {
                path: "readme.md".to_owned(),
                text: "See [gone](missing.md).\n".to_owned(),
            }],
        );
        assert_eq!(report.total, 1);
        assert_eq!(report.links[0].written, "missing.md");
        assert_eq!(report.links[0].repairs, Vec::<String>::new());
    }

    #[test]
    fn a_repair_keeps_the_anchor_and_drops_a_missing_md_extension() {
        let text = "[a](old/guide.md#Setup)";
        let with_anchor = Dest {
            start: 4,
            end: text.len() - 1,
            form: Form::Bare,
            kind: Kind::Inline,
        };
        assert_eq!(
            destination_for("readme.md", text, &with_anchor, "docs/guide.md").as_deref(),
            Some("docs/guide.md#Setup")
        );
        let bare = "[a](old/guide)";
        let extless = Dest {
            start: 4,
            end: bare.len() - 1,
            form: Form::Bare,
            kind: Kind::Inline,
        };
        assert_eq!(
            destination_for("readme.md", bare, &extless, "docs/guide.md").as_deref(),
            Some("docs/guide")
        );
    }

    #[test]
    fn prepare_refuses_a_file_whose_name_is_different() {
        let (dir, vault) = vault(&[
            ("readme.md", "See [g](old/guide.md).\n"),
            ("docs/guide.md", "a"),
            ("img/logo.png", "p"),
        ]);
        let text = fs::read_to_string(dir.path().join("readme.md")).expect("read");
        let start = text.find("old/guide.md").expect("link");
        let end = start + "old/guide.md".len();
        assert!(matches!(
            prepare(&vault, "readme.md", &text, start, end, "docs/guide.md"),
            Prepared::Edit(_)
        ));
        assert!(matches!(
            prepare(&vault, "readme.md", &text, start, end, "img/logo.png"),
            Prepared::NotAMatch
        ));
        assert!(matches!(
            prepare(&vault, "readme.md", &text, start, end + 1, "docs/guide.md"),
            Prepared::Stale
        ));
    }
}
