//! Property checks of the sanitizer over generated hostile markup: whatever goes in, nothing that runs, loads a page
//! or reaches outside the project survives, and a second pass (a parse of the output, as the page does) keeps its text
//! and attributes. Few cases by default; `PROPTEST_CASES` raises them.

use std::collections::BTreeSet;

use proptest::prelude::*;

use super::{normalized_url, sanitize, ASSET_PREFIX};
use crate::links::scheme_of;

/// Elements that run code, load another page or change how the page reads its URLs.
const FORBIDDEN_TAGS: [&str; 17] = [
    "script", "iframe", "object", "embed", "base", "meta", "style", "form", "link", "svg", "math",
    "frame", "frameset", "applet", "template", "textarea", "button",
];

/// Schemes a link may keep (ammonia's list); pictures add raster `data:` and the asset scheme.
const LINK_SCHEMES: [&str; 25] = [
    "bitcoin",
    "ftp",
    "ftps",
    "geo",
    "http",
    "https",
    "im",
    "irc",
    "ircs",
    "magnet",
    "mailto",
    "mms",
    "mx",
    "news",
    "nntp",
    "openpgp4fpr",
    "sip",
    "sms",
    "smsto",
    "ssh",
    "tel",
    "url",
    "webcal",
    "wtai",
    "xmpp",
];

const URL_ATTRS: [&str; 11] = [
    "href",
    "src",
    "cite",
    "action",
    "formaction",
    "poster",
    "data",
    "ping",
    "longdesc",
    "background",
    "xlink:href",
];

/// A piece of serialized output: text, or one tag from its `<` to its `>`.
enum Piece<'a> {
    Text(&'a str),
    Tag(&'a str),
}

/// Sanitizer output split into text and tags. Text escapes `<` and comments are stripped, so every `<` opens a tag; a
/// tag runs to the first `>` outside a value (values are double-quoted with `"` escaped, `>` is not). Script | style
/// text (never escaped) would read as tags, which only makes the checks stricter.
fn pieces(html: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        if open > 0 {
            out.push(Piece::Text(&rest[..open]));
        }
        rest = &rest[open..];
        let mut quoted = false;
        let end = rest
            .char_indices()
            .find(|&(_, c)| {
                quoted ^= c == '"';
                c == '>' && !quoted
            })
            .map_or(rest.len(), |(at, _)| at + 1);
        out.push(Piece::Tag(&rest[..end]));
        rest = &rest[end..];
    }
    if !rest.is_empty() {
        out.push(Piece::Text(rest));
    }
    out
}

fn unescape(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&nbsp;", "\u{a0}")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// One start tag: its name and attributes (values with the serializer's escapes undone).
struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
}

/// The start tag `tag` (`<` to `>`), or `None` for an end tag.
fn start_tag(tag: &str) -> Option<Tag> {
    let inner = tag.strip_prefix('<')?;
    let mut rest = inner.strip_suffix('>').unwrap_or(inner);
    if rest.starts_with('/') {
        return None;
    }
    let name_end = rest
        .find(|c: char| c.is_whitespace() || c == '/')
        .unwrap_or(rest.len());
    let name = rest[..name_end].to_ascii_lowercase();
    rest = &rest[name_end..];
    let mut attrs = Vec::new();
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
        if rest.is_empty() {
            return Some(Tag { name, attrs });
        }
        let attr_end = rest
            .find(|c: char| c.is_whitespace() || c == '=')
            .unwrap_or(rest.len());
        let attr = rest[..attr_end].to_ascii_lowercase();
        rest = &rest[attr_end..];
        let mut value = String::new();
        if let Some(after) = rest.strip_prefix("=\"") {
            let close = after.find('"').unwrap_or(after.len());
            value = unescape(&after[..close]);
            rest = after.get(close + 1..).unwrap_or("");
        }
        attrs.push((attr, value));
    }
}

fn start_tags(html: &str) -> impl Iterator<Item = Tag> + '_ {
    pieces(html).into_iter().filter_map(|piece| match piece {
        Piece::Tag(tag) => start_tag(tag),
        Piece::Text(_) => None,
    })
}

