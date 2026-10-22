//! Secret values in text that leaves the app (agent payloads, search): a key whose name ends in a secret word
//! (`DB_PASSWORD`, `apiKey`, `GITHUB_TOKEN`) followed by `=` | `:` and a value that looks like a secret keeps the key
//! and loses the value; so do the password of an address, known token formats wherever they are (`sk-…`, `ghp_…`,
//! `AKIA…`, JWT) and the body of a private key block. Format-free on purpose: it covers config files, code, Markdown
//! and selections alike; the config formats add structural masking of their own. Mirror: `src/core/config/secrets.ts`;
//! both read `testdata/secrets/cases.json`.

use std::borrow::Cow;
use std::ops::Range;

/// What a masked value reads as.
pub const HIDDEN: &str = "<hidden>";

/// Last words of a key that name a secret (`DB_PASSWORD`, `clientSecret`, `GITHUB_TOKEN`, `basic_auth`).
const SECRET_WORDS: &[&str] = &[
    "secret",
    "secrets",
    "password",
    "passwords",
    "passwd",
    "pwd",
    "pass",
    "passphrase",
    "credential",
    "credentials",
    "token",
    "tokens",
    "auth",
    "authorization",
    "apikey",
    "apikeys",
    "accesskey",
    "secretkey",
    "privatekey",
];

/// Ends of a run-together last word that name a secret (`dbpassword`, `apitoken`, `clientsecret`).
const SECRET_ENDINGS: &[&str] = &["password", "passwd", "secret", "token", "tokens"];

/// Words before `key` that make it a secret (`api_key`, `privateKey`, `SIGNING_KEY`).
const KEY_QUALIFIERS: &[&str] = &[
    "api",
    "access",
    "secret",
    "private",
    "client",
    "app",
    "application",
    "master",
    "encryption",
    "decryption",
    "encrypt",
    "signing",
    "sign",
    "signature",
    "license",
    "licence",
    "subscription",
    "consumer",
    "service",
    "account",
    "auth",
    "hmac",
    "jwt",
    "ssh",
    "deploy",
    "admin",
    "root",
    "shared",
    "session",
    "cookie",
    "crypto",
    "cipher",
    "aes",
    "rsa",
    "gpg",
    "pgp",
    "server",
    "webhook",
    "write",
    "developer",
    "partner",
    "merchant",
    "integration",
    "recovery",
    "otp",
    "totp",
    "mfa",
    "vault",
    "kms",
    "sas",
];

/// Words before `KEY` in an env-style name (`STRIPE_KEY` is secret) that keep it ordinary (`PUBLIC_KEY`, `CACHE_KEY`).
const PLAIN_KEY_QUALIFIERS: &[&str] = &[
    "public",
    "pub",
    "site",
    "publishable",
    "cache",
    "settings",
    "setting",
    "config",
    "state",
    "query",
    "sort",
    "primary",
    "foreign",
    "partition",
    "route",
    "event",
    "i18n",
    "translation",
    "locale",
    "lang",
    "map",
    "lookup",
    "index",
    "hot",
    "short",
    "shortcut",
    "context",
    "group",
    "item",
    "row",
    "field",
    "table",
    "unique",
    "idempotency",
    "prefix",
    "object",
    "menu",
    "label",
    "message",
    "type",
    "name",
    "id",
    "data",
    "list",
    "hash",
    "local",
    "sub",
    "parent",
    "child",
    "node",
    "cursor",
    "page",
    "tab",
    "theme",
    "mod",
    "modifier",
];

/// Words before `tokens` that make it a count, not a secret (`max_tokens`, `input_tokens`, `ntokens`).
const COUNT_WORDS: &[&str] = &[
    "max",
    "min",
    "num",
    "n",
    "input",
    "output",
    "total",
    "prompt",
    "completion",
    "cached",
    "cache",
    "reasoning",
    "context",
    "remaining",
    "estimated",
];

