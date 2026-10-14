//! Which filesystem events are noise: editor temp files, build folders, git internals, gitignored
//! paths and the user's extra `ignore` globs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// Folders that are always noise, wherever they appear.
const NOISE_DIRS: [&str; 2] = ["node_modules", "target"];

/// What to do with an event for a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Ignore,
    /// Not shown to the user, but it should wake the git worker (a `.git` entry appeared or went away).
    GitOnly,
    Emit,
}

/// Editor and tool scratch files: `*.tmp`, `*~`, `.#*`, `*.swp` (and its `.swo`/`.swx` siblings), Vim's
/// `4913`, and the hidden `.<name>.<pid>.<n>.part` files this app writes before it renames them into place.
pub fn is_temp_name(name: &str) -> bool {
    let has_ext = |wanted: &str| {
        Path::new(name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(wanted))
    };
    name.ends_with('~')
        || name.starts_with(".#")
        || name == "4913"
        || (name.starts_with('.') && has_ext("part"))
        || ["tmp", "swp", "swo", "swx"].into_iter().any(has_ext)
}

/// `.gitignore` files are loaded per folder on first use and cached; the cache is dropped when one changes.
///
/// A folder with a `.git` of its own is a repository nested in the project. The project's `.gitignore`
/// never hides it (the tree always lists it), and inside it only its own `.gitignore` files and
/// `.git/info/exclude` apply, not the project's.
#[derive(Debug)]
struct LazyGitignore {
    root: PathBuf,
    /// `.git/info/exclude`.
    base: Option<Gitignore>,
    /// The user's global excludes file; lowest priority.
    global: Option<Gitignore>,
    cache: HashMap<PathBuf, Option<Gitignore>>,
    /// Folders looked at for a `.git` of their own.
    repos: HashMap<PathBuf, bool>,
    /// `.git/info/exclude` of the nested repositories.
    excludes: HashMap<PathBuf, Option<Gitignore>>,
}

impl LazyGitignore {
    fn new(root: PathBuf, info_exclude: Option<&Path>) -> Self {
        let mut builder = GitignoreBuilder::new(&root);
        if let Some(path) = info_exclude.filter(|p| p.is_file()) {
            let _ = builder.add(path);
        }
        let base = builder.build().ok().filter(|g| !g.is_empty());
        let (global, _) = Gitignore::global();
        Self {
            root,
            base,
            global: (!global.is_empty()).then_some(global),
            cache: HashMap::new(),
            repos: HashMap::new(),
            excludes: HashMap::new(),
        }
    }

    fn for_dir(&mut self, dir: &Path) -> Option<&Gitignore> {
        self.cache
            .entry(dir.to_path_buf())
            .or_insert_with(|| {
                let file = dir.join(".gitignore");
                file.is_file().then(|| Gitignore::new(&file).0)
            })
            .as_ref()
    }

    fn is_repo(&mut self, dir: &Path) -> bool {
        *self
            .repos
            .entry(dir.to_path_buf())
            .or_insert_with(|| dir.join(".git").exists())
    }

    /// `.git/info/exclude` of a nested repository.
    fn exclude_of(&mut self, repo: &Path) -> Option<&Gitignore> {
        self.excludes
            .entry(repo.to_path_buf())
            .or_insert_with(|| {
                let file = repo.join(".git").join("info").join("exclude");
                file.is_file().then(|| {
                    let mut builder = GitignoreBuilder::new(repo);
                    let _ = builder.add(&file);
                    builder.build().unwrap_or_else(|_| Gitignore::empty())
                })
            })
            .as_ref()
    }

    /// Is the path, or any folder above it inside the root, ignored?
    fn is_ignored(&mut self, comps: &[&str], is_dir: bool) -> bool {
        let mut repo = self.root.clone();
        let mut path = self.root.clone();
        for (i, comp) in comps.iter().enumerate() {
            path.push(comp);
            let dir = i + 1 < comps.len() || is_dir;
            if dir && self.is_repo(&path) {
                repo.clone_from(&path);
                continue;
            }
            if self.matches(&repo, &path, dir) {
                return true;
            }
        }
        false
    }

