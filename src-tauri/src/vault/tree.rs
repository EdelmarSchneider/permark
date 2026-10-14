//! Folder walking and the file tree sent to the UI.
//!
//! The regular walk lists what git would show: it honors `.gitignore`, `.ignore` and `.git/info/exclude`
//! (also outside a git repository) and never follows links. Two kinds of folders get special care:
//!
//! - **Nested repositories** (a folder holding its own `.git`, which is not a registered worktree) are
//!   always part of the tree, even when the enclosing project ignores them: each is walked on its own,
//!   under its own ignore rules instead of the enclosing project's.
//! - **Linked worktrees** are extra roots (`@wt/<name>/...`) and are skipped where they sit inside a root.
//!
//! With [`TreeOptions::show_ignored`] the tree also lists what git ignores, except that an ignored folder
//! holding more than [`IGNORED_FOLDER_CAP`] entries is listed unopened (see [`TreeNode::large`]).

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs::FileType;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ignore::WalkBuilder;
use serde::Serialize;

use super::{strip_root, Vault, WORKTREE_PREFIX};

/// Git's own folder; never listed. A `.git` *file* (a linked worktree or a submodule) is skipped the same way.
const GIT_DIR: &str = ".git";
/// Our own folder. Only its committed files are listed, and only on request ([`TreeOptions::show_state_dir`]).
const STATE_DIR: &str = ".trailmark";
/// Its machine-local folders are never listed.
const STATE_PRIVATE: [&str; 2] = ["cache", "local"];
/// Dependency folders are left out of the regular walk (they are listed unopened, when ignored ones are shown).
const DEPENDENCY_DIR: &str = "node_modules";
/// Safety valve for pathological folders.
const MAX_ENTRIES: usize = 100_000;
/// An ignored folder with more entries than this is listed unopened when ignored files are shown.
pub const IGNORED_FOLDER_CAP: usize = 2_000;

/// What the tree lists beyond the regular walk.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TreeOptions {
    /// Also list what git ignores (large ignored folders are listed unopened).
    pub show_ignored: bool,
    /// List `.trailmark/` (its `cache` and `local` folders stay out).
    pub show_state_dir: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeKind {
    Dir,
    File,
    Worktree,
    Link,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    pub kind: NodeKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<TreeNode>>,
    /// Set on an ignored folder that was too large to list: it holds more than this many entries,
    /// and `children` is empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub large: Option<usize>,
}

/// One entry found by [`Vault::walk`].
#[derive(Debug, Clone)]
pub struct WalkItem {
    pub abs: PathBuf,
    /// Vault-relative path in the UI form.
    pub rel: String,
    pub name: String,
    pub kind: NodeKind,
    /// See [`TreeNode::large`].
    pub large: Option<usize>,
}

fn same_path(a: &Path, b: &Path) -> bool {
    strip_root(a, b).is_some_and(|rest| rest.is_empty())
}