/// Words after a secret word that only say which one (`SECRET_KEY_BASE`, `DB_PASSWORD_PROD`, `password_hash`).
const TAIL_WORDS: &[&str] = &[
    "base",
    "value",
    "val",
    "hash",
    "prod",
    "production",
    "dev",
    "development",
    "stage",
    "staging",
    "test",
    "live",
    "local",
    "ro",
    "rw",
    "raw",
    "enc",
    "encrypted",
    "b64",
    "base64",
    "old",
    "new",
    "current",
    "primary",
    "secondary",
    "backup",
];

/// Keys that hold a count when their value is a number (`tokens: 1200`, `"tokens": 3400`), and tokens when it is
/// not (`tokens: [a, b]`); `api_tokens` is never one.
const COUNT_KEYS: &[&str] = &["tokens", "ntokens"];

/// Words of code that start an expression, not a value (`token = new Token()`), and bool | null words.
const CODE_WORDS: &[&str] = &[
    "new",
    "await",
    "this",
    "self",
    "function",
    "async",
    "typeof",
    "require",
    "import",
    "lambda",
    "yield",
    "not",
    "true",
    "false",
    "null",
    "nil",
    "none",
    "undefined",
    "yes",
    "no",
    "on",
    "off",
];

/// Keys whose value names a variable (`- name: DB_PASSWORD`), and keys that then hold its value (`value: x`).
const NAME_KEYS: &[&str] = &["name", "key", "env", "var", "variable"];
const VALUE_KEYS: &[&str] = &["value", "val", "default"];

/// Pieces of text that every secret this module finds contains (lowercase); text with none is passed over at once.
const HINTS: &[&str] = &[
    "key",
    "secret",
    "token",
    "pass",
    "pwd",
    "credential",
    "auth",
    "://",
    "sk-",
    "sk_",
    "rk_",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "akia",
    "asia",
    "aiza",
    "xox",
    "hf_",
    "npm_",
    "eyj",
];

/// Prefixes of known tokens whose rest must look random ([`is_random`]): `OpenAI` | Anthropic, Stripe, GitHub,
/// GitLab, Hugging Face, npm.
const RANDOM_PREFIXES: &[&str] = &[
    "sk-",
    "sk_live_",
    "sk_test_",
    "rk_live_",
    "rk_test_",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "hf_",
    "npm_",
];

/// A number under a count key (`tokens: 1200`, `"tokens": 3400`): a size, not a secret.
#[must_use]
pub fn is_count(key: &str, value: &str) -> bool {
    let flat: String = key
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    let value = value.trim();
    COUNT_KEYS.contains(&flat.as_str())
        && !value.is_empty()
        && value.parse::<f64>().is_ok_and(f64::is_finite)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharKind {
    Upper,
    Lower,
    Digit,
}

fn char_kind(c: char) -> Option<CharKind> {
    if c.is_uppercase() {
        Some(CharKind::Upper)
    } else if c.is_lowercase() || c.is_alphabetic() {
        Some(CharKind::Lower)
    } else if c.is_numeric() {
        Some(CharKind::Digit)
    } else {
        None
    }
}

/// The words of a name, lowercase, numbers left out: `apiKey2` -> `api key`, `DB_PASSWORD` -> `db password`.
#[must_use]
pub fn key_words(name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut prev: Option<CharKind> = None;
    let chars: Vec<char> = name.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let Some(kind) = char_kind(c) else {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            prev = None;
            continue;
        };
        let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
        let split = prev.is_some_and(|p| {
            (p == CharKind::Lower && kind == CharKind::Upper)
                || ((p == CharKind::Digit) != (kind == CharKind::Digit))
                || (p == CharKind::Upper && kind == CharKind::Upper && next_lower)
        });
        if split && !word.is_empty() {
            out.push(std::mem::take(&mut word));
        }
        word.push(c);
        prev = Some(kind);
    }
    if !word.is_empty() {
        out.push(word);
    }
    out.into_iter()
        .map(|w| w.to_lowercase())
        .filter(|w| !w.chars().all(char::is_numeric))
        .collect()
}