    /// Deepest `.gitignore` first; the first file with an opinion wins. Only the files of the repository
    /// `repo` (the project root, or a repository nested in it) are consulted.
    fn matches(&mut self, repo: &Path, path: &Path, is_dir: bool) -> bool {
        let mut dirs: Vec<PathBuf> = path
            .ancestors()
            .skip(1)
            .take_while(|a| a.starts_with(repo))
            .map(Path::to_path_buf)
            .collect();
        if dirs.is_empty() {
            dirs.push(repo.to_path_buf());
        }
        for dir in dirs {
            if let Some(gi) = self.for_dir(&dir) {
                let m = gi.matched(path, is_dir);
                if m.is_ignore() {
                    return true;
                }
                if m.is_whitelist() {
                    return false;
                }
            }
        }
        let own = if repo == self.root {
            self.base.as_ref().map(|g| g.matched(path, is_dir))
        } else {
            self.exclude_of(repo).map(|g| g.matched(path, is_dir))
        };
        own.is_some_and(|m| m.is_ignore())
            || self
                .global
                .as_ref()
                .is_some_and(|g| g.matched(path, is_dir).is_ignore())
    }

    fn invalidate(&mut self) {
        self.cache.clear();
        self.repos.clear();
        self.excludes.clear();
    }
}

#[derive(Debug)]
struct RootFilter {
    gitignore: Mutex<LazyGitignore>,
    extra: Option<Gitignore>,
}

/// Decides, per root, which events matter.
#[derive(Debug)]
pub struct PathFilter {
    roots: Vec<RootFilter>,
}

impl PathFilter {
    /// `roots` are `(root path, optional .git/info/exclude)` in vault root order; `extra_ignore` are the
    /// user's globs from `.trailmark/config.toml`, applied relative to each root.
    pub fn new(roots: &[(PathBuf, Option<PathBuf>)], extra_ignore: &[String]) -> Self {
        let roots = roots
            .iter()
            .map(|(root, info_exclude)| {
                let extra = (!extra_ignore.is_empty())
                    .then(|| {
                        let mut b = GitignoreBuilder::new(root);
                        for glob in extra_ignore {
                            let _ = b.add_line(None, glob);
                        }
                        b.build().ok()
                    })
                    .flatten();
                RootFilter {
                    gitignore: Mutex::new(LazyGitignore::new(
                        root.clone(),
                        info_exclude.as_deref(),
                    )),
                    extra,
                }
            })
            .collect();
        Self { roots }
    }

    fn forget_repos(&self, root: usize) {
        if let Some(Ok(mut gi)) = self.roots.get(root).map(|rf| rf.gitignore.lock()) {
            gi.invalidate();
        }
    }

