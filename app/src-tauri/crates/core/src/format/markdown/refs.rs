//! Where the `@path` references reading marks sit in the source, so a move can keep them pointing at their files.
//!
//! The parse reading does ([`reading_text`], [`parsed`]) and the scan it marks with ([`inline::references`], outside
//! [`in_link`]): text, not code, links, images or HTML. Every line keeps its place through the cuts and markers, so a text node's line and
//! column are its place in the source. A text node whose source does not read as its literal (escapes, entities) is
//! left out: a reference not located with certainty is never edited.

use std::ops::Range;

use comrak::nodes::NodeValue;
use comrak::Arena;

use super::{in_link, inline, options, parsed, reading_text, too_many_container_markers};
use crate::format::{DocEnv, MarkdownFeatures};
use crate::links::Lines;

const BOM: &str = "\u{feff}";

/// One `@path` reference of a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtRef {
    /// Byte range of the path as written (after the `@`).
    pub range: Range<usize>,
    /// The project path it names (`/` between folders, no leading `/`).
    pub path: String,
    /// 1-based line.
    pub line: usize,
    /// The path as written.
    written: String,
    /// What follows the path up to the first character a path cannot hold (a sentence's `.`): it decides where a path
    /// written in its place ends.
    follows: String,
}

/// Asks the caller whether a project path names an existing file.
struct Exists<'a>(&'a dyn Fn(&str) -> bool);

impl DocEnv for Exists<'_> {
    fn file_exists(&self, project_path: &str) -> bool {
        (self.0)(project_path)
    }
}

impl AtRef {
    /// The path to write instead so that the reference names `target` (a project path), written the way this one is
    /// (a leading `/`, `\` between folders); `None` when reading would not see a reference to `target` there (a name
    /// with a space or a parenthesis, no extension).
    #[must_use]
    pub fn rewritten(&self, target: &str) -> Option<String> {
        let backslash = self.written.contains('\\') && !self.written.contains('/');
        let mut path = if backslash {
            target.replace('/', "\\")
        } else {
            target.to_owned()
        };
        if let Some(root) = self
            .written
            .chars()
            .next()
            .filter(|c| matches!(c, '/' | '\\'))
        {
            path.insert(0, root);
        }
        // read back by the rule, with what follows it: the new text names exactly `target`
        let only = |p: &str| p == target;
        let next = format!("{path}{}", self.follows);
        (inline::reference_at(&next, &Exists(&only)) == Some(path.as_str())).then_some(path)
    }
}

