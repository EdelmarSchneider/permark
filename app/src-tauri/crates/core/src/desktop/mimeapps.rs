//! `mimeapps.list` (freedesktop): reading and changing one key of `[Default Applications]`, nothing else.
//!
//! Every other line (comments, other sections, other keys, blank lines, order) and the file's line breaks stay as they
//! are; only the value of the key changes, or its line goes, or one line is added.

const SECTION: &str = "[Default Applications]";

/// The line break the file uses (its first one; LF for a file without any).
fn line_break(text: &str) -> &'static str {
    match text.find('\n') {
        Some(at) if at > 0 && text.as_bytes()[at - 1] == b'\r' => "\r\n",
        _ => "\n",
    }
}

/// Lines with their breaks (the last one may have none).
fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

fn content(line: &str) -> &str {
    line.trim_end_matches(['\r', '\n'])
}

fn is_section_header(line: &str) -> bool {
    let line = content(line).trim();
    line.starts_with('[') && line.ends_with(']')
}

/// The key of a `key=value` line (comments and headers have none).
fn key_of(line: &str) -> Option<&str> {
    let line = content(line);
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') || trimmed.starts_with('[') {
        return None;
    }
    trimmed.split_once('=').map(|(key, _)| key.trim())
}

/// The line range (start, end exclusive) of the section's body, after its header line; `None` when it is not there.
fn section(lines: &[&str]) -> Option<(usize, usize)> {
    let header = lines
        .iter()
        .position(|line| content(line).trim() == SECTION)?;
    let end = lines[header + 1..]
        .iter()
        .position(|line| is_section_header(line))
        .map_or(lines.len(), |n| header + 1 + n);
    Some((header + 1, end))
}

fn push_key(out: &mut String, mime: &str, value: &str, eol: &str) {
    out.push_str(mime);
    out.push('=');
    out.push_str(value);
    out.push_str(eol);
}

/// The value of `mime` in `[Default Applications]` (trimmed, as written: `a.desktop;b.desktop;`).
#[must_use]
pub fn default_of(text: &str, mime: &str) -> Option<String> {
    let lines = lines(text);
    let (start, end) = section(&lines)?;
    lines[start..end].iter().find_map(|line| {
        (key_of(line) == Some(mime)).then(|| {
            content(line)
                .split_once('=')
                .map_or(String::new(), |(_, value)| value.trim().to_owned())
        })
    })
}

/// `text` with `mime` set to `value` in `[Default Applications]` (`None`: its line removed). The section is made at the
/// end when it is missing; a new key goes after the last line of the section that is not blank.
#[must_use]
pub fn with_default(text: &str, mime: &str, value: Option<&str>) -> String {
    let eol = line_break(text);
    let lines = lines(text);
    let Some((start, end)) = section(&lines) else {
        let Some(value) = value else {
            return text.to_owned();
        };
        let mut out = text.to_owned();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push_str(eol);
        }
        out.push_str(SECTION);
        out.push_str(eol);
        push_key(&mut out, mime, value, eol);
        return out;
    };
    let mut out = String::with_capacity(text.len() + mime.len() + 32);
    let mut found = false;
    for (n, line) in lines.iter().enumerate() {
        let in_section = n >= start && n < end;
        if in_section && key_of(line) == Some(mime) {
            found = true;
            if let Some(value) = value {
                let kept_break = &line[content(line).len()..];
                let key_part = content(line).split_once('=').map_or("", |(key, _)| key);
                out.push_str(key_part);
                out.push('=');
                out.push_str(value);
                out.push_str(if kept_break.is_empty() && n + 1 < lines.len() {
                    eol
                } else {
                    kept_break
                });
            }
            continue;
        }
        out.push_str(line);
    }
    if found || value.is_none() {
        return out;
    }
    let Some(value) = value else {
        return out;
    };
    // a new key: after the section's last line that is not blank
    let last = (start..end)
        .rev()
        .find(|n| !content(lines[*n]).trim().is_empty())
        .map_or(start, |n| n + 1);
    let mut out = String::with_capacity(text.len() + mime.len() + 32);
    for (n, line) in lines.iter().enumerate() {
        if n == last {
            push_key(&mut out, mime, value, eol);
        }
        out.push_str(line);
        if n + 1 == last && !line.ends_with('\n') {
            out.push_str(eol);
        }
    }
    if last == lines.len() {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push_str(eol);
        }
        push_key(&mut out, mime, value, eol);
    }
    out
}
