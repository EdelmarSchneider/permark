//! Folder walking and the file tree sent to the UI.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use serde::Serialize;

use super::{strip_root, Vault, WORKTREE_PREFIX};

/// Folders never shown in the tree nor indexed. `.trailmark` holds our own state.
const HARD_EXCLUDES: [&str; 3] = [".git", ".trailmark", "node_modules"];
/// Safety valve for pathological folders.
const MAX_ENTRIES: usize = 100_000;

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
}

/// One entry found by [`Vault::walk`].
#[derive(Debug, Clone)]
pub struct WalkItem {
    pub abs: PathBuf,
    /// Vault-relative path in the UI form.
    pub rel: String,
    pub name: String,
    pub kind: NodeKind,
}

fn same_path(a: &Path, b: &Path) -> bool {
    strip_root(a, b).is_some_and(|rest| rest.is_empty())
}

fn parent_of(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(parent, _)| parent)
}

impl Vault {
    /// Walks every root, honoring `.gitignore`/`.ignore` (also outside a git repository), never
    /// following links. Directories come before their contents. Worktree roots nested inside the
    /// main root are skipped there and walked as their own root.
    pub fn walk(&self) -> Vec<WalkItem> {
        let mut out = Vec::new();
        for (idx, root) in self.root_paths().into_iter().enumerate() {
            let nested: Vec<PathBuf> = self
                .worktrees()
                .iter()
                .map(|w| w.path.clone())
                .filter(|p| !same_path(p, root) && strip_root(p, root).is_some())
                .collect();
            let walker = WalkBuilder::new(root)
                .follow_links(false)
                .hidden(false)
                .require_git(false)
                // Simple mode opened a single file: the folder it lives in (often Downloads or the
                // home folder) is not a project, so only its direct entries are listed.
                .max_depth(self.is_shallow().then_some(1))
                .filter_entry(move |e| {
                    if e.depth() == 0 {
                        return true;
                    }
                    let name = e.file_name().to_string_lossy();
                    !HARD_EXCLUDES.iter().any(|x| name.eq_ignore_ascii_case(x))
                        && !nested.iter().any(|n| same_path(e.path(), n))
                })
                .build();
            let prefix = self.prefix_of(idx);
            for entry in walker.filter_map(Result::ok) {
                if entry.depth() == 0 {
                    continue;
                }
                if out.len() >= MAX_ENTRIES {
                    return out;
                }
                let Some(ft) = entry.file_type() else {
                    continue;
                };
                let Some(rest) = strip_root(entry.path(), root) else {
                    continue;
                };
                let segs: Vec<_> = rest.iter().map(|s| s.to_string_lossy()).collect();
                let kind = if ft.is_symlink() {
                    NodeKind::Link
                } else if ft.is_dir() {
                    NodeKind::Dir
                } else {
                    NodeKind::File
                };
                out.push(WalkItem {
                    abs: entry.path().to_path_buf(),
                    rel: format!("{prefix}{}", segs.join("/")),
                    name: entry.file_name().to_string_lossy().into_owned(),
                    kind,
                });
            }
        }
        out
    }

    /// The tree for the UI: the main root, with worktrees as extra top-level children (`@wt/<name>`).
    pub fn tree(&self) -> TreeNode {
        let mut children_of: HashMap<String, Vec<(String, String, NodeKind)>> = HashMap::new();
        for item in self.walk() {
            children_of
                .entry(parent_of(&item.rel).to_owned())
                .or_default()
                .push((item.name, item.rel, item.kind));
        }
        let mut root = build(String::new(), self.name(), NodeKind::Dir, &mut children_of);
        let wt_nodes = self.worktrees().iter().map(|w| {
            build(
                format!("{WORKTREE_PREFIX}{}", w.name),
                w.name.clone(),
                NodeKind::Worktree,
                &mut children_of,
            )
        });
        root.children.get_or_insert_with(Vec::new).extend(wt_nodes);
        root
    }
}

fn sort_children(kids: &mut [(String, String, NodeKind)]) {
    kids.sort_by_cached_key(|(name, _, kind)| {
        (
            u8::from(*kind != NodeKind::Dir),
            name.to_lowercase(),
            name.clone(),
        )
    });
}

fn build(
    path: String,
    name: String,
    kind: NodeKind,
    children_of: &mut HashMap<String, Vec<(String, String, NodeKind)>>,
) -> TreeNode {
    let children = matches!(kind, NodeKind::Dir | NodeKind::Worktree).then(|| {
        let mut kids = children_of.remove(&path).unwrap_or_default();
        sort_children(&mut kids);
        kids.into_iter()
            .map(|(n, p, k)| build(p, n, k, children_of))
            .collect()
    });
    TreeNode {
        name,
        path,
        kind,
        children,
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

    fn names(node: &TreeNode) -> Vec<String> {
        node.children
            .as_ref()
            .map(|c| c.iter().map(|n| n.name.clone()).collect())
            .unwrap_or_default()
    }

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
        };
        let json = serde_json::to_value(&node).expect("json");
        assert_eq!(
            json,
            serde_json::json!({"name": "a.md", "path": "a.md", "kind": "file"})
        );
    }

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
}