/// The words of a key with the tail (`_prod`, `_base`, ...) set aside.
fn head_words(name: &str) -> Vec<String> {
    let mut words = key_words(name);
    while words.len() > 1
        && words
            .last()
            .is_some_and(|w| TAIL_WORDS.contains(&w.as_str()))
    {
        words.pop();
    }
    words
}

/// Whether a key names a secret: its last word (after `_prod`, `_base`, ... are set aside) is a secret word
/// (`DB_PASSWORD`, `clientSecret`, `authToken`, `basic_auth`), or it is `key` after a word that makes it one
/// (`api_key`, `privateKey`; env-style `STRIPE_KEY`). `keys`, `keydown`, `keyJump`, `KeyW`, `monkey`, `tokenizer`,
/// `max_tokens`, `token_count`, `authorName`, `secretName`, `PASSWORD_FILE` are not.
#[must_use]
pub fn is_secret_key(name: &str) -> bool {
    let words = head_words(name);
    let Some(last) = words.last().map(String::as_str) else {
        return false;
    };
    let prev = words.len().checked_sub(2).map(|i| words[i].as_str());
    if last == "key" || last == "keys" {
        let Some(prev) = prev else {
            return false;
        };
        if KEY_QUALIFIERS.contains(&prev) {
            return true;
        }
        return last == "key"
            && !name.chars().any(char::is_lowercase)
            && !PLAIN_KEY_QUALIFIERS.contains(&prev);
    }
    if prev.is_some_and(|p| SECRET_WORDS.contains(&format!("{p}{last}").as_str())) {
        return true;
    }
    if last == "token" || last == "tokens" {
        return !prev.is_some_and(|p| COUNT_WORDS.contains(&p));
    }
    if SECRET_WORDS.contains(&last) {
        return true;
    }
    for ending in SECRET_ENDINGS {
        if last.len() > ending.len() && last.ends_with(ending) {
            let head = &last[..last.len() - ending.len()];
            return !(ending.starts_with("token") && COUNT_WORDS.contains(&head));
        }
    }
    false
}

/// Whether the path of a config value names a secret (the key name is the signal there): a segment is a secret key,
/// or the last one is `key` | `keys` under a named parent (`[stripe] key`, `mapbox.key`; not `[0].key`,
/// `cache.key`, `i18n.key`).
#[must_use]
pub fn is_secret_path<S: AsRef<str>>(segments: &[S]) -> bool {
    if segments.iter().any(|s| is_secret_key(s.as_ref())) {
        return true;
    }
    let n = segments.len();
    if n < 2
        || !matches!(key_words(segments[n - 1].as_ref()).as_slice(), [w] if w == "key" || w == "keys")
    {
        return false;
    }
    key_words(segments[n - 2].as_ref())
        .last()
        .is_some_and(|w| !PLAIN_KEY_QUALIFIERS.contains(&w.as_str()))
}

/// Whether `text` holds a known token (`sk-…`, `ghp_…`, `AKIA…`, JWT, ...), whatever its key.
#[must_use]
pub fn has_token(text: &str) -> bool {
    text.lines().any(|line| !token_spans(line).is_empty())
}

/// Secret only as an env-style `<WORD>_KEY` (`STRIPE_KEY`): such a key may name a slot (`STORAGE_KEY = "app.theme"`).
fn is_weak_key(name: &str) -> bool {
    let words = head_words(name);
    let n = words.len();
    n > 0 && words[n - 1] == "key" && !(n > 1 && KEY_QUALIFIERS.contains(&words[n - 2].as_str()))
}

/// `${VAR}`, `$VAR`, `{{ secrets.NAME }}`: a pointer to a secret, not the secret.
fn is_reference(value: &str) -> bool {
    let t = value.trim();
    t.starts_with('$') || (t.starts_with("{{") && t.ends_with("}}"))
}

