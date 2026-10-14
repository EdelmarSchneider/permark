//! A small YAML subset for front matter cards: flat `key: value` pairs.
//!
//! Supported: plain/quoted scalars, trailing comments, inline lists (`[a, b]`), block lists (`- a`),
//! block scalars (`|` and `>`), and nested maps (flattened to `parent.child`). Anything fancier is kept
//! as its raw text rather than failing.

/// Extracts the YAML text from comrak's front matter node (`---\n...\n---\n`).
pub(crate) fn strip_delimiters(raw: &str) -> &str {
    let body = raw.trim_start_matches('\u{feff}');
    let body = body
        .strip_prefix("---")
        .unwrap_or(body)
        .trim_start_matches(['\r', '\n']);
    let trimmed = body.trim_end();
    trimmed
        .strip_suffix("---")
        .map_or(trimmed, |t| t.trim_end_matches(['\r', '\n']))
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

fn unquote(value: &str) -> String {
    let v = value.trim();
    let bytes = v.as_bytes();
    if bytes.len() >= 2
        && (bytes[0] == b'"' || bytes[0] == b'\'')
        && bytes[bytes.len() - 1] == bytes[0]
    {
        let inner = &v[1..v.len() - 1];
        if bytes[0] == b'"' {
            return inner.replace("\\\"", "\"").replace("\\n", "\n");
        }
        return inner.replace("''", "'");
    }
    // Strip a trailing ` # comment` from unquoted scalars.
    let v = v.find(" #").map_or(v, |i| v[..i].trim_end());
    if v.starts_with('[') && v.ends_with(']') {
        return v[1..v.len() - 1]
            .split(',')
            .map(unquote)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
    }
    v.to_owned()
}

/// Splits `key: value`; the colon must be followed by a space or end the line.
fn split_key(line: &str) -> Option<(String, &str)> {
    let t = line.trim_start();
    if t.starts_with('"') || t.starts_with('\'') {
        let q = t.as_bytes()[0] as char;
        let end = t[1..].find(q)? + 1;
        let rest = t[end + 1..].trim_start().strip_prefix(':')?;
        return Some((t[1..end].to_owned(), rest.trim()));
    }
    let mut search = 0;
    while let Some(i) = t[search..].find(':') {
        let at = search + i;
        let after = &t[at + 1..];
        if after.is_empty() || after.starts_with(' ') || after.starts_with('\t') {
            let key = t[..at].trim();
            if key.is_empty() {
                return None;
            }
            return Some((key.to_owned(), after.trim()));
        }
        search = at + 1;
    }
    None
}

/// Parses the YAML subset described in the module docs. Empty input gives an empty list.
pub(crate) fn parse(yaml: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = yaml.lines().collect();
    let mut pairs: Vec<(String, String)> = Vec::new();
    // Open parent keys of nested maps: (indent, key).
    let mut parents: Vec<(usize, String)> = Vec::new();
    // Index in `pairs` of the list currently being filled by `- item` lines.
    let mut list_pair: Option<usize> = None;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let indent = indent_of(line);
        if let Some(item) = t.strip_prefix("- ").or_else(|| (t == "-").then_some("")) {
            let item = unquote(item);
            if let Some((_, key)) = parents.last() {
                let idx = *list_pair.get_or_insert_with(|| {
                    pairs.push((parents_key(&parents, key), String::new()));
                    pairs.len() - 1
                });
                let slot = &mut pairs[idx].1;
                if !slot.is_empty() {
                    slot.push_str(", ");
                }
                slot.push_str(&item);
            }
            continue;
        }
        let Some((key, value)) = split_key(t) else {
            continue;
        };
        while parents.last().is_some_and(|(pi, _)| indent <= *pi) {
            parents.pop();
        }
        list_pair = None;
        let full = parents_key(&parents, &key);
        if value.is_empty() {
            parents.push((indent, key));
        } else if value == "|" || value == ">" || value.starts_with("|-") || value.starts_with(">-")
        {
            let folded = value.starts_with('>');
            let mut block: Vec<&str> = Vec::new();
            while i < lines.len() && (lines[i].trim().is_empty() || indent_of(lines[i]) > indent) {
                block.push(lines[i].trim());
                i += 1;
            }
            let sep = if folded { " " } else { "\n" };
            pairs.push((full, block.join(sep).trim().to_owned()));
        } else {
            pairs.push((full, unquote(value)));
        }
    }
    pairs
}

/// `parent.child` for a key under open parents. The key already pushed as the last parent is not repeated.
fn parents_key(parents: &[(usize, String)], key: &str) -> String {
    let mut parts: Vec<&str> = parents.iter().map(|(_, k)| k.as_str()).collect();
    if parts.last() == Some(&key) {
        parts.pop();
    }
    parts.push(key);
    parts.join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn strips_delimiters() {
        assert_eq!(strip_delimiters("---\ntitle: A\n---\n\n"), "title: A");
        assert_eq!(strip_delimiters("---\r\ntitle: A\r\n---"), "title: A");
    }

    #[test]
    fn parses_scalars_quotes_and_comments() {
        let y = "title: Hello world\nauthor: \"Ana: the dev\"\nstatus: 'it''s ok'\nflag: true # note\nurl: http://x.dev/a";
        assert_eq!(
            parse(y),
            kv(&[
                ("title", "Hello world"),
                ("author", "Ana: the dev"),
                ("status", "it's ok"),
                ("flag", "true"),
                ("url", "http://x.dev/a"),
            ])
        );
    }

    #[test]
    fn parses_inline_and_block_lists() {
        let y = "tags: [a, \"b c\", d]\nauthors:\n  - Ana\n  - Bo\nafter: x";
        assert_eq!(
            parse(y),
            kv(&[
                ("tags", "a, b c, d"),
                ("authors", "Ana, Bo"),
                ("after", "x")
            ])
        );
    }

    #[test]
    fn flattens_nested_maps() {
        let y = "meta:\n  owner: Ana\n  team:\n    name: Docs\nnext: 1";
        assert_eq!(
            parse(y),
            kv(&[
                ("meta.owner", "Ana"),
                ("meta.team.name", "Docs"),
                ("next", "1")
            ])
        );
    }

    #[test]
    fn parses_block_scalars() {
        let y = "summary: |\n  line one\n  line two\nfolded: >\n  a\n  b\nend: ok";
        assert_eq!(
            parse(y),
            kv(&[
                ("summary", "line one\nline two"),
                ("folded", "a b"),
                ("end", "ok")
            ])
        );
    }

    #[test]
    fn ignores_garbage_and_empty_input() {
        assert!(parse("").is_empty());
        assert!(parse("# just a comment\n\n").is_empty());
        assert!(parse("not a pair\n---").is_empty());
    }
}