/// What a second pass must keep of output: its text (white space aside: the parser drops the line break that opens a
/// `<pre>`) and its attributes. Its tree may differ (the parser re-nests blocks the first pass left misnested, e.g. a
/// `<pre>` inside a `<p>` reads as its sibling), and the asset URLs of pictures and players go (written by the
/// sanitizer from relative URLs, dropped like any author's URL with that scheme).
fn content(html: &str) -> (String, BTreeSet<(String, String, String)>) {
    let mut text = String::new();
    let mut attrs = BTreeSet::new();
    for piece in pieces(html) {
        match piece {
            Piece::Text(t) => text.extend(unescape(t).split_whitespace()),
            Piece::Tag(tag) => {
                let Some(tag) = start_tag(tag) else { continue };
                for (attr, value) in tag.attrs {
                    let asset = matches!(attr.as_str(), "src" | "poster")
                        && value.starts_with(ASSET_PREFIX);
                    if !asset {
                        attrs.insert((tag.name.clone(), attr, value));
                    }
                }
            }
        }
    }
    (text, attrs)
}

/// Why `html` (sanitizer output) is unsafe, or `None`.
fn unsafe_part(html: &str) -> Option<String> {
    for tag in start_tags(html) {
        if FORBIDDEN_TAGS.contains(&tag.name.as_str()) {
            return Some(format!("element <{}>", tag.name));
        }
        // every input is inert (the task-list checkbox is the only one meant to exist)
        if tag.name == "input" && !tag.attrs.iter().any(|(a, _)| a == "disabled") {
            return Some("an input that is not disabled".to_owned());
        }
        for (attr, value) in &tag.attrs {
            if attr.starts_with("on") || matches!(attr.as_str(), "style" | "srcdoc" | "srcset") {
                return Some(format!("attribute {attr} on <{}>", tag.name));
            }
            if !URL_ATTRS.contains(&attr.as_str()) {
                continue;
            }
            let normalized = normalized_url(value);
            let media = matches!(tag.name.as_str(), "img" | "video" | "audio" | "source")
                && matches!(attr.as_str(), "src" | "poster");
            let allowed = match scheme_of(&normalized).as_deref() {
                // relative: pictures | players leave rewritten to the asset scheme (empty loads nothing)
                None => {
                    !media
                        || normalized.is_empty()
                        || normalized.starts_with('#')
                        || normalized.starts_with("//")
                }
                Some("data") => tag.name == "img" && attr == "src",
                Some(scheme) if format!("{scheme}:") == ASSET_PREFIX => media,
                Some(scheme) => LINK_SCHEMES.contains(&scheme),
            };
            if !allowed {
                return Some(format!("{attr}={value:?} on <{}>", tag.name));
            }
        }
    }
    None
}

fn tag_name() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "p", "div", "a", "img", "video", "audio", "source", "span", "code", "pre", "details",
        "summary", "table", "tr", "td", "ul", "li", "h1", "mark", "section", "input", "script",
        "iframe", "object", "embed", "base", "meta", "style", "form", "button", "svg", "math",
        "noscript", "textarea", "template", "xmp", "title", "select", "link", "frame", "noembed",
        "mi",
    ])
}

fn attr_name() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "href",
        "src",
        "poster",
        "onclick",
        "onerror",
        "onload",
        "OnMouseOver",
        "style",
        "class",
        "id",
        "name",
        "align",
        "srcdoc",
        "formaction",
        "action",
        "data",
        "xlink:href",
        "data-title",
        "data-lines",
        "type",
        "checked",
        "width",
        "background",
    ])
}

fn attr_value() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::sample::select(vec![
            "javascript:alert(1)",
            " JaVaScRiPt:alert(1)",
            "java\tscript:alert(1)",
            "java&#x09;script:alert(1)",
            "&#106;avascript:alert(1)",
            "javascript&colon;alert(1)",
            "\u{1}javascript:alert(1)",
            "vbscript:msgbox(1)",
            "data:text/html,<script>alert(1)</script>",
            "data:image/png;base64,AAAA",
            "data:image/svg+xml,<svg onload=alert(1)>",
            "DATA:image/png;base64,AA",
            "pm-asset:secret",
            "file:///etc/passwd",
            "blob:https://x/1",
            "img/a.png",
            "../../../../x.png",
            "https://example.com/a",
            "mailto:a@example.com",
            "//evil.example/x",
            "#top",
            "\" onerror=\"alert(1)",
            "x' onload='alert(1)",
            "markdown-alert-note evil",
            "language-rs",
            "checkbox",
            "center",
        ])
        .prop_map(str::to_owned),
        "[ -~]{0,12}",
    ]
}