    /// `inner` is the path relative to the root of index `root`, `/`-separated.
    pub fn classify(&self, root: usize, inner: &str, is_dir: bool) -> Verdict {
        let comps: Vec<&str> = inner.split('/').filter(|c| !c.is_empty()).collect();
        let Some(last) = comps.last().copied() else {
            return Verdict::Ignore;
        };
        if let Some(pos) = comps.iter().position(|c| c.eq_ignore_ascii_case(".git")) {
            if pos > 0 && pos + 1 == comps.len() {
                // A nested repository appeared or went away: what is remembered about it is stale.
                self.forget_repos(root);
            }
            return if pos == 0 && comps.len() == 1 {
                Verdict::GitOnly
            } else {
                Verdict::Ignore
            };
        }
        if comps
            .iter()
            .any(|c| NOISE_DIRS.iter().any(|n| c.eq_ignore_ascii_case(n)))
        {
            return Verdict::Ignore;
        }
        if comps.len() >= 2
            && comps[0].eq_ignore_ascii_case(".trailmark")
            && (comps[1].eq_ignore_ascii_case("cache") || comps[1].eq_ignore_ascii_case("local"))
        {
            return Verdict::Ignore;
        }
        if is_temp_name(last) {
            return Verdict::Ignore;
        }
        let Some(rf) = self.roots.get(root) else {
            return Verdict::Ignore;
        };
        let Ok(mut gi) = rf.gitignore.lock() else {
            return Verdict::Emit;
        };
        if last == ".gitignore" {
            gi.invalidate();
            return Verdict::Emit;
        }
        if let Some(extra) = &rf.extra {
            let mut path = gi.root.clone();
            path.extend(&comps);
            if extra.matched_path_or_any_parents(&path, is_dir).is_ignore() {
                return Verdict::Ignore;
            }
        }
        if gi.is_ignored(&comps, is_dir) {
            Verdict::Ignore
        } else {
            Verdict::Emit
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn filter(dir: &Path, extra: &[&str]) -> PathFilter {
        let extra: Vec<String> = extra.iter().map(|s| (*s).to_owned()).collect();
        PathFilter::new(&[(dir.to_path_buf(), None)], &extra)
    }

    #[test]
    fn temp_file_patterns() {
        for n in [
            "a.tmp",
            "A.TMP",
            "doc.md~",
            ".#doc.md",
            "doc.md.swp",
            ".doc.swo",
            "4913",
            ".meta.toml.1234.part",
        ] {
            assert!(is_temp_name(n), "{n}");
        }
        for n in [
            "a.md",
            "tmp.md",
            "4914",
            "notes~1.md",
            "swp.md",
            "my.templates",
            "download.part",
        ] {
            assert!(!is_temp_name(n), "{n}");
        }
    }

    #[test]
    fn git_internals_are_ignored_but_the_git_entry_itself_wakes_git() {
        let d = tempfile::tempdir().expect("tempdir");
        let f = filter(d.path(), &[]);
        assert_eq!(f.classify(0, ".git", true), Verdict::GitOnly);
        assert_eq!(f.classify(0, ".git/HEAD", false), Verdict::Ignore);
        assert_eq!(
            f.classify(0, ".git/objects/ab/cdef", false),
            Verdict::Ignore
        );
        assert_eq!(f.classify(0, "sub/.git/index", false), Verdict::Ignore);
        assert_eq!(f.classify(0, ".gitattributes", false), Verdict::Emit);
        assert_eq!(
            f.classify(0, ".github/workflows/ci.yml", false),
            Verdict::Emit
        );
    }

    #[test]
    fn build_folders_and_trailmark_state_are_ignored() {
        let d = tempfile::tempdir().expect("tempdir");
        let f = filter(d.path(), &[]);
        assert_eq!(f.classify(0, "node_modules/x/y.md", false), Verdict::Ignore);
        assert_eq!(f.classify(0, "app/target/debug/x", false), Verdict::Ignore);
        assert_eq!(
            f.classify(0, ".trailmark/cache/index.bin", false),
            Verdict::Ignore
        );
        assert_eq!(
            f.classify(0, ".trailmark/local/session.json", false),
            Verdict::Ignore
        );
        assert_eq!(
            f.classify(0, ".trailmark/config.toml", false),
            Verdict::Emit
        );
        assert_eq!(f.classify(0, "docs/a.md", false), Verdict::Emit);
    }

    #[test]
    fn temp_files_are_ignored_anywhere() {
        let d = tempfile::tempdir().expect("tempdir");
        let f = filter(d.path(), &[]);
        assert_eq!(f.classify(0, "docs/.a.md.swp", false), Verdict::Ignore);
        assert_eq!(f.classify(0, "4913", false), Verdict::Ignore);
        assert_eq!(f.classify(0, "docs/a.md~", false), Verdict::Ignore);
    }

    #[test]
    fn gitignored_paths_are_ignored_without_a_repo() {
        let d = tempfile::tempdir().expect("tempdir");
        fs::write(d.path().join(".gitignore"), "build/\n*.log\n!keep.log\n").expect("write");
        fs::create_dir_all(d.path().join("sub")).expect("mkdir");
        fs::write(d.path().join("sub").join(".gitignore"), "secret.md\n").expect("write");
        let f = filter(d.path(), &[]);
        assert_eq!(f.classify(0, "build/out.md", false), Verdict::Ignore);
        assert_eq!(f.classify(0, "build", true), Verdict::Ignore);
        assert_eq!(f.classify(0, "x/y/debug.log", false), Verdict::Ignore);
        assert_eq!(f.classify(0, "keep.log", false), Verdict::Emit);
        assert_eq!(f.classify(0, "sub/secret.md", false), Verdict::Ignore);
        assert_eq!(f.classify(0, "secret.md", false), Verdict::Emit);
        assert_eq!(f.classify(0, "docs/a.md", false), Verdict::Emit);
    }

    /// A project that ignores `Design/` (and `*.log`), where `Design/` is a repository with rules of its own.
    fn project_with_nested_repo(dir: &Path) {
        fs::write(dir.join(".gitignore"), "Design/\n*.log\n").expect("write");
        fs::create_dir_all(dir.join("Design").join(".git").join("info")).expect("mkdir");
        fs::write(dir.join("Design").join(".gitignore"), "secret.md\n").expect("write");
        fs::write(
            dir.join("Design").join(".git").join("info").join("exclude"),
            "private/\n",
        )
        .expect("write");
    }

    #[test]
    fn a_nested_repository_is_never_ignored_for_being_inside_the_project() {
        let d = tempfile::tempdir().expect("tempdir");
        project_with_nested_repo(d.path());
        let f = filter(d.path(), &[]);
        assert_eq!(f.classify(0, "Design", true), Verdict::Emit);
        assert_eq!(f.classify(0, "Design/notes.md", false), Verdict::Emit);
        assert_eq!(f.classify(0, "Design/sub/deep.md", false), Verdict::Emit);
        // The project's own rules still apply to the project's files.
        assert_eq!(f.classify(0, "app.log", false), Verdict::Ignore);
        assert_eq!(f.classify(0, "other/x.md", false), Verdict::Emit);
    }

    #[test]
    fn inside_a_nested_repository_only_its_own_rules_apply() {
        let d = tempfile::tempdir().expect("tempdir");
        project_with_nested_repo(d.path());
        let f = filter(d.path(), &[]);
        // `*.log` is the project's rule, not the repository's
        assert_eq!(f.classify(0, "Design/debug.log", false), Verdict::Emit);
        assert_eq!(f.classify(0, "Design/secret.md", false), Verdict::Ignore);
        assert_eq!(
            f.classify(0, "Design/sub/secret.md", false),
            Verdict::Ignore
        );
        // `.git/info/exclude` of the repository
        assert_eq!(f.classify(0, "Design/private/x.md", false), Verdict::Ignore);
        assert_eq!(f.classify(0, "private/x.md", false), Verdict::Emit);
        // its own internals are still noise
        assert_eq!(f.classify(0, "Design/.git/index", false), Verdict::Ignore);
    }

    #[test]
    fn repositories_nested_in_nested_repositories_follow_the_same_rule() {
        let d = tempfile::tempdir().expect("tempdir");
        project_with_nested_repo(d.path());
        let inner = d.path().join("Design").join("docs").join("inner");
        fs::create_dir_all(inner.join(".git")).expect("mkdir");
        fs::write(inner.join(".gitignore"), "*.tmpl\n").expect("write");
        let f = filter(d.path(), &[]);
        // Design's rule (`secret.md`) applies in Design, and the inner repository has its own rules.
        assert_eq!(
            f.classify(0, "Design/docs/secret.md", false),
            Verdict::Ignore
        );
        assert_eq!(
            f.classify(0, "Design/docs/inner/a.md", false),
            Verdict::Emit
        );
        assert_eq!(
            f.classify(0, "Design/docs/inner/a.tmpl", false),
            Verdict::Ignore
        );
        assert_eq!(
            f.classify(0, "Design/docs/inner/secret.md", false),
            Verdict::Emit
        );
    }

    #[test]
    fn a_repository_that_appears_later_is_noticed() {
        let d = tempfile::tempdir().expect("tempdir");
        fs::write(d.path().join(".gitignore"), "Design/\n").expect("write");
        fs::create_dir_all(d.path().join("Design")).expect("mkdir");
        let f = filter(d.path(), &[]);
        // not a repository yet: the project's rule hides it
        assert_eq!(f.classify(0, "Design/a.md", false), Verdict::Ignore);
        fs::create_dir_all(d.path().join("Design").join(".git")).expect("mkdir");
        // the event for the new `.git` makes the filter look again
        assert_eq!(f.classify(0, "Design/.git", true), Verdict::Ignore);
        assert_eq!(f.classify(0, "Design/a.md", false), Verdict::Emit);
    }

    #[test]
    fn changing_a_gitignore_refreshes_the_cache() {
        let d = tempfile::tempdir().expect("tempdir");
        let f = filter(d.path(), &[]);
        assert_eq!(f.classify(0, "notes.md", false), Verdict::Emit);
        fs::write(d.path().join(".gitignore"), "notes.md\n").expect("write");
        // Event for the .gitignore itself drops the cached (empty) state.
        assert_eq!(f.classify(0, ".gitignore", false), Verdict::Emit);
        assert_eq!(f.classify(0, "notes.md", false), Verdict::Ignore);
    }

    #[test]
    fn extra_config_globs_apply() {
        let d = tempfile::tempdir().expect("tempdir");
        let f = filter(d.path(), &["drafts/", "*.generated.md"]);
        assert_eq!(f.classify(0, "drafts/a.md", false), Verdict::Ignore);
        assert_eq!(f.classify(0, "x/api.generated.md", false), Verdict::Ignore);
        assert_eq!(f.classify(0, "docs/a.md", false), Verdict::Emit);
    }

    #[test]
    fn empty_path_and_unknown_root_are_ignored() {
        let d = tempfile::tempdir().expect("tempdir");
        let f = filter(d.path(), &[]);
        assert_eq!(f.classify(0, "", true), Verdict::Ignore);
        assert_eq!(f.classify(5, "a.md", false), Verdict::Ignore);
    }

    #[test]
    fn info_exclude_is_honored() {
        let d = tempfile::tempdir().expect("tempdir");
        let exclude = d.path().join("exclude");
        fs::write(&exclude, "private/\n").expect("write");
        let f = PathFilter::new(&[(d.path().to_path_buf(), Some(exclude))], &[]);
        assert_eq!(f.classify(0, "private/a.md", false), Verdict::Ignore);
        assert_eq!(f.classify(0, "public/a.md", false), Verdict::Emit);
    }
}
