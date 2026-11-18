//! An agent's finished message shown as a document: the project's Markdown pipeline renders it (sanitized like every
//! document), as a file at the project root, so relative links and pictures resolve from there. The agent works in the
//! copy and names files by their place there: such paths are written as project paths first.

use std::borrow::Cow;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use permark_core::document::DocScope;
use permark_core::format::{Body, FormatRegistry, MarkdownFeatures, RenderCx};
use permark_core::paths::{self, Root};
use permark_core::{ErrorKind, IpcError};

use crate::dto::RenderedReply;
use crate::work::copy_dir;

/// Longest message rendered (longer: refused, `replyTooLarge`; the window keeps the plain text).
pub const MAX_REPLY_BYTES: usize = 256 << 10;
/// The document a message is rendered as.
pub const REPLY_DOC: &str = "assistant.md";

/// Bytes the format pick reads.
const HEAD_BYTES: usize = 4096;
/// Characters that end a path in prose (after them the text goes on).
const PATH_END: [char; 11] = ['`', '"', '\'', '<', '>', '(', ')', '[', ']', '|', '*'];

/// The places the copy of `root` under `place` is named by: as made and in its real spelling (links, 8.3 names).
#[must_use]
pub fn copy_spellings(place: Option<&Path>, root: &Root) -> Vec<PathBuf> {
    let Some(place) = place else {
        return Vec::new();
    };
    let given = copy_dir(place, root);
    let real = paths::canonicalize_lenient(&given);
    let mut spellings = vec![given];
    spellings.extend(real.filter(|real| *real != spellings[0]));
    spellings
}

/// The blocks of `text` rendered as [`REPLY_DOC`] of `scope`. Plugin features (front matter card, prompt tags) stay
/// off: a message is no prompt file, and its first `---` is no card.
///
/// # Errors
/// `TooLarge` `replyTooLarge`; `Unavailable` `noMarkdown` (no format renders Markdown blocks).
pub fn render(
    text: &str,
    formats: &FormatRegistry,
    scope: &DocScope,
    copy: &[PathBuf],
) -> Result<RenderedReply, IpcError> {
    if text.len() > MAX_REPLY_BYTES {
        return Err(IpcError::new(ErrorKind::TooLarge).detail("replyTooLarge"));
    }
    let text = project_paths(text, copy);
    let bytes = text.as_bytes();
    let format = formats.pick(REPLY_DOC, bytes.get(..HEAD_BYTES).unwrap_or(bytes));
    let known = HashSet::new();
    let features = MarkdownFeatures::default();
    let cx = RenderCx {
        path: REPLY_DOC,
        env: scope,
        features: &features,
        known: &known,
    };
    match format.map(|format| format.render(&text, &cx).body) {
        Some(Body::Markdown { blocks, .. }) => Ok(RenderedReply { blocks }),
        _ => Err(IpcError::new(ErrorKind::Unavailable).detail("noMarkdown")),
    }
}

/// The text of the path prefixes of a copy folder: `/` and `\` separated, as a file URI too; longest first, folded
/// where names ignore case.
fn prefixes(copy: &[PathBuf]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for dir in copy {
        let text = dir.to_string_lossy();
        let text = text.trim_end_matches(['/', '\\']);
        if text.is_empty() {
            continue;
        }
        let slashed = text.replace('\\', "/");
        let uri = if slashed.starts_with('/') {
            format!("file://{slashed}")
        } else {
            format!("file:///{slashed}")
        };
        for prefix in [uri, text.replace('/', "\\"), slashed] {
            let prefix = if paths::CASE_INSENSITIVE {
                prefix.to_ascii_lowercase()
            } else {
                prefix
            };
            if !found.contains(&prefix) {
                found.push(prefix);
            }
        }
    }
    found.sort_by_key(|prefix| std::cmp::Reverse(prefix.len()));
    found
}

/// A character that continues a name: a prefix right after one is part of a longer path.
fn continues_name(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '-')
}

