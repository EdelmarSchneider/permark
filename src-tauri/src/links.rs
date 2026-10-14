//! Resolving the `href` of a link inside a document to something the UI can act on.
//!
//! Handles what AI-written Markdown actually contains: `%20` and raw spaces, Windows `\` separators,
//! missing `.md` extensions, `#anchors`, root-relative (`/docs/x.md`) links, `file:` URLs and drive
//! letters that point inside the vault, and external schemes.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::vault::{Vault, WORKTREE_PREFIX};

/// Where a link leads. Matches the UI's `LinkTarget`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LinkTarget {
    Internal {
        path: String,
        anchor: Option<String>,
    },
    External {
        url: String,
    },
    Anchor {
        anchor: String,
    },
    Missing {
        path: String,
    },
}

/// Schemes that must never be opened or navigated to.
const BLOCKED_SCHEMES: [&str; 3] = ["javascript", "data", "vbscript"];

/// Decodes `%XX` escapes (invalid sequences are kept as written; invalid UTF-8 becomes U+FFFD).
pub(crate) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hi = bytes.get(i + 1).and_then(|b| char::from(*b).to_digit(16));
            let lo = bytes.get(i + 2).and_then(|b| char::from(*b).to_digit(16));
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push(u8::try_from(hi * 16 + lo).unwrap_or(b'%'));
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The URL scheme of `href` (`https`, `mailto`, ...), lowercased, if it has one.
fn scheme_of(href: &str) -> Option<String> {
    let colon = href.find(':')?;
    let scheme = &href[..colon];
    let mut chars = scheme.chars();
    let first = chars.next()?;
    (first.is_ascii_alphabetic()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
    .then(|| scheme.to_ascii_lowercase())
}

/// Resolves `href`, found in the document `from` (a vault-relative path), against the vault.
pub fn resolve(vault: &Vault, from: &str, href: &str) -> LinkTarget {
    let href = href.trim();
    if href.is_empty() {
        return LinkTarget::Anchor {
            anchor: String::new(),
        };
    }
    if let Some(frag) = href.strip_prefix('#') {
        return LinkTarget::Anchor {
            anchor: percent_decode(frag),
        };
    }

    let (target, anchor) = match href.split_once('#') {
        Some((t, a)) => (t, Some(percent_decode(a)).filter(|a| !a.is_empty())),
        None => (href, None),
    };

    if let Some(scheme) = scheme_of(target) {
        // One letter is a Windows drive (`C:\docs\x.md`), not a URL scheme.
        if scheme.len() == 1 {
            return resolve_absolute(vault, Path::new(&percent_decode(target)), anchor);
        }
        if scheme == "file" {
            return match file_url_to_path(target) {
                Some(p) => resolve_absolute(vault, &p, anchor),
                None => LinkTarget::Missing {
                    path: href.to_owned(),
                },
            };
        }
        if BLOCKED_SCHEMES.contains(&scheme.as_str()) {
            return LinkTarget::Missing {
                path: href.to_owned(),
            };
        }
        return LinkTarget::External {
            url: href.to_owned(),
        };
    }

    // Local path: drop `?query`, decode, normalize separators.
    let path_part = target.split_once('?').map_or(target, |(p, _)| p);
    if path_part.is_empty() {
        return LinkTarget::Anchor {
            anchor: anchor.unwrap_or_default(),
        };
    }
    let decoded = percent_decode(path_part).replace('\\', "/");

    let Some(joined) = join_relative(from, &decoded) else {
        return LinkTarget::Missing { path: decoded };
    };
    find_existing(vault, &joined, anchor)
}

/// Tries the candidate itself, then `.md`, then `README.md` / `index.md` for folders.
fn find_existing(vault: &Vault, rel: &str, anchor: Option<String>) -> LinkTarget {
    let has_ext = Path::new(rel).extension().is_some();
    let mut candidates = vec![rel.to_owned()];
    if !has_ext || !rel.to_ascii_lowercase().ends_with(".md") {
        candidates.push(format!("{rel}.md"));
    }
    let base = rel.trim_end_matches('/');
    candidates.push(format!("{base}/README.md"));
    candidates.push(format!("{base}/index.md"));
    for candidate in candidates {
        if let Ok(resolved) = vault.resolve(&candidate) {
            if resolved.abs.is_file() {
                return LinkTarget::Internal {
                    path: resolved.rel,
                    anchor,
                };
            }
        }
    }
    LinkTarget::Missing {
        path: rel.to_owned(),
    }
}

/// Joins `link` (already decoded, `/`-separated) onto the folder of `from`, within one root.
/// `None` when the link climbs above its root.
fn join_relative(from: &str, link: &str) -> Option<String> {
    let from = from.replace('\\', "/");
    // Split `@wt/<name>/inner/path` into the root prefix and the part inside the root.
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
    // A worktree link such as `/@wt/x/a.md` is not supported; absolute links stay in the current root.
    for seg in link.split('/') {
        match seg {
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
    // `file:///C:/x` -> `C:/x`; `file:///home/x` -> `/home/x`.
    let is_drive = decoded.as_bytes().get(1) == Some(&b':');
    Some(if is_drive || cfg!(windows) {
        PathBuf::from(decoded)
    } else {
        PathBuf::from(format!("/{decoded}"))
    })
}

/// An absolute filesystem path: internal when it lies inside a root, missing otherwise.
fn resolve_absolute(vault: &Vault, abs: &Path, anchor: Option<String>) -> LinkTarget {
    match vault.to_rel(abs) {
        Some((rel, _)) => find_existing(vault, &rel, anchor),
        None => LinkTarget::Missing {
            path: abs.display().to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn vault() -> (tempfile::TempDir, Vault) {
        let d = tempfile::tempdir().expect("tempdir");
        for f in [
            "README.md",
            "docs/guide.md",
            "docs/my file.md",
            "docs/sub/deep.md",
            "docs/Mixed Case.md",
            "área/ção.md",
            "folder/README.md",
            "img/pic.png",
        ] {
            let p = d.path().join(f);
            fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
            fs::write(p, "x").expect("write");
        }
        let v = Vault::open(d.path()).expect("open");
        (d, v)
    }

    fn internal(path: &str, anchor: Option<&str>) -> LinkTarget {
        LinkTarget::Internal {
            path: path.to_owned(),
            anchor: anchor.map(str::to_owned),
        }
    }

    #[test]
    fn relative_links_resolve_from_the_document_folder() {
        let (_d, v) = vault();
        assert_eq!(
            resolve(&v, "docs/guide.md", "sub/deep.md"),
            internal("docs/sub/deep.md", None)
        );
        assert_eq!(
            resolve(&v, "docs/sub/deep.md", "../guide.md"),
            internal("docs/guide.md", None)
        );
        assert_eq!(
            resolve(&v, "docs/guide.md", "./my%20file.md"),
            internal("docs/my file.md", None)
        );
        assert_eq!(
            resolve(&v, "README.md", "docs/guide.md"),
            internal("docs/guide.md", None)
        );
    }

    #[test]
    fn anchors_are_split_and_decoded() {
        let (_d, v) = vault();
        assert_eq!(
            resolve(&v, "README.md", "docs/guide.md#Setup%20Steps"),
            internal("docs/guide.md", Some("Setup Steps"))
        );
        assert_eq!(
            resolve(&v, "README.md", "#descri%C3%A7%C3%A3o"),
            LinkTarget::Anchor {
                anchor: "descrição".to_owned()
            }
        );
        assert_eq!(
            resolve(&v, "README.md", "docs/guide.md#"),
            internal("docs/guide.md", None)
        );
    }

    #[test]
    fn spaces_percent_encoding_and_backslashes_are_normalized() {
        let (_d, v) = vault();
        assert_eq!(
            resolve(&v, "README.md", "docs/my file.md"),
            internal("docs/my file.md", None)
        );
        assert_eq!(
            resolve(&v, "README.md", "docs\\sub\\deep.md"),
            internal("docs/sub/deep.md", None)
        );
        assert_eq!(
            resolve(&v, "README.md", "%C3%A1rea/%C3%A7%C3%A3o.md"),
            internal("área/ção.md", None)
        );
        assert_eq!(
            resolve(&v, "README.md", "docs/guide.md?plain=1"),
            internal("docs/guide.md", None)
        );
    }

    #[test]
    fn missing_extension_falls_back_to_md_and_folders_to_readme() {
        let (_d, v) = vault();
        assert_eq!(
            resolve(&v, "README.md", "docs/guide"),
            internal("docs/guide.md", None)
        );
        assert_eq!(
            resolve(&v, "README.md", "folder"),
            internal("folder/README.md", None)
        );
        assert_eq!(
            resolve(&v, "README.md", "folder/"),
            internal("folder/README.md", None)
        );
        assert_eq!(
            resolve(&v, "README.md", "docs/guide#intro"),
            internal("docs/guide.md", Some("intro"))
        );
    }

    #[test]
    fn root_relative_links_start_at_the_vault_root() {
        let (_d, v) = vault();
        assert_eq!(
            resolve(&v, "docs/sub/deep.md", "/docs/guide.md"),
            internal("docs/guide.md", None)
        );
    }

    #[test]
    fn non_markdown_files_resolve_too() {
        let (_d, v) = vault();
        assert_eq!(
            resolve(&v, "docs/guide.md", "../img/pic.png"),
            internal("img/pic.png", None)
        );
    }

    #[test]
    fn missing_files_report_the_normalized_path() {
        let (_d, v) = vault();
        assert_eq!(
            resolve(&v, "docs/guide.md", "nope/gone.md"),
            LinkTarget::Missing {
                path: "docs/nope/gone.md".to_owned()
            }
        );
    }

    #[test]
    fn links_climbing_out_of_the_vault_are_missing() {
        let (_d, v) = vault();
        assert!(matches!(
            resolve(&v, "README.md", "../../etc/passwd"),
            LinkTarget::Missing { .. }
        ));
        assert!(matches!(
            resolve(&v, "docs/guide.md", "../../x.md"),
            LinkTarget::Missing { .. }
        ));
    }

    #[test]
    fn external_schemes_are_external() {
        let (_d, v) = vault();
        for url in [
            "https://example.com/a?b=1#c",
            "http://x.dev",
            "mailto:a@b.dev",
            "tel:+155501",
            "ftp://h/f",
        ] {
            assert_eq!(
                resolve(&v, "README.md", url),
                LinkTarget::External {
                    url: url.to_owned()
                },
                "{url}"
            );
        }
    }

    #[test]
    fn dangerous_schemes_are_never_external() {
        let (_d, v) = vault();
        for url in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "data:text/html,x",
            "vbscript:x",
        ] {
            assert!(
                matches!(resolve(&v, "README.md", url), LinkTarget::Missing { .. }),
                "{url}"
            );
        }
    }

    #[test]
    fn absolute_paths_inside_the_vault_become_internal() {
        let (d, v) = vault();
        let abs = d.path().join("docs").join("guide.md");
        let as_path = abs.to_string_lossy().replace('\\', "/");
        assert_eq!(
            resolve(&v, "README.md", &as_path),
            internal("docs/guide.md", None)
        );
        let as_file_url = format!("file:///{}", as_path.trim_start_matches('/'));
        assert_eq!(
            resolve(&v, "README.md", &as_file_url),
            internal("docs/guide.md", None)
        );
        assert!(matches!(
            resolve(&v, "README.md", "file:///definitely/not/inside.md"),
            LinkTarget::Missing { .. }
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_style_absolute_paths_are_handled() {
        let (d, v) = vault();
        let abs = d.path().join("docs").join("guide.md");
        assert_eq!(
            resolve(&v, "README.md", &abs.display().to_string()),
            internal("docs/guide.md", None)
        );
        assert!(matches!(
            resolve(&v, "README.md", "Z:\\nowhere\\x.md"),
            LinkTarget::Missing { .. }
        ));
    }

    #[test]
    fn same_document_and_empty_links_are_anchors() {
        let (_d, v) = vault();
        assert_eq!(
            resolve(&v, "README.md", ""),
            LinkTarget::Anchor {
                anchor: String::new()
            }
        );
        assert_eq!(
            resolve(&v, "README.md", "#top"),
            LinkTarget::Anchor {
                anchor: "top".to_owned()
            }
        );
        assert_eq!(
            resolve(&v, "README.md", "?x=1#top"),
            LinkTarget::Anchor {
                anchor: "top".to_owned()
            }
        );
    }

    #[cfg(windows)]
    #[test]
    fn case_differences_resolve_to_the_on_disk_case() {
        let (_d, v) = vault();
        assert_eq!(
            resolve(&v, "README.md", "DOCS/mixed case.MD"),
            internal("docs/Mixed Case.md", None)
        );
    }

    #[test]
    fn links_inside_a_worktree_stay_in_the_worktree() {
        let (_d, v) = vault();
        let wt = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(wt.path().join("docs")).expect("mkdir");
        fs::write(wt.path().join("docs").join("a.md"), "x").expect("write");
        fs::write(wt.path().join("b.md"), "x").expect("write");
        let v = v.with_worktrees(vec![crate::vault::Worktree {
            name: "feat".to_owned(),
            path: wt.path().to_path_buf(),
            branch: None,
        }]);
        assert_eq!(
            resolve(&v, "@wt/feat/docs/a.md", "../b.md"),
            internal("@wt/feat/b.md", None)
        );
        assert_eq!(
            resolve(&v, "@wt/feat/docs/a.md", "/b.md"),
            internal("@wt/feat/b.md", None)
        );
        assert!(matches!(
            resolve(&v, "@wt/feat/docs/a.md", "../../README.md"),
            LinkTarget::Missing { .. }
        ));
    }

    #[test]
    fn serializes_to_the_ui_contract() {
        let j = |t: &LinkTarget| serde_json::to_value(t).expect("json");
        assert_eq!(
            j(&internal("a.md", None)),
            serde_json::json!({"kind": "internal", "path": "a.md", "anchor": null})
        );
        assert_eq!(
            j(&LinkTarget::External {
                url: "https://x".to_owned()
            }),
            serde_json::json!({"kind": "external", "url": "https://x"})
        );
        assert_eq!(
            j(&LinkTarget::Anchor {
                anchor: "a".to_owned()
            }),
            serde_json::json!({"kind": "anchor", "anchor": "a"})
        );
        assert_eq!(
            j(&LinkTarget::Missing {
                path: "p".to_owned()
            }),
            serde_json::json!({"kind": "missing", "path": "p"})
        );
    }

    #[test]
    fn percent_decode_edge_cases() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("%2"), "%2");
        assert_eq!(percent_decode("%E4"), "\u{FFFD}");
    }
}
