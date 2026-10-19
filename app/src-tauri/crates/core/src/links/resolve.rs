//! Where the `href` of a link inside a document leads. Handles what AI-written Markdown contains: `%20` and raw
//! spaces, Windows `\` separators, missing `.md` extensions, `#anchors`, root-relative links, `file:` URLs and drive
//! letters that point inside the vault, and external schemes.

use std::path::{Path, PathBuf};

use super::{percent_decode, scheme_of};
use crate::paths::{Vault, VaultPath, WORKTREE_PREFIX};
use crate::tree::{category_of, Category};

/// Schemes that are never navigated to.
const BLOCKED_SCHEMES: [&str; 3] = ["javascript", "data", "vbscript"];

/// Where a link leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A Markdown document (`README.md | index.md` for a folder link), with its `#section`.
    Doc(VaultPath, Option<String>),
    /// A folder without a readme.
    Folder(VaultPath),
    /// Any other existing file.
    Asset(VaultPath),
    /// `https:`, `mailto:`, ... as written.
    Web(String),
    /// `#section` of the same document.
    Anchor(String),
    /// Points to something that does not exist (path as the link names it).
    Missing(String),
    /// A path outside every root of the vault.
    Outside,
}

/// Resolves `href`, found in the document `from` (a project path), against the vault.
#[must_use]
pub fn resolve(vault: &Vault, from: &VaultPath, href: &str) -> Target {
    let href = href.trim();
    if href.is_empty() {
        return Target::Anchor(String::new());
    }
    if let Some(fragment) = href.strip_prefix('#') {
        return Target::Anchor(percent_decode(fragment));
    }
    let (target, anchor) = match href.split_once('#') {
        Some((t, a)) => (t, Some(percent_decode(a)).filter(|a| !a.is_empty())),
        None => (href, None),
    };
    if let Some(scheme) = scheme_of(target) {
        // one letter is a Windows drive (`C:\docs\x.md`), not a URL scheme
        if scheme.len() == 1 {
            return resolve_absolute(vault, Path::new(&percent_decode(target)), anchor);
        }
        if scheme == "file" {
            return match file_url_to_path(target) {
                Some(path) => resolve_absolute(vault, &path, anchor),
                None => Target::Missing(href.to_owned()),
            };
        }
        if BLOCKED_SCHEMES.contains(&scheme.as_str()) {
            return Target::Missing(href.to_owned());
        }
        return Target::Web(href.to_owned());
    }
    // local path: `?query` dropped, decoded, `/` separators
    let path_part = target.split_once('?').map_or(target, |(p, _)| p);
    if path_part.is_empty() {
        return Target::Anchor(anchor.unwrap_or_default());
    }
    let decoded = percent_decode(path_part).replace('\\', "/");
    // an absolute filesystem path inside a root is that file; otherwise a leading `/` means the project root
    if Path::new(&decoded).is_absolute() {
        if let Some(rel) = vault.relativize(Path::new(&decoded)) {
            return find_existing(vault, rel.as_str(), anchor);
        }
    }
    match join_relative(from.as_str(), &decoded) {
        Some(joined) => find_existing(vault, &joined, anchor),
        None => Target::Missing(decoded),
    }
}

/// The project path `href` (found in the document `from`) names, without looking at the disk: anchor and query
/// dropped, percent-decoded, `\` read as `/`, joined onto the document's folder (a leading `/` is the project root).
/// `None` for anchors only, anything with a scheme (web, `file:`, a drive letter) and paths that climb above their root.
/// The candidate may not exist, and a folder link or a missing `.md` is not completed.
#[must_use]
pub fn local_target(from: &str, href: &str) -> Option<String> {
    let href = href.trim();
    let target = href.split_once('#').map_or(href, |(t, _)| t);
    if scheme_of(target).is_some() {
        return None;
    }
    let path_part = target.split_once('?').map_or(target, |(p, _)| p);
    if path_part.is_empty() {
        return None;
    }
    let decoded = percent_decode(path_part).replace('\\', "/");
    join_relative(from, &decoded)
}

/// The candidate itself, then `.md`, then `README.md | index.md` for folders.
fn find_existing(vault: &Vault, rel: &str, anchor: Option<String>) -> Target {
    let base = rel.trim_end_matches('/');
    let mut candidates = vec![base.to_owned()];
    if !base.to_ascii_lowercase().ends_with(".md") {
        candidates.push(format!("{base}.md"));
    }
    let mut folder = None;
    for candidate in &candidates {
        let Ok(path) = VaultPath::parse(candidate) else {
            continue;
        };
        let Ok(abs) = vault.resolve(&path) else {
            continue;
        };
        let Ok(meta) = std::fs::metadata(&abs) else {
            continue;
        };
        if meta.is_file() {
            return if category_of(path.name().unwrap_or("")) == Category::Markdown {
                Target::Doc(path, anchor)
            } else {
                Target::Asset(path)
            };
        }
        if meta.is_dir() && folder.is_none() {
            folder = Some(path);
        }
    }
    if let Some(folder) = folder {
        for name in ["README.md", "index.md"] {
            let readme = folder.join_name(name);
            if vault
                .resolve(&readme)
                .is_ok_and(|abs| std::fs::metadata(abs).is_ok_and(|m| m.is_file()))
            {
                return Target::Doc(readme, anchor);
            }
        }
        return Target::Folder(folder);
    }
    Target::Missing(rel.to_owned())
}

/// Joins `link` (decoded, `/`-separated) onto the folder of `from`, within one root. `None`: it climbs above its root.
#[must_use]
pub fn join_relative(from: &str, link: &str) -> Option<String> {
    let from = from.replace('\\', "/");
    // `@wt/<name>/inner/path` -> root prefix + the part inside the root
    let (prefix, inner) = match from.strip_prefix(WORKTREE_PREFIX) {
        Some(rest) => {
            let (name, inner) = rest.split_once('/').unwrap_or((rest, ""));
            (format!("{WORKTREE_PREFIX}{name}/"), inner.to_owned())
        }
        None => (String::new(), from.clone()),
    };
    let mut parts: Vec<&str> = if link.starts_with('/') {
        Vec::new()
    } else {
        let mut dir: Vec<&str> = inner.split('/').filter(|s| !s.is_empty()).collect();
        dir.pop(); // the document's own file name
        dir
    };
    for segment in link.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(format!("{prefix}{}", parts.join("/")))
}

fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let rest = url.get(5..)?; // after `file:`
    let rest = rest.trim_start_matches('/');
    let decoded = percent_decode(rest);
    if decoded.is_empty() {
        return None;
    }
    // `file:///C:/x` -> `C:/x`; `file:///home/x` -> `/home/x`
    let is_drive = decoded.as_bytes().get(1) == Some(&b':');
    Some(if is_drive || cfg!(windows) {
        PathBuf::from(decoded)
    } else {
        PathBuf::from(format!("/{decoded}"))
    })
}

/// An absolute filesystem path: a file of the vault when it lies inside a root, outside otherwise.
fn resolve_absolute(vault: &Vault, abs: &Path, anchor: Option<String>) -> Target {
    match vault.relativize(abs) {
        Some(rel) => find_existing(vault, rel.as_str(), anchor),
        None => Target::Outside,
    }
}