fn parent_of(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// Whether the folder is the root of a repository of its own (it has a `.git` folder or file).
fn holds_repo(dir: &Path) -> bool {
    dir.join(GIT_DIR).exists()
}

/// Whether a walk leaves an entry out by name. `parent` is the name of the folder it sits in.
fn left_out(name: &str, parent: Option<&str>, state_dir: bool, deps: bool) -> bool {
    let is = |x: &str| name.eq_ignore_ascii_case(x);
    if is(GIT_DIR) {
        return true;
    }
    if is(STATE_DIR) {
        return !state_dir;
    }
    if is(DEPENDENCY_DIR) {
        return deps;
    }
    parent.is_some_and(|p| p.eq_ignore_ascii_case(STATE_DIR)) && STATE_PRIVATE.iter().any(|x| is(x))
}

fn kind_of(ft: FileType) -> NodeKind {
    if ft.is_symlink() {
        NodeKind::Link
    } else if ft.is_dir() {
        NodeKind::Dir
    } else {
        NodeKind::File
    }
}

/// An ignored folder whose entries are being listed.
struct OpenDir {
    dir: PathBuf,
    /// Where its own item sits in the output.
    at: usize,
    /// The entries met inside it so far.
    count: usize,
}

fn set_gate(gate: &Mutex<Option<PathBuf>>, dir: Option<PathBuf>) {
    if let Ok(mut gate) = gate.lock() {
        *gate = dir;
    }
}

/// One root of the vault (the main folder or a worktree) and how to list it.
struct Scope {
    root: PathBuf,
    /// `""` for the main root, `@wt/<name>/` for a worktree.
    prefix: String,
    /// Worktrees that sit inside this root: they are listed as roots of their own.
    skip: Vec<PathBuf>,
    /// Simple mode looked at one file: only the direct entries of its folder are listed.
    shallow: bool,
    state_dir: bool,
}

impl Scope {
    fn item(&self, abs: &Path, name: &OsStr, kind: NodeKind) -> Option<WalkItem> {
        let rest = strip_root(abs, &self.root)?;
        let segs: Vec<_> = rest.iter().map(|s| s.to_string_lossy()).collect();
        Some(WalkItem {
            abs: abs.to_path_buf(),
            rel: format!("{}{}", self.prefix, segs.join("/")),
            name: name.to_string_lossy().into_owned(),
            kind,
            large: None,
        })
    }

    fn is_skipped(&self, path: &Path) -> bool {
        self.skip.iter().any(|n| same_path(path, n))
    }

    /// The regular walk: what git would show, nested repositories included.
    fn list(&self, out: &mut Vec<WalkItem>) {
        let mut pending: Vec<(PathBuf, bool)> = vec![(self.root.clone(), false)];
        let mut seen: HashSet<PathBuf> = HashSet::new();
        while let Some((start, nested)) = pending.pop() {
            let Some(found) = self.list_one(&start, nested, out) else {
                return;
            };
            for repo in found {
                if !seen.insert(repo.clone()) {
                    continue;
                }
                let name = repo.file_name().unwrap_or_default().to_os_string();
                if let Some(item) = self.item(&repo, &name, NodeKind::Dir) {
                    out.push(item);
                }
                pending.push((repo, true));
            }
        }
    }

    /// Walks one repository (or the root) under its own ignore rules. Nested repositories are not entered;
    /// they come back to be walked on their own. `None` when the entry limit was reached.
    fn list_one(
        &self,
        start: &Path,
        nested: bool,
        out: &mut Vec<WalkItem>,
    ) -> Option<Vec<PathBuf>> {
        let mut builder = WalkBuilder::new(start);
        builder
            .follow_links(false)
            .hidden(false)
            .require_git(false)
            // Simple mode opened a single file: the folder it lives in (often Downloads or the
            // home folder) is not a project, so only its direct entries are listed.
            .max_depth(self.shallow.then_some(1));
        if nested {
            // A repository is not part of the project around it: that project's `.gitignore` files do not apply.
            builder.parents(false);
        }
        let skip = self.skip.clone();
        let (state_dir, shallow) = (self.state_dir, self.shallow);
        builder.filter_entry(move |e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            let parent = e
                .path()
                .parent()
                .and_then(Path::file_name)
                .map(|n| n.to_string_lossy());
            if left_out(&name, parent.as_deref(), state_dir, true)
                || skip.iter().any(|n| same_path(e.path(), n))
            {
                return false;
            }
            !(!shallow && e.file_type().is_some_and(|t| t.is_dir()) && holds_repo(e.path()))
        });

        let mut dirs = vec![start.to_path_buf()];
        for entry in builder.build().filter_map(Result::ok) {
            if entry.depth() == 0 {
                continue;
            }
            if out.len() >= MAX_ENTRIES {
                return None;
            }
            let Some(ft) = entry.file_type() else {
                continue;
            };
            let Some(item) = self.item(entry.path(), entry.file_name(), kind_of(ft)) else {
                continue;
            };
            if item.kind == NodeKind::Dir {
                dirs.push(item.abs.clone());
            }
            out.push(item);
        }
        Some(if self.shallow {
            Vec::new()
        } else {
            self.nested_repos(&dirs)
        })
    }

    /// Repositories that sit directly in any of the folders: the ones the walk entered, and the ones it
    /// could not see because the project ignores them.
    fn nested_repos(&self, dirs: &[PathBuf]) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                let name = entry.file_name();
                let path = entry.path();
                if left_out(&name.to_string_lossy(), None, true, true)
                    || self.is_skipped(&path)
                    || !holds_repo(&path)
                {
                    continue;
                }
                found.push(path);
            }
        }
        found.sort();
        found.dedup();
        found
    }

    /// What the regular walk left out because git ignores it. `known` is everything the regular walk listed.
    fn list_ignored(&self, known: &HashSet<PathBuf>, out: &mut Vec<WalkItem>) {
        // The folder that grew past the cap: the walker is told to leave the rest of it alone.
        let closed: Arc<Mutex<Option<PathBuf>>> = Arc::default();
        let gate = Arc::clone(&closed);
        let skip = self.skip.clone();
        let state_dir = self.state_dir;
        let walker = WalkBuilder::new(&self.root)
            .standard_filters(false)
            .follow_links(false)
            .max_depth(self.shallow.then_some(1))
            .filter_entry(move |e| {
                if e.depth() == 0 {
                    return true;
                }
                let name = e.file_name().to_string_lossy();
                let parent = e
                    .path()
                    .parent()
                    .and_then(Path::file_name)
                    .map(|n| n.to_string_lossy());
                !(left_out(&name, parent.as_deref(), state_dir, false)
                    || skip.iter().any(|n| same_path(e.path(), n))
                    || gate
                        .lock()
                        .ok()
                        .and_then(|g| g.clone())
                        .is_some_and(|dir| e.path().starts_with(dir)))
            })
            .build();

        // The ignored folder being listed: where its item sits in `out` and how many entries it holds so far.
        let mut open: Option<OpenDir> = None;
        for entry in walker.filter_map(Result::ok) {
            if entry.depth() == 0 {
                continue;
            }
            if out.len() >= MAX_ENTRIES {
                return;
            }
            let path = entry.path();
            if open.as_ref().is_some_and(|o| !path.starts_with(&o.dir)) {
                open = None;
                set_gate(&closed, None);
            }
            if let Some(o) = open.as_mut() {
                o.count += 1;
                if o.count == IGNORED_FOLDER_CAP + 1 {
                    out.truncate(o.at + 1);
                    out[o.at].large = Some(IGNORED_FOLDER_CAP);
                    set_gate(&closed, Some(o.dir.clone()));
                }
                if o.count > IGNORED_FOLDER_CAP {
                    continue;
                }
            } else if known.contains(path) {
                continue;
            }
            let Some(ft) = entry.file_type() else {
                continue;
            };
            let Some(item) = self.item(path, entry.file_name(), kind_of(ft)) else {
                continue;
            };
            if open.is_none() && item.kind == NodeKind::Dir {
                open = Some(OpenDir {
                    dir: item.abs.clone(),
                    at: out.len(),
                    count: 0,
                });
            }
            out.push(item);
        }
    }
}