/// Letters, digits and nothing else, lowercase: `API key` and `apiKey` read the same.
fn flat(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn digits(s: &str) -> usize {
    s.chars().take_while(char::is_ascii_digit).count()
}

/// `12`, `-3.5`, `.5`, `1e9`: a decimal number as written.
fn is_number(s: &str) -> bool {
    let s = s.strip_prefix(['+', '-']).unwrap_or(s);
    let whole = digits(s);
    let mut rest = &s[whole..];
    let mut any = whole > 0;
    if let Some(r) = rest.strip_prefix('.') {
        let frac = digits(r);
        any |= frac > 0;
        rest = &r[frac..];
    }
    if !any {
        return false;
    }
    if let Some(r) = rest.strip_prefix(['e', 'E']) {
        let r = r.strip_prefix(['+', '-']).unwrap_or(r);
        let exp = digits(r);
        return exp > 0 && exp == r.len();
    }
    rest.is_empty()
}

/// `#fff`, `#ff0000`, `#ff000080`: a color.
fn is_color(s: &str) -> bool {
    s.strip_prefix('#').is_some_and(|hex| {
        matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit())
    })
}

/// A word of a sentence: letters (and apostrophes), maybe one end mark (`required.`).
fn is_prose_word(word: &str) -> bool {
    let body = word
        .strip_suffix(['.', ',', '!', '?', ':', ';'])
        .unwrap_or(word);
    !body.is_empty()
        && body
            .chars()
            .all(|c| c.is_alphabetic() || matches!(c, '\'' | '’'))
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || matches!(c, '_' | '$')
}

/// `token`, `accessToken`, `_x1`: a name in code.
fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(is_ident_start)
        && chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | '$'))
}

/// `process.env.TOKEN`, `this.key`, `options.auth`: where a value is, not the value (no digits: `pk.eyJ1.x9` is one).
fn is_member(s: &str) -> bool {
    let mut parts = 0;
    for part in s.split('.') {
        if part.is_empty() || !part.chars().all(is_ident_start) {
            return false;
        }
        parts += 1;
    }
    parts > 1
}

/// A variable named like a secret (`accessToken`, `API_KEY`, `newPassword`): two words or more, no digits. A single
/// word (`mysecretpassword`) or one with digits (`abc123secret`) is a value.
fn is_variable(value: &str) -> bool {
    !value.chars().any(char::is_numeric) && key_words(value).len() > 1 && is_secret_key(value)
}

/// Lowercase words joined by `.`, `-`, `_`, `:` or `/`: a name (`app.theme`, `user-cache`), not a secret.
fn is_slug(s: &str) -> bool {
    let sep = |c: char| matches!(c, '.' | '_' | ':' | '/' | '-');
    let low = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.contains(sep)
        && s.split(sep)
            .all(|part| !part.is_empty() && part.chars().all(low))
}

/// A YAML block scalar mark (`key: |`): the value is on the next lines.
fn is_block_mark(s: &str) -> bool {
    let Some(rest) = s.strip_prefix(['|', '>']) else {
        return false;
    };
    let rest = rest.strip_prefix(['+', '-']).unwrap_or(rest);
    rest.is_empty() || (rest.len() == 1 && rest.chars().all(|c| c.is_ascii_digit()))
}

