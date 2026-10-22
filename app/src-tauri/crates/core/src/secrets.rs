//! Secret values in text that leaves the app (agent payloads, search): a key whose name ends in a secret word
//! (`DB_PASSWORD`, `apiKey`, `GITHUB_TOKEN`) followed by `=` | `:` | `=>` and a value that may be a secret keeps the
//! key and loses the value; so do the password of an address, known token formats wherever they are (`sk-…`, `ghp_…`,
//! `AKIA…`, JWT) and the body of a private key block. Principle: in a slot named like a secret, mask when in doubt
//! (a false mask on a secret name is cheap, a leak is not); names that only contain a secret word (`keys`, `keyJump`)
//! are no slot. Format-free on purpose: it covers config files, code, Markdown and selections alike; the config
//! formats add structural masking of their own. Mirror: `src/core/config/secrets.ts`; both read
//! `testdata/secrets/cases.json` and `testdata/secrets/leaks.json`.

use std::borrow::Cow;
use std::ops::Range;

/// What a masked value reads as.
pub const HIDDEN: &str = "<hidden>";

/// Last words of a key that name a password-class secret (`DB_PASSWORD`, `clientSecret`, `credentials`).
const STRONG_WORDS: &[&str] = &[
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
];

/// Last words of a key that name a token-class secret (`GITHUB_TOKEN`, `basic_auth`, `apikey`).
const NORMAL_WORDS: &[&str] = &[
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

/// Ends of a run-together last word that name a password-class secret (`dbpassword`, `clientsecret`).
const STRONG_ENDINGS: &[&str] = &["password", "passwd", "secret"];

/// Ends of a run-together last word that name a token (`apitoken`); `maxtokens` is a count.
const TOKEN_ENDINGS: &[&str] = &["token", "tokens"];

/// Words before `key` that make it a token-class secret (`api_key`, `privateKey`, `SIGNING_KEY`).
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

/// Words before `key` that keep it ordinary (`PUBLIC_KEY`, `cacheKey`, `i18nKey` (words `i n key`)); any other word
/// makes a weak secret slot (`STRIPE_KEY`, `openai_key`, `tls.key`).
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
    "n",
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
    "list",
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

/// Words after a secret word that only say which one (`SECRET_KEY_BASE`, `DB_PASSWORD_PROD`,
/// `password_confirmation`).
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
    "confirmation",
    "confirm",
    "repeat",
    "again",
    "retype",
    "verify",
];

/// Keys that hold a count when their value is a number (`tokens: 1200`, `"tokens": 3400`), and tokens when it is
/// not (`tokens: [a, b]`); `api_tokens` is never one.
const COUNT_KEYS: &[&str] = &["tokens", "ntokens"];

/// Bool | null words: a switch, never a secret.
const BOOL_WORDS: &[&str] = &[
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

/// Words of code that start an expression, not a value (`token = new Token()`, `key = await load()`).
const CODE_WORDS: &[&str] = &[
    "new", "await", "this", "self", "function", "async", "typeof", "require", "import", "lambda",
    "yield", "not",
];

/// First words of a line that declare a variable: the line is code.
const DECLARATIONS: &[&str] = &[
    "const",
    "let",
    "var",
    "val",
    "final",
    "auto",
    "my",
    "local",
    "static",
    "private",
    "public",
    "protected",
    "readonly",
];

/// Words of an `Authorization` header before the credential (`Bearer <token>`, `Basic <b64>`).
const AUTH_SCHEMES: &[&str] = &[
    "bearer",
    "basic",
    "token",
    "digest",
    "bot",
    "negotiate",
    "apikey",
];

/// Keys whose value names a variable (`- name: DB_PASSWORD`), and keys that then hold its value (`value: x`).
const NAME_KEYS: &[&str] = &["name", "key", "env", "var", "variable"];
const VALUE_KEYS: &[&str] = &["value", "val", "default"];

/// Pieces of text that every secret this module finds by name or format contains (lowercase); text with none (and
/// no line of a private key) is passed over at once.
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

/// The words of a key with the tail (`_prod`, `_base`, `_confirmation`, ...) set aside.
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

/// How a key names a secret.
#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyClass {
    /// `<word>_KEY`, `openai_key`, `tls.key`: secret when the value looks like one (not a slot name).
    Weak,
    /// Tokens, API keys, `auth`: secret unless the value is a number, code or a sentence.
    Normal,
    /// Passwords, secrets, credentials, passphrases: any value but a bool, a reference or code.
    Strong,
}

