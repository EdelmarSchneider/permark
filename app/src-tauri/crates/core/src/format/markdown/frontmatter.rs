//! Front matter: the block at the very top of a document (`---` YAML or `+++` TOML). Cut out before the parser
//! sees the text; the lines stay (empty, or a fence) so every later line keeps its number.

use std::borrow::Cow;

use crate::format::FrontMatterSyntax;

/// The front matter found at the top of a text.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Cut<'a> {
    pub syntax: FrontMatterSyntax,
    /// Text between the delimiter lines.
    pub raw: String,
    /// Delimiter lines included.
    pub block: &'a str,
}

fn trim_eol(line: &str) -> &str {
    line.trim_end_matches(['\n', '\r'])
}

/// Splits off the front matter block when the text opens with one that is closed.
pub(super) fn find(text: &str) -> Option<Cut<'_>> {
    let mut lines = text.split_inclusive('\n');
    let first = lines.next()?;
    let delimiter = trim_eol(first).trim_end();
    let syntax = match delimiter {
        "---" => FrontMatterSyntax::Yaml,
        "+++" => FrontMatterSyntax::Toml,
        _ => return None,
    };
    let mut end = first.len();
    let mut raw = String::new();
    for line in lines {
        end += line.len();
        let bare = trim_eol(line).trim_end();
        if bare == delimiter || (syntax == FrontMatterSyntax::Yaml && bare == "...") {
            return Some(Cut {
                syntax,
                raw,
                block: &text[..end],
            });
        }
        raw.push_str(trim_eol(line));
        raw.push('\n');
    }
    None
}

/// The text with the block's lines turned into empty lines (`fence: None`) or a fenced code block
/// (`fence: Some(info)`; falls back to empty lines when the content has a line that could end the fence).
pub(super) fn replace<'a>(text: &'a str, cut: &Cut<'_>, fence: Option<&str>) -> Cow<'a, str> {
    let rest = &text[cut.block.len()..];
    let lines: Vec<&str> = cut.block.split_inclusive('\n').collect();
    let fenced = fence.filter(|_| {
        !lines
            .iter()
            .skip(1)
            .take(lines.len().saturating_sub(2))
            .any(|line| line.trim_start().starts_with("~~~"))
    });
    let mut out = String::with_capacity(text.len());
    for (index, line) in lines.iter().enumerate() {
        let eol = &line[trim_eol(line).len()..];
        match fenced {
            Some(info) if index == 0 => {
                out.push_str("~~~");
                out.push_str(info);
            }
            Some(_) if index + 1 == lines.len() => out.push_str("~~~"),
            Some(_) => out.push_str(trim_eol(line)),
            None => {}
        }
        out.push_str(eol);
    }
    out.push_str(rest);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_yaml_and_toml_blocks() {
        let cut = find("---\ntitle: A\ntags: [x]\n---\n# Body\n").unwrap();
        assert_eq!(cut.syntax, FrontMatterSyntax::Yaml);
        assert_eq!(cut.raw, "title: A\ntags: [x]\n");
        assert_eq!(cut.block, "---\ntitle: A\ntags: [x]\n---\n");
        let toml = find("+++\nname = \"x\"\n+++\ntext").unwrap();
        assert_eq!(toml.syntax, FrontMatterSyntax::Toml);
    }

    #[test]
    fn needs_the_first_line_and_a_closing_line() {
        assert!(find("\n---\na: 1\n---\n").is_none());
        assert!(find("---\na: 1\n").is_none());
        assert!(find("--- x\na: 1\n---\n").is_none());
        assert!(find("---\na: 1\n+++\n").is_none());
        assert!(find("").is_none());
        // YAML may close with three dots
        assert!(find("---\na: 1\n...\nx").is_some());
        // CRLF and trailing spaces on the delimiter lines
        let cut = find("---  \r\na: 1\r\n---\r\nrest").unwrap();
        assert_eq!(cut.raw, "a: 1\n");
        assert_eq!(cut.block, "---  \r\na: 1\r\n---\r\n");
    }

    #[test]
    fn the_replacement_keeps_every_line() {
        let text = "---\na: 1\n---\nBody\n";
        let cut = find(text).unwrap();
        assert_eq!(replace(text, &cut, None), "\n\n\nBody\n");
        assert_eq!(
            replace(text, &cut, Some("yaml")),
            "~~~yaml\na: 1\n~~~\nBody\n"
        );
        // a content line that could close the fence -> empty lines instead
        let tricky = "---\n~~~\n---\nB";
        let cut = find(tricky).unwrap();
        assert_eq!(replace(tricky, &cut, Some("yaml")), "\n\n\nB");
        // CRLF endings stay
        let crlf = "---\r\na: 1\r\n---\r\nB";
        let cut = find(crlf).unwrap();
        assert_eq!(replace(crlf, &cut, None), "\r\n\r\n\r\nB");
        assert_eq!(
            replace(crlf, &cut, Some("yaml")),
            "~~~yaml\r\na: 1\r\n~~~\r\nB"
        );
    }

    #[test]
    fn a_closing_line_at_the_end_of_the_text_works() {
        let text = "---\na: 1\n---";
        let cut = find(text).unwrap();
        assert_eq!(cut.block, text);
        assert_eq!(replace(text, &cut, None), "\n\n");
        assert_eq!(replace(text, &cut, Some("yaml")), "~~~yaml\na: 1\n~~~");
    }
}
