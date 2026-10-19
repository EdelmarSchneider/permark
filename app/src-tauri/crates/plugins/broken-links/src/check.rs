//! The checker: which links of the Markdown documents lead nowhere, kept true as the project changes.
//!
//! Per document the links are kept (line, destination as written, the project path they name). A document that changes
//! is read again; a file that appears, disappears or moves only makes the links that name it (or a place it stands for:
//! the same name without `.md`, the folder of a `README.md`) be resolved again.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use permark_core::fsx;
use permark_core::links::{local_target, parse, resolve, Target};
use permark_core::paths::{key_of, Vault, VaultPath, WORKTREE_PREFIX};
use permark_core::tree::{category_of, Category};
use permark_core::watch::{ChangeKind, FsBatch};

/// Documents larger than this are not checked.
pub const MAX_DOC_BYTES: u64 = 5 * 1024 * 1024;
/// The most links of one document that are looked at (a hostile file cannot grow the picture without bound).
pub const MAX_LINKS_PER_DOC: usize = 20_000;
/// The longest destination kept as written, in characters.
const MAX_WRITTEN: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRec {
    /// 1-based line of the destination.
    pub line: u32,
    /// The destination as it is written, shortened when very long.
    pub written: String,
    /// The whole destination, to resolve it again.
    href: String,
    /// Key of the project path the destination names (no disk): what makes it worth resolving again.
    target: Option<String>,
    pub broken: bool,
}

/// What the checker knows.
#[derive(Debug, Default)]
pub struct Checker {
    docs: HashMap<String, Vec<LinkRec>>,
    /// Target key -> documents that name it.
    by_target: HashMap<String, HashSet<String>>,
    /// Documents too large, not text or unreadable.
    skipped: HashSet<String>,
}

fn is_document(path: &str) -> bool {
    category_of(path.rsplit('/').next().unwrap_or(path)) == Category::Markdown
}

fn shorten(text: &str) -> String {
    if text.chars().count() <= MAX_WRITTEN {
        text.to_owned()
    } else {
        let cut: String = text.chars().take(MAX_WRITTEN).collect();
        format!("{cut}…")
    }
}

/// The keys of the project paths a link may name that a change of `path` (a file or a folder) can affect.
fn affected(path: &str, is_dir: bool) -> (Vec<String>, Option<String>) {
    let key = key_of(path);
    let mut exact = vec![key.clone()];
    if !is_dir {
        let lower = key.to_lowercase();
        if let Some(stem) = lower.strip_suffix(".md") {
            // a link may leave the extension out
            exact.push(stem.to_owned());
        }
        let name = lower.rsplit('/').next().unwrap_or(&lower);
        if name == "readme.md" || name == "index.md" {
            // a link to the folder leads to its readme
            exact.push(
                key.rsplit_once('/')
                    .map_or_else(String::new, |(p, _)| p.to_owned()),
            );
        }
    }
    (exact, is_dir.then(|| format!("{key}/")))
}

impl Checker {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The broken links, by document and line (documents without any are left out).
    #[must_use]
    pub fn broken(&self) -> BTreeMap<&str, Vec<&LinkRec>> {
        let mut out = BTreeMap::new();
        for (path, links) in &self.docs {
            let mut broken: Vec<&LinkRec> = links.iter().filter(|l| l.broken).collect();
            if !broken.is_empty() {
                broken.sort_by_key(|l| l.line);
                out.insert(path.as_str(), broken);
            }
        }
        out
    }

    #[must_use]
    pub fn skipped(&self) -> usize {
        self.skipped.len()
    }

    #[must_use]
    pub fn documents(&self) -> usize {
        self.docs.len()
    }

    /// Total number of broken links.
    #[must_use]
    pub fn total(&self) -> usize {
        self.docs
            .values()
            .map(|links| links.iter().filter(|l| l.broken).count())
            .sum()
    }

    /// Reads a document (again): its links, checked against the disk. A document that cannot be read is dropped and
    /// counted as skipped.
    pub fn read(&mut self, vault: &Vault, path: &str, abs: &Path) {
        self.forget(path);
        let Ok(from) = VaultPath::parse(path) else {
            return;
        };
        let Ok(decoded) = fsx::read_text(abs, MAX_DOC_BYTES) else {
            self.skipped.insert(path.to_owned());
            return;
        };
        let text = &decoded.text;
        let mut links = Vec::new();
        for dest in parse(text).into_iter().take(MAX_LINKS_PER_DOC) {
            let href = dest.inner(text).trim();
            if href.is_empty() {
                continue;
            }
            let broken = matches!(resolve(vault, &from, href), Target::Missing(_));
            links.push(LinkRec {
                line: u32::try_from(dest.line).unwrap_or(u32::MAX),
                written: shorten(href),
                href: href.to_owned(),
                target: local_target(path, href).map(|t| key_of(&t)),
                broken,
            });
        }
        for link in &links {
            if let Some(target) = &link.target {
                self.by_target
                    .entry(target.clone())
                    .or_default()
                    .insert(path.to_owned());
            }
        }
        self.docs.insert(path.to_owned(), links);
    }