/// Whether the value of a secret key looks like a secret: not empty, hidden, a reference, a color, a sentence, the
/// key's own name (`password: "Password"`), a slot name under an env-style key; unquoted, also not a bool | null |
/// number, a block mark, a YAML tag (`!Ref`), an expression (`getToken()`, `new X`, `env["X"]`, `process.env.X`), a
/// variable named like a secret (`token: accessToken`) or ending a statement (`token = t;`), or a word starting a
/// sentence.
fn looks_secret(key: &str, value: &str, quoted: bool, after: &str) -> bool {
    if value.is_empty() || value == HIDDEN || is_reference(value) || is_color(value) {
        return false;
    }
    if flat(value) == flat(key) || (is_slug(value) && is_weak_key(key)) {
        return false;
    }
    if quoted {
        let words: Vec<&str> = value.split_whitespace().collect();
        return !(words.len() > 1 && words.iter().all(|w| is_prose_word(w)));
    }
    if CODE_WORDS.contains(&value.to_lowercase().as_str())
        || is_number(value)
        || is_block_mark(value)
    {
        return false;
    }
    // `!Ref x`, `&anchor`, `*alias`: YAML tags and pointers
    if value.contains(['(', '[', '"', '\''])
        || value.starts_with(['!', '&', '*'])
        || is_member(value)
    {
        return false;
    }
    if is_identifier(value) && (is_variable(value) || after.starts_with(';')) {
        return false;
    }
    // a word followed by more words: a sentence (`The password: must be long`); `hunter2 is rotated` stays secret
    let mut rest = after.chars().skip_while(|c| matches!(c, ' ' | '\t'));
    !(value.chars().all(char::is_alphabetic)
        && after.starts_with([' ', '\t'])
        && rest.next().is_some_and(char::is_alphanumeric))
}

/// The random part of a prefixed token: 20+ characters with upper, lower case and digits (a slug is not one).
fn is_random(body: &str) -> bool {
    body.len() >= 20
        && body.chars().any(|c| c.is_ascii_uppercase())
        && body.chars().any(|c| c.is_ascii_lowercase())
        && body.chars().any(|c| c.is_ascii_digit())
}

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-')
}

/// Bytes of the run of token characters (`[A-Za-z0-9_-]`, or without `_`) at the start of `s`.
fn run(s: &str, underscore: bool) -> usize {
    s.find(|c: char| !(is_token_char(c) && (underscore || c != '_')))
        .unwrap_or(s.len())
}

/// Length of a known token at the start of `s` (which follows a boundary), if one is there.
fn token_at(s: &str) -> Option<usize> {
    for prefix in RANDOM_PREFIXES {
        if let Some(rest) = s.strip_prefix(prefix) {
            let body = run(rest, true);
            return is_random(&rest[..body]).then_some(prefix.len() + body);
        }
    }
    let after_ok = |s: &str, end: usize| !s[end..].starts_with(is_token_char);
    if s.starts_with("AKIA") || s.starts_with("ASIA") {
        let n = s[4..]
            .chars()
            .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            .count();
        return (n >= 16 && after_ok(s, 20)).then_some(20);
    }
    if let Some(rest) = s.strip_prefix("AIza") {
        let n = run(rest, true);
        return (n == 35).then_some(4 + 35);
    }
    if s.starts_with("xox")
        && s[3..].starts_with(['a', 'b', 'p', 'r', 's'])
        && s[4..].starts_with('-')
    {
        let n = run(&s[5..], false);
        return (n >= 10 && after_ok(s, 5 + n)).then_some(5 + n);
    }
    if let Some(rest) = s.strip_prefix("eyJ") {
        let head = run(rest, true);
        let second = rest[head..].strip_prefix('.')?;
        let body = run(second, true);
        let third = second[body..].strip_prefix('.')?;
        let sign = run(third, true);
        return (head >= 8 && body >= 8 && sign >= 8).then_some(3 + head + 1 + body + 1 + sign);
    }
    None
}

/// Bytes of the known tokens in `line`, in order.
fn token_spans(line: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut prev: Option<char> = None;
    let mut skip_to = 0;
    for (at, c) in line.char_indices() {
        let boundary = !prev.is_some_and(|p| p.is_alphanumeric() || matches!(p, '_' | '-'));
        prev = Some(c);
        if at < skip_to || !boundary || !matches!(c, 's' | 'r' | 'g' | 'h' | 'n' | 'A' | 'x' | 'e')
        {
            continue;
        }
        if let Some(len) = token_at(&line[at..]) {
            out.push(at..at + len);
            skip_to = at + len;
        }
    }
    out
}

