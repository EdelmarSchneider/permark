//! What every config syntax must keep, as checks the plugins run over their corpus: parse never panics, a value edit
//! changes only the value token, comments and line breaks stay, read-only parts refuse.

use super::{ConfigEdit, EditRefusal, NewValue, RowValue, Scalar, ScalarKind, Sheet};
use crate::format::{Editability, Format};

fn scalars(sheet: &Sheet) -> Vec<&Scalar> {
    sheet
        .rows()
        .flat_map(|(_, row)| match &row.value {
            RowValue::Scalar(s) => vec![s],
            RowValue::List { items } => items.iter().collect(),
            RowValue::Table(t) => t.rows.iter().flat_map(|r| r.cells.iter()).collect(),
            RowValue::Raw { .. } | RowValue::Pattern { .. } => Vec::new(),
        })
        .collect()
}

fn text_value(text: &str) -> NewValue {
    NewValue::Text {
        text: text.to_owned(),
    }
}

fn literal_value(text: &str) -> NewValue {
    NewValue::Literal {
        text: text.to_owned(),
    }
}

/// New values tried for a scalar of `kind`.
fn candidates(kind: ScalarKind) -> Vec<NewValue> {
    match kind {
        ScalarKind::String => vec![
            text_value("x"),
            text_value("hello world"),
            text_value(""),
            text_value("with \"double\" and 'single' quotes"),
            text_value("back\\slash and C:\\dir"),
            text_value("# not a comment"),
            text_value("a: b, c = d"),
            text_value("ünï ☃ 日本"),
            text_value("true"),
            text_value("123"),
            text_value(" edge "),
            text_value("tab\there"),
            text_value("${VAR} {{x}} $HOME"),
        ],
        ScalarKind::Number => vec![
            literal_value("0"),
            literal_value("42"),
            literal_value("-7"),
            literal_value("3.5"),
        ],
        ScalarKind::Bool => vec![literal_value("true"), literal_value("false")],
        ScalarKind::Date | ScalarKind::Null => Vec::new(),
    }
}

fn comment_lines(text: &str) -> usize {
    text.lines()
        .filter(|l| l.trim_start().starts_with(['#', ';', '!']))
        .count()
}

/// Every editable scalar of `text` takes several new values: only its token changes and the file reads back as typed.
///
/// # Panics
/// A broken guarantee (the message names the sample).
pub fn check_edits(format: &dyn Format, name: &str, text: &str) {
    let sheet = format
        .sheet(text)
        .unwrap_or_else(|| panic!("{name}: the sample does not parse"));
    let found = scalars(&sheet);
    assert!(!found.is_empty(), "{name}: no value found");
    let mut edited = 0;
    for scalar in found {
        let path = scalar.path.clone();
        let Some(span) = scalar.span.clone() else {
            continue;
        };
        if let Editability::ReadOnly { reason } = &scalar.edit {
            let refusal = format.edit(
                text,
                &ConfigEdit::Set {
                    path,
                    value: text_value("x"),
                },
            );
            assert_eq!(
                refusal,
                Err(EditRefusal::ReadOnly(reason.clone())),
                "{name}: read-only part must refuse"
            );
            continue;
        }
        for value in candidates(scalar.kind) {
            let result = format.edit(
                text,
                &ConfigEdit::Set {
                    path: path.clone(),
                    value: value.clone(),
                },
            );
            let splice = match result {
                Ok(splice) => splice,
                // the syntax may decline a value it cannot write
                Err(EditRefusal::Invalid(_)) => continue,
                Err(other) => panic!("{name}: {path} <- {value:?}: {other}"),
            };
            assert_eq!(
                (splice.start, splice.end),
                (span.start, span.end),
                "{name}: {path}: only the value token is replaced"
            );
            let new = splice.apply(text);
            assert!(
                new.starts_with(&text[..span.start]),
                "{name}: {path}: head moved"
            );
            assert!(
                new.ends_with(&text[span.end..]),
                "{name}: {path}: tail moved"
            );
            assert_eq!(
                comment_lines(&new),
                comment_lines(text),
                "{name}: {path}: comments"
            );
            assert_eq!(
                new.matches('\n').count(),
                text.matches('\n').count(),
                "{name}: {path}: lines"
            );
            let again = format
                .sheet(&new)
                .unwrap_or_else(|| panic!("{name}: {path} <- {value:?}: no longer parses"));
            assert_eq!(
                again.scalar(&path).map(|s| s.text.as_str()),
                Some(value.text()),
                "{name}: {path}: reads back as typed"
            );
            assert_eq!(again.row_count(), sheet.row_count(), "{name}: {path}: rows");
            edited += 1;
        }
    }
    assert!(edited > 0, "{name}: nothing could be edited");
}

