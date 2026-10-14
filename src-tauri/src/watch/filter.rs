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
/// `4913`, and the hidden `.<name>.<pid>.part` files this app writes before it renames them into place.
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
#[derive(Debug)]
struct LazyGitignore {
    root: PathBuf,
    /// `.git/info/exclude`.
    base: Option<Gitignore>,
    /// The user's global excludes file; lowest priority.
    global: Option<Gitignore>,
    cache: HashMap<PathBuf, Option<Gitignore>>,
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

    /// Is the path, or any folder above it inside the root, ignored?
    fn is_ignored(&mut self, comps: &[&str], is_dir: bool) -> bool {
        for depth in 1..=comps.len() {
            let mut path = self.root.clone();
            path.extend(&comps[..depth]);
            let dir = depth < comps.len() || is_dir;
            if self.matches(&path, dir) {
                return true;
            }
        }
        false
    }

    /// Deepest `.gitignore` first; the first file with an opinion wins.
    fn matches(&mut self, path: &Path, is_dir: bool) -> bool {
        let mut dirs: Vec<PathBuf> = path
            .ancestors()
            .skip(1)
            .take_while(|a| a.starts_with(&self.root))
            .map(Path::to_path_buf)
            .collect();
        if dirs.is_empty() {
            dirs.push(self.root.clone());
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
        [&self.base, &self.global]
            .into_iter()
            .flatten()
            .any(|g| g.matched(path, is_dir).is_ignore())
    }

    fn invalidate(&mut self) {
        self.cache.clear();
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

    /// `inner` is the path relative to the root of index `root`, `/`-separated.
    pub fn classify(&self, root: usize, inner: &str, is_dir: bool) -> Verdict {
        let comps: Vec<&str> = inner.split('/').filter(|c| !c.is_empty()).collect();
        let Some(last) = comps.last().copied() else {
            return Verdict::Ignore;
        };
        if let Some(pos) = comps.iter().position(|c| c.eq_ignore_ascii_case(".git")) {
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
