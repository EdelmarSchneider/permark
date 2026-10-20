//! Structure before content: the outline of a file (Markdown headings, JSON top-level keys, the top-level parts of a
//! config file, plain line count) and the text of one part of it (`at`: lines, a heading path, an anchor, a key path).
//! Every text that leaves is masked: config files by their structure (the secret values of the format), all by key.

use permark_core::format::config::SheetKind;
use permark_core::format::{ext_of, sections, FormatRegistry, Sheet, Span};
use permark_core::paths::fold;
use permark_core::secrets::{mask_text, HIDDEN};
use permark_core::tokens::{estimate, Counted};
use serde_json::{json, Value};

use crate::tools::MAX_READ_CHARS;
use crate::ToolError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Markdown,
    Json,
    Text,
}

/// The sheet of a config file when a config format that is on reads it.
fn sheet_of(formats: &FormatRegistry, path: &str, text: &str) -> Option<(Sheet, Vec<Span>)> {
    let head = &text.as_bytes()[..text.len().min(4096)];
    let format = formats.pick(path, head)?;
    let sheet = format.sheet(text)?;
    Some((sheet, format.secrets(text)))
}

fn kind_of(path: &str) -> Kind {
    match ext_of(path).as_deref() {
        Some("md" | "markdown" | "mdown" | "mkd" | "mdx" | "mdc") => Kind::Markdown,
        Some("json" | "jsonc" | "json5") => Kind::Json,
        _ => Kind::Text,
    }
}

/// One part of a file: a label, its depth, the lines it spans (1-based, both included).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Part {
    at: String,
    label: String,
    level: usize,
    anchor: Option<String>,
    from: u32,
    to: u32,
}

fn line_count(text: &str) -> u32 {
    let n = text.lines().count();
    u32::try_from(n.max(1)).unwrap_or(u32::MAX)
}

fn markdown_parts(text: &str) -> Vec<Part> {
    let all = sections(text);
    let last = line_count(text);
    let headed: Vec<_> = all.iter().filter(|s| !s.heading_path.is_empty()).collect();
    headed
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let level = s.heading_path.len();
            let to = headed[i + 1..]
                .iter()
                .find(|next| next.heading_path.len() <= level)
                .map_or(last, |next| next.line.saturating_sub(1).max(s.line));
            Part {
                at: s.heading_path.join(" > "),
                label: s.heading_path.last().cloned().unwrap_or_default(),
                level,
                anchor: s.anchor.clone(),
                from: s.line,
                to,
            }
        })
        .collect()
}

/// Top-level keys of a JSON object with the lines their values span (a light scan: strings and nesting only).
fn json_parts(text: &str) -> Vec<(Part, &'static str)> {
    let mut parts: Vec<(Part, &'static str)> = Vec::new();
    let mut depth = 0_usize;
    let mut line: u32 = 1;
    let mut chars = text.char_indices().peekable();
    let mut last_string: Option<(String, u32)> = None;
    while let Some((_, c)) = chars.next() {
        match c {
            '\n' => line += 1,
            '"' => {
                let start_line = line;
                let mut value = String::new();
                while let Some((_, d)) = chars.next() {
                    match d {
                        '\\' => {
                            if let Some((_, e)) = chars.next() {
                                value.push(e);
                            }
                        }
                        '"' => break,
                        '\n' => {
                            line += 1;
                            value.push(d);
                        }
                        _ => value.push(d),
                    }
                }
                last_string = Some((value, start_line));
            }
            ':' if depth == 1 => {
                let pending_key = last_string.take();
                // the value's kind is the next non-space character
                let kind = loop {
                    match chars.peek() {
                        Some((_, w)) if w.is_whitespace() => {
                            if *w == '\n' {
                                line += 1;
                            }
                            chars.next();
                        }
                        Some((_, '{')) => break "object",
                        Some((_, '[')) => break "array",
                        Some((_, '"')) => break "string",
                        Some((_, 't' | 'f')) => break "boolean",
                        Some((_, 'n')) | None => break "null",
                        Some(_) => break "number",
                    }
                };
                if let Some((key, at_line)) = pending_key {
                    if let Some((prev, _)) = parts.last_mut() {
                        prev.to = at_line.saturating_sub(1).max(prev.from);
                    }
                    parts.push((
                        Part {
                            at: key.clone(),
                            label: key,
                            level: 1,
                            anchor: None,
                            from: at_line,
                            to: at_line,
                        },
                        kind,
                    ));
                }
            }
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if let Some((prev, _)) = parts.last_mut() {
                        prev.to = line.max(prev.from);
                    }
                }
            }
            _ => {}
        }
    }
    parts
}