/// A path that is not there is not found.
///
/// # Panics
/// A broken guarantee.
pub fn check_refusals(format: &dyn Format, name: &str, text: &str) {
    let missing = format.edit(
        text,
        &ConfigEdit::Set {
            path: "no.such.path".to_owned(),
            value: text_value("x"),
        },
    );
    assert_eq!(missing, Err(EditRefusal::NotFound), "{name}");
}

/// Tiny deterministic generator for the fuzz-ish checks.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % (n.max(1) as u64)).unwrap_or(0)
    }
}

const POOL: &[&str] = &[
    "[", "]", "{", "}", "\"", "'", "#", ":", "=", "-", "\n", "\r\n", "\t", " ", "  ", "\\", "!",
    ";", "|", ">", "&", "*", "?", "%", "`", "$", "{{", "}}", "[[", "]]", "\"\"\"", "'''", "key",
    "a.b", "0", "1e3", "true", "null", "~", "é", "日", "😀", "\u{0}", "\u{feff}", "export ", "---",
    "...", "- ", ": ", " = ", "\\\n",
];

fn touch(format: &dyn Format, text: &str) {
    let _ = format.searchable(text);
    if let Some(sheet) = format.sheet(text) {
        for (i, scalar) in scalars(&sheet).into_iter().enumerate() {
            if let Some(span) = &scalar.span {
                assert!(
                    text.get(span.clone()).is_some(),
                    "span {span:?} of {} in {text:?}",
                    scalar.path
                );
            }
            if i < 3 {
                let _ = format.edit(
                    text,
                    &ConfigEdit::Set {
                        path: scalar.path.clone(),
                        value: text_value("z\"'\\\n"),
                    },
                );
            }
        }
        let _ = sheet.outline();
        let _ = sheet.keys_text(1000);
    }
}

/// Parse, search and edit never panic: every prefix of the samples, mutated samples, random strings.
pub fn check_fuzz(format: &dyn Format, samples: &[&str]) {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for text in samples {
        let step = (text.len() / 60).max(1);
        let mut at = 0;
        while at <= text.len() {
            if text.is_char_boundary(at) {
                touch(format, &text[..at]);
            }
            at += step;
        }
        for _ in 0..120 {
            let mut chars: Vec<String> = text.chars().map(String::from).collect();
            for _ in 0..=rng.below(4) {
                let at = rng.below(chars.len() + 1);
                match rng.below(3) {
                    0 if at < chars.len() => {
                        chars.remove(at);
                    }
                    1 => chars.insert(at, POOL[rng.below(POOL.len())].to_owned()),
                    _ if at < chars.len() => POOL[rng.below(POOL.len())].clone_into(&mut chars[at]),
                    _ => {}
                }
            }
            touch(format, &chars.concat());
        }
    }
    for _ in 0..400 {
        let mut text = String::new();
        for _ in 0..rng.below(40) {
            text.push_str(POOL[rng.below(POOL.len())]);
        }
        touch(format, &text);
    }
    // long lines and deep nesting stay quick and quiet
    touch(format, &"[".repeat(5000));
    touch(format, &"- ".repeat(3000));
    touch(format, &format!("k = \"{}\"", "x".repeat(200_000)));
    touch(format, &"a: ".repeat(20_000));
}

/// All of the above for a corpus; each sample also as CRLF and without a final line break.
///
/// # Panics
/// A broken guarantee.
pub fn check_corpus(format: &dyn Format, samples: &[(&str, &str)]) {
    let all: Vec<&str> = samples.iter().map(|(_, t)| *t).collect();
    check_fuzz(format, &all);
    for (name, text) in samples {
        check_refusals(format, name, text);
        let variants = [
            ((*name).to_owned(), (*text).to_owned()),
            (format!("{name} (CRLF)"), text.replace('\n', "\r\n")),
            (
                format!("{name} (no final break)"),
                text.trim_end().to_owned(),
            ),
        ];
        for (label, variant) in &variants {
            check_edits(format, label, variant);
        }
    }
}