/// `text` with every secret value replaced by [`HIDDEN`]; borrowed when nothing was hidden.
#[must_use]
pub fn mask_text(text: &str) -> Cow<'_, str> {
    let lower = text.to_lowercase();
    if !HINTS.iter().any(|hint| lower.contains(hint)) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    let mut named = false; // the line before named a secret variable (`- name: DB_PASSWORD`)
    let mut in_key = false; // inside the body of a private key block
    let mut rest = text;
    while !rest.is_empty() {
        let line_end = rest.find('\n').map_or(rest.len(), |i| i + 1);
        let (line, tail) = rest.split_at(line_end);
        rest = tail;
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        if in_key {
            named = false;
            let trimmed = content.trim();
            if content
                .trim_start_matches([' ', '\t'])
                .starts_with("-----END")
            {
                in_key = false;
            } else if !trimmed.is_empty() && trimmed != HIDDEN {
                let lead = content.len() - content.trim_start().len();
                out.push_str(&content[..lead]);
                out.push_str(HIDDEN);
                out.push_str(&line[content.len()..]);
                changed = true;
                continue;
            }
            out.push_str(line);
            continue;
        }
        if content.contains("-----BEGIN ") && is_private_key_begin(content) && line.ends_with('\n')
        {
            in_key = true;
        }
        match mask_line(content, &mut named) {
            Cow::Borrowed(_) => out.push_str(line),
            Cow::Owned(masked) => {
                changed = true;
                out.push_str(&masked);
                out.push_str(&line[content.len()..]);
            }
        }
    }
    if changed {
        Cow::Owned(out)
    } else {
        Cow::Borrowed(text)
    }
}

/// `-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----` somewhere in the line.
fn is_private_key_begin(line: &str) -> bool {
    line.match_indices("-----BEGIN ").any(|(at, m)| {
        let rest = &line[at + m.len()..];
        let label = rest
            .find(|c: char| !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == ' '))
            .map_or(rest, |end| &rest[..end]);
        label.ends_with("PRIVATE KEY") && rest[label.len()..].starts_with("-----")
    })
}

/// Where the password of a `scheme://user:password@host` address sits in `text`: the first one.
#[must_use]
pub fn url_password(text: &str) -> Option<Range<usize>> {
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

/// One line (without its line break): the password of every address; each `key <sep> value` pair whose key is
/// secret; known tokens.
fn mask_line<'a>(line: &'a str, named: &mut bool) -> Cow<'a, str> {
    let mut out = String::new();
    let mut rest = line;
    let mut changed = false;
    while let Some(range) = url_password(rest) {
        changed = true;
        out.push_str(&rest[..range.start]);
        out.push_str(HIDDEN);
        rest = &rest[range.end..];
    }
    let pairs = if changed {
        out.push_str(rest);
        Cow::Owned(mask_pairs(&out, named).into_owned())
    } else {
        mask_pairs(line, named)
    };
    let spans = token_spans(&pairs);
    if spans.is_empty() {
        return pairs;
    }
    let mut masked = String::with_capacity(pairs.len());
    let mut at = 0;
    for span in spans {
        masked.push_str(&pairs[at..span.start]);
        masked.push_str(HIDDEN);
        at = span.end;
    }
    masked.push_str(&pairs[at..]);
    Cow::Owned(masked)
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.')
}