impl Vault {
    /// Walks every root as the regular walk does (see the module docs), never following links.
    /// Directories come before their contents. Worktree roots nested inside the main root are skipped
    /// there and walked as their own root.
    pub fn walk(&self) -> Vec<WalkItem> {
        self.walk_with(TreeOptions::default())
    }

    pub(crate) fn walk_with(&self, opts: TreeOptions) -> Vec<WalkItem> {
        let mut out = Vec::new();
        for (idx, root) in self.root_paths().into_iter().enumerate() {
            let scope = Scope {
                root: root.to_path_buf(),
                prefix: self.prefix_of(idx),
                skip: self
                    .worktrees()
                    .iter()
                    .map(|w| w.path.clone())
                    .filter(|p| !same_path(p, root) && strip_root(p, root).is_some())
                    .collect(),
                shallow: self.is_shallow(),
                state_dir: opts.show_state_dir,
            };
            let first = out.len();
            scope.list(&mut out);
            if opts.show_ignored {
                let known: HashSet<PathBuf> = out[first..].iter().map(|i| i.abs.clone()).collect();
                scope.list_ignored(&known, &mut out);
            }
        }
        out
    }

    /// The tree for the UI with the regular walk.
    #[cfg(test)]
    pub fn tree(&self) -> TreeNode {
        self.tree_with(TreeOptions::default())
    }

    /// The tree for the UI: the main root, with worktrees as extra top-level children (`@wt/<name>`).
    pub fn tree_with(&self, opts: TreeOptions) -> TreeNode {
        let mut children_of: HashMap<String, Vec<Entry>> = HashMap::new();
        for item in self.walk_with(opts) {
            children_of
                .entry(parent_of(&item.rel).to_owned())
                .or_default()
                .push(Entry {
                    name: item.name,
                    path: item.rel,
                    kind: item.kind,
                    large: item.large,
                });
        }
        let mut root = build(
            Entry {
                name: self.name(),
                path: String::new(),
                kind: NodeKind::Dir,
                large: None,
            },
            &mut children_of,
        );
        let wt_nodes = self.worktrees().iter().map(|w| {
            build(
                Entry {
                    name: w.name.clone(),
                    path: format!("{WORKTREE_PREFIX}{}", w.name),
                    kind: NodeKind::Worktree,
                    large: None,
                },
                &mut children_of,
            )
        });
        root.children.get_or_insert_with(Vec::new).extend(wt_nodes);
        root
    }
}

struct Entry {
    name: String,
    path: String,
    kind: NodeKind,
    large: Option<usize>,
}

fn sort_children(kids: &mut [Entry]) {
    kids.sort_by_cached_key(|e| {
        (
            u8::from(e.kind != NodeKind::Dir),
            e.name.to_lowercase(),
            e.name.clone(),
        )
    });
}

