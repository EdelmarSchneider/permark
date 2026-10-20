//! The pinned TOON specification: decode (strict) and encode through the official implementation, the indent size a
//! file uses, and the reason a text is not valid TOON.

use permark_core::format::Problem;
use serde_json::Value;
use toon_format::types::{DecodeOptions, EncodeOptions, Indent};
use toon_format::ToonError;

/// The one version of the specification this build reads and writes.
pub const SPEC_VERSION: &str = "4.4";
/// How the app names it.
pub const SPEC_LABEL: &str = "TOON 4.4";

/// Spaces per level: the smallest indentation of a content line (2 when no line is indented).
#[must_use]
pub fn indent_of(text: &str) -> usize {
    text.lines()
        .filter_map(|line| {
            let spaces = line.bytes().take_while(|b| *b == b' ').count();
            let rest = line[spaces..].trim_end();
            (spaces > 0 && !rest.is_empty() && !rest.starts_with('#')).then_some(spaces)
        })
        .min()
        .map_or(2, |n| n.min(16))
}

/// The data of a TOON text, decoded strictly by the pinned specification.
///
/// # Errors
/// The text is not valid TOON 4.4: the line and the decoder's message.
pub fn decode(text: &str) -> Result<Value, Problem> {
    let indent = indent_of(text);
    if let Some(line) = too_deep(text, indent) {
        return Err(Problem {
            line,
            detail: format!("nesting deeper than {MAX_DEPTH} levels"),
        });
    }
    let options = DecodeOptions::new()
        .with_strict(true)
        .with_indent(Indent::Spaces(indent));
    toon_format::decode::<Value>(text, &options).map_err(|e| problem(text, &e))
}

/// Deepest nesting read (indentation levels; braces of a field list): readers recurse over it.
pub const MAX_DEPTH: usize = 256;

/// The first line (1-based) nested deeper than [`MAX_DEPTH`].
fn too_deep(text: &str, indent: usize) -> Option<u32> {
    text.lines().zip(1_u32..).find_map(|(line, n)| {
        let spaces = line.bytes().take_while(|b| *b == b' ').count();
        let mut depth = 0_usize;
        let mut braces = 0_usize;
        for b in line.bytes() {
            match b {
                b'{' => {
                    depth += 1;
                    braces = braces.max(depth);
                }
                b'}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        (spaces / indent.max(1) > MAX_DEPTH || braces > MAX_DEPTH / 4).then_some(n)
    })
}

/// A value as TOON text (comma delimiter, 2 spaces, canonical form of the specification).
///
/// # Errors
/// The value holds something TOON cannot write (the encoder's message).
pub fn encode(value: &Value) -> Result<String, String> {
    toon_format::encode(value, &EncodeOptions::new()).map_err(|e| e.to_string())
}

fn problem(text: &str, error: &ToonError) -> Problem {
    // a mark only older versions had: say so instead of guessing what the file meant
    if let Some(line) = old_length_marker(text) {
        return Problem {
            line,
            detail: format!(
                "`[#N]` length marker: written for TOON before 2.0, not valid {SPEC_LABEL}"
            ),
        };
    }
    match error {
        ToonError::ParseError { line, message, .. } => Problem {
            line: u32::try_from(*line).unwrap_or(0),
            detail: message.clone(),
        },
        other => Problem {
            line: 0,
            detail: other.to_string(),
        },
    }
}

/// The first line with a `[#N]` header (1-based).
fn old_length_marker(text: &str) -> Option<u32> {
    text.lines().zip(1_u32..).find_map(|(line, n)| {
        let at = line.find("[#")?;
        let digits = line[at + 2..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        (digits > 0 && line[at + 2 + digits..].starts_with(']')).then_some(n)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indent_is_the_smallest_used() {
        assert_eq!(indent_of("a: 1"), 2);
        assert_eq!(indent_of("a:\n    b: 1\n    c:\n        d: 2"), 4);
        assert_eq!(indent_of("a:\n  b:\n    c: 1"), 2);
        assert_eq!(indent_of("a:\n  # comment\n  b: 1"), 2);
    }

    #[test]
    fn decodes_strictly_and_names_the_line() {
        assert_eq!(decode("a: 1").ok(), Some(serde_json::json!({ "a": 1 })));
        let err = decode("a: 1\nitems[3]: x,y").err().map(|p| p.line);
        assert_eq!(err, Some(2));
        let old = decode("items[#2]: a,b").err();
        assert!(old.is_some_and(|p| p.line == 1 && p.detail.contains("before 2.0")));
    }

    #[test]
    fn four_space_files_decode() {
        assert!(decode("a:\n    b: 1\n    c:\n        d: 2").is_ok());
    }

    #[test]
    fn encodes_tables() {
        let value = serde_json::json!({ "users": [{ "id": 1, "name": "Ada" }, { "id": 2, "name": "Bob" }] });
        assert_eq!(
            encode(&value).ok().as_deref(),
            Some("users[2]{id,name}:\n  1,Ada\n  2,Bob")
        );
    }
}