fn key_class(name: &str) -> Option<KeyClass> {
    let words = head_words(name);
    let last = words.last()?.as_str();
    let prev = words.len().checked_sub(2).map(|i| words[i].as_str());
    if last == "key" || last == "keys" {
        let prev = prev?;
        if KEY_QUALIFIERS.contains(&prev) {
            return Some(KeyClass::Normal);
        }
        return (last == "key" && !PLAIN_KEY_QUALIFIERS.contains(&prev)).then_some(KeyClass::Weak);
    }
    if let Some(p) = prev {
        let joined = format!("{p}{last}");
        if STRONG_WORDS.contains(&joined.as_str()) {
            return Some(KeyClass::Strong);
        }
        if NORMAL_WORDS.contains(&joined.as_str()) {
            return Some(KeyClass::Normal);
        }
    }
    if last == "token" || last == "tokens" {
        return (!prev.is_some_and(|p| COUNT_WORDS.contains(&p))).then_some(KeyClass::Normal);
    }
    if STRONG_WORDS.contains(&last) {
        return Some(KeyClass::Strong);
    }
    if NORMAL_WORDS.contains(&last) {
        return Some(KeyClass::Normal);
    }
    if STRONG_ENDINGS
        .iter()
        .any(|e| last.len() > e.len() && last.ends_with(e))
    {
        return Some(KeyClass::Strong);
    }
    for ending in TOKEN_ENDINGS {
        if last.len() > ending.len() && last.ends_with(ending) {
            let head = &last[..last.len() - ending.len()];
            return (!COUNT_WORDS.contains(&head)).then_some(KeyClass::Normal);
        }
    }
    None
}

/// Whether a key names a secret: its last word (after `_prod`, `_base`, `_confirmation`, ... are set aside) is a
/// secret word (`DB_PASSWORD`, `clientSecret`, `authToken`, `basic_auth`), or it is `key` after a word that is not a
/// plain one (`api_key`, `privateKey`, `STRIPE_KEY`, `openai_key`, `tls.key`). `keys`, `keydown`, `keyJump`, `KeyW`,
/// `monkey`, `tokenizer`, `max_tokens`, `token_count`, `authorName`, `secretName`, `PASSWORD_FILE`, `cacheKey`,
/// `PUBLIC_KEY` are not.
#[must_use]
pub fn is_secret_key(name: &str) -> bool {
    key_class(name).is_some()
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

/// `${VAR}`, `${{ secrets.NAME }}`, `$VAR`, `{{ name }}`: a pointer to a secret, not the secret.
fn is_reference(value: &str) -> bool {
    let t = value.trim();
    if t.starts_with("${{") || t.starts_with("{{") {
        return t.ends_with("}}");
    }
    if let Some(inner) = t.strip_prefix("${").and_then(|r| r.strip_suffix('}')) {
        return !inner.contains('}');
    }
    t.strip_prefix('$').is_some_and(|name| {
        !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')
    })
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

/// `process.env.API_KEY2`, `this.key`, `options.auth`: where a value is, not the value. Digits only in the last
/// part (`pk.eyJ1Ijoi.x9` is a token).
fn is_member(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() > 1
        && parts.iter().all(|p| is_identifier(p))
        && parts[..parts.len() - 1]
            .iter()
            .all(|p| !p.chars().any(char::is_numeric))
}

/// `getPassword()`, `config.get('db'`, `os.environ["X"]`: a call | an index; the name before `(` | `[` has no digits
/// (`Xy7(kL9!q` is a password).
fn is_call(s: &str) -> bool {
    s.find(['(', '[']).is_some_and(|i| {
        let name = &s[..i];
        i > 0 && name.split('.').all(is_identifier) && !name.chars().any(char::is_numeric)
    })
}

/// Lowercase words joined by `.`, `-`, `_`, `:` or `/`: a name (`app.theme`, `user-cache`, `app-theme-v2`), not a
/// secret. A part longer than 3 that mixes in digits (`a1b2c3d4`, `3ax6xnjp29`) makes it a secret.
fn is_slug(s: &str) -> bool {
    let sep = |c: char| matches!(c, '.' | '_' | ':' | '/' | '-');
    s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.contains(sep)
        && s.split(sep).all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                && (part.len() <= 3 || !part.chars().any(|c| c.is_ascii_digit()))
        })
}

