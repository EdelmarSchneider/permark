//! Turning files into HTML for the UI, through a registry of renderers.
//!
//! Adding a format means one new [`Renderer`] impl and one line in [`Registry::with_defaults`].
//! Binary files and files over [`MAX_RENDER_BYTES`] never reach a renderer.

pub(crate) mod blocks;
mod decode;
mod frontmatter;
pub(crate) mod markdown;
mod plain;

use std::io;
use std::path::Path;

use serde::Serialize;

use crate::util::{modified_ms, read_with_retry};
pub(crate) use decode::normalize_newlines;
use decode::{decode, Decoded};
pub(crate) use frontmatter::RawValue;

/// Decodes file bytes to text (`None` for binary files); the note about odd encodings is dropped.
pub(crate) fn decode_text(bytes: &[u8]) -> Option<String> {
    match decode(bytes) {
        Decoded::Text { text, .. } => Some(text),
        Decoded::Binary => None,
    }
}

/// Key/value pairs of a front matter block as found in the parsed Markdown (delimiters included).
pub(crate) fn front_matter_pairs(raw: &str) -> Vec<(String, String)> {
    frontmatter::parse(frontmatter::strip_delimiters(raw))
}

/// The value of a top-level front matter key in a whole document (`None` without front matter or key).
pub(crate) fn front_matter_value(doc: &str, key: &str) -> Option<RawValue> {
    frontmatter::top_level(frontmatter::block_of(doc)?, key)
}

/// Files larger than this are not rendered (`tooLarge`).
pub const MAX_RENDER_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("cannot read {path}: {source}")]
    Io { path: String, source: io::Error },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DocKind {
    Markdown,
    Json,
    Xml,
    Text,
    Binary,
    TooLarge,
}

/// Matches the UI's `DocumentView`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentView {
    pub path: String,
    pub kind: DocKind,
    pub html: String,
    /// One hash per top-level element of `html`, in order (see [`blocks`]).
    pub blocks: Vec<String>,
    pub title: Option<String>,
    pub frontmatter: Option<Vec<(String, String)>>,
    pub modified_ms: u64,
    pub size_bytes: u64,
    pub encoding_note: Option<String>,
}

/// Output of one renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub kind: DocKind,
    pub html: String,
    pub title: Option<String>,
    pub frontmatter: Option<Vec<(String, String)>>,
    /// A short remark for the reader when the renderer had to fall back (for example "shown as plain
    /// text"). It is merged into the view's `encoding_note`.
    pub note: Option<String>,
}

pub struct RenderCtx<'a> {
    /// The file being rendered (not every format needs it).
    #[allow(dead_code)]
    pub path: &'a Path,
}

/// One document format.
pub trait Renderer: Send + Sync {
    fn can_handle(&self, path: &Path) -> bool;
    /// `src` is decoded text with `\n` line endings. The returned HTML must already be safe to inject.
    fn render(&self, src: &str, ctx: &RenderCtx<'_>) -> Rendered;
}

/// Renderers in priority order; the first one that can handle a path wins.
pub struct Registry {
    renderers: Vec<Box<dyn Renderer>>,
}

impl Registry {
    /// A registry with exactly these renderers, tried in order. End the list with one that accepts
    /// any path (like the plain-text renderer) so every file gets rendered.
    pub fn from_renderers(renderers: Vec<Box<dyn Renderer>>) -> Self {
        Self { renderers }
    }

    pub fn with_defaults() -> Self {
        Self::from_renderers(vec![
            Box::new(markdown::MarkdownRenderer),
            Box::new(plain::JsonRenderer),
            Box::new(plain::XmlRenderer),
            // Must stay last: it accepts everything.
            Box::new(plain::TextRenderer),
        ])
    }