/// Counts the text of a part by the project's primary tokenizer; `None` = nothing chosen | no vocabulary.
pub type Tokens<'a> = &'a dyn Fn(&str) -> Option<Counted>;

/// Sizes the parts of one file: `size_tok` (the core estimator, always) and `tokens` (the person's tokenizer, when
/// one is on and ready) of the text a read of the part returns, secrets hidden.
struct Sizer<'a> {
    text: &'a str,
    /// Byte offset where each line starts.
    starts: Vec<usize>,
    spans: Option<&'a [Span]>,
    tokens: Option<Tokens<'a>>,
    /// The tokenizer of the first count: id, estimate or not.
    used: std::cell::Cell<Option<(&'static str, bool)>>,
}

impl<'a> Sizer<'a> {
    fn new(text: &'a str, spans: Option<&'a [Span]>, tokens: Option<Tokens<'a>>) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(at, _)| at + 1));
        Self {
            text,
            starts,
            spans,
            tokens,
            used: std::cell::Cell::new(None),
        }
    }

    /// The text of lines `from..=to` (1-based) as a read returns it (secrets hidden).
    fn shown(&self, from: u32, to: u32) -> String {
        let count = self.starts.len();
        let first = (from.max(1) as usize).min(count + 1);
        let last = (to as usize).min(count);
        let (base, part) = if first > last {
            (0, "")
        } else {
            let begin = self.starts[first - 1];
            let end = if last == count {
                self.text.len()
            } else {
                self.starts[last]
            };
            (begin, &self.text[begin..end])
        };
        masked(part, base, self.spans)
    }

    /// Adds the sizes of lines `from..=to` (1-based) to a part's object.
    fn add(&self, item: &mut Value, from: u32, to: u32) {
        let shown = self.shown(from, to);
        let Some(map) = item.as_object_mut() else {
            return;
        };
        map.insert("size_tok".to_owned(), json!(estimate(&shown)));
        if let Some(counted) = self.tokens.and_then(|count| count(&shown)) {
            map.insert("tokens".to_owned(), json!(counted.value));
            if self.used.get().is_none() {
                self.used.set(Some((counted.tokenizer, counted.approx)));
            }
        }
    }

    /// Names the tokenizer on the answer (once, not on every part).
    fn name_tokenizer(&self, answer: &mut Value) {
        if let (Some((id, approx)), Some(map)) = (self.used.get(), answer.as_object_mut()) {
            map.insert("tokenizer".to_owned(), json!(id));
            if approx {
                map.insert("tokens_approx".to_owned(), json!(true));
            }
        }
    }
}

/// The outline answer. `tokens`: the project's primary tokenizer, when the token count plugin is on and ready.
#[must_use]
pub fn outline(
    formats: &FormatRegistry,
    path: &str,
    text: &str,
    tokens: Option<Tokens<'_>>,
) -> Value {
    let lines = line_count(text);
    if let Some((sheet, spans)) = sheet_of(formats, path, text) {
        let sizer = Sizer::new(text, Some(&spans), tokens);
        let items: Vec<Value> = sheet
            .outline()
            .into_iter()
            .map(|p| {
                let mut item = json!({ "at": p.path, "label": p.label, "level": 1, "line": p.line, "lines": p.end.saturating_sub(p.line) + 1 });
                sizer.add(&mut item, p.line, p.end);
                item
            })
            .collect();
        // a data sheet (TOON) says so, and names the version of the format it was read with
        let kind = if sheet.kind == SheetKind::Data {
            "data"
        } else {
            "config"
        };
        let mut answer = json!({ "path": path, "kind": kind, "lines": lines, "items": items });
        if let Some(spec) = &sheet.spec {
            answer["spec"] = json!(spec);
        }
        sizer.name_tokenizer(&mut answer);
        return answer;
    }
    let sizer = Sizer::new(text, None, tokens);
    let mut answer = match kind_of(path) {
        Kind::Markdown => {
            let items: Vec<Value> = markdown_parts(text)
                .into_iter()
                .map(|p| {
                    let mut item = json!({ "at": p.at, "level": p.level, "line": p.from, "lines": p.to - p.from + 1 });
                    sizer.add(&mut item, p.from, p.to);
                    item
                })
                .collect();
            json!({ "path": path, "kind": "markdown", "lines": lines, "items": items })
        }
        Kind::Json => {
            let valid = serde_json::from_str::<Value>(text).is_ok();
            let items: Vec<Value> = json_parts(text)
                .into_iter()
                .map(|(p, kind)| {
                    let mut item = json!({ "at": p.at, "type": kind, "line": p.from, "lines": p.to - p.from + 1 });
                    sizer.add(&mut item, p.from, p.to);
                    item
                })
                .collect();
            json!({ "path": path, "kind": "json", "valid": valid, "lines": lines, "items": items })
        }
        Kind::Text => json!({ "path": path, "kind": "text", "lines": lines, "items": [] }),
    };
    sizer.name_tokenizer(&mut answer);
    answer
}