/// 12+ characters of a token alphabet with letters and digits, not a slug (`0a1b2c3d4e5f`, `LS0tLS1CRUdJTi…`).
fn looks_random(s: &str) -> bool {
    s.len() >= 12
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '/' | '='))
        && s.chars().any(|c| c.is_ascii_digit())
        && s.chars().any(|c| c.is_ascii_alphabetic())
        && !is_slug(s)
}

/// `DB_PASSWORD`, `API_KEY`: a name written env-style.
fn is_env_style(key: &str) -> bool {
    key.starts_with(|c: char| c.is_ascii_uppercase() || c == '_')
        && key
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// A YAML block scalar mark (`key: |`): the value is on the next lines.
fn is_block_mark(s: &str) -> bool {
    let Some(rest) = s.strip_prefix(['|', '>']) else {
        return false;
    };
    let rest = rest.strip_prefix(['+', '-']).unwrap_or(rest);
    rest.is_empty() || (rest.len() == 1 && rest.chars().all(|c| c.is_ascii_digit()))
}

/// A variable named like a secret (`accessToken`, `API_KEY`, `newPassword`): two words or more, no digits. A single
/// word (`mysecretpassword`) or one with digits (`abc123secret`) is a value.
fn is_variable(value: &str) -> bool {
    !value.chars().any(char::is_numeric) && key_words(value).len() > 1 && is_secret_key(value)
}

/// Whether the value sits in code: a separator | closing bracket follows (`{ password: newPassword, … }`; `;` only on
/// a line with one `=`) or the line declares a variable (`const token = accessToken`).
fn in_code(after: &str, line: &str) -> bool {
    let after = after.trim_start();
    // `;` ends a statement only on a line with one assignment (`Server=db;Password=x;` is a connection string)
    if after.starts_with([',', ')', '}'])
        || (after.starts_with(';') && line.matches('=').count() == 1)
    {
        return true;
    }
    let mut words = line.split_whitespace();
    let first = match words.next() {
        Some("export") => words.next(),
        other => other,
    };
    first.is_some_and(|w| DECLARATIONS.contains(&w))
}

/// Whether the value of a secret key may be a secret (see the module doc for the principle). Never: empty, hidden,
/// a reference, a bool, a command (`$(…)`). Weak slots: only a value that is no slot name and (env-style key | looks
/// random). Not strong: a number, a color. Env lines (`DB_PASSWORD=…`, dotenv): everything else. Quoted: not the
/// key's own name (`password: "Password"`) nor a sentence (strong: only with an end mark | 4+ words; passphrase:
/// never). Unquoted: not code (`new`, `getPassword()`, `os.environ["X"]`, `process.env.X`), a block mark, a YAML tag
/// (`!Ref`), the key's own name (strong: only in code), a variable named like a secret in code, an identifier ending
/// a lone statement (`token = t;`), or a word starting a sentence (`The password: must be long`).
fn looks_secret(
    class: KeyClass,
    key: &str,
    value: &str,
    quoted: bool,
    after: &str,
    line: &str,
) -> bool {
    let lower = value.to_lowercase();
    if value.is_empty()
        || value == HIDDEN
        || is_reference(value)
        || BOOL_WORDS.contains(&lower.as_str())
    {
        return false;
    }
    if !quoted && value.starts_with("$(") {
        return false;
    }
    let strong = class == KeyClass::Strong;
    if class == KeyClass::Weak && (is_slug(value) || !(is_env_style(key) || looks_random(value))) {
        return false;
    }
    if !strong && (is_number(value) || is_color(value)) {
        return false;
    }
    if quoted {
        if flat(value) == flat(key) {
            return false;
        }
        let words: Vec<&str> = value.split_whitespace().collect();
        let sentence = words.len() > 1 && words.iter().all(|w| is_prose_word(w));
        if !strong {
            return !sentence;
        }
        if key_words(key).iter().any(|w| w.ends_with("passphrase")) {
            return true;
        }
        let end_mark = value.trim_end().ends_with(['.', '!', '?', ':']);
        return !(sentence && (end_mark || words.len() >= 4));
    }
    if CODE_WORDS.contains(&lower.as_str())
        || is_block_mark(value)
        || value.starts_with(['!', '&', '*'])
        || is_call(value)
        || is_member(value)
    {
        return false;
    }
    let code = in_code(after, line);
    if flat(value) == flat(key) && (!strong || code) {
        return false;
    }
    if is_identifier(value) && !value.chars().any(char::is_numeric) {
        if is_variable(value) && code {
            return false;
        }
        if after.starts_with(';') && line.matches('=').count() == 1 {
            return false;
        }
    }
    let mut rest = after.chars().skip_while(|c| matches!(c, ' ' | '\t'));
    !(value.chars().all(char::is_alphabetic)
        && after.starts_with([' ', '\t'])
        && rest.next().is_some_and(char::is_alphanumeric))
}

/// Type names a value of a secret field may be declared with (`password: string`, `token: str`, `key: String`).
const PRIMITIVE_TYPES: &[&str] = &[
    "string", "str", "int", "integer", "float", "double", "number", "bool", "boolean", "bytes",
    "byte", "any", "unknown", "object", "void", "never", "bigint", "symbol", "char", "rune",
    "long", "short", "decimal", "date", "datetime", "uuid", "i8", "i16", "i32", "i64", "i128",
    "isize", "u8", "u16", "u32", "u64", "u128", "usize", "f32", "f64",
];

/// `AuthConfig`, `Secret`: a type name (capital first, letters only, some lowercase).
fn is_pascal(value: &str) -> bool {
    value.starts_with(|c: char| c.is_ascii_uppercase())
        && value.chars().all(|c| c.is_ascii_alphabetic())
        && value.chars().any(|c| c.is_ascii_lowercase())
}

/// A type annotation after `:` (`password: string;`, `token: Option<String>,`, `auth: AuthConfig`, `secret:
/// Secret;`): a primitive type | a generic | a type name, followed by `; , ) = | > } ]` or the line end. A type name
/// at the line end only with 2+ words that name no secret (`password: AdminPassword` | `password: Hunter` stay
/// secret).
fn is_type_annotation(value: &str, after: &str) -> bool {
    let next = after.trim_start();
    let followed = next.starts_with([';', ',', ')', '=', '|', '>', '}', ']']);
    if !(followed || next.is_empty()) {
        return false;
    }
    let value = value.strip_suffix('?').unwrap_or(value);
    let primitive = |v: &str| PRIMITIVE_TYPES.contains(&v.to_ascii_lowercase().as_str());
    if let Some((head, _)) = value.split_once('<') {
        return is_identifier(head) && (primitive(head) || is_pascal(head));
    }
    primitive(value)
        || (is_pascal(value) && (followed || (key_words(value).len() > 1 && !is_secret_key(value))))
}

/// The random part of a prefixed token: 20+ characters with upper, lower case and digits (a slug is not one).
fn is_random(body: &str) -> bool {
    body.len() >= 20
        && body.chars().any(|c| c.is_ascii_uppercase())
        && body.chars().any(|c| c.is_ascii_lowercase())
        && body.chars().any(|c| c.is_ascii_digit())
}

/// The rest of a Hugging Face token: 30+ letters | digits with upper and lower case (often no digit).
fn is_random_letters(body: &str) -> bool {
    body.len() >= 30
        && body.chars().all(|c| c.is_ascii_alphanumeric())
        && body.chars().any(|c| c.is_ascii_uppercase())
        && body.chars().any(|c| c.is_ascii_lowercase())
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
            let random = if *prefix == "hf_" {
                is_random_letters(&rest[..body])
            } else {
                is_random(&rest[..body])
            };
            return random.then_some(prefix.len() + body);
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

/// A line of a PEM body: exactly 64 base64 characters with upper, lower case and digits (a hex hash is not one).
fn is_pem_line(content: &str) -> bool {
    let t = content.trim();
    t.len() == 64
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/'))
        && t.chars().any(|c| c.is_ascii_uppercase())
        && t.chars().any(|c| c.is_ascii_lowercase())
        && t.chars().any(|c| c.is_ascii_digit())
}

/// The labels of the `-----BEGIN <label>-----` (`marker` `BEGIN` | `END`) markers of a line.
fn marker_labels<'a>(line: &'a str, marker: &str) -> Vec<&'a str> {
    let open = format!("-----{marker} ");
    line.match_indices(open.as_str())
        .filter_map(|(at, m)| {
            let rest = &line[at + m.len()..];
            let label = rest
                .find(|c: char| !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == ' '))
                .map_or(rest, |end| &rest[..end]);
            rest[label.len()..].starts_with("-----").then_some(label)
        })
        .collect()
}