/// One piece of hostile markup: a start tag with attributes (any quoting), an end tag, text or a comment.
fn piece() -> impl Strategy<Value = String> {
    let attr = (attr_name(), attr_value(), 0..4_u8).prop_map(|(name, value, quote)| match quote {
        0 => format!(" {name}=\"{value}\""),
        1 => format!(" {name}='{value}'"),
        2 => format!(" {name}={}", value.replace(' ', "")),
        _ => format!(" {name}"),
    });
    prop_oneof![
        (tag_name(), prop::collection::vec(attr, 0..4), any::<bool>()).prop_map(
            |(name, attrs, closed)| format!(
                "<{name}{}{}>",
                attrs.concat(),
                if closed { "/" } else { "" }
            )
        ),
        tag_name().prop_map(|name| format!("</{name}>")),
        prop::sample::select(vec![
            "text",
            "<",
            ">",
            "&",
            "\"",
            "'",
            "<!--",
            "-->",
            "<![CDATA[",
            "]]>",
            "</",
            "&lt;",
            "&#60;script&#62;",
            "\n\n",
            "`",
            "**b**",
            "<!doctype html>",
            "\u{0}",
        ])
        .prop_map(str::to_owned),
        "[ -~]{0,8}",
    ]
}

fn markup() -> impl Strategy<Value = String> {
    prop::collection::vec(piece(), 0..24).prop_map(|pieces| pieces.concat())
}

fn config() -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64);
    // a failure prints its smallest input; no regression files are written into the source tree
    ProptestConfig {
        cases,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn nothing_that_runs_or_loads_survives(html in markup()) {
        let out = sanitize(&html, "docs/guide/a.md");
        prop_assert_eq!(unsafe_part(&out), None, "{} -> {}", html, out);
    }

    #[test]
    fn sanitizing_again_keeps_text_and_attributes(html in markup()) {
        let once = sanitize(&html, "docs/guide/a.md");
        let twice = sanitize(&once, "docs/guide/a.md");
        prop_assert_eq!(content(&twice), content(&once), "{} -> {} -> {}", html, once, twice);
    }
}

#[test]
fn the_checker_sees_what_it_looks_for() {
    for bad in [
        "<script>x</script>",
        "<p onclick=\"x\">a</p>",
        "<a href=\"javascript:x\">a</a>",
        "<a href=\" java\tscript:x\">a</a>",
        "<img src=\"data:image/png,x\" style=\"a\">",
        "<a href=\"data:image/png,x\">a</a>",
        "<a href=\"pm-asset:x\">a</a>",
        "<img src=\"x.png\">",
        "<input type=\"text\">",
    ] {
        assert!(unsafe_part(bad).is_some(), "{bad}");
    }
    for good in [
        "<p>a &lt;script&gt;</p>",
        "<a href=\"https://x\">a</a>",
        "<a href=\"#top\">a</a>",
        "<a href=\"guide.md\">a</a>",
        "<img src=\"pm-asset:docs/a.png\" alt=\"x\">",
        "<img src=\"data:image/png;base64,AA\">",
        "<input type=\"checkbox\" disabled=\"\">",
        "<input disabled=\"\">",
    ] {
        assert_eq!(unsafe_part(good), None, "{good}");
    }
    let (text, attrs) = content(
        "<p>a &lt;b&gt;</p><img src=\"pm-asset:a.png\" alt=\"x &quot;y&quot;\"><img src=\"https://x\">",
    );
    assert_eq!(text, "a<b>");
    let attrs: Vec<String> = attrs
        .iter()
        .map(|(t, a, v)| format!("{t} {a}={v}"))
        .collect();
    assert_eq!(attrs, ["img alt=x \"y\"", "img src=https://x"]);
}