/// The approximate tokens of reading one section (`heading_path`, as search reports it) of a file; `None`: the file
/// has no such part (or no structure).
#[must_use]
pub fn section_size(
    formats: &FormatRegistry,
    path: &str,
    text: &str,
    heading_path: &[String],
    line: u32,
) -> Option<u64> {
    let at = heading_path.join(" > ");
    let config = sheet_of(formats, path, text);
    let (from, to) = match &config {
        Some((sheet, _)) if !at.is_empty() => sheet.range_of(&at)?,
        None if kind_of(path) == Kind::Markdown => {
            let parts = markdown_parts(text);
            if at.is_empty() {
                // the lines before the first heading
                let end = parts
                    .first()
                    .map_or(line_count(text), |p| p.from.saturating_sub(1));
                (1, end.max(1))
            } else {
                parts
                    .iter()
                    .find(|p| p.at == at && p.from <= line.max(p.from))
                    .map(|p| (p.from, p.to))
                    .or_else(|| find_part(&parts, &at))?
            }
        }
        _ => return None,
    };
    let sizer = Sizer::new(
        text,
        config.as_ref().map(|(_, spans)| spans.as_slice()),
        None,
    );
    Some(estimate(&sizer.shown(from, to)))
}

/// The approximate tokens of reading a whole file (secrets hidden).
#[must_use]
pub fn file_size(formats: &FormatRegistry, path: &str, text: &str) -> u64 {
    let config = sheet_of(formats, path, text);
    let sizer = Sizer::new(
        text,
        config.as_ref().map(|(_, spans)| spans.as_slice()),
        None,
    );
    estimate(&sizer.shown(1, line_count(text)))
}

/// `L12` | `L12-L40` (also `12-40`).
fn line_range(at: &str) -> Option<(u32, u32)> {
    let number = |s: &str| s.trim().trim_start_matches(['L', 'l']).parse::<u32>().ok();
    let (a, b) = if let Some((a, b)) = at.split_once('-') {
        (number(a)?, number(b)?)
    } else {
        let n = number(at)?;
        (n, n)
    };
    (a >= 1 && a <= b).then_some((a, b))
}

fn same(a: &str, b: &str) -> bool {
    fold(a.trim()) == fold(b.trim())
}

/// Normalizes `A>B`, `A › B`, `A / B`-free heading paths to `A > B` parts.
fn heading_parts(at: &str) -> Vec<String> {
    at.split(['>', '›'])
        .map(|p| p.trim().to_owned())
        .filter(|p| !p.is_empty())
        .collect()
}