/// The value that starts at char `cur` (as written: quoted, a list, or up to a space | separator | closing
/// bracket): its bytes inside quotes | brackets, the char index after it, whether it is quoted.
fn value_bounds(chars: &[(usize, char)], len: usize, cur: usize) -> (usize, usize, usize, bool) {
    let byte = |i: usize| chars.get(i).map_or(len, |(b, _)| *b);
    let (value_start, first) = chars[cur];
    if first == '[' {
        let mut end = cur + 1;
        while end < chars.len() && chars[end].1 != ']' {
            end += 1;
        }
        (value_start + 1, byte(end), end + 1, false)
    } else if matches!(first, '"' | '\'' | '`') {
        let mut end = cur + 1;
        while end < chars.len() && chars[end].1 != first {
            if chars[end].1 == '\\' {
                end += 1;
            }
            end += 1;
        }
        let end = end.min(chars.len());
        (value_start + first.len_utf8(), byte(end), end + 1, true)
    } else {
        let mut end = cur;
        while end < chars.len()
            && !chars[end].1.is_whitespace()
            && !matches!(chars[end].1, ',' | ';' | ')' | ']' | '}')
        {
            end += 1;
        }
        (value_start, byte(end), end, false)
    }
}

/// `key <sep> value` pairs whose key is secret (or that are the `value` of a `name: <secret>` on this line or the
/// one before; `named` carries that from line to line).
fn mask_pairs<'a>(line: &'a str, named: &mut bool) -> Cow<'a, str> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let byte = |i: usize| chars.get(i).map_or(line.len(), |(b, _)| *b);
    let named_before = std::mem::take(named);
    let mut out = String::new();
    let mut copied = 0; // byte offset in `line` copied so far
    let mut pos = 0;
    while pos < chars.len() {
        let ch = chars[pos].1;
        if !is_ident(ch) || (pos > 0 && is_ident(chars[pos - 1].1)) {
            pos += 1;
            continue;
        }
        // an identifier starts here
        let mut key_end_at = pos;
        while key_end_at < chars.len() && is_ident(chars[key_end_at].1) {
            key_end_at += 1;
        }
        let key = &line[byte(pos)..byte(key_end_at)];
        // optional closing quote of a quoted key, spaces, then the separator
        let mut cur = key_end_at;
        if cur < chars.len() && matches!(chars[cur].1, '"' | '\'') {
            cur += 1;
        }
        while cur < chars.len() && matches!(chars[cur].1, ' ' | '\t') {
            cur += 1;
        }
        let next_char = |i: usize| chars.get(i).map(|(_, c)| *c);
        let separated = match next_char(cur) {
            // `::` is a path (Rust), `:=` is still an assignment
            Some(':') => next_char(cur + 1) != Some(':'),
            // `==`, `===` compare, `=>` is an arrow
            Some('=') => !matches!(next_char(cur + 1), Some('=' | '>')),
            _ => false,
        };
        if !separated {
            pos = key_end_at;
            continue;
        }
        cur += 1;
        if chars[cur - 1].1 == ':' && next_char(cur) == Some('=') {
            cur += 1;
        }
        while cur < chars.len() && matches!(chars[cur].1, ' ' | '\t') {
            cur += 1;
        }
        let Some(&(_, first)) = chars.get(cur) else {
            break;
        };
        let (value_from, value_to, next, quoted) = value_bounds(&chars, line.len(), cur);
        let lowered = key.to_lowercase();
        let value_text = &line[value_from..value_to.max(value_from)];
        if NAME_KEYS.contains(&lowered.as_str()) && is_secret_key(value_text) {
            *named = true;
        }
        let key_secret = is_secret_key(key)
            || (VALUE_KEYS.contains(&lowered.as_str()) && (named_before || *named));
        if !key_secret {
            // only the key is used up: a value may hold a pair of its own (`x: password=abc`)
            pos = key_end_at;
            continue;
        }
        if first == '{' {
            // a nested table: its own keys are looked at one by one
            pos = cur + 1;
            continue;
        }
        let value_end = byte(next.min(chars.len()));
        let after = &line[value_end..];
        let template = first == '`' && value_text.contains("${");
        let secret = if first == '[' {
            !value_text.trim().is_empty() && value_text != HIDDEN
        } else {
            !template && looks_secret(key, value_text, quoted, after)
        };
        if value_to > value_from && secret && !is_count(key, value_text) {
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
    use serde_json::Value;

    /// The cases both mirrors read (`src/core/config/secrets.test.ts` reads the same file).
    fn cases() -> Value {
        serde_json::from_str(include_str!("../../../../testdata/secrets/cases.json"))
            .unwrap_or(Value::Null)
    }

    fn strings(cases: &Value, name: &str) -> Vec<String> {
        cases[name]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn the_shared_cases_hold() {
        let cases = cases();
        let masked = cases["masked"].as_array().cloned().unwrap_or_default();
        assert!(masked.len() > 20, "the cases file is read");
        for pair in masked {
            let (input, expected) = (
                pair[0].as_str().unwrap_or(""),
                pair[1].as_str().unwrap_or(""),
            );
            assert_eq!(mask_text(input), expected, "{input:?}");
        }
        let plain = strings(&cases, "plain");
        assert!(plain.len() > 20);
        for text in plain {
            assert_eq!(mask_text(&text), text.as_str(), "{text:?}");
            assert!(matches!(mask_text(&text), Cow::Borrowed(_)));
        }
        for key in strings(&cases, "secretKeys") {
            assert!(is_secret_key(&key), "{key}");
        }
        for key in strings(&cases, "plainKeys") {
            assert!(!is_secret_key(&key), "{key}");
        }
    }

    #[test]
    fn words_of_a_name() {
        assert_eq!(key_words("apiKey2"), ["api", "key"]);
        assert_eq!(key_words("DB_PASSWORD"), ["db", "password"]);
        assert_eq!(key_words("APIKey"), ["api", "key"]);
        assert_eq!(key_words("x-api-key"), ["x", "api", "key"]);
        assert_eq!(key_words("KeyW"), ["key", "w"]);
        assert_eq!(key_words("s3Key"), ["s", "key"]);
        assert!(key_words("").is_empty() && key_words("__").is_empty());
    }

    #[test]
    fn counts_of_tokens_are_sizes() {
        assert!(is_count("tokens", "1200") && is_count("Tokens", " 3.5 "));
        assert!(
            !is_count("tokens", "abc") && !is_count("api_tokens", "12") && !is_count("token", "12")
        );
    }

    #[test]
    fn numbers_colors_and_marks() {
        for n in ["12", "-3.5", ".5", "1e9", "+2E-3", "7."] {
            assert!(is_number(n), "{n}");
        }
        for n in ["", ".", "e5", "1e", "0x1f", "1.2.3", "-"] {
            assert!(!is_number(n), "{n}");
        }
        assert!(
            is_color("#fff") && is_color("#ff000080") && !is_color("#ggg") && !is_color("#12345")
        );
        assert!(
            is_block_mark("|")
                && is_block_mark(">-")
                && is_block_mark("|2")
                && !is_block_mark("|x")
        );
        assert!(
            is_slug("app.theme")
                && is_slug("user-cache")
                && !is_slug("hunter2")
                && !is_slug("App.x")
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
            "api_key=\"x\\",
            "token=",
            "token:=",
            "eyJ",
            "eyJabcdefgh.",
            "xoxb-",
            "AKIA",
            "sk-",
            "-----BEGIN PRIVATE KEY-----",
            "-----BEGIN PRIVATE KEY-----\n",
            "ä€😀ß token=ä€😀ß",
        ] {
            let _ = mask_text(text);
        }
    }

    #[test]
    fn masking_twice_changes_nothing_more() {
        for text in [
            "API_KEY=abc",
            "-----BEGIN PRIVATE KEY-----\nabc\n-----END PRIVATE KEY-----\n",
            // a fake token, written in two parts so secret scanners pass it by
            concat!("x: sk-", "proj-Ab3dEf6hIj9kLm2nOp5qRs8t"),
        ] {
            let once = mask_text(text).into_owned();
            assert_eq!(mask_text(&once), once);
        }
    }
}
