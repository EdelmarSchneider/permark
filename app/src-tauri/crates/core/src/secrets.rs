//! Secret values in text that leaves the app (agent payloads): `KEY=value`, `key: value`, `"token": "value"` where
//! the key names a secret keep the key and lose the value. Format-free on purpose: it covers config files, code
//! blocks in Markdown and selections alike; the config formats add structural masking of their own.

use std::borrow::Cow;

/// What a masked value reads as.
pub const HIDDEN: &str = "<hidden>";

/// Words that make a key secret (inside an identifier, any case).
const SECRET_WORDS: [&str; 9] = [
    "key",
    "secret",
    "token",
    "password",
    "passwd",
    "pwd",
    "credential",
    "auth",
    "private",
];

/// Words that contain a secret word and are not secrets (compared with separators removed).
const HARMLESS: [&str; 10] = [
    "author",
    "keyword",
    "keyboard",
    "keymap",
    "maxtokens",
    "numtokens",
    "tokenizer",
    "monkey",
    "turkey",
    "donkey",
];

/// Whether a key names a secret (`API_KEY`, `client-secret`, `authToken`, `db.password`). Separators are ignored
/// (`pass_word` counts) and a few harmless words are set aside (`authors`, `keywords`, `max_tokens`).
#[must_use]
pub fn is_secret_key(key: &str) -> bool {
    let mut flat: String = key
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    for word in HARMLESS {
        if flat.contains(word) {
            flat = flat.replace(word, "");
        }
    }
    SECRET_WORDS.iter().any(|word| flat.contains(word))
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.')
}

/// `text` with every secret value replaced by [`HIDDEN`]; borrowed when nothing was hidden.
#[must_use]
pub fn mask_text(text: &str) -> Cow<'_, str> {
    let flat: String = text
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    if !SECRET_WORDS.iter().any(|word| flat.contains(word)) && !text.contains("://") {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    let mut rest = text;
    while !rest.is_empty() {
        let line_end = rest.find('\n').map_or(rest.len(), |i| i + 1);
        let (line, tail) = rest.split_at(line_end);
        match mask_line(line) {
            Cow::Borrowed(_) => out.push_str(line),
            Cow::Owned(masked) => {
                changed = true;
                out.push_str(&masked);
            }
        }
        rest = tail;
    }
    if changed {
        Cow::Owned(out)
    } else {
        Cow::Borrowed(text)
    }
}

/// Where the password of a `scheme://user:password@host` address sits in `text`: the first one.
#[must_use]
pub fn url_password(text: &str) -> Option<std::ops::Range<usize>> {
    let mut from = 0;
    while let Some(found) = text[from..].find("://") {
        let start = from + found + 3;
        let end = text[start..]
            .find(|c: char| matches!(c, '/' | '?' | '#' | '"' | '\'' | '\\') || c.is_whitespace())
            .map_or(text.len(), |i| start + i);
        let authority = &text[start..end];
        if let Some(at) = authority.rfind('@') {
            if let Some(colon) = authority[..at].find(':') {
                let password = start + colon + 1..start + at;
                if !password.is_empty() && &text[password.clone()] != HIDDEN {
                    return Some(password);
                }
            }
        }
        from = start;
    }
    None
}

/// One line: each `key <sep> value` pair whose key is secret; the password of every address.
fn mask_line(line: &str) -> Cow<'_, str> {
    let mut out = String::new();
    let mut rest = line;
    let mut changed = false;
    while let Some(range) = url_password(rest) {
        changed = true;
        out.push_str(&rest[..range.start]);
        out.push_str(HIDDEN);
        rest = &rest[range.end..];
    }
    if !changed {
        return mask_pairs(line);
    }
    out.push_str(rest);
    Cow::Owned(mask_pairs(&out).into_owned())
}

