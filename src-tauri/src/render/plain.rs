//! JSON, XML and plain-text renderers: formatted, escaped `<pre>` blocks (rich views come in v0.2).

use std::path::Path;

use super::{DocKind, RenderCtx, Rendered, Renderer};
use crate::util::escape_html;

/// A `<pre><code>` block with the text escaped.
pub(super) fn pre(lang: &str, text: &str) -> String {
    format!(
        "<pre><code class=\"language-{lang}\">{}</code></pre>\n",
        escape_html(text)
    )
}

fn has_extension(path: &Path, wanted: &[&str]) -> bool {
    path.extension().is_some_and(|e| {
        let e = e.to_string_lossy().to_ascii_lowercase();
        wanted.contains(&e.as_str())
    })
}

pub(crate) struct JsonRenderer;

impl Renderer for JsonRenderer {
    fn can_handle(&self, path: &Path) -> bool {
        has_extension(path, &["json", "jsonc", "geojson", "webmanifest"])
    }

    /// Pretty-prints valid JSON (keeping key order); anything else is shown as written.
    fn render(&self, src: &str, _ctx: &RenderCtx<'_>) -> Rendered {
        let text = serde_json::from_str::<serde_json::Value>(src)
            .ok()
            .and_then(|v| serde_json::to_string_pretty(&v).ok())
            .unwrap_or_else(|| src.to_owned());
        Rendered {
            kind: DocKind::Json,
            html: pre("json", &text),
            title: None,
            frontmatter: None,
            note: None,
        }
    }
}

pub(crate) struct XmlRenderer;

impl Renderer for XmlRenderer {
    fn can_handle(&self, path: &Path) -> bool {
        has_extension(
            path,
            &[
                "xml", "xsd", "xsl", "xslt", "svg", "plist", "csproj", "rss", "atom",
            ],
        )
    }

    fn render(&self, src: &str, _ctx: &RenderCtx<'_>) -> Rendered {
        Rendered {
            kind: DocKind::Xml,
            html: pre("xml", src),
            title: None,
            frontmatter: None,
            note: None,
        }
    }
}

/// The fallback: shows any text file as-is.
pub(crate) struct TextRenderer;

impl Renderer for TextRenderer {
    fn can_handle(&self, _path: &Path) -> bool {
        true
    }

    fn render(&self, src: &str, _ctx: &RenderCtx<'_>) -> Rendered {
        Rendered {
            kind: DocKind::Text,
            html: pre("text", src),
            title: None,
            frontmatter: None,
            note: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RenderCtx<'static> {
        RenderCtx {
            path: Path::new("x"),
        }
    }

    #[test]
    fn json_is_pretty_printed_in_key_order_and_escaped() {
        let r = JsonRenderer.render("{\"b\":1,\"a\":\"<x>\",\"c\":[1,2]}", &ctx());
        assert_eq!(r.kind, DocKind::Json);
        let b = r.html.find("&quot;b&quot;").expect("b");
        let a = r.html.find("&quot;a&quot;").expect("a");
        assert!(b < a, "key order must be preserved: {}", r.html);
        assert!(r.html.contains("&lt;x&gt;"));
        assert!(r.html.contains('\n'));
        assert!(!r.html.contains("<x>"));
    }

    #[test]
    fn invalid_json_is_shown_as_written() {
        let r = JsonRenderer.render("{ // comment\n \"a\": 1, }", &ctx());
        assert!(r.html.contains("// comment"), "{}", r.html);
    }

    #[test]
    fn xml_and_text_are_escaped_pre_blocks() {
        let x = XmlRenderer.render("<a href=\"x\">&</a>", &ctx());
        assert_eq!(x.kind, DocKind::Xml);
        assert!(x
            .html
            .contains("&lt;a href=&quot;x&quot;&gt;&amp;&lt;/a&gt;"));
        let t = TextRenderer.render("a < b", &ctx());
        assert_eq!(t.kind, DocKind::Text);
        assert!(t.html.contains("a &lt; b"));
    }

    #[test]
    fn can_handle_matches_extensions_case_insensitively() {
        assert!(JsonRenderer.can_handle(Path::new("a/B.JSON")));
        assert!(!JsonRenderer.can_handle(Path::new("a/b.md")));
        assert!(XmlRenderer.can_handle(Path::new("pom.xml")));
        assert!(TextRenderer.can_handle(Path::new("anything.xyz")));
    }
}