/// The `@path` references of the Markdown `text` that name an existing file (`exists`, asked with a project path), as
/// reading marks them; none when the prompt features are off (reading marks none then). In text order.
///
/// A reference in the same text run as an escape or an entity (`\*`, `&amp;`) is left as written: there the source
/// bytes are not the text reading sees, so its place is not known for certain and a guessed edit could break the
/// document.
#[must_use]
pub fn at_refs(
    text: &str,
    features: &MarkdownFeatures,
    exists: &dyn Fn(&str) -> bool,
) -> Vec<AtRef> {
    if features.prompt_blocks.is_none() || !text.contains('@') || too_many_container_markers(text) {
        return Vec::new();
    }
    // reading gets the text decoded, without its byte order mark
    let offset = if text.starts_with(BOM) { BOM.len() } else { 0 };
    let body = &text[offset..];
    let prepared = reading_text(body, features);
    let arena = Arena::new();
    let Some(root) = parsed(&arena, &prepared, &options()) else {
        return Vec::new();
    };
    let lines = Lines::new(body);
    let env = Exists(exists);
    let mut found = Vec::new();
    for node in root.descendants() {
        let data = node.data();
        let NodeValue::Text(literal) = &data.value else {
            continue;
        };
        if !literal.contains('@') {
            continue;
        }
        let Some(range) = lines.range_of(data.sourcepos, body) else {
            continue;
        };
        if in_link(node) || body.get(range.clone()) != Some(literal.as_ref()) {
            continue;
        }
        for (at, written) in inline::references(literal, &env) {
            let start = range.start + at;
            let after = &literal[at + written.len()..];
            let follows = after
                .char_indices()
                .find(|(_, c)| !inline::is_path_char(*c))
                .map_or(after, |(end, _)| &after[..end]);
            found.push(AtRef {
                range: offset + start..offset + start + written.len(),
                path: crate::links::percent_decode(written)
                    .replace('\\', "/")
                    .trim_start_matches('/')
                    .to_owned(),
                line: lines.line_of(start),
                written: written.to_owned(),
                follows: follows.to_owned(),
            });
        }
    }
    found.sort_by_key(|r| r.range.start);
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{PromptBlocks, DEFAULT_PROMPT_TAGS};

    const PROMPTS: MarkdownFeatures = MarkdownFeatures {
        prompt_blocks: Some(PromptBlocks {
            tags: DEFAULT_PROMPT_TAGS,
        }),
        front_matter_card: None,
    };

    fn files(path: &str) -> bool {
        ["docs/guide.md", "src/main.rs"].contains(&path)
    }

    /// The references found, as the bytes their ranges cover and their lines.
    fn found(text: &str) -> Vec<(String, usize)> {
        at_refs(text, &PROMPTS, &files)
            .into_iter()
            .map(|r| (text[r.range.clone()].to_owned(), r.line))
            .collect()
    }

    #[test]
    fn references_in_text_are_found_where_their_paths_are_written() {
        let text = "Read @docs/guide.md first.\n\n- see @src/main.rs, and *@docs/guide.md*\n\n# About @docs/guide.md\n\n| a |\n|---|\n| @src/main.rs |\n";
        assert_eq!(
            found(text),
            [
                ("docs/guide.md".to_owned(), 1),
                ("src/main.rs".to_owned(), 3),
                ("docs/guide.md".to_owned(), 3),
                ("docs/guide.md".to_owned(), 5),
                ("src/main.rs".to_owned(), 9),
            ]
        );
    }

    #[test]
    fn code_links_html_front_matter_and_missing_files_are_not_references() {
        let text = "---\nsee: \"@docs/guide.md\"\n---\n`@docs/guide.md` [@docs/guide.md](x.md) <b title=\"@docs/guide.md\">b</b>\n\n```\n@docs/guide.md\n```\n\n    @docs/guide.md\n\n@docs/missing.md me@docs/guide.md\n";
        assert_eq!(found(text), []);
    }

    #[test]
    fn prompt_blocks_hold_references_and_line_endings_and_a_bom_keep_their_place() {
        let text = "\u{feff}<context>\r\n@docs/guide.md\r\n</context>\r\nSee \\* @src/main.rs\r\n";
        let refs = at_refs(text, &PROMPTS, &files);
        assert_eq!(
            refs.iter()
                .map(|r| (&text[r.range.clone()], r.path.as_str(), r.line))
                .collect::<Vec<_>>(),
            [("docs/guide.md", "docs/guide.md", 2)]
        );
    }

    #[test]
    fn text_before_a_reference_on_its_line_may_be_any_script() {
        // the parser counts columns in bytes: a wider character before a reference shifts nothing
        let text = "É @docs/guide.md, 日本 @src/main.rs\n- ü *@docs/guide.md*\n";
        assert_eq!(
            found(text),
            [
                ("docs/guide.md".to_owned(), 1),
                ("src/main.rs".to_owned(), 1),
                ("docs/guide.md".to_owned(), 2),
            ]
        );
    }

    fn no_card(_: crate::format::FrontMatterSyntax, _: &str) -> Vec<crate::format::Field> {
        Vec::new()
    }

    /// The `@path` texts reading makes links of, with the lines of the block holding each.
    fn marked(text: &str, features: &MarkdownFeatures) -> Vec<(String, u32, u32)> {
        let known = std::collections::HashSet::new();
        let rendered = super::super::render(
            text.strip_prefix(BOM).unwrap_or(text),
            &crate::format::RenderCx {
                path: "README.md",
                env: &Exists(&files),
                features,
                known: &known,
            },
        );
        let crate::format::Body::Markdown { blocks, .. } = rendered.body else {
            panic!("not markdown");
        };
        let mut out = Vec::new();
        for block in blocks {
            let html = block.html.unwrap_or_default();
            for rest in html.split("<a class=\"at-ref\"").skip(1) {
                let start = rest.find("\">").unwrap() + 2;
                let end = rest.find("</a>").unwrap();
                out.push((
                    rest[start..end].to_owned(),
                    block.line_start,
                    block.line_end,
                ));
            }
        }
        out
    }

    #[test]
    fn every_reference_found_is_one_reading_marks() {
        // (text, whether every reference shares no text run with an escape | entity)
        let cases = [
            ("---\ntitle: \"@docs/guide.md\"\n---\nRead @docs/guide.md.\n", true),
            ("+++\nsee = \"@src/main.rs\"\n+++\n# @src/main.rs\n", true),
            ("<context>\n@docs/guide.md\n</context>\n\n<rules>\n- see @src/main.rs\n</rules>\n", true),
            ("\u{feff}---\r\na: 1\r\n---\r\n<context>\r\n@docs/guide.md\r\n</context>\r\n", true),
            ("É @docs/guide.md\n\n| a | b |\n|---|---|\n| @src/main.rs | x |\n\n> ü @docs/guide.md\n", true),
            ("See \\* @src/main.rs and &amp; @docs/guide.md\n\n@docs/guide.md\n", false),
        ];
        let card = MarkdownFeatures {
            front_matter_card: Some(no_card),
            ..PROMPTS
        };
        for features in [PROMPTS, card] {
            for (text, plain) in cases {
                let marked = marked(text, &features);
                let refs = at_refs(text, &features, &files);
                let mut left = marked.iter();
                for r in &refs {
                    let written = format!("@{}", &text[r.range.clone()]);
                    let line = u32::try_from(r.line).unwrap();
                    assert!(
                        left.any(|(m, start, end)| *m == written && (*start..=*end).contains(&line)),
                        "{written} on line {line} is not marked in {text:?}: {marked:?}"
                    );
                }
                if plain {
                    assert_eq!(refs.len(), marked.len(), "{text:?}");
                } else {
                    assert!(refs.len() < marked.len(), "{text:?}");
                }
            }
        }
    }

    #[test]
    fn nothing_is_a_reference_without_the_prompt_features() {
        assert_eq!(
            at_refs("Read @docs/guide.md.", &MarkdownFeatures::default(), &files),
            []
        );
    }

    #[test]
    fn a_new_path_is_written_like_the_old_one_and_only_when_it_still_reads_as_a_reference() {
        let one = |text: &str| at_refs(text, &PROMPTS, &files).remove(0);
        let plain = one("See @docs/guide.md.");
        assert_eq!(plain.path, "docs/guide.md");
        assert_eq!(
            plain.rewritten("docs/manual.md").as_deref(),
            Some("docs/manual.md")
        );
        assert_eq!(plain.rewritten("docs/my guide.md"), None);
        assert_eq!(plain.rewritten("docs/guide (1).md"), None);
        assert_eq!(plain.rewritten("docs/guide"), None);
        let rooted = one("See @/docs/guide.md");
        assert_eq!(rooted.path, "docs/guide.md");
        assert_eq!(rooted.rewritten("a/b.md").as_deref(), Some("/a/b.md"));
        let back = one("See @docs\\guide.md");
        assert_eq!(back.rewritten("a/b.md").as_deref(), Some("a\\b.md"));
        let unicode = one("See @docs/guide.md");
        assert_eq!(unicode.rewritten("café/ü.md").as_deref(), Some("café/ü.md"));
    }
}