/// `key <sep> value` pairs whose key is secret.
fn mask_pairs(line: &str) -> Cow<'_, str> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut out = String::new();
    let mut copied = 0; // byte offset in `line` copied so far
    let mut pos = 0;
    while pos < chars.len() {
        let (start, ch) = chars[pos];
        if !is_ident(ch) || (pos > 0 && is_ident(chars[pos - 1].1)) {
            pos += 1;
            continue;
        }
        // an identifier starts here
        let mut key_end_at = pos;
        while key_end_at < chars.len() && is_ident(chars[key_end_at].1) {
            key_end_at += 1;
        }
        let key_end = chars.get(key_end_at).map_or(line.len(), |(b, _)| *b);
        let key = &line[start..key_end];
        // optional closing quote of a quoted key, spaces, then the separator
        let mut cur = key_end_at;
        if cur < chars.len() && matches!(chars[cur].1, '"' | '\'') {
            cur += 1;
        }
        while cur < chars.len() && matches!(chars[cur].1, ' ' | '\t') {
            cur += 1;
        }
        let separated = cur < chars.len() && matches!(chars[cur].1, '=' | ':');
        if !separated || !is_secret_key(key) {
            pos = key_end_at;
            continue;
        }
        // `::` is a path (Rust), `:=` is still an assignment
        if chars[cur].1 == ':' && chars.get(cur + 1).is_some_and(|(_, c)| *c == ':') {
            pos = cur + 2;
            continue;
        }
        cur += 1;
        if cur < chars.len() && chars[cur].1 == '=' {
            cur += 1;
        }
        while cur < chars.len() && matches!(chars[cur].1, ' ' | '\t') {
            cur += 1;
        }
        let Some(&(value_start, first)) = chars.get(cur) else {
            break;
        };
        if first == '{' {
            // a nested table: its own keys are looked at one by one
            pos = cur;
            continue;
        }
        let (value_from, value_to, next) = if first == '[' {
            // a list: everything inside the brackets on this line
            let mut end = cur + 1;
            while end < chars.len() && chars[end].1 != ']' {
                end += 1;
            }
            let to = chars.get(end).map_or(line.len(), |(b, _)| *b);
            (value_start + 1, to, end + 1)
        } else if matches!(first, '"' | '\'' | '`') {
            // quoted: up to the matching quote (or the line end)
            let mut end = cur + 1;
            while end < chars.len() && chars[end].1 != first {
                if chars[end].1 == '\\' {
                    end += 1;
                }
                end += 1;
            }
            let to = chars.get(end).map_or(line.len(), |(b, _)| *b);
            (value_start + first.len_utf8(), to, end + 1)
        } else {
            let mut end = cur;
            while end < chars.len() && !matches!(chars[end].1, ' ' | '\t' | ',' | ';' | '\r' | '\n')
            {
                end += 1;
            }
            let to = chars.get(end).map_or(line.len(), |(b, _)| *b);
            (value_start, to, end)
        };
        if value_to > value_from && line[value_from..value_to] != *HIDDEN {
            out.push_str(&line[copied..value_from]);
            out.push_str(HIDDEN);
            copied = value_to;
        }
        pos = next.max(key_end_at);
    }
    if copied == 0 {
        Cow::Borrowed(line)
    } else {
        out.push_str(&line[copied..]);
        Cow::Owned(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_of_secret_keys_are_hidden_keys_stay() {
        let cases = [
            ("API_KEY=abc123", "API_KEY=<hidden>"),
            ("export GITHUB_TOKEN=ghp_x", "export GITHUB_TOKEN=<hidden>"),
            ("password: hunter2", "password: <hidden>"),
            (
                "  db.password = \"p@ss w\"  # x",
                "  db.password = \"<hidden>\"  # x",
            ),
            (
                "{\"apiKey\": \"sk-1\", \"name\": \"n\"}",
                "{\"apiKey\": \"<hidden>\", \"name\": \"n\"}",
            ),
            ("client_secret='s3'", "client_secret='<hidden>'"),
            ("AUTH=x,PWD=y", "AUTH=<hidden>,PWD=<hidden>"),
            ("PRIVATE_KEY := zz", "PRIVATE_KEY := <hidden>"),
            ("token=ä€😀ß;next", "token=<hidden>;next"),
            ("\"tokens\": [\"a\", \"b\"],", "\"tokens\": [<hidden>],"),
            ("\"auth\": {", "\"auth\": {"),
            ("auth: { token: x }", "auth: { token: <hidden> }"),
        ];
        for (input, expected) in cases {
            assert_eq!(mask_text(input), expected, "{input}");
        }
    }

    #[test]
    fn the_password_of_an_address_is_hidden() {
        assert_eq!(
            mask_text("DATABASE_URL=postgres://user:pass@localhost:5432/app"),
            "DATABASE_URL=postgres://user:<hidden>@localhost:5432/app"
        );
        assert_eq!(
            mask_text("see https://a:b@x.org/p and ftp://u:p2@y"),
            "see https://a:<hidden>@x.org/p and ftp://u:<hidden>@y"
        );
        for plain in [
            "https://example.org/a@b",
            "mailto:a@b.c",
            "ssh://git@github.com/x/y",
            "http://h:80/p",
        ] {
            assert_eq!(mask_text(plain), plain);
        }
        assert_eq!(url_password("x://u:p@h"), Some(6..7));
    }

    #[test]
    fn harmless_words_are_not_secret_keys() {
        for key in [
            "authors",
            "keywords",
            "max_tokens",
            "keyboard",
            "name",
            "version",
        ] {
            assert!(!is_secret_key(key), "{key}");
        }
        for key in [
            "API_KEY",
            "pass_word",
            "authToken",
            "client-secret",
            "db.pwd",
            "author_token",
        ] {
            assert!(is_secret_key(key), "{key}");
        }
        assert_eq!(mask_text("authors = [\"a\"]"), "authors = [\"a\"]");
    }

    #[test]
    fn ordinary_text_is_untouched_and_borrowed() {
        for text in [
            "The key idea: keep it simple.",
            "name = value",
            "use crate::auth::login;",
            "password",
            "",
            "token:",
            "a = 1\r\nb = 2\r\n",
        ] {
            let masked = mask_text(text);
            // "The key idea: keep" -> `idea` is the key before the colon, not secret
            assert_eq!(masked, text, "{text}");
        }
        assert!(matches!(mask_text("plain words"), Cow::Borrowed(_)));
    }

    #[test]
    fn lines_and_line_endings_are_kept() {
        let text = "a=1\r\nSECRET=x\r\npwd: y\nend";
        assert_eq!(
            mask_text(text),
            "a=1\r\nSECRET=<hidden>\r\npwd: <hidden>\nend"
        );
    }

    #[test]
    fn unterminated_quotes_and_odd_input_never_panic() {
        for text in [
            "token=\"open",
            "key='",
            "key=\\",
            "token=\"a\\\"b\" x",
            "key:::",
            "=key=",
            "k\u{0}ey=1",
            "鍵key=値",
        ] {
            let _ = mask_text(text);
        }
        assert_eq!(mask_text("token=\"open"), "token=\"<hidden>");
        assert_eq!(mask_text("token=\"a\\\"b\" x"), "token=\"<hidden>\" x");
    }

    #[test]
    fn masking_twice_changes_nothing_more() {
        let once = mask_text("API_KEY=abc").into_owned();
        assert_eq!(mask_text(&once), once);
    }
}