/// `text` with every path into the copy written as a project path: the copy's folder and the separator after it
/// dropped, the rest of the path `/` separated. The folder named alone stays as written.
fn project_paths<'t>(text: &'t str, copy: &[PathBuf]) -> Cow<'t, str> {
    let prefixes = prefixes(copy);
    if prefixes.is_empty() {
        return Cow::Borrowed(text);
    }
    // ASCII folding keeps every byte offset
    let hay: Cow<'_, str> = if paths::CASE_INSENSITIVE {
        Cow::Owned(text.to_ascii_lowercase())
    } else {
        Cow::Borrowed(text)
    };
    let mut hits: Vec<(usize, usize)> = Vec::new();
    for prefix in &prefixes {
        for (at, _) in hay.match_indices(prefix.as_str()) {
            let end = at + prefix.len();
            let before = hay[..at].chars().next_back();
            let after = hay[end..].chars().next();
            if matches!(after, Some('/' | '\\')) && !before.is_some_and(continues_name) {
                hits.push((at, end + 1));
            }
        }
    }
    if hits.is_empty() {
        return Cow::Borrowed(text);
    }
    // the earliest first; at one place the longest (sorted so above) wins
    hits.sort_by_key(|&(at, end)| (at, std::cmp::Reverse(end)));
    let mut out = String::with_capacity(text.len());
    let mut done = 0;
    for (at, end) in hits {
        if at < done {
            continue;
        }
        out.push_str(&text[done..at]);
        let rest = &text[end..];
        let len = rest
            .find(|c: char| c.is_whitespace() || PATH_END.contains(&c))
            .unwrap_or(rest.len());
        out.extend(rest[..len].chars().map(|c| if c == '\\' { '/' } else { c }));
        done = end + len;
    }
    out.push_str(&text[done..]);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use permark_core::format::{
        Contributions, Field, FrontMatterSyntax, MarkdownFeature, PromptBlocks,
    };

    use super::*;
    use crate::testing;

    /// The HTML of every block of `text` rendered for a project at a temp folder, the copy at `copy`.
    fn html(text: &str, copy: &[PathBuf]) -> String {
        html_with(text, copy, &FormatRegistry::core())
    }

    fn html_with(text: &str, copy: &[PathBuf], formats: &FormatRegistry) -> String {
        let project = tempfile::tempdir().unwrap();
        let (_vault, documents) = testing::open(project.path());
        let reply = render(text, formats, documents.scope(), copy).unwrap();
        reply
            .blocks
            .iter()
            .filter_map(|block| block.html.clone())
            .collect::<String>()
    }

    #[test]
    fn a_message_renders_as_document_blocks() {
        let text = "# Plan\n\n- one\n- two\n\n```rust\nfn a() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n$x^2$\n";
        let html = html(text, &[]);
        assert!(html.contains("<h1"), "{html}");
        assert!(html.contains(">one</li>"), "{html}");
        assert!(html.contains("language-rust"), "{html}");
        assert!(html.contains("<table"), "{html}");
        assert!(html.contains("data-math-style"), "{html}");
    }

    #[test]
    fn markup_in_a_message_is_sanitized() {
        let html = html(
            "<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>\n\n[x](javascript:alert(1)) <b onclick=\"a()\">b</b>\n",
            &[],
        );
        assert!(!html.contains("<script"), "{html}");
        assert!(!html.contains("onerror"), "{html}");
        assert!(!html.contains("onclick"), "{html}");
        assert!(!html.contains("javascript:"), "{html}");
    }

    #[test]
    fn links_and_pictures_resolve_from_the_project_root() {
        let html = html("[guide](docs/guide.md) ![p](img/a.png)\n", &[]);
        assert!(html.contains("href=\"docs/guide.md\""), "{html}");
        assert!(html.contains("src=\"pm-asset:img/a.png\""), "{html}");
    }

    fn no_fields(_: FrontMatterSyntax, _: &str) -> Vec<Field> {
        Vec::new()
    }

    #[test]
    fn prompt_tags_and_front_matter_are_never_restyled() {
        let formats = FormatRegistry::build(&Contributions {
            markdown: vec![
                MarkdownFeature::PromptBlocks(PromptBlocks { tags: &["task"] }),
                MarkdownFeature::FrontMatterCard(no_fields),
            ],
            ..Contributions::default()
        });
        let html = html_with("<task>\nDo it.\n</task>\n", &[], &formats);
        assert!(!html.contains("<details"), "{html}");
        assert!(html.contains("Do it."), "{html}");
        // a card would take the lines out of the blocks: a message keeps every word it said
        let html = html_with("---\ntitle: x\n---\nHello\n", &[], &formats);
        assert!(html.contains("title: x"), "{html}");
        assert!(html.contains("Hello"), "{html}");
    }

    #[test]
    fn paths_into_the_copy_read_as_project_paths() {
        let base = tempfile::tempdir().unwrap();
        let copy = base.path().join("work").join("proj");
        let given = copy.display().to_string();
        let slashed = given.replace('\\', "/");
        let uri = if slashed.starts_with('/') {
            format!("file://{slashed}")
        } else {
            format!("file:///{slashed}")
        };
        let sep = std::path::MAIN_SEPARATOR;
        let text = format!(
            "See {given}{sep}src{sep}a.md:3 and `{slashed}/b.md`, [c]({uri}/docs/c.md); {given} stays.\n"
        );
        let copy = [copy];
        // the folder named alone is no file of the project
        assert_eq!(
            project_paths(&text, &copy),
            format!("See src/a.md:3 and `b.md`, [c](docs/c.md); {given} stays.\n")
        );
        let html = html(&text, &copy);
        assert!(html.contains("See src/a.md:3 and"), "{html}");
        assert!(html.contains("<code>b.md</code>"), "{html}");
        assert!(html.contains("href=\"docs/c.md\""), "{html}");
    }

    #[test]
    fn a_path_is_matched_whatever_its_separators_and_case_where_names_ignore_case() {
        let copy = [PathBuf::from(if cfg!(windows) {
            r"C:\Profile\work\proj"
        } else {
            "/profile/work/proj"
        })];
        let back = project_paths(r"at C:\Profile\work\proj\src\a.md now", &copy);
        let slash = project_paths("at /profile/work/proj/src/a.md now", &copy);
        if cfg!(windows) {
            assert_eq!(back, "at src/a.md now");
            assert_eq!(
                project_paths("at c:/PROFILE/work/proj/x.md", &copy),
                "at x.md"
            );
        } else {
            assert_eq!(slash, "at src/a.md now");
        }
        // a longer name that only ends like the folder is another place
        let other = if cfg!(windows) {
            r"D:\xC:\Profile\work\proj\a.md"
        } else {
            "/x/profile/work/projects/a.md"
        };
        assert_eq!(project_paths(other, &copy), other);
    }

    #[test]
    fn the_real_spelling_of_the_copy_is_known_too() {
        let base = tempfile::tempdir().unwrap();
        let root = Root::open(base.path()).unwrap();
        // another spelling of the same folder: the copy is made under it, the agent names its real place
        let place = base
            .path()
            .join("..")
            .join(base.path().file_name().unwrap());
        let name = root.path().file_name().unwrap();
        let real = root.path().join(name);
        let spellings = copy_spellings(Some(&place), &root);
        assert_eq!(spellings, vec![copy_dir(&place, &root), real.clone()]);
        let text = format!("{}{}a.md", real.display(), std::path::MAIN_SEPARATOR);
        assert_eq!(project_paths(&text, &spellings), "a.md");
        assert_eq!(copy_spellings(None, &root), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_message_past_the_cap_is_refused() {
        let project = tempfile::tempdir().unwrap();
        let (_vault, documents) = testing::open(project.path());
        let text = "a".repeat(MAX_REPLY_BYTES + 1);
        let refused = render(&text, &FormatRegistry::core(), documents.scope(), &[]).unwrap_err();
        assert_eq!(refused.kind, ErrorKind::TooLarge);
        assert_eq!(refused.detail.as_deref(), Some("replyTooLarge"));
        assert!(render(
            &"a".repeat(MAX_REPLY_BYTES),
            &FormatRegistry::core(),
            documents.scope(),
            &[]
        )
        .is_ok());
    }
}
