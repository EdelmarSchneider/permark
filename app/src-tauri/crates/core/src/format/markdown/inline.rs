//! Inside text: `{{name}}` template variables and `@path/to/file.ext` file references (prompt-blocks plugin).
//! Pure over a text and a way to ask whether a file exists; returns the HTML that replaces the text, or `None`.

use std::fmt::Write;

use crate::format::DocEnv;
use crate::links::percent_decode;

/// The longest `{{ ... }}` body that is a variable name.
const MAX_VARIABLE: usize = 80;
/// The longest `@path` that is looked up.
const MAX_REF: usize = 400;

fn escape(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

fn is_variable_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

pub(super) fn is_path_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '/' | '\\' | '-' | '@' | '+' | '~')
}

/// Whether an `@` after `previous` (`None`: the start of the text) may start a reference: not inside a word or a path.
fn may_start(previous: Option<char>) -> bool {
    previous.is_none_or(|p| !(p.is_alphanumeric() || is_path_char(p)))
}

/// An extension of 1..12 characters with at least one letter after the last dot of the last segment.
fn has_extension(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && (1..=12).contains(&ext.chars().count())
        && ext.chars().all(char::is_alphanumeric)
        && ext.chars().any(char::is_alphabetic)
}

/// Percent-encodes what a URL path cannot hold (keeps `/`).
fn encode(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~' | b'@' | b'+')
        {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// The `@path` at the start of `text` (without the `@`), when it names an existing file.
pub(super) fn reference_at<'t>(text: &'t str, env: &dyn DocEnv) -> Option<&'t str> {
    let run: &str = {
        let end = text
            .char_indices()
            .find(|(_, c)| !is_path_char(*c))
            .map_or(text.len(), |(i, _)| i);
        &text[..end]
    };
    // a trailing `.` or `,`-like end of a sentence is not part of the path
    let path = run.trim_end_matches(['.', '-', '_', '/', '\\']);
    if path.is_empty() || path.len() > MAX_REF || !has_extension(path) {
        return None;
    }
    let normalized = percent_decode(path).replace('\\', "/");
    let normalized = normalized.trim_start_matches('/');
    if normalized
        .split('/')
        .any(|part| part == ".." || part.is_empty())
    {
        return None;
    }
    env.file_exists(normalized).then_some(path)
}

/// The `@path` references [`mark`] marks in `text`, in order: the offset of each path (after its `@`) and the path as
/// written.
pub(super) fn references<'t>(text: &'t str, env: &dyn DocEnv) -> Vec<(usize, &'t str)> {
    let mut found = Vec::new();
    if !text.contains('@') {
        return found;
    }
    let mut previous: Option<char> = None;
    // a path is read as one piece: an `@` inside it starts nothing
    let mut inside_until = 0;
    for (at, c) in text.char_indices() {
        if at >= inside_until && c == '@' && may_start(previous) {
            if let Some(path) = reference_at(&text[at + 1..], env) {
                found.push((at + 1, path));
                inside_until = at + 1 + path.len();
            }
        }
        previous = Some(c);
    }
    found
}

/// The `{{name}}` variable at the start of `text`: the name and the length of the whole `{{ ... }}`.
fn variable_at(text: &str) -> Option<(&str, usize)> {
    let body = text.strip_prefix("{{")?;
    let close = body.find("}}").filter(|n| *n <= MAX_VARIABLE)?;
    let name = body[..close].trim();
    is_variable_name(name).then_some((name, 2 + close + 2))
}