    /// Drops a document (it is gone or will be read again).
    pub fn forget(&mut self, path: &str) {
        self.skipped.remove(path);
        if let Some(links) = self.docs.remove(path) {
            for link in links {
                if let Some(target) = link.target {
                    if let Some(set) = self.by_target.get_mut(&target) {
                        set.remove(path);
                        if set.is_empty() {
                            self.by_target.remove(&target);
                        }
                    }
                }
            }
        }
    }

    /// Resolves again the links that name a place a change could affect.
    fn recheck(&mut self, vault: &Vault, exact: &[String], below: Option<&str>) {
        let mut docs: HashSet<String> = HashSet::new();
        for key in exact {
            if let Some(set) = self.by_target.get(key) {
                docs.extend(set.iter().cloned());
            }
        }
        if let Some(prefix) = below {
            for (key, set) in &self.by_target {
                if key.starts_with(prefix) {
                    docs.extend(set.iter().cloned());
                }
            }
        }
        for path in docs {
            let Ok(from) = VaultPath::parse(&path) else {
                continue;
            };
            let Some(links) = self.docs.get_mut(&path) else {
                continue;
            };
            for link in links.iter_mut().filter(|l| {
                l.target
                    .as_ref()
                    .is_some_and(|t| exact.contains(t) || below.is_some_and(|b| t.starts_with(b)))
            }) {
                link.broken = matches!(resolve(vault, &from, &link.href), Target::Missing(_));
            }
        }
    }

    /// Follows one batch of changes. `read_doc` finds the file of a document path. `true` when the picture may have
    /// changed. A batch that says "everything" (`bulk`, `rescan`) is the caller's to answer with a full scan.
    pub fn apply(
        &mut self,
        vault: &Vault,
        batch: &FsBatch,
        abs_of: &dyn Fn(&str) -> Option<std::path::PathBuf>,
    ) -> bool {
        let before = self.fingerprint();
        let mut reread: Vec<String> = Vec::new();
        let mut places: Vec<(Vec<String>, Option<String>)> = Vec::new();
        for change in &batch.changes {
            match &change.kind {
                ChangeKind::Created | ChangeKind::Modified => {
                    if !change.is_dir && is_document(&change.path) {
                        reread.push(change.path.clone());
                    }
                    if matches!(change.kind, ChangeKind::Created) {
                        places.push(affected(&change.path, change.is_dir));
                    }
                }
                ChangeKind::Removed => {
                    if change.is_dir {
                        let prefix = format!("{}/", key_of(&change.path));
                        let gone: Vec<String> = self
                            .docs
                            .keys()
                            .chain(self.skipped.iter())
                            .filter(|p| key_of(p).starts_with(&prefix))
                            .cloned()
                            .collect();
                        for path in gone {
                            self.forget(&path);
                        }
                    } else {
                        self.forget(&change.path);
                    }
                    places.push(affected(&change.path, change.is_dir));
                }
                ChangeKind::Renamed { from } => {
                    if change.is_dir {
                        let prefix = format!("{}/", key_of(from));
                        let moved: Vec<String> = self
                            .docs
                            .keys()
                            .chain(self.skipped.iter())
                            .filter(|p| key_of(p).starts_with(&prefix))
                            .cloned()
                            .collect();
                        for path in moved {
                            self.forget(&path);
                        }
                        // the documents at the new place are read as the watch reports them (or by the next scan)
                        reread.extend(Self::documents_below(vault, &change.path));
                    } else {
                        self.forget(from);
                        if is_document(&change.path) {
                            reread.push(change.path.clone());
                        }
                    }
                    places.push(affected(from, change.is_dir));
                    places.push(affected(&change.path, change.is_dir));
                }
            }
        }
        reread.sort();
        reread.dedup();
        for path in &reread {
            if let Some(abs) = abs_of(path) {
                self.read(vault, path, &abs);
            }
        }
        for (exact, below) in &places {
            self.recheck(vault, exact, below.as_deref());
        }
        before != self.fingerprint()
    }

    /// The Markdown documents found below a folder that moved (a shallow walk of what is there now).
    fn documents_below(vault: &Vault, folder: &str) -> Vec<String> {
        let Ok(path) = VaultPath::parse(folder) else {
            return Vec::new();
        };
        let Ok(abs) = vault.resolve(&path) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        let mut stack = vec![(abs, folder.to_owned())];
        let mut budget = 20_000usize;
        while let Some((dir, rel)) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                if budget == 0 {
                    return found;
                }
                budget -= 1;
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with(WORKTREE_PREFIX) || name == ".git" {
                    continue;
                }
                let child = format!("{rel}/{name}");
                match entry.file_type() {
                    Ok(t) if t.is_dir() => stack.push((entry.path(), child)),
                    Ok(t) if t.is_file() && is_document(&child) => found.push(child),
                    _ => {}
                }
            }
        }
        found
    }

    /// What the report would say, as a cheap comparison.
    fn fingerprint(&self) -> (usize, u64, usize) {
        let mut hash = 0u64;
        let mut total = 0usize;
        for (path, links) in &self.docs {
            for link in links.iter().filter(|l| l.broken) {
                total += 1;
                hash ^= fsx::hash(format!("{path}\0{}\0{}", link.line, link.href).as_bytes()).0;
            }
        }
        (total, hash, self.skipped.len())
    }
}