fn find_part(parts: &[Part], at: &str) -> Option<(u32, u32)> {
    if let Some(anchor) = at.strip_prefix('#') {
        return parts
            .iter()
            .find(|p| p.anchor.as_deref().is_some_and(|a| same(a, anchor)))
            .map(|p| (p.from, p.to));
    }
    let wanted = heading_parts(at);
    if wanted.is_empty() {
        return None;
    }
    // the full path first, then a path that ends with the asked one (unique), then the last heading alone
    let full = parts.iter().find(|p| {
        let have = heading_parts(&p.at);
        have.len() == wanted.len() && have.iter().zip(&wanted).all(|(a, b)| same(a, b))
    });
    if let Some(p) = full {
        return Some((p.from, p.to));
    }
    let ending: Vec<&Part> = parts
        .iter()
        .filter(|p| {
            let have = heading_parts(&p.at);
            have.len() >= wanted.len()
                && have[have.len() - wanted.len()..]
                    .iter()
                    .zip(&wanted)
                    .all(|(a, b)| same(a, b))
        })
        .collect();
    ending.first().map(|p| (p.from, p.to))
}

/// The first line of a heading of a Markdown text (`A > B`, a tail of it, or `#anchor`).
#[must_use]
pub fn heading_line(path: &str, text: &str, heading: &str) -> Option<u32> {
    if kind_of(path) == Kind::Markdown {
        find_part(&markdown_parts(text), heading).map(|(from, _)| from)
    } else {
        None
    }
}

/// The text of one part (all of it when `at` is missing), masked, cut at [`MAX_READ_CHARS`].
///
/// # Errors
/// `NotFound(at)` when the file has no such part.
pub fn read(
    formats: &FormatRegistry,
    path: &str,
    text: &str,
    at: Option<&str>,
) -> Result<Value, ToolError> {
    let total = line_count(text);
    let at = at.map(str::trim).filter(|a| !a.is_empty());
    let config = sheet_of(formats, path, text);
    let (from, to) = match at {
        None => (1, total),
        Some(at) => match line_range(at) {
            Some((a, b)) if a <= total => (a, b.min(total)),
            Some(_) => return Err(ToolError::NotFound(at.to_owned())),
            None => {
                let found = match kind_of(path) {
                    _ if config.is_some() => {
                        config.as_ref().and_then(|(sheet, _)| sheet.range_of(at))
                    }
                    Kind::Markdown => find_part(&markdown_parts(text), at),
                    Kind::Json => {
                        let parts: Vec<Part> =
                            json_parts(text).into_iter().map(|(p, _)| p).collect();
                        parts.iter().find(|p| p.at == at).map(|p| (p.from, p.to))
                    }
                    Kind::Text => None,
                };
                found.ok_or_else(|| ToolError::NotFound(at.to_owned()))?
            }
        },
    };
    let mut out = String::new();
    let mut last = from.saturating_sub(1);
    let mut cut = None;
    let mut base = 0_usize;
    for (n, line) in (1_u32..).zip(text.split_inclusive('\n')) {
        if n < from {
            base += line.len();
            continue;
        }
        if n > to {
            break;
        }
        if out.chars().count() + line.chars().count() > MAX_READ_CHARS && n > from {
            cut = Some(n);
            break;
        }
        out.push_str(line);
        last = n;
    }
    let mut answer = json!({
        "path": path,
        "lines": [from, last.max(from)],
        "text": masked(&out, base, config.as_ref().map(|(_, spans)| spans.as_slice())),
    });
    if let (Some(map), Some(at)) = (answer.as_object_mut(), at) {
        map.insert("at".to_owned(), json!(at));
    }
    if let (Some(map), Some(next)) = (answer.as_object_mut(), cut) {
        map.insert("cut".to_owned(), json!(format!("L{next}-L{to}")));
    }
    Ok(answer)
}