/// HTML for `text` with its variables and references marked, or `None` when it has none.
pub(super) fn mark(text: &str, env: &dyn DocEnv) -> Option<String> {
    if !text.contains("{{") && !text.contains('@') {
        return None;
    }
    // a reference and a variable never share a character (`{` ends a path, `@` is no name character): each is found on
    // its own
    let mut refs = references(text, env).into_iter().peekable();
    let mut out = String::with_capacity(text.len() + 32);
    let mut plain_from = 0;
    for (at, c) in text.char_indices() {
        if at < plain_from {
            continue;
        }
        if let Some((_, path)) = refs.next_if(|(path_at, _)| *path_at == at + 1) {
            escape(&text[plain_from..at], &mut out);
            out.push_str("<a class=\"at-ref\" href=\"/");
            out.push_str(&encode(
                percent_decode(path)
                    .replace('\\', "/")
                    .trim_start_matches('/'),
            ));
            out.push_str("\">@");
            escape(path, &mut out);
            out.push_str("</a>");
            plain_from = at + 1 + path.len();
        } else if c == '{' {
            if let Some((name, len)) = variable_at(&text[at..]) {
                escape(&text[plain_from..at], &mut out);
                out.push_str("<span class=\"tvar\">{{");
                escape(name, &mut out);
                out.push_str("}}</span>");
                plain_from = at + len;
            }
        }
    }
    // every piece marked is longer than nothing: no piece, nothing moved on
    if plain_from == 0 {
        return None;
    }
    escape(&text[plain_from..], &mut out);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Files(&'static [&'static str]);
    impl DocEnv for Files {
        fn file_exists(&self, path: &str) -> bool {
            self.0.contains(&path)
        }
    }

    const FILES: Files = Files(&["docs/plan.md", "src/main.rs", "a b.md"]);

    #[test]
    fn variables_are_marked_in_text_only() {
        assert_eq!(
            mark("Hello {{ user_name }}, bye {{x.y-z}}!", &FILES).unwrap(),
            "Hello <span class=\"tvar\">{{user_name}}</span>, bye <span class=\"tvar\">{{x.y-z}}</span>!"
        );
        for text in ["{{}}", "{{ a b }}", "{{a", "{ {a} }", "{{a<b}}", "plain"] {
            assert_eq!(mark(text, &FILES), None, "{text}");
        }
        // too long
        let long = format!("{{{{{}}}}}", "a".repeat(200));
        assert_eq!(mark(&long, &FILES), None);
    }

    #[test]
    fn references_need_an_existing_file() {
        assert_eq!(
            mark("See @docs/plan.md.", &FILES).unwrap(),
            "See <a class=\"at-ref\" href=\"/docs/plan.md\">@docs/plan.md</a>."
        );
        assert_eq!(
            mark("(@src/main.rs)", &FILES).unwrap(),
            "(<a class=\"at-ref\" href=\"/src/main.rs\">@src/main.rs</a>)"
        );
        for text in [
            "@docs/missing.md",
            "mail me@docs/plan.md",
            "@docs/plan",
            "@../etc/passwd.txt",
            "@/docs/plan.md.",
            "@",
            "@@docs/plan.md",
        ] {
            let got = mark(text, &FILES);
            // the leading-slash form is the same file; everything else stays text
            assert!(got.is_none() || text.starts_with("@/"), "{text} -> {got:?}");
        }
    }

    #[test]
    fn text_around_is_escaped() {
        assert_eq!(
            mark("<b> & {{x}} \"q\"", &FILES).unwrap(),
            "&lt;b&gt; &amp; <span class=\"tvar\">{{x}}</span> &quot;q&quot;"
        );
        let out = mark("a<script> @docs/plan.md", &FILES).unwrap();
        assert!(
            out.starts_with("a&lt;script&gt; <a class=\"at-ref\""),
            "{out}"
        );
    }

    /// Shared with vitest (`app/src/core/editing/visual/promptText.test.ts`): Edit's mirror of these rules marks the
    /// same pieces.
    const FIXTURE: &str =
        include_str!("../../../../../../src/core/editing/visual/promptText.fixture.json");

    #[derive(serde::Deserialize)]
    struct Fixture {
        files: Vec<String>,
        cases: Vec<Case>,
    }

    #[derive(serde::Deserialize)]
    struct Case {
        text: String,
        html: Option<String>,
    }

    struct Listed(Vec<String>);
    impl DocEnv for Listed {
        fn file_exists(&self, path: &str) -> bool {
            self.0.iter().any(|file| file == path)
        }
    }

    #[test]
    fn shared_fixture_cases() {
        let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
        assert!(fixture.cases.len() >= 30);
        let files = Listed(fixture.files);
        for case in fixture.cases {
            assert_eq!(mark(&case.text, &files), case.html, "{}", case.text);
        }
    }

    /// The references `mark` makes into links, as `@path` texts.
    fn marked_references(html: &str) -> Vec<String> {
        html.split("<a class=\"at-ref\"")
            .skip(1)
            .map(|rest| {
                let start = rest.find("\">").unwrap() + 2;
                rest[start..rest.find("</a>").unwrap()].to_owned()
            })
            .collect()
    }

    #[test]
    fn references_are_the_pieces_mark_makes_links() {
        let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
        let files = Listed(fixture.files);
        for case in fixture.cases {
            let found: Vec<String> = references(&case.text, &files)
                .into_iter()
                .map(|(at, path)| {
                    assert_eq!(&case.text[at - 1..at + path.len()], format!("@{path}"));
                    format!("@{path}")
                })
                .collect();
            let marked = case
                .html
                .as_deref()
                .map(marked_references)
                .unwrap_or_default();
            assert_eq!(found, marked, "{}", case.text);
        }
    }

    #[test]
    fn spaces_and_unicode_in_references() {
        // a space ends the path: names with spaces are not referenced this way
        assert_eq!(mark("@a b.md", &FILES), None);
        let env = Files(&["café/ü.md"]);
        assert_eq!(
            mark("@café/ü.md", &env).unwrap(),
            "<a class=\"at-ref\" href=\"/caf%C3%A9/%C3%BC.md\">@café/ü.md</a>"
        );
    }
}