    pub fn render(&self, path: &Path, src: &str) -> Rendered {
        let ctx = RenderCtx { path };
        // `with_defaults` ends with a renderer that accepts any path, so a match always exists.
        let renderer = self
            .renderers
            .iter()
            .find(|r| r.can_handle(path))
            .or_else(|| self.renderers.last());
        renderer.map_or_else(
            || Rendered {
                kind: DocKind::Text,
                html: String::new(),
                title: None,
                frontmatter: None,
                note: None,
            },
            |r| r.render(src, &ctx),
        )
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

fn empty_view(path: &str, kind: DocKind, modified_ms: u64, size_bytes: u64) -> DocumentView {
    DocumentView {
        path: path.to_owned(),
        kind,
        html: String::new(),
        blocks: Vec::new(),
        title: None,
        frontmatter: None,
        modified_ms,
        size_bytes,
        encoding_note: None,
    }
}

/// Renders already-read bytes. `rel` is the UI path echoed back in the view.
pub fn render_bytes(
    registry: &Registry,
    abs: &Path,
    rel: &str,
    bytes: &[u8],
    modified_ms: u64,
) -> DocumentView {
    let size = bytes.len() as u64;
    match decode(bytes) {
        Decoded::Binary => empty_view(rel, DocKind::Binary, modified_ms, size),
        Decoded::Text { text, note } => {
            let r = registry.render(abs, &text);
            // Both notes end up in the one chip next to the title.
            let note = match (note, r.note) {
                (Some(encoding), Some(fallback)) => Some(format!("{encoding}; {fallback}")),
                (encoding, fallback) => encoding.or(fallback),
            };
            DocumentView {
                path: rel.to_owned(),
                kind: r.kind,
                blocks: blocks::block_hashes(&r.html),
                html: r.html,
                title: r.title,
                frontmatter: r.frontmatter,
                modified_ms,
                size_bytes: size,
                encoding_note: note,
            }
        }
    }
}

/// Reads and renders a file from disk (retrying transient sharing violations).
pub fn load_document(
    registry: &Registry,
    abs: &Path,
    rel: &str,
) -> Result<DocumentView, RenderError> {
    let io_err = |source| RenderError::Io {
        path: rel.to_owned(),
        source,
    };
    let meta = std::fs::metadata(abs).map_err(io_err)?;
    let mtime = modified_ms(&meta);
    if meta.len() > MAX_RENDER_BYTES {
        return Ok(empty_view(rel, DocKind::TooLarge, mtime, meta.len()));
    }
    let bytes = read_with_retry(abs).map_err(io_err)?;
    Ok(render_bytes(registry, abs, rel, &bytes, mtime))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn view_of(name: &str, content: &[u8]) -> DocumentView {
        let d = tempfile::tempdir().expect("tempdir");
        let p = d.path().join(name);
        fs::write(&p, content).expect("write");
        load_document(&Registry::with_defaults(), &p, name).expect("load")
    }

    #[test]
    fn markdown_view_has_blocks_matching_top_level_elements() {
        let v = view_of("a.md", b"# Title\n\nOne.\n\nTwo.\n");
        assert_eq!(v.kind, DocKind::Markdown);
        assert_eq!(v.title.as_deref(), Some("Title"));
        assert_eq!(v.blocks.len(), blocks::top_level_elements(&v.html).len());
        assert_eq!(v.blocks.len(), 3);
        assert!(v.modified_ms > 0);
        assert_eq!(v.size_bytes, 20);
        assert_eq!(v.path, "a.md");
    }

    #[test]
    fn inserting_a_block_keeps_the_other_hashes() {
        let a = view_of("a.md", b"# T\n\nOne.\n\nTwo.\n");
        let b = view_of("a.md", b"# T\n\nNEW.\n\nOne.\n\nTwo.\n");
        assert_eq!(b.blocks.len(), 4);
        assert_eq!(a.blocks[0], b.blocks[0]);
        assert_eq!(a.blocks[1], b.blocks[2]);
        assert_eq!(a.blocks[2], b.blocks[3]);
    }

    #[test]
    fn registry_dispatches_by_extension() {
        assert_eq!(view_of("a.json", b"{\"a\":1}").kind, DocKind::Json);
        assert_eq!(view_of("a.xml", b"<a/>").kind, DocKind::Xml);
        assert_eq!(view_of("a.txt", b"hello").kind, DocKind::Text);
        assert_eq!(view_of("Makefile", b"all:").kind, DocKind::Text);
        assert_eq!(view_of("a.md", b"x").kind, DocKind::Markdown);
    }

    #[test]
    fn text_formats_have_a_single_block() {
        let v = view_of("a.txt", b"hello");
        assert_eq!(v.blocks.len(), 1);
    }

    #[test]
    fn binary_files_are_detected_by_nul() {
        let v = view_of("img.png", b"\x89PNG\r\n\x1a\n\0\0\0");
        assert_eq!(v.kind, DocKind::Binary);
        assert_eq!(v.size_bytes, 11);
        assert!(v.html.is_empty() && v.blocks.is_empty());
    }

    #[test]
    fn files_over_the_limit_are_too_large_and_not_read() {
        let d = tempfile::tempdir().expect("tempdir");
        let p = d.path().join("big.md");
        let f = fs::File::create(&p).expect("create");
        f.set_len(MAX_RENDER_BYTES + 1).expect("set_len");
        let v = load_document(&Registry::with_defaults(), &p, "big.md").expect("load");
        assert_eq!(v.kind, DocKind::TooLarge);
        assert_eq!(v.size_bytes, MAX_RENDER_BYTES + 1);
        assert_eq!(v.html.len(), 0);
    }

    #[test]
    fn encoding_issues_are_reported_in_the_note() {
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend("# Hi\r\n".encode_utf16().flat_map(u16::to_le_bytes));
        let v = view_of("a.md", &utf16);
        assert_eq!(v.kind, DocKind::Markdown);
        assert_eq!(v.title.as_deref(), Some("Hi"));
        assert!(v.encoding_note.is_some_and(|n| n.contains("UTF-16")));

        let v = view_of("b.md", b"# Caf\xE9\n");
        assert!(v.encoding_note.is_some());
        assert!(v.html.contains('\u{FFFD}'));
    }

    #[test]
    fn a_rendering_fallback_is_reported_in_the_note() {
        let v = view_of("deep.md", format!("{} x\n", ">".repeat(500)).as_bytes());
        assert_eq!(v.kind, DocKind::Text);
        assert!(v.encoding_note.is_some_and(|n| n.contains("nesting")));
        assert_eq!(v.blocks.len(), 1);
        assert!(v.html.contains("&gt;&gt;"));
    }

    #[test]
    fn crlf_documents_render_like_lf_documents() {
        let lf = view_of("a.md", b"# T\n\nBody\n");
        let crlf = view_of("a.md", b"# T\r\n\r\nBody\r\n");
        assert_eq!(lf.html, crlf.html);
        assert_eq!(lf.blocks, crlf.blocks);
    }

    #[test]
    fn empty_file_is_an_empty_markdown_document() {
        let v = view_of("empty.md", b"");
        assert_eq!(v.kind, DocKind::Markdown);
        assert_eq!(v.blocks.len(), 0);
    }

    #[test]
    fn missing_file_is_an_io_error() {
        let d = tempfile::tempdir().expect("tempdir");
        let r = load_document(
            &Registry::with_defaults(),
            &d.path().join("nope.md"),
            "nope.md",
        );
        assert!(matches!(r, Err(RenderError::Io { .. })));
    }

    #[test]
    fn custom_renderers_take_priority_over_the_fallback() {
        struct Csv;
        impl Renderer for Csv {
            fn can_handle(&self, path: &Path) -> bool {
                path.extension().is_some_and(|e| e == "csv")
            }
            fn render(&self, _src: &str, _ctx: &RenderCtx<'_>) -> Rendered {
                Rendered {
                    kind: DocKind::Text,
                    html: "<table></table>".to_owned(),
                    title: None,
                    frontmatter: None,
                    note: None,
                }
            }
        }
        let reg = Registry::from_renderers(vec![Box::new(Csv), Box::new(plain::TextRenderer)]);
        assert_eq!(reg.render(Path::new("a.csv"), "x").html, "<table></table>");
        assert!(reg.render(Path::new("a.txt"), "x").html.contains("<pre>"));
    }

    #[test]
    fn serializes_to_the_ui_contract() {
        let v = view_of("a.md", b"# T\n");
        let json = serde_json::to_value(&v).expect("json");
        for field in [
            "path",
            "kind",
            "html",
            "blocks",
            "title",
            "frontmatter",
            "modifiedMs",
            "sizeBytes",
            "encodingNote",
        ] {
            assert!(json.get(field).is_some(), "missing {field}");
        }
        assert_eq!(json["kind"], "markdown");
        assert!(json["frontmatter"].is_null());
    }
}