/// `part` (the bytes of the file from `base` on) with the secret `spans` of the format hidden, then the key-based
/// mask over what is left.
fn masked(part: &str, base: usize, spans: Option<&[Span]>) -> String {
    let Some(spans) = spans.filter(|s| !s.is_empty()) else {
        return mask_text(part).into_owned();
    };
    let end = base + part.len();
    let mut out = String::with_capacity(part.len());
    let mut at = base;
    for span in spans {
        let (from, to) = (span.start.max(base), span.end.min(end));
        if from >= to || from < at {
            continue;
        }
        out.push_str(part.get(at - base..from - base).unwrap_or_default());
        out.push_str(HIDDEN);
        at = to;
    }
    out.push_str(part.get(at - base..).unwrap_or_default());
    mask_text(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    const DOC: &str = "# Plan\n\nIntro.\n\n## Rollout\n\nShip it.\nAPI_KEY=abc\n\n### Steps\n\n1. a\n\n## Risks\n\nNone.\n";

    #[test]
    fn markdown_outline_has_headings_with_lines() {
        let value = outline(&FormatRegistry::core(), "docs/plan.md", DOC, None);
        let items = value["items"].as_array().unwrap();
        let ats: Vec<&str> = items.iter().map(|i| i["at"].as_str().unwrap()).collect();
        assert_eq!(
            ats,
            [
                "Plan",
                "Plan > Rollout",
                "Plan > Rollout > Steps",
                "Plan > Risks"
            ]
        );
        assert_eq!(items[1]["line"], 5);
        assert_eq!(items[1]["lines"], 9, "Rollout holds Steps");
    }

    #[test]
    fn read_a_section_by_path_tail_anchor_and_lines() {
        let full = read(&FormatRegistry::core(), "p.md", DOC, Some("Plan > Rollout")).unwrap();
        assert!(full["text"].as_str().unwrap().starts_with("## Rollout"));
        assert!(full["text"].as_str().unwrap().contains("### Steps"));
        assert!(!full["text"].as_str().unwrap().contains("## Risks"));
        assert!(full["text"].as_str().unwrap().contains("API_KEY=<hidden>"));
        let tail = read(
            &FormatRegistry::core(),
            "p.md",
            DOC,
            Some("rollout › steps"),
        )
        .unwrap();
        assert!(tail["text"].as_str().unwrap().starts_with("### Steps"));
        let anchor = read(&FormatRegistry::core(), "p.md", DOC, Some("#risks")).unwrap();
        assert_eq!(anchor["text"], "## Risks\n\nNone.\n");
        let lines = read(&FormatRegistry::core(), "p.md", DOC, Some("L1-L3")).unwrap();
        assert_eq!(lines["text"], "# Plan\n\nIntro.\n");
        assert_eq!(lines["lines"], json!([1, 3]));
        assert!(matches!(
            read(&FormatRegistry::core(), "p.md", DOC, Some("Nowhere")),
            Err(ToolError::NotFound(_))
        ));
        assert!(matches!(
            read(&FormatRegistry::core(), "p.md", DOC, Some("L900")),
            Err(ToolError::NotFound(_))
        ));
        assert!(matches!(
            read(&FormatRegistry::core(), "p.md", DOC, Some("L5-L2")),
            Err(ToolError::NotFound(_))
        ));
    }

    #[test]
    fn json_outline_and_read_by_key() {
        let text = "{\n  \"name\": \"x\",\n  \"auth\": {\n    \"token\": \"t-1\"\n  },\n  \"list\": [1, 2]\n}\n";
        let value = outline(&FormatRegistry::core(), "a.json", text, None);
        let items = value["items"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[1]["at"], "auth");
        assert_eq!(items[1]["type"], "object");
        assert_eq!(items[1]["line"], 3);
        let auth = read(&FormatRegistry::core(), "a.json", text, Some("auth")).unwrap();
        assert_eq!(
            auth["text"],
            "  \"auth\": {\n    \"token\": \"<hidden>\"\n  },\n"
        );
    }

    #[test]
    fn long_reads_are_cut_with_the_next_range() {
        let text = (0..5000).fold(String::new(), |mut text, i| {
            let _ = writeln!(text, "line {i} with some words");
            text
        });
        let value = read(&FormatRegistry::core(), "big.txt", &text, None).unwrap();
        assert!(value["text"].as_str().unwrap().chars().count() <= MAX_READ_CHARS);
        let cut = value["cut"].as_str().unwrap();
        assert!(cut.starts_with('L') && cut.ends_with("-L5000"), "{cut}");
    }

    #[test]
    fn odd_input_never_panics() {
        for text in [
            "",
            "\n",
            "{",
            "}",
            "\"",
            "{\"a\":",
            "# \n#\n",
            "\u{feff}# x",
            "[\"k\": 1]",
        ] {
            let _ = outline(&FormatRegistry::core(), "x.json", text, None);
            let _ = outline(&FormatRegistry::core(), "x.md", text, None);
            let _ = read(&FormatRegistry::core(), "x.json", text, Some("a"));
            let _ = read(&FormatRegistry::core(), "x.md", text, Some("x"));
        }
    }
}
