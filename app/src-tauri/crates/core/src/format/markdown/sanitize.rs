//! The sanitizer: an allow-list over everything comrak (and raw HTML in a document) produced. Relative `src | poster`
//! of pictures and players leave here as `pm-asset:<project path>`; no other way writes that scheme.

use std::borrow::Cow;
use std::fmt::Write;

use crate::links::{join_relative, percent_decode, scheme_of};

/// Prefix of rewritten asset URLs (the UI maps it onto the asset protocol of its workspace).
pub const ASSET_PREFIX: &str = "pm-asset:";

/// Raster image types that may be embedded as `data:` URLs. SVG is left out on purpose.
const DATA_IMAGE_TYPES: [&str; 4] = ["png", "jpeg", "gif", "webp"];

/// A URL attribute value the way the URL parser reads its scheme: tabs, line breaks, other control characters and
/// spaces are dropped (browsers ignore them, so `da<TAB>ta:` is `data:`), and the case is folded.
fn normalized_url(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control() && !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_raster_data_url(normalized: &str) -> bool {
    normalized.strip_prefix("data:image/").is_some_and(|rest| {
        DATA_IMAGE_TYPES.iter().any(|ty| {
            rest.strip_prefix(ty)
                .is_some_and(|after| after.starts_with([';', ',']))
        })
    })
}

fn is_url_attribute(attr: &str) -> bool {
    matches!(
        attr,
        "href" | "src" | "cite" | "action" | "formaction" | "poster" | "data" | "ping" | "longdesc"
    )
}

fn class_token_allowed(token: &str) -> bool {
    if let Some(lang) = token.strip_prefix("language-") {
        return !lang.is_empty()
            && lang.chars().all(|c| {
                c.is_alphanumeric()
                    || matches!(c, '+' | '-' | '_' | '#' | '.' | ',' | ':' | '{' | '}' | '=')
            });
    }
    token.starts_with("markdown-alert")
        || matches!(
            token,
            "footnotes"
                | "footnote-ref"
                | "footnote-backref"
                | "task-list-item"
                | "task-list-item-checkbox"
                | "contains-task-list"
                | "prompt-block"
                | "tvar"
                | "at-ref"
        )
}

/// Percent-encodes a project path for `pm-asset:` (keeps `/`).
fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len() + 8);
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~' | b'@') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// A relative picture | player URL as the asset URL of the file it names, or `None` (dropped) when it climbs above
/// the project root.
fn asset_url(doc: &str, value: &str) -> Option<String> {
    let path = value.split(['#', '?']).next().unwrap_or(value);
    let decoded = percent_decode(path).replace('\\', "/");
    if decoded.trim().is_empty() {
        return None;
    }
    let joined = join_relative(doc, decoded.trim())?;
    Some(format!("{ASSET_PREFIX}{}", encode_path(&joined)))
}

/// Whether a URL attribute value names a local file (no scheme, not network-path, not an anchor).
fn is_relative(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && !value.starts_with('#')
        && !value.starts_with("//")
        && scheme_of(&normalized_url(value)).is_none()
}

fn filter_attribute<'v>(
    doc: &str,
    element: &str,
    attr: &str,
    value: &'v str,
) -> Option<Cow<'v, str>> {
    match (element, attr) {
        (_, "class") => {
            let kept: Vec<&str> = value
                .split_whitespace()
                .filter(|token| class_token_allowed(token))
                .collect();
            (!kept.is_empty()).then(|| Cow::Owned(kept.join(" ")))
        }
        (_, "id") => (!value.is_empty() && !value.contains(char::is_whitespace))
            .then_some(Cow::Borrowed(value)),
        ("details", "data-tag") => value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            .then_some(Cow::Borrowed(value)),
        (_, "data-tag") => None,
        ("input", "type") => (value == "checkbox").then_some(Cow::Borrowed(value)),
        ("img" | "video" | "audio" | "source", "src") | ("video", "poster") => {
            let normalized = normalized_url(value);
            if normalized.starts_with("data:") {
                // only pictures may travel inline
                return (element == "img" && attr == "src" && is_raster_data_url(&normalized))
                    .then_some(Cow::Borrowed(value));
            }
            if is_relative(value) {
                asset_url(doc, value).map(Cow::Owned)
            } else {
                Some(Cow::Borrowed(value))
            }
        }
        (_, attr) if is_url_attribute(attr) => {
            // `pm-asset:` cannot be written by an author; `data:` only on pictures (above)
            let normalized = normalized_url(value);
            (!normalized.starts_with("data:") && !normalized.starts_with(ASSET_PREFIX))
                .then_some(Cow::Borrowed(value))
        }
        _ => Some(Cow::Borrowed(value)),
    }
}