fn build(entry: Entry, children_of: &mut HashMap<String, Vec<Entry>>) -> TreeNode {
    let children = matches!(entry.kind, NodeKind::Dir | NodeKind::Worktree).then(|| {
        let mut kids = children_of.remove(&entry.path).unwrap_or_default();
        sort_children(&mut kids);
        kids.into_iter().map(|k| build(k, children_of)).collect()
    });
    TreeNode {
        name: entry.name,
        path: entry.path,
        kind: entry.kind,
        children,
        large: entry.large,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::{Vault, Worktree};
    use std::fs;

    fn write(root: &Path, rel: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        fs::write(p, "x").expect("write");
    }

    fn write_text(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        fs::write(p, text).expect("write");
    }

    /// Makes `dir` look like the root of a repository.
    fn repo(root: &Path, dir: &str) {
        write(root, &format!("{dir}/.git/HEAD"));
    }

    fn names(node: &TreeNode) -> Vec<String> {
        node.children
            .as_ref()
            .map(|c| c.iter().map(|n| n.name.clone()).collect())
            .unwrap_or_default()
    }

    fn child<'a>(node: &'a TreeNode, name: &str) -> &'a TreeNode {
        node.children
            .as_ref()
            .and_then(|c| c.iter().find(|n| n.name == name))
            .unwrap_or_else(|| panic!("{name} in {:?}", names(node)))
    }

    fn rels(v: &Vault, opts: TreeOptions) -> Vec<String> {
        let mut paths: Vec<String> = v.walk_with(opts).into_iter().map(|i| i.rel).collect();
        paths.sort();
        paths
    }

    const ALL: TreeOptions = TreeOptions {
        show_ignored: true,
        show_state_dir: false,
    };

    #[test]
    fn tree_sorts_dirs_first_then_case_insensitively() {
        let d = tempfile::tempdir().expect("tempdir");
        for f in ["b.md", "A.md", "zeta/x.md", "Alpha/y.md", "c.md"] {
            write(d.path(), f);
        }
        let t = Vault::open(d.path()).expect("open").tree();
        assert_eq!(t.path, "");
        assert_eq!(t.kind, NodeKind::Dir);
        assert_eq!(names(&t), ["Alpha", "zeta", "A.md", "b.md", "c.md"]);
        let alpha = &t.children.as_ref().expect("children")[0];
        assert_eq!(alpha.path, "Alpha");
        assert_eq!(
            alpha.children.as_ref().expect("children")[0].path,
            "Alpha/y.md"
        );
    }

    #[test]
    fn tree_hides_git_trailmark_and_node_modules_but_shows_other_dotfolders() {
        let d = tempfile::tempdir().expect("tempdir");
        for f in [
            ".git/HEAD",
            ".trailmark/config.toml",
            "node_modules/x/y.js",
            ".claude/plan.md",
            "a.md",
        ] {
            write(d.path(), f);
        }
        let t = Vault::open(d.path()).expect("open").tree();
        assert_eq!(names(&t), [".claude", "a.md"]);
    }

    #[test]
    fn tree_respects_gitignore_without_a_git_repo() {
        let d = tempfile::tempdir().expect("tempdir");
        write(d.path(), ".gitignore");
        fs::write(d.path().join(".gitignore"), "build/\n*.log\n").expect("write");
        for f in ["build/out.md", "debug.log", "keep.md"] {
            write(d.path(), f);
        }
        let t = Vault::open(d.path()).expect("open").tree();
        assert_eq!(names(&t), [".gitignore", "keep.md"]);
    }

    #[test]
    fn worktrees_are_included_even_when_gitignored() {
        let d = tempfile::tempdir().expect("tempdir");
        fs::write(d.path().join(".gitignore"), ".claude/worktrees/\n").expect("write");
        write(d.path(), "a.md");
        write(d.path(), ".claude/worktrees/feat/plan.md");
        let v = Vault::open(d.path())
            .expect("open")
            .with_worktrees(vec![Worktree {
                name: "feat".to_owned(),
                path: d.path().join(".claude").join("worktrees").join("feat"),
                branch: None,
            }]);
        let t = v.tree();
        let wt = t
            .children
            .as_ref()
            .expect("children")
            .iter()
            .find(|n| n.kind == NodeKind::Worktree)
            .expect("worktree node");
        assert_eq!(wt.path, "@wt/feat");
        assert_eq!(names(wt), ["plan.md"]);
        assert_eq!(
            wt.children.as_ref().expect("children")[0].path,
            "@wt/feat/plan.md"
        );
        // Not duplicated inside the main tree.
        assert!(
            !names(&t).contains(&".claude".to_owned()) || {
                let claude = t
                    .children
                    .as_ref()
                    .expect("children")
                    .iter()
                    .find(|n| n.name == ".claude")
                    .expect("claude");
                names(claude).is_empty()
            }
        );
    }

    #[test]
    fn nested_worktree_is_not_duplicated_in_the_main_tree() {
        let d = tempfile::tempdir().expect("tempdir");
        write(d.path(), "a.md");
        write(d.path(), "wts/feat/plan.md");
        let v = Vault::open(d.path())
            .expect("open")
            .with_worktrees(vec![Worktree {
                name: "feat".to_owned(),
                path: d.path().join("wts").join("feat"),
                branch: None,
            }]);
        let paths: Vec<String> = v.walk().into_iter().map(|i| i.rel).collect();
        assert!(paths.contains(&"@wt/feat/plan.md".to_owned()));
        assert!(!paths.contains(&"wts/feat/plan.md".to_owned()));
        assert!(!paths.contains(&"wts/feat".to_owned()));
    }

    #[test]
    fn a_worktree_is_not_taken_for_a_nested_repository() {
        // A linked worktree has a `.git` file, not a folder. It stays a root of its own, ignored or not.
        let d = tempfile::tempdir().expect("tempdir");
        fs::write(d.path().join(".gitignore"), "wts/\n").expect("write");
        write(d.path(), "a.md");
        write(d.path(), "wts/feat/plan.md");
        write_text(
            d.path(),
            "wts/feat/.git",
            "gitdir: ../../.git/worktrees/feat\n",
        );
        let v = Vault::open(d.path())
            .expect("open")
            .with_worktrees(vec![Worktree {
                name: "feat".to_owned(),
                path: d.path().join("wts").join("feat"),
                branch: None,
            }]);
        assert_eq!(
            rels(&v, TreeOptions::default()),
            [".gitignore", "@wt/feat/plan.md", "a.md"]
        );
        let t = v.tree();
        assert_eq!(names(&t), [".gitignore", "a.md", "feat"]);
        assert_eq!(child(&t, "feat").kind, NodeKind::Worktree);
    }

    #[test]
    fn simple_mode_lists_only_the_files_own_folder() {
        let d = tempfile::tempdir().expect("tempdir");
        for f in ["notes/todo.md", "notes/other.md", "notes/sub/deep.md"] {
            write(d.path(), f);
        }
        let v = Vault::open(&d.path().join("notes").join("todo.md")).expect("open");
        assert!(v.is_shallow());
        let t = v.tree();
        assert_eq!(names(&t), ["sub", "other.md", "todo.md"]);
        let sub = &t.children.as_ref().expect("children")[0];
        assert!(sub.children.as_ref().is_none_or(Vec::is_empty), "{sub:?}");
        // Links to nested documents still resolve.
        assert!(v.resolve("sub/deep.md").expect("resolve").abs.is_file());
    }

    #[test]
    fn serializes_to_the_ui_contract() {
        let node = TreeNode {
            name: "a.md".to_owned(),
            path: "a.md".to_owned(),
            kind: NodeKind::File,
            children: None,
            large: None,
        };
        let json = serde_json::to_value(&node).expect("json");
        assert_eq!(
            json,
            serde_json::json!({"name": "a.md", "path": "a.md", "kind": "file"})
        );
        let big = TreeNode {
            name: "target".to_owned(),
            path: "target".to_owned(),
            kind: NodeKind::Dir,
            children: Some(Vec::new()),
            large: Some(2000),
        };
        assert_eq!(
            serde_json::to_value(&big).expect("json"),
            serde_json::json!({
                "name": "target", "path": "target", "kind": "dir", "children": [], "large": 2000
            })
        );
    }

    // ---- loose files and nested repositories --------------------------------------------------------

    /// A project folder that git ignores three sibling repositories of: the folder reported with a script
    /// and a README next to them.
    fn team_folder(d: &Path) {
        write_text(d, ".gitignore", "Design/\nEngineering/\nArt/\n");
        for f in ["README.md", "CLAUDE.md", "start-team.bat"] {
            write(d, f);
        }
        for sub in ["Design", "Engineering", "Art"] {
            repo(d, sub);
            write(d, &format!("{sub}/notes.md"));
        }
    }

    #[test]
    fn every_kind_of_file_is_listed_not_only_documents() {
        let d = tempfile::tempdir().expect("tempdir");
        team_folder(d.path());
        for f in [
            "Makefile",
            "run.sh",
            "data.bin",
            "no-extension",
            "photo.JPG",
        ] {
            write(d.path(), f);
        }
        let t = Vault::open(d.path()).expect("open").tree();
        let files: Vec<_> = t
            .children
            .as_ref()
            .expect("children")
            .iter()
            .filter(|n| n.kind == NodeKind::File)
            .map(|n| n.name.as_str())
            .collect();
        assert_eq!(
            files,
            [
                ".gitignore",
                "CLAUDE.md",
                "data.bin",
                "Makefile",
                "no-extension",
                "photo.JPG",
                "README.md",
                "run.sh",
                "start-team.bat"
            ]
        );
    }

    #[test]
    fn a_nested_repository_the_project_ignores_is_still_in_the_tree() {
        let d = tempfile::tempdir().expect("tempdir");
        team_folder(d.path());
        let v = Vault::open(d.path()).expect("open");
        let t = v.tree();
        assert_eq!(
            names(&t),
            [
                "Art",
                "Design",
                "Engineering",
                ".gitignore",
                "CLAUDE.md",
                "README.md",
                "start-team.bat"
            ]
        );
        let design = child(&t, "Design");
        assert_eq!(design.kind, NodeKind::Dir);
        assert_eq!(design.path, "Design");
        assert_eq!(names(design), ["notes.md"]);
        assert_eq!(
            design.children.as_ref().expect("children")[0].path,
            "Design/notes.md"
        );
        // The walk (index, tags, quick open) sees the same files, and never the repository's internals.
        let paths = rels(&v, TreeOptions::default());
        assert!(paths.contains(&"Art/notes.md".to_owned()), "{paths:?}");
        assert!(paths.iter().all(|p| !p.contains(".git/")), "{paths:?}");
    }

    #[test]
    fn a_nested_repository_applies_its_own_ignore_rules_and_not_the_projects() {
        let d = tempfile::tempdir().expect("tempdir");
        // The project ignores `*.log` and the repository; the repository ignores `secret.md` and `private/`.
        write_text(d.path(), ".gitignore", "Design/\n*.log\n");
        repo(d.path(), "Design");
        write_text(d.path(), "Design/.gitignore", "secret.md\n");
        write_text(d.path(), "Design/.git/info/exclude", "private/\n");
        for f in [
            "Design/notes.md",
            "Design/secret.md",
            "Design/debug.log",
            "Design/private/x.md",
            "Design/sub/deep.md",
            "Design/sub/secret.md",
            "app.log",
        ] {
            write(d.path(), f);
        }
        let v = Vault::open(d.path()).expect("open");
        assert_eq!(
            rels(&v, TreeOptions::default()),
            [
                ".gitignore",
                "Design",
                "Design/.gitignore",
                "Design/debug.log",
                "Design/notes.md",
                "Design/sub",
                "Design/sub/deep.md",
            ]
        );
    }

    #[test]
    fn a_nested_repository_that_is_not_ignored_also_uses_only_its_own_rules() {
        let d = tempfile::tempdir().expect("tempdir");
        write_text(d.path(), ".gitignore", "*.log\nbuild/\n");
        repo(d.path(), "lib");
        for f in [
            "lib/a.log",
            "lib/build/out.md",
            "lib/a.md",
            "top.log",
            "top.md",
        ] {
            write(d.path(), f);
        }
        let v = Vault::open(d.path()).expect("open");
        assert_eq!(
            rels(&v, TreeOptions::default()),
            [
                ".gitignore",
                "lib",
                "lib/a.log",
                "lib/a.md",
                "lib/build",
                "lib/build/out.md",
                "top.md"
            ]
        );
    }

    #[test]
    fn repositories_nested_in_repositories_are_found_at_any_depth() {
        let d = tempfile::tempdir().expect("tempdir");
        write_text(d.path(), ".gitignore", "outer/\n");
        repo(d.path(), "outer");
        write_text(d.path(), "outer/.gitignore", "inner/\n");
        repo(d.path(), "outer/docs/inner");
        write(d.path(), "outer/docs/inner/deep.md");
        write(d.path(), "outer/readme.md");
        let v = Vault::open(d.path()).expect("open");
        let paths = rels(&v, TreeOptions::default());
        assert!(
            paths.contains(&"outer/docs/inner/deep.md".to_owned()),
            "{paths:?}"
        );
        assert!(paths.contains(&"outer/readme.md".to_owned()), "{paths:?}");
        // Each folder is listed once.
        let mut unique = paths.clone();
        unique.dedup();
        assert_eq!(unique, paths);
    }

    #[test]
    fn a_nested_repository_may_be_a_submodule_with_a_git_file() {
        let d = tempfile::tempdir().expect("tempdir");
        write_text(d.path(), ".gitignore", "vendor/\n");
        write_text(
            d.path(),
            "vendor/lib/.git",
            "gitdir: ../../.git/modules/lib\n",
        );
        write(d.path(), "vendor/lib/readme.md");
        let v = Vault::open(d.path()).expect("open");
        // `vendor/` itself is ignored and holds no repository of its own: the submodule inside it is not looked for.
        assert_eq!(rels(&v, TreeOptions::default()), [".gitignore"]);

        let e = tempfile::tempdir().expect("tempdir");
        write_text(e.path(), ".gitignore", "lib/\n");
        write_text(e.path(), "lib/.git", "gitdir: ../.git/modules/lib\n");
        write(e.path(), "lib/readme.md");
        let v = Vault::open(e.path()).expect("open");
        assert_eq!(
            rels(&v, TreeOptions::default()),
            [".gitignore", "lib", "lib/readme.md"]
        );
    }

    #[test]
    fn nested_repositories_work_in_a_worktree_too() {
        let d = tempfile::tempdir().expect("tempdir");
        write(d.path(), "a.md");
        write_text(d.path(), "wts/feat/.gitignore", "Design/\n");
        write(d.path(), "wts/feat/plan.md");
        repo(d.path(), "wts/feat/Design");
        write(d.path(), "wts/feat/Design/notes.md");
        let v = Vault::open(d.path())
            .expect("open")
            .with_worktrees(vec![Worktree {
                name: "feat".to_owned(),
                path: d.path().join("wts").join("feat"),
                branch: None,
            }]);
        let paths = rels(&v, TreeOptions::default());
        assert!(
            paths.contains(&"@wt/feat/Design/notes.md".to_owned()),
            "{paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p.starts_with("wts/")),
            "not listed twice: {paths:?}"
        );
    }

    #[test]
    fn simple_mode_does_not_look_for_nested_repositories() {
        let d = tempfile::tempdir().expect("tempdir");
        write_text(d.path(), "notes/.gitignore", "Design/\n");
        write(d.path(), "notes/todo.md");
        repo(d.path(), "notes/Design");
        write(d.path(), "notes/Design/x.md");
        let v = Vault::open(&d.path().join("notes").join("todo.md")).expect("open");
        assert_eq!(rels(&v, TreeOptions::default()), [".gitignore", "todo.md"]);
    }

    // ---- ignored files ----------------------------------------------------------------------------

    #[test]
    fn ignored_files_are_listed_only_on_request() {
        let d = tempfile::tempdir().expect("tempdir");
        write_text(d.path(), ".gitignore", "build/\n*.log\n.env\n");
        for f in [
            "build/out.md",
            "build/sub/deep.txt",
            "debug.log",
            ".env",
            "keep.md",
            ".git/HEAD",
            ".git/objects/ab/cd",
        ] {
            write(d.path(), f);
        }
        let v = Vault::open(d.path()).expect("open");
        assert_eq!(rels(&v, TreeOptions::default()), [".gitignore", "keep.md"]);
        assert_eq!(
            rels(&v, ALL),
            [
                ".env",
                ".gitignore",
                "build",
                "build/out.md",
                "build/sub",
                "build/sub/deep.txt",
                "debug.log",
                "keep.md"
            ]
        );
        let t = v.tree_with(ALL);
        assert_eq!(
            names(&t),
            ["build", ".env", ".gitignore", "debug.log", "keep.md"]
        );
        assert_eq!(child(&t, "build").large, None);
    }

    #[test]
    fn a_file_excluded_locally_is_an_ignored_file_too() {
        // `.git/info/exclude` is where a person keeps a local-only script out of the repository. Nothing in
        // `.gitignore` names it, yet git (and so the tree) leaves it out until ignored files are asked for.
        let d = tempfile::tempdir().expect("tempdir");
        write_text(d.path(), ".git/info/exclude", "start-team.bat\n");
        write(d.path(), ".git/HEAD");
        for f in ["README.md", "start-team.bat"] {
            write(d.path(), f);
        }
        let v = Vault::open(d.path()).expect("open");
        assert_eq!(rels(&v, TreeOptions::default()), ["README.md"]);
        assert_eq!(rels(&v, ALL), ["README.md", "start-team.bat"]);
    }

    #[test]
    fn ignored_files_of_a_nested_repository_are_listed_on_request() {
        let d = tempfile::tempdir().expect("tempdir");
        write_text(d.path(), ".gitignore", "Design/\n");
        repo(d.path(), "Design");
        write_text(d.path(), "Design/.gitignore", "secret.md\n");
        write(d.path(), "Design/notes.md");
        write(d.path(), "Design/secret.md");
        let v = Vault::open(d.path()).expect("open");
        let plain = rels(&v, TreeOptions::default());
        assert!(!plain.contains(&"Design/secret.md".to_owned()), "{plain:?}");
        let all = rels(&v, ALL);
        assert!(all.contains(&"Design/secret.md".to_owned()), "{all:?}");
        assert!(all.contains(&"Design/notes.md".to_owned()), "{all:?}");
        assert!(all.iter().all(|p| !p.contains(".git/")), "{all:?}");
        // Each file once.
        let mut unique = all.clone();
        unique.dedup();
        assert_eq!(unique, all);
    }

    #[test]
    fn a_huge_ignored_folder_is_listed_unopened() {
        let d = tempfile::tempdir().expect("tempdir");
        write_text(d.path(), ".gitignore", "target/\nout/\n");
        write(d.path(), "keep.md");
        write(d.path(), "out/result.txt");
        for i in 0..IGNORED_FOLDER_CAP + 50 {
            write(d.path(), &format!("target/debug/deps/f{i}.o"));
        }
        write(d.path(), "zz-after.md");
        let v = Vault::open(d.path()).expect("open");
        let t = v.tree_with(ALL);
        let target = child(&t, "target");
        assert_eq!(target.large, Some(IGNORED_FOLDER_CAP));
        assert!(
            target.children.as_ref().is_none_or(Vec::is_empty),
            "{target:?}"
        );
        // The small ignored folder is listed in full, and what comes after the big one is not lost.
        assert_eq!(names(child(&t, "out")), ["result.txt"]);
        assert_eq!(child(&t, "out").large, None);
        assert!(names(&t).contains(&"zz-after.md".to_owned()));
        assert!(names(&t).contains(&"keep.md".to_owned()));
        // The count of entries the walk kept is small.
        assert!(v.walk_with(ALL).len() < 20, "{}", v.walk_with(ALL).len());
    }

    #[test]
    fn a_dependency_folder_is_listed_unopened_only_when_ignored_files_are_shown() {
        let d = tempfile::tempdir().expect("tempdir");
        write_text(d.path(), ".gitignore", "node_modules/\n");
        write(d.path(), "a.md");
        write(d.path(), "node_modules/pkg/index.js");
        let v = Vault::open(d.path()).expect("open");
        assert_eq!(names(&v.tree()), [".gitignore", "a.md"]);
        let t = v.tree_with(ALL);
        assert_eq!(names(&t), ["node_modules", ".gitignore", "a.md"]);
    }

    #[test]
    fn worktrees_stay_roots_of_their_own_when_ignored_files_are_shown() {
        let d = tempfile::tempdir().expect("tempdir");
        write_text(d.path(), ".gitignore", ".claude/worktrees/\n");
        write(d.path(), "a.md");
        write(d.path(), ".claude/worktrees/feat/plan.md");
        let v = Vault::open(d.path())
            .expect("open")
            .with_worktrees(vec![Worktree {
                name: "feat".to_owned(),
                path: d.path().join(".claude").join("worktrees").join("feat"),
                branch: None,
            }]);
        let paths = rels(&v, ALL);
        assert!(paths.contains(&"@wt/feat/plan.md".to_owned()), "{paths:?}");
        assert!(
            !paths.contains(&".claude/worktrees/feat/plan.md".to_owned()),
            "{paths:?}"
        );
    }

    // ---- the state folder -------------------------------------------------------------------------

    #[test]
    fn the_state_folder_lists_its_committed_files_only_on_request() {
        let d = tempfile::tempdir().expect("tempdir");
        for f in [
            ".trailmark/config.toml",
            ".trailmark/meta.toml",
            ".trailmark/cache/index.bin",
            ".trailmark/local/session.json",
            "a.md",
        ] {
            write(d.path(), f);
        }
        let v = Vault::open(d.path()).expect("open");
        let with_state = TreeOptions {
            show_ignored: false,
            show_state_dir: true,
        };
        assert_eq!(
            rels(&v, with_state),
            [
                ".trailmark",
                ".trailmark/config.toml",
                ".trailmark/meta.toml",
                "a.md"
            ]
        );
        assert_eq!(rels(&v, TreeOptions::default()), ["a.md"]);
    }

    // ---- links ---------------------------------------------------------------------------------------

    #[cfg(unix)]
    #[test]
    fn symlinks_are_link_nodes_and_not_followed() {
        let d = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("tempdir");
        write(outside.path(), "secret.md");
        write(d.path(), "a.md");
        std::os::unix::fs::symlink(outside.path(), d.path().join("link")).expect("symlink");
        let t = Vault::open(d.path()).expect("open").tree();
        let link = t
            .children
            .as_ref()
            .expect("children")
            .iter()
            .find(|n| n.name == "link")
            .expect("link");
        assert_eq!(link.kind, NodeKind::Link);
        assert!(link.children.is_none());
    }

    /// `mklink /J` needs no privileges, unlike symlinks.
    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) {
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .expect("mklink");
        assert!(status.status.success(), "{status:?}");
    }

    #[cfg(windows)]
    #[test]
    fn junctions_are_not_followed_out_of_the_vault() {
        let d = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("tempdir");
        write(outside.path(), "secret.md");
        write(d.path(), "a.md");
        junction(&d.path().join("link"), outside.path());
        let v = Vault::open(d.path()).expect("open");
        let t = v.tree();
        let link = t
            .children
            .as_ref()
            .expect("children")
            .iter()
            .find(|n| n.name == "link")
            .expect("the junction itself is listed");
        assert!(
            link.children.as_ref().is_none_or(Vec::is_empty),
            "must not list content outside the vault: {link:?}"
        );
        assert!(v.walk().iter().all(|i| !i.rel.contains("secret")));
        assert!(matches!(
            v.resolve("link/secret.md"),
            Err(crate::vault::VaultError::Escapes(_))
        ));
        fs::remove_dir(d.path().join("link")).expect("remove junction");
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_to_a_repository_outside_is_not_walked_as_a_nested_repository() {
        let d = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("tempdir");
        repo(outside.path(), ".");
        write(outside.path(), "secret.md");
        write(d.path(), "a.md");
        junction(&d.path().join("link"), outside.path());
        let v = Vault::open(d.path()).expect("open");
        assert!(rels(&v, ALL).iter().all(|p| !p.contains("secret")));
        fs::remove_dir(d.path().join("link")).expect("remove junction");
    }
}