fn is_private_label(label: &str) -> bool {
    label.ends_with("PRIVATE KEY") || label.ends_with("PRIVATE KEY BLOCK")
}

/// `-----BEGIN [A-Z0-9 ]*PRIVATE KEY( BLOCK)?-----` (`marker` `BEGIN` | `END`) somewhere in the line.
fn is_private_key_marker(line: &str, marker: &str) -> bool {
    marker_labels(line, marker)
        .into_iter()
        .any(is_private_label)
}

/// A marker of a block that is no secret (`CERTIFICATE`, `PUBLIC KEY`, `PGP PUBLIC KEY BLOCK`, ...).
fn is_public_marker(line: &str, marker: &str) -> bool {
    marker_labels(line, marker)
        .into_iter()
        .any(|label| !is_private_label(label))
}

/// Line numbers (0-based) of public blocks (certificates, public keys): their body, also a part of one read without
/// its `BEGIN` (base64 lines right above its `END`). Their 64-char lines stay visible.
fn public_block_lines(text: &str) -> Vec<usize> {
    let mut out = Vec::new();
    if !text.contains("-----") {
        return out;
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut inside = false;
    for (i, line) in lines.iter().enumerate() {
        if inside {
            inside = !line.trim_start_matches([' ', '\t']).starts_with("-----END");
            if inside {
                out.push(i);
            }
        } else if is_public_marker(line, "BEGIN") {
            inside = true;
        } else if is_public_marker(line, "END") {
            out.extend(base64_above(&lines, i));
        }
    }
    out
}

/// The base64 lines right above line `end` (blank lines between them skipped).
fn base64_above(lines: &[&str], end: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for j in (0..end).rev() {
        let t = lines[j].trim();
        let base64 = t.len() >= 8
            && t.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='));
        if !(t.is_empty() || base64) {
            break;
        }
        if !t.is_empty() {
            out.push(j);
        }
    }
    out
}

/// Line numbers (0-based) of a private key body whose `BEGIN` line is not in the text (a part read from the middle
/// of a key): the base64 lines | blank lines right above an `END … PRIVATE KEY` line that has no `BEGIN` before it.
fn orphan_key_lines(text: &str) -> Vec<usize> {
    let mut out = Vec::new();
    if !text.contains("PRIVATE KEY") {
        return out;
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut inside = false;
    for (i, line) in lines.iter().enumerate() {
        if inside {
            inside = !line.trim_start_matches([' ', '\t']).starts_with("-----END");
        } else if is_private_key_marker(line, "BEGIN") {
            inside = true;
        } else if is_private_key_marker(line, "END") {
            out.extend(base64_above(&lines, i));
        }
    }
    out
}

/// `text` with every secret value replaced by [`HIDDEN`]; borrowed when nothing was hidden.
#[must_use]
pub fn mask_text(text: &str) -> Cow<'_, str> {
    let lower = text.to_lowercase();
    if !HINTS.iter().any(|hint| lower.contains(hint)) && !text.lines().any(is_pem_line) {
        return Cow::Borrowed(text);
    }
    let orphans = orphan_key_lines(text);
    let public = public_block_lines(text);
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    let mut named = false; // the line before named a secret variable (`- name: DB_PASSWORD`)
    let mut in_key = false; // inside the body of a private key block
    let mut rest = text;
    let mut index = 0;
    while !rest.is_empty() {
        let line_end = rest.find('\n').map_or(rest.len(), |i| i + 1);
        let (line, tail) = rest.split_at(line_end);
        rest = tail;
        let number = index;
        index += 1;
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        let end = content
            .trim_start_matches([' ', '\t'])
            .starts_with("-----END");
        let body = if in_key {
            in_key = !end;
            !end
        } else {
            orphans.contains(&number) || (is_pem_line(content) && !public.contains(&number))
        };
        if body {
            named = false;
            let trimmed = content.trim();
            if !trimmed.is_empty() && trimmed != HIDDEN {
                let lead = content.len() - content.trim_start().len();
                out.push_str(&content[..lead]);
                out.push_str(HIDDEN);
                out.push_str(&line[content.len()..]);
                changed = true;
            } else {
                out.push_str(line);
            }
            continue;
        }
        if !end && is_private_key_marker(content, "BEGIN") && line.ends_with('\n') {
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

/// A value found after a key: bytes `from..to` (inside quotes | brackets), the char index after it, quoted or not.
struct Found {
    from: usize,
    to: usize,
    next: usize,
    quoted: bool,
    list: bool,
}

/// The value that starts at char `cur`: quoted (also Python `b"…"`, `r'…'`), a list `[…]`, a reference `${…}` |
/// `{{ … }}`, else up to whitespace, `,` | `;` (not in an env line: `literal`) or the quote `delim` the pair sits in;
/// a closing bracket it did not open is left out (`{password: pwd}`).
fn value_at(
    chars: &[(usize, char)],
    len: usize,
    cur: usize,
    delim: Option<char>,
    literal: bool,
) -> Found {
    let byte = |i: usize| chars.get(i).map_or(len, |(b, _)| *b);
    let at = |i: usize| chars.get(i).map(|(_, c)| *c);
    let quoted = |open: usize| {
        let quote = chars[open].1;
        let mut end = open + 1;
        while end < chars.len() && chars[end].1 != quote {
            if chars[end].1 == '\\' {
                end += 1;
            }
            end += 1;
        }
        let end = end.min(chars.len());
        Found {
            from: chars[open].0 + 1,
            to: byte(end),
            next: end + 1,
            quoted: true,
            list: false,
        }
    };
    let mut open = cur;
    while open < cur + 2
        && at(open).is_some_and(|c| matches!(c, 'b' | 'B' | 'r' | 'R' | 'u' | 'U' | 'f' | 'F'))
    {
        open += 1;
    }
    if open > cur && matches!(at(open), Some('"' | '\'')) {
        return quoted(open);
    }
    let first = chars[cur].1;
    if matches!(first, '"' | '\'' | '`') {
        return quoted(cur);
    }
    if first == '[' {
        let mut end = cur + 1;
        while end < chars.len() && chars[end].1 != ']' {
            end += 1;
        }
        return Found {
            from: chars[cur].0 + 1,
            to: byte(end),
            next: end + 1,
            quoted: false,
            list: true,
        };
    }
    let template =
        (first == '$' && at(cur + 1) == Some('{')) || (first == '{' && at(cur + 1) == Some('{'));
    if template {
        let double = at(cur + 1) == Some('{') && (first == '{' || at(cur + 2) == Some('{'));
        let mut end = cur + 1;
        while end < chars.len() {
            if chars[end].1 == '}' && (!double || at(end + 1) == Some('}')) {
                end += if double { 2 } else { 1 };
                break;
            }
            end += 1;
        }
        let end = end.min(chars.len());
        return Found {
            from: chars[cur].0,
            to: byte(end),
            next: end,
            quoted: false,
            list: false,
        };
    }
    let mut end = cur;
    while end < chars.len() {
        let c = chars[end].1;
        if c.is_whitespace() || Some(c) == delim || (!literal && matches!(c, ',' | ';')) {
            break;
        }
        end += 1;
    }
    if !literal {
        // a closing bracket the value did not open belongs to the code around it
        while end > cur {
            let opener = match chars[end - 1].1 {
                ')' => '(',
                ']' => '[',
                '}' => '{',
                _ => break,
            };
            let closer = chars[end - 1].1;
            let count = |ch: char| chars[cur..end].iter().filter(|(_, c)| *c == ch).count();
            if count(closer) > count(opener) {
                end -= 1;
            } else {
                break;
            }
        }
    }
    Found {
        from: chars[cur].0,
        to: byte(end),
        next: end,
        quoted: false,
        list: false,
    }
}

/// `key <sep> value` pairs whose key is secret (or that are the `value` of a `name: <secret>` on this line or the
/// one before; `named` carries that from line to line).
#[allow(clippy::too_many_lines)]
fn mask_pairs<'a>(line: &'a str, named: &mut bool) -> Cow<'a, str> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let byte = |i: usize| chars.get(i).map_or(line.len(), |(b, _)| *b);
    let at = |i: usize| chars.get(i).map(|(_, c)| *c);
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
        let before = pos.checked_sub(1).and_then(at);
        if before == Some('/') && pos.checked_sub(2).and_then(at) == Some('/') {
            // `scheme://user:password@host`: an address, its password is masked as one
            pos = key_end_at;
            continue;
        }
        // optional closing quote of a quoted key, spaces, then the separator
        let mut cur = key_end_at;
        let closed = at(cur).is_some_and(|c| matches!(c, '"' | '\''));
        if closed {
            cur += 1;
            // an index assignment: `$config['db_password'] = …`, `os.environ["TOKEN"] = …`
            if at(cur) == Some(']') {
                cur += 1;
            }
        }
        // a pair inside quotes (`-H "X-Api-Key: v"`): the value ends at that quote
        let delim = before
            .filter(|q| matches!(q, '"' | '\'' | '`') && !(closed && at(key_end_at) == Some(*q)));
        while at(cur).is_some_and(|c| matches!(c, ' ' | '\t')) {
            cur += 1;
        }
        let sep = at(cur);
        let arrow = sep == Some('=') && at(cur + 1) == Some('>');
        let separated = match sep {
            // `::` is a path (Rust), `:=` is still an assignment
            Some(':') => at(cur + 1) != Some(':'),
            // `==`, `===` compare; `=>` pairs a quoted key | a Ruby symbol (`'password' => 'x'`), else an arrow
            Some('=') => at(cur + 1) != Some('=') && (!arrow || closed || before == Some(':')),
            _ => false,
        };
        if !separated {
            pos = key_end_at;
            continue;
        }
        let env_line = sep == Some('=')
            && !arrow
            && cur == key_end_at
            && is_env_style(key)
            && matches!(line[..byte(pos)].trim(), "" | "export");
        cur += 1;
        if (sep == Some(':') && at(cur) == Some('=')) || arrow {
            cur += 1;
        }
        while at(cur).is_some_and(|c| matches!(c, ' ' | '\t')) {
            cur += 1;
        }
        let Some(first) = at(cur) else {
            break;
        };
        let mut found = value_at(&chars, line.len(), cur, delim, env_line);
        let lowered = key.to_lowercase();
        if NAME_KEYS.contains(&lowered.as_str())
            && is_secret_key(&line[found.from..found.to.max(found.from)])
        {
            *named = true;
        }
        let class = key_class(key).or_else(|| {
            (VALUE_KEYS.contains(&lowered.as_str()) && (named_before || *named))
                .then_some(KeyClass::Normal)
        });
        let Some(class) = class else {
            // only the key is used up: a value may hold a pair of its own (`x: password=abc`)
            pos = key_end_at;
            continue;
        };
        if first == '{' && at(cur + 1) != Some('{') {
            // a nested table: its own keys are looked at one by one
            pos = cur + 1;
            continue;
        }
        // `Authorization: Bearer <token>`: the credential is the word after the scheme
        let scheme = &line[found.from..found.to.max(found.from)];
        if !found.quoted && AUTH_SCHEMES.contains(&scheme.to_lowercase().as_str()) {
            let mut word = found.next;
            while at(word).is_some_and(|c| matches!(c, ' ' | '\t')) {
                word += 1;
            }
            if word > found.next && at(word).is_some_and(|c| Some(c) != delim && !c.is_whitespace())
            {
                found = value_at(&chars, line.len(), word, delim, true);
            }
        }
        let value = &line[found.from..found.to.max(found.from)];
        let after = &line[byte(found.next.min(chars.len()))..];
        let secret = if found.list {
            !value.trim().is_empty() && value != HIDDEN
        } else if first == '`' && value.contains("${") {
            false
        } else if env_line {
            env_value_secret(class, key, value)
        } else if sep == Some(':') && !found.quoted && is_type_annotation(value, after) {
            false
        } else {
            looks_secret(class, key, value, found.quoted, after, line)
        };
        if found.to > found.from && secret && !is_count(key, value) {
            out.push_str(&line[copied..found.from]);
            out.push_str(HIDDEN);
            copied = found.to;
        }
        pos = found.next.max(key_end_at);
    }
    if copied == 0 {
        Cow::Borrowed(line)
    } else {
        out.push_str(&line[copied..]);
        Cow::Owned(out)
    }
}

/// The value of an env line (`DB_PASSWORD=…`, dotenv | shell): literal to the end of its token; only a reference,
/// a command (`$(…)`), a bool, a slot name under a weak key and (not strong) a number are no secret.
fn env_value_secret(class: KeyClass, key: &str, value: &str) -> bool {
    let lower = value.to_lowercase();
    if value.is_empty()
        || value == HIDDEN
        || is_reference(value)
        || value.starts_with("$(")
        || BOOL_WORDS.contains(&lower.as_str())
    {
        return false;
    }
    match class {
        KeyClass::Strong => true,
        KeyClass::Normal => !is_number(value),
        KeyClass::Weak => {
            !is_number(value) && !is_slug(value) && (is_env_style(key) || looks_random(value))
        }
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
                && is_slug("app-theme-v2")
                && !is_slug("a1b2c3d4-e5f6")
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

    /// Verifier proof: real secrets that must not survive (`testdata/secrets/leaks.json`).
    #[test]
    fn verifier_leaks_are_hidden() {
        let doc: Value =
            serde_json::from_str(include_str!("../../../../testdata/secrets/leaks.json"))
                .unwrap_or(Value::Null);
        let leaks = doc["leaks"].as_array().cloned().unwrap_or_default();
        assert!(leaks.len() > 10, "the leaks file is read");
        let through: Vec<String> = leaks
            .iter()
            .filter(|l| {
                let (input, secret) = (l[1].as_str().unwrap_or(""), l[2].as_str().unwrap_or(""));
                mask_text(input).contains(secret)
            })
            .map(|l| l[0].as_str().unwrap_or("").to_owned())
            .collect();
        assert!(
            through.is_empty(),
            "{} let through: {through:#?}",
            through.len()
        );
    }
}