/// The sanitizer for one document (built once, used from several threads).
pub(super) struct Cleaner(ammonia::Builder<'static>);

impl Cleaner {
    /// For the document at project path `doc` (relative media URLs are made absolute against it).
    pub(super) fn new(doc: &str) -> Self {
        Self(builder(doc))
    }

    pub(super) fn clean(&self, html: &str) -> String {
        self.0.clean(html).to_string()
    }
}

/// Sanitizes `html` for the document at project path `doc`.
#[cfg(test)]
pub(super) fn sanitize(html: &str, doc: &str) -> String {
    Cleaner::new(doc).clean(html)
}

fn builder(doc: &str) -> ammonia::Builder<'static> {
    let doc = doc.to_owned();
    let mut builder = ammonia::Builder::default();
    builder
        .add_tags(["section", "input", "video", "audio", "source"])
        .add_generic_attributes([
            "id",
            "class",
            "data-sourcepos",
            "data-math-style",
            "aria-label",
        ])
        .add_tag_attributes("input", ["type", "disabled", "checked"])
        .add_tag_attributes("section", ["data-footnotes"])
        .add_tag_attributes(
            "a",
            [
                "data-footnote-ref",
                "data-footnote-backref",
                "data-footnote-backref-idx",
            ],
        )
        .add_tag_attributes("details", ["open", "data-tag"])
        .add_tag_attributes("img", ["width", "height", "alt", "title", "src"])
        // Players: where the file is and how it looks. Never `autoplay` (nothing starts by itself, least of all with
        // sound) and no event handlers or styles; the file comes through the asset protocol.
        .add_tag_attributes(
            "video",
            [
                "src", "controls", "loop", "muted", "poster", "width", "height", "preload",
            ],
        )
        .add_tag_attributes("audio", ["src", "controls", "loop", "muted", "preload"])
        .add_tag_attributes("source", ["src", "type"])
        .add_url_schemes(["data"])
        .link_rel(None)
        // Any `<input>` in a document is inert: only the disabled task-list checkbox is meant to exist.
        .set_tag_attribute_value("input", "disabled", "")
        // A player without controls cannot be played, and one that loads its whole file when the document opens would
        // read every film of a long document at once: controls are always there, and only the length is read ahead.
        .set_tag_attribute_value("video", "controls", "")
        .set_tag_attribute_value("audio", "controls", "")
        .set_tag_attribute_value("video", "preload", "metadata")
        .set_tag_attribute_value("audio", "preload", "metadata")
        .attribute_filter(move |element, attr, value| filter_attribute(&doc, element, attr, value));
    builder
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(html: &str) -> String {
        sanitize(html, "docs/guide/a.md")
    }

    #[test]
    fn scripts_handlers_styles_and_forms_go() {
        let hostile = [
            "<script>alert(1)</script><p>ok</p>",
            "<p onclick=\"x()\">a</p>",
            "<img src=x onerror=alert(1)>",
            "<style>body{display:none}</style>t",
            "<form action=\"http://x\"><input type=text name=a><button>b</button></form>",
            "<iframe src=\"http://x\"></iframe>",
            "<object data=\"x\"></object><embed src=\"x\">",
            "<base href=\"http://evil/\">",
            "<meta http-equiv=refresh content=\"0;url=x\">",
            "<svg><script>1</script><circle onload=x /></svg>",
            "<math><mi xlink:href=\"javascript:1\">x</mi></math>",
            "<a href=\"javascript:alert(1)\">x</a>",
            "<a href=\" JaVa\tScRiPt:alert(1)\">x</a>",
            "<a href=\"vbscript:x\">x</a>",
            "<a href=\"data:text/html;base64,PHNjcmlwdD4=\">x</a>",
        ];
        for html in hostile {
            let out = clean(html).to_lowercase();
            for bad in [
                "<script",
                "onclick",
                "onerror",
                "onload",
                "<style",
                "<form",
                "<iframe",
                "<object",
                "<embed",
                "<base",
                "<meta",
                "javascript:",
                "vbscript:",
                "data:text",
                "<svg",
                "<button",
                "type=\"text\"",
            ] {
                assert!(!out.contains(bad), "{html} -> {out}");
            }
        }
        assert!(clean("<script>alert(1)</script><p>ok</p>").contains("<p>ok</p>"));
    }

    #[test]
    fn data_urls_only_for_raster_images() {
        assert!(clean("<img src=\"data:image/png;base64,AAAA\">").contains("data:image/png"));
        assert!(clean("<img src=\"data:image/jpeg,xx\">").contains("data:image/jpeg"));
        for bad in [
            "<img src=\"data:image/svg+xml;base64,PHN2Zz4=\">",
            "<img src=\"data:text/html,<b>\">",
            "<img src=\"da\tta:image/svg+xml,x\">",
            "<video src=\"data:video/mp4;base64,AA\"></video>",
            "<img src=\"data:image/pngx,xx\">",
        ] {
            assert!(!clean(bad).contains("data:"), "{bad}");
        }
    }

    #[test]
    fn relative_media_urls_become_asset_urls() {
        assert_eq!(
            clean("<img src=\"img/a b.png\" alt=\"x\">"),
            "<img src=\"pm-asset:docs/guide/img/a%20b.png\" alt=\"x\">"
        );
        assert!(clean("<img src=\"../x.png\">").contains("pm-asset:docs/x.png"));
        assert!(clean("<img src=\"/top.png\">").contains("pm-asset:top.png"));
        assert!(clean("<img src=\"img%5Ca.png\">").contains("pm-asset:docs/guide/img/a.png"));
        // above the root: dropped
        assert!(!clean("<img src=\"../../../../x.png\">").contains("src"));
        // web pictures stay
        assert!(
            clean("<img src=\"https://example.com/a.png\">").contains("https://example.com/a.png")
        );
        // authors cannot write the scheme
        assert!(!clean("<a href=\"pm-asset:docs/a.md\">x</a>").contains("pm-asset"));
        assert!(!clean("<img src=\"pm-asset:secret\">").contains("pm-asset:secret"));
        // players
        let video =
            clean("<video src=\"clips/a.mp4\" poster=\"p.png\" autoplay onplay=\"x\"></video>");
        assert!(
            video.contains("src=\"pm-asset:docs/guide/clips/a.mp4\""),
            "{video}"
        );
        assert!(
            video.contains("poster=\"pm-asset:docs/guide/p.png\""),
            "{video}"
        );
        assert!(video.contains("controls") && video.contains("preload=\"metadata\""));
        assert!(!video.contains("autoplay") && !video.contains("onplay"));
        // unicode and spaces in names
        assert!(clean("<img src=\"fotos/caf%C3%A9.png\">")
            .contains("pm-asset:docs/guide/fotos/caf%C3%A9.png"));
    }

    #[test]
    fn classes_ids_and_inputs_are_filtered() {
        let out = clean("<p class=\"evil markdown-alert-note language-rs\" id=\"x y\">t</p>");
        assert!(
            out.contains("class=\"markdown-alert-note language-rs\""),
            "{out}"
        );
        assert!(!out.contains("evil") && !out.contains("id="));
        let code = clean("<pre><code class=\"language-c++ x\">a</code></pre>");
        assert!(code.contains("class=\"language-c++\""), "{code}");
        let input = clean("<input type=\"checkbox\" checked><input type=\"text\">");
        assert!(input.contains("type=\"checkbox\"") && !input.contains("text"));
        assert!(input.contains("disabled"));
        let details = clean("<details class=\"prompt-block\" data-tag=\"context\" open><summary>context</summary>x</details>");
        assert!(
            details.contains("data-tag=\"context\"") && details.contains("class=\"prompt-block\"")
        );
        assert!(!clean("<details data-tag=\"a b\">x</details>").contains("data-tag"));
        assert!(!clean("<p data-tag=\"x\">x</p>").contains("data-tag"));
    }

    #[test]
    fn markup_the_pipeline_needs_survives() {
        let html = "<h2 id=\"intro\" data-sourcepos=\"1:1-1:8\">Intro</h2><table><thead><tr><th>a</th></tr></thead><tbody><tr><td>1</td></tr></tbody></table><span data-math-style=\"inline\">x</span><sup class=\"footnote-ref\"><a href=\"#fn-1\" data-footnote-ref>1</a></sup>";
        let out = clean(html);
        for needle in [
            "<h2 id=\"intro\"",
            "data-sourcepos",
            "<table>",
            "data-math-style=\"inline\"",
            "data-footnote-ref",
            "footnote-ref",
        ] {
            assert!(out.contains(needle), "{needle} in {out}");
        }
    }
}
