// Secrets on screen: what a hidden value looks like, and which values of a loose text (a code block, a code file) are
// secret. Config files get their secret spans from the backend (the structure of the format); text without a format
// uses the rule here, a line-by-line port of `secrets::mask_text` (Rust): a key whose name ends in a secret word,
// followed by `=` | `:` | `=>` and a value that may be a secret; the password of an address; known token formats
// wherever they are; the body of a private key. Principle: in a slot named like a secret, mask when in doubt; names
// that only contain a secret word (`keys`, `keyJump`) are no slot. Both mirrors read `testdata/secrets/cases.json`
// and `testdata/secrets/leaks.json`.

/** What a hidden value shows. Fixed: the length of the value is not given away. */
export const MASK = "••••••";

/** What the backend writes for a hidden value; never hidden twice. */
const HIDDEN = "<hidden>";

/** Last words of a key that name a password-class secret (`DB_PASSWORD`, `clientSecret`, `credentials`). */
const STRONG_WORDS = new Set([
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
]);
/** Last words of a key that name a token-class secret (`GITHUB_TOKEN`, `basic_auth`, `apikey`). */
const NORMAL_WORDS = new Set([
  "token",
  "tokens",
  "auth",
  "authorization",
  "apikey",
  "apikeys",
  "accesskey",
  "secretkey",
  "privatekey",
]);
/** Ends of a run-together last word that name a password-class secret (`dbpassword`, `clientsecret`). */
const STRONG_ENDINGS = ["password", "passwd", "secret"];
/** Ends of a run-together last word that name a token (`apitoken`); `maxtokens` is a count. */
const TOKEN_ENDINGS = ["token", "tokens"];
/** Words before `key` that make it a token-class secret (`api_key`, `privateKey`, `SIGNING_KEY`). */
const KEY_QUALIFIERS = new Set([
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
]);
/**
 * Words before `key` that keep it ordinary (`PUBLIC_KEY`, `cacheKey`, `i18nKey` (words `i n key`)); any other word
 * makes a weak secret slot (`STRIPE_KEY`, `openai_key`, `tls.key`).
 */
const PLAIN_KEY_QUALIFIERS = new Set([
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
]);
/** Words before `tokens` that make it a count, not a secret (`max_tokens`, `input_tokens`, `ntokens`). */
const COUNT_WORDS = new Set([
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
]);
/** Words after a secret word that only say which one (`SECRET_KEY_BASE`, `password_confirmation`). */
const TAIL_WORDS = new Set([
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
]);
/** Keys that hold a count when their value is a number (`tokens: 1200`); `api_tokens` is never one. */
const COUNT_KEYS = ["tokens", "ntokens"];
/** Bool | null words: a switch, never a secret. */
const BOOL_WORDS = new Set([
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
]);
/** Words of code that start an expression, not a value (`token = new Token()`, `key = await load()`). */
const CODE_WORDS = new Set([
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
]);
/** First words of a line that declare a variable: the line is code. */
const DECLARATIONS = new Set([
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
]);
/** Words of an `Authorization` header before the credential (`Bearer <token>`, `Basic <b64>`). */
const AUTH_SCHEMES = new Set(["bearer", "basic", "token", "digest", "bot", "negotiate", "apikey"]);
/** Keys whose value names a variable (`- name: DB_PASSWORD`), and keys that then hold its value (`value: x`). */
const NAME_KEYS = new Set(["name", "key", "env", "var", "variable"]);
const VALUE_KEYS = new Set(["value", "val", "default"]);

type CharKind = "upper" | "lower" | "digit" | null;

function charKind(c: string): CharKind {
  if (/\p{Uppercase}/u.test(c)) return "upper";
  if (/\p{Lowercase}/u.test(c) || /\p{Alphabetic}/u.test(c)) return "lower";
  if (/\p{N}/u.test(c)) return "digit";
  return null;
}

/** The words of a name, lowercase, numbers left out: `apiKey2` -> `api key`, `DB_PASSWORD` -> `db password`. */
export function keyWords(name: string): string[] {
  const out: string[] = [];
  let word = "";
  let prev: CharKind = null;
  const chars = [...name];
  for (let i = 0; i < chars.length; i += 1) {
    const c = chars[i] ?? "";
    const kind = charKind(c);
    if (kind === null) {
      if (word !== "") out.push(word);
      word = "";
      prev = null;
      continue;
    }
    const next = chars[i + 1];
    const nextLower = next !== undefined && /\p{Lowercase}/u.test(next);
    const split =
      prev !== null &&
      ((prev === "lower" && kind === "upper") ||
        (prev === "digit") !== (kind === "digit") ||
        (prev === "upper" && kind === "upper" && nextLower));
    if (split && word !== "") {
      out.push(word);
      word = "";
    }
    word += c;
    prev = kind;
  }
  if (word !== "") out.push(word);
  return out.map((w) => w.toLowerCase()).filter((w) => !/^\p{N}+$/u.test(w));
}

/** The words of a key with the tail (`_prod`, `_base`, `_confirmation`, ...) set aside. */
function headWords(name: string): string[] {
  const words = keyWords(name);
  while (words.length > 1 && TAIL_WORDS.has(words[words.length - 1] ?? "")) words.pop();
  return words;
}

/** A number under a count key (`tokens: 1200`): a size, not a secret. */
export function isCount(key: string, value: string): boolean {
  const flat = key.toLowerCase().replace(/[^\p{L}\p{N}]/gu, "");
  const v = value.trim();
  return COUNT_KEYS.includes(flat) && v !== "" && Number.isFinite(Number(v));
}

/**
 * How a key names a secret. `weak`: `<word>_KEY`, `openai_key`, `tls.key` (secret when the value looks like one);
 * `normal`: tokens, API keys, `auth`; `strong`: passwords, secrets, credentials, passphrases.
 */
type KeyClass = "weak" | "normal" | "strong";

function keyClass(name: string): KeyClass | null {
  const words = headWords(name);
  const last = words[words.length - 1];
  const prev = words[words.length - 2];
  if (last === undefined) return null;
  if (last === "key" || last === "keys") {
    if (prev === undefined) return null;
    if (KEY_QUALIFIERS.has(prev)) return "normal";
    return last === "key" && !PLAIN_KEY_QUALIFIERS.has(prev) ? "weak" : null;
  }
  if (prev !== undefined) {
    if (STRONG_WORDS.has(prev + last)) return "strong";
    if (NORMAL_WORDS.has(prev + last)) return "normal";
  }
  if (last === "token" || last === "tokens") {
    return prev !== undefined && COUNT_WORDS.has(prev) ? null : "normal";
  }
  if (STRONG_WORDS.has(last)) return "strong";
  if (NORMAL_WORDS.has(last)) return "normal";
  if (STRONG_ENDINGS.some((e) => last.length > e.length && last.endsWith(e))) return "strong";
  for (const ending of TOKEN_ENDINGS) {
    if (last.length > ending.length && last.endsWith(ending)) {
      return COUNT_WORDS.has(last.slice(0, -ending.length)) ? null : "normal";
    }
  }
  return null;
}

/**
 * Whether a key names a secret: its last word (after `_prod`, `_base`, `_confirmation`, ... are set aside) is a
 * secret word (`DB_PASSWORD`, `clientSecret`, `authToken`, `basic_auth`), or it is `key` after a word that is not a
 * plain one (`api_key`, `privateKey`, `STRIPE_KEY`, `openai_key`, `tls.key`). `keys`, `keydown`, `keyJump`, `KeyW`,
 * `monkey`, `tokenizer`, `max_tokens`, `token_count`, `authorName`, `secretName`, `PASSWORD_FILE`, `cacheKey`,
 * `PUBLIC_KEY` are not.
 */
export function isSecretKey(name: string): boolean {
  return keyClass(name) !== null;
}

export type Span = { from: number; to: number };

/** `${VAR}`, `${{ secrets.NAME }}`, `$VAR`, `{{ name }}`: a pointer to a secret, not the secret. */
function isReference(value: string): boolean {
  const t = value.trim();
  if (t.startsWith("${{") || t.startsWith("{{")) return t.endsWith("}}");
  if (t.startsWith("${") && t.endsWith("}")) return !t.slice(2, -1).includes("}");
  return /^\$[\p{L}\p{N}_]+$/u.test(t);
}

/** Letters, digits and nothing else, lowercase: `API key` and `apiKey` read the same. */
function flat(text: string): string {
  return text.toLowerCase().replace(/[^\p{L}\p{N}]/gu, "");
}

const NUMBER = /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/;
const COLOR = /^#([0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;
/** A word of a sentence: letters (and apostrophes), maybe one end mark (`required.`). */
const PROSE_WORD = /^[\p{L}'’]+[.,!?:;]?$/u;
const IDENT = /^[\p{L}_$][\p{L}\p{N}_$]*$/u;
/** One word of letters, capitalized at most (`reset`, `Optional`): `is_plain_word`. */
const PLAIN_WORD = /^\p{L}(?:(?!\p{Lu})\p{L})*$/u;
const isPlainWord = (word: string): boolean => PLAIN_WORD.test(word);
/** A YAML block scalar mark (`key: |`): the value is on the next lines. */
const BLOCK_MARK = /^[|>][+-]?[0-9]?$/;

/** `process.env.API_KEY2`, `this.key`: where a value is. Digits only in the last part (`pk.eyJ1Ijoi.x9` is a token). */
function isMember(s: string): boolean {
  const parts = s.split(".");
  return (
    parts.length > 1 &&
    parts.every((p) => IDENT.test(p)) &&
    parts.slice(0, -1).every((p) => !/\p{N}/u.test(p))
  );
}

/** `getPassword()`, `config.get('db'`, `os.environ["X"]`: a call | an index; the name before has no digits. */
function isCall(s: string): boolean {
  const i = s.search(/[([]/);
  if (i <= 0) return false;
  const name = s.slice(0, i);
  return name.split(".").every((p) => IDENT.test(p)) && !/\p{N}/u.test(name);
}

/**
 * Lowercase words joined by `.`, `-`, `_`, `:` or `/`: a name (`app.theme`, `user-cache`, `app-theme-v2`), not a
 * secret. A part longer than 3 that mixes in digits (`a1b2c3d4`, `3ax6xnjp29`) makes it a secret.
 */
function isSlug(s: string): boolean {
  if (!/^[a-z]/.test(s) || !/[._:/-]/.test(s)) return false;
  return s
    .split(/[._:/-]/)
    .every((part) => /^[a-z0-9]+$/.test(part) && (part.length <= 3 || !/[0-9]/.test(part)));
}

/** 12+ characters of a token alphabet with letters and digits, not a slug (`0a1b2c3d4e5f`, `LS0tLS1CRUdJTi…`). */
function looksRandom(s: string): boolean {
  return (
    s.length >= 12 &&
    /^[A-Za-z0-9_\-+/=]+$/.test(s) &&
    /[0-9]/.test(s) &&
    /[A-Za-z]/.test(s) &&
    !isSlug(s)
  );
}

/** `DB_PASSWORD`, `API_KEY`: a name written env-style. */
function isEnvStyle(key: string): boolean {
  return /^[A-Z_][A-Z0-9_]*$/.test(key);
}

/**
 * A variable named like a secret (`accessToken`, `API_KEY`, `newPassword`): two words or more, no digits. A single
 * word (`mysecretpassword`) or one with digits (`abc123secret`) is a value.
 */
function isVariable(value: string): boolean {
  return !/\p{N}/u.test(value) && keyWords(value).length > 1 && isSecretKey(value);
}

/** Whether the value sits in code: a separator | closing bracket follows, or the line declares a variable. */
function inCode(after: string, line: string): boolean {
  const rest = after.trimStart();
  // `;` ends a statement only on a line with one assignment (`Server=db;Password=x;` is a connection string)
  if (/^[,)}]/.test(rest) || (rest.startsWith(";") && line.split("=").length === 2)) return true;
  const words = line.split(/\s+/).filter((w) => w !== "");
  const first = words[0] === "export" ? words[1] : words[0];
  return first !== undefined && DECLARATIONS.has(first);
}

/** Whether the value of a secret key may be a secret: the mirror of `looks_secret` (Rust), which says why. */
function looksSecret(
  cls: KeyClass,
  key: string,
  value: string,
  quoted: boolean,
  after: string,
  line: string,
): boolean {
  const lower = value.toLowerCase();
  if (value === "" || value === HIDDEN || isReference(value) || BOOL_WORDS.has(lower)) return false;
  if (!quoted && value.startsWith("$(")) return false;
  const strong = cls === "strong";
  if (cls === "weak" && (isSlug(value) || !(isEnvStyle(key) || looksRandom(value)))) return false;
  if (!strong && (NUMBER.test(value) || COLOR.test(value))) return false;
  if (quoted) {
    if (flat(value) === flat(key)) return false;
    const words = value.split(/\s+/u).filter((w) => w !== "");
    const sentence = words.length > 1 && words.every((w) => PROSE_WORD.test(w));
    if (!strong) return !sentence;
    if (keyWords(key).some((w) => w.endsWith("passphrase"))) return true;
    const endMark = /[.!?:]$/.test(value.trimEnd());
    // a label that names the slot in words (`password: "Enter your password"`) is no value
    const own = headWords(key).at(-1);
    if (sentence && own !== undefined && words.some((w) => flat(w) === own)) return false;
    return !(sentence && (endMark || words.length >= 4));
  }
  if (CODE_WORDS.has(lower) || BLOCK_MARK.test(value) || /^[!&*]/.test(value)) return false;
  if (isCall(value) || isMember(value)) return false;
  const code = inCode(after, line);
  if (flat(value) === flat(key) && (!strong || code)) return false;
  if (IDENT.test(value) && !/\p{N}/u.test(value)) {
    if (isVariable(value) && code) return false;
    if (after.startsWith(";") && line.split("=").length === 2) return false;
  }
  // a word followed by more words: a sentence (`The password: must be long`); `hunter2 is rotated` stays secret
  return !(/^\p{Alphabetic}+$/u.test(value) && /^[ \t]+[\p{L}\p{N}]/u.test(after));
}

/** The value of an env line (`DB_PASSWORD=…`, dotenv | shell): the mirror of `env_value_secret` (Rust). */
function envValueSecret(cls: KeyClass, key: string, value: string): boolean {
  if (
    value === "" ||
    value === HIDDEN ||
    isReference(value) ||
    value.startsWith("$(") ||
    BOOL_WORDS.has(value.toLowerCase())
  ) {
    return false;
  }
  if (cls === "strong") return true;
  if (cls === "normal") return !NUMBER.test(value);
  return !NUMBER.test(value) && !isSlug(value) && (isEnvStyle(key) || looksRandom(value));
}

/** One line as code points: `at(i)` the point, `off(i)` its UTF-16 offset (`line.length` past the end). */
type Line = { points: string[]; offsets: number[]; text: string };

function asLine(text: string): Line {
  const points: string[] = [];
  const offsets: number[] = [];
  let at = 0;
  for (const p of text) {
    points.push(p);
    offsets.push(at);
    at += p.length;
  }
  return { points, offsets, text };
}

const off = (l: Line, i: number): number => l.offsets[i] ?? l.text.length;
const pt = (l: Line, i: number): string | undefined => l.points[i];
const isIdentChar = (c: string | undefined): boolean =>
  c !== undefined && /[\p{L}\p{N}_.-]/u.test(c);

type Found = { from: number; to: number; next: number; quoted: boolean; list: boolean };

/** The value that starts at point `cur`: the mirror of `value_at` (Rust). */
function valueAt(l: Line, cur: number, delim: string | null, literal: boolean): Found {
  const n = l.points.length;
  const quotedAt = (open: number): Found => {
    const quote = pt(l, open);
    let end = open + 1;
    while (end < n && pt(l, end) !== quote) {
      if (pt(l, end) === "\\") end += 1;
      end += 1;
    }
    end = Math.min(end, n);
    return { from: off(l, open) + 1, to: off(l, end), next: end + 1, quoted: true, list: false };
  };
  let open = cur;
  while (open < cur + 2 && /^[bBrRuUfF]$/.test(pt(l, open) ?? "")) open += 1;
  if (open > cur && (pt(l, open) === '"' || pt(l, open) === "'")) return quotedAt(open);
  const first = pt(l, cur) ?? "";
  if (first === '"' || first === "'" || first === "`") return quotedAt(cur);
  if (first === "[") {
    let end = cur + 1;
    while (end < n && pt(l, end) !== "]") end += 1;
    return { from: off(l, cur) + 1, to: off(l, end), next: end + 1, quoted: false, list: true };
  }
  if ((first === "$" && pt(l, cur + 1) === "{") || (first === "{" && pt(l, cur + 1) === "{")) {
    const double = pt(l, cur + 1) === "{" && (first === "{" || pt(l, cur + 2) === "{");
    let end = cur + 1;
    while (end < n) {
      if (pt(l, end) === "}" && (!double || pt(l, end + 1) === "}")) {
        end += double ? 2 : 1;
        break;
      }
      end += 1;
    }
    end = Math.min(end, n);
    return { from: off(l, cur), to: off(l, end), next: end, quoted: false, list: false };
  }
  let end = cur;
  while (end < n) {
    const c = pt(l, end) ?? "";
    if (/\s/u.test(c) || c === delim || (!literal && (c === "," || c === ";"))) break;
    end += 1;
  }
  if (!literal) {
    // a closing bracket the value did not open belongs to the code around it
    const pairs: Record<string, string> = { ")": "(", "]": "[", "}": "{" };
    while (end > cur) {
      const closer = pt(l, end - 1) ?? "";
      const opener = pairs[closer];
      if (opener === undefined) break;
      const part = l.points.slice(cur, end);
      const count = (ch: string): number => part.filter((c) => c === ch).length;
      if (count(closer) > count(opener)) end -= 1;
      else break;
    }
  }
  return { from: off(l, cur), to: off(l, end), next: end, quoted: false, list: false };
}

/** Spans (in the line) of the values of secret keys: the mirror of `mask_pairs` (Rust); `named` carries over. */
function pairSpans(text: string, named: { value: boolean }): Span[] {
  const l = asLine(text);
  const n = l.points.length;
  const namedBefore = named.value;
  named.value = false;
  const found: Span[] = [];
  let pos = 0;
  while (pos < n) {
    if (!isIdentChar(pt(l, pos)) || (pos > 0 && isIdentChar(pt(l, pos - 1)))) {
      pos += 1;
      continue;
    }
    let keyEnd = pos;
    while (keyEnd < n && isIdentChar(pt(l, keyEnd))) keyEnd += 1;
    const key = text.slice(off(l, pos), off(l, keyEnd));
    const before = pos > 0 ? pt(l, pos - 1) : undefined;
    if (before === "/" && pos > 1 && pt(l, pos - 2) === "/") {
      // `scheme://user:password@host`: an address, its password is masked as one
      pos = keyEnd;
      continue;
    }
    let cur = keyEnd;
    const closed = pt(l, cur) === '"' || pt(l, cur) === "'";
    if (closed) {
      cur += 1;
      // an index assignment: `$config['db_password'] = …`, `os.environ["TOKEN"] = …`
      if (pt(l, cur) === "]") cur += 1;
    }
    const delim =
      before !== undefined && /^["'`]$/.test(before) && !(closed && pt(l, keyEnd) === before)
        ? before
        : null;
    while (pt(l, cur) === " " || pt(l, cur) === "\t") cur += 1;
    const sep = pt(l, cur);
    const arrow = sep === "=" && pt(l, cur + 1) === ">";
    const separated =
      sep === ":"
        ? pt(l, cur + 1) !== ":"
        : sep === "="
          ? pt(l, cur + 1) !== "=" && (!arrow || closed || before === ":")
          : false;
    if (!separated) {
      pos = keyEnd;
      continue;
    }
    const envLine =
      sep === "=" &&
      !arrow &&
      cur === keyEnd &&
      isEnvStyle(key) &&
      ["", "export"].includes(text.slice(0, off(l, pos)).trim());
    cur += 1;
    if ((sep === ":" && pt(l, cur) === "=") || arrow) cur += 1;
    while (pt(l, cur) === " " || pt(l, cur) === "\t") cur += 1;
    const first = pt(l, cur);
    if (first === undefined) break;
    let value = valueAt(l, cur, delim, envLine);
    const lowered = key.toLowerCase();
    if (
      NAME_KEYS.has(lowered) &&
      isSecretKey(text.slice(value.from, Math.max(value.to, value.from)))
    ) {
      named.value = true;
    }
    const cls: KeyClass | null =
      keyClass(key) ?? (VALUE_KEYS.has(lowered) && (namedBefore || named.value) ? "normal" : null);
    if (cls === null) {
      // only the key is used up: a value may hold a pair of its own (`x: password=abc`)
      pos = keyEnd;
      continue;
    }
    if (first === "{" && pt(l, cur + 1) !== "{") {
      // a nested table: its own keys are looked at one by one
      pos = cur + 1;
      continue;
    }
    // `Authorization: Bearer <token>`: the credential is the word after the scheme
    const scheme = text.slice(value.from, Math.max(value.to, value.from));
    if (!value.quoted && AUTH_SCHEMES.has(scheme.toLowerCase())) {
      let word = value.next;
      while (pt(l, word) === " " || pt(l, word) === "\t") word += 1;
      const c = pt(l, word);
      if (word > value.next && c !== undefined && c !== delim && !/\s/u.test(c)) {
        value = valueAt(l, word, delim, true);
      }
    }
    const inner = text.slice(value.from, Math.max(value.to, value.from));
    const after = text.slice(off(l, Math.min(value.next, n)));
    let secret: boolean;
    if (value.list) secret = inner.trim() !== "" && inner !== HIDDEN;
    else if (first === "`" && inner.includes("${")) secret = false;
    else if (envLine) secret = envValueSecret(cls, key, inner);
    else if (sep === ":" && !value.quoted && isTypeAnnotation(inner, after)) secret = false;
    else secret = looksSecret(cls, key, inner, value.quoted, after, text);
    if (value.to > value.from && secret && !isCount(key, inner)) {
      found.push({ from: value.from, to: value.to });
    }
    pos = Math.max(value.next, keyEnd);
  }
  return found;
}

/** The address `scheme://user:password@host`: where the password is. */
const ADDRESS = /:\/\/[^\s/?#"'\\@]*?:([^\s/?#"'\\@]+)@/g;

/** Known token formats, secret whatever their key: OpenAI | Anthropic, Stripe, GitHub, GitLab, AWS, Google, Slack, Hugging Face, npm, JWT. */
const TOKEN =
  /(?<![\p{L}\p{N}_-])(?:(sk-|sk_live_|sk_test_|rk_live_|rk_test_|ghp_|gho_|ghu_|ghs_|ghr_|github_pat_|glpat-|hf_|npm_)([A-Za-z0-9_-]+)|(?:AKIA|ASIA)[A-Z0-9]{16}|AIza[A-Za-z0-9_-]{35}|xox[abprs]-[A-Za-z0-9-]{10,}|eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,})(?![A-Za-z0-9_-])/gu;

/** Type names a value of a secret field may be declared with (`password: string`, `token: str`, `key: String`). */
const PRIMITIVE_TYPES = new Set([
  "string",
  "str",
  "int",
  "integer",
  "float",
  "double",
  "number",
  "bool",
  "boolean",
  "bytes",
  "byte",
  "any",
  "unknown",
  "object",
  "void",
  "never",
  "bigint",
  "symbol",
  "char",
  "rune",
  "long",
  "short",
  "decimal",
  "date",
  "datetime",
  "uuid",
  "i8",
  "i16",
  "i32",
  "i64",
  "i128",
  "isize",
  "u8",
  "u16",
  "u32",
  "u64",
  "u128",
  "usize",
  "f32",
  "f64",
]);

/** `AuthConfig`, `Secret`: a type name (capital first, letters only, some lowercase). */
const PASCAL = /^[A-Z][A-Za-z]*$/;

/** A type annotation after `:` (`password: string;`, `auth: AuthConfig`): the mirror of `is_type_annotation` (Rust). */
function isTypeAnnotation(value: string, after: string): boolean {
  const follower = after.trimStart();
  const followed = /^[;,)=|>}\]]/.test(follower);
  if (!(followed || follower === "")) return false;
  const v = value.endsWith("?") ? value.slice(0, -1) : value;
  const primitive = (t: string): boolean => PRIMITIVE_TYPES.has(t.toLowerCase());
  const pascal = (t: string): boolean => PASCAL.test(t) && /[a-z]/.test(t);
  const lt = v.indexOf("<");
  if (lt >= 0) {
    const head = v.slice(0, lt);
    return IDENT.test(head) && (primitive(head) || pascal(head));
  }
  return primitive(v) || (pascal(v) && (followed || (keyWords(v).length > 1 && !isSecretKey(v))));
}

/** The random part of a prefixed token: 20+ characters with upper, lower case and digits (a slug is not one). */
function isRandom(prefix: string, body: string): boolean {
  if (prefix === "hf_") {
    // Hugging Face: 30+ letters | digits with upper and lower case, often no digit
    return (
      body.length >= 30 && /^[A-Za-z0-9]+$/.test(body) && /[A-Z]/.test(body) && /[a-z]/.test(body)
    );
  }
  return body.length >= 20 && /[A-Z]/.test(body) && /[a-z]/.test(body) && /[0-9]/.test(body);
}

/** A line of a PEM body: exactly 64 base64 characters with upper, lower case and digits (a hex hash is not one). */
function isPemLine(line: string): boolean {
  const t = line.trim();
  return (
    t.length === 64 &&
    /^[A-Za-z0-9+/]+$/.test(t) &&
    /[A-Z]/.test(t) &&
    /[a-z]/.test(t) &&
    /[0-9]/.test(t)
  );
}

/** `-----BEGIN | END [A-Z0-9 ]*PRIVATE KEY( BLOCK)?-----` in a line. */
const BEGIN_KEY = /-----BEGIN [A-Z0-9 ]*PRIVATE KEY( BLOCK)?-----/;
/** A marker of a block that is no secret (`CERTIFICATE`, `PUBLIC KEY`, `PGP PUBLIC KEY BLOCK`, ...). */
function isPublicMarker(line: string, marker: "BEGIN" | "END"): boolean {
  for (const match of line.matchAll(/-----(BEGIN|END) ([A-Z0-9 ]*)-----/g)) {
    const label = match[2] ?? "";
    if (match[1] === marker && !/PRIVATE KEY( BLOCK)?$/.test(label)) return true;
  }
  return false;
}
const END_KEY = /-----END [A-Z0-9 ]*PRIVATE KEY( BLOCK)?-----/;
const END_LINE = /^[ \t]*-----END/;

/** Lines (0-based) of private key bodies: inside BEGIN..END, a body above an END with no BEGIN (a part read from the middle of a key). */
function keyBodyLines(lines: readonly string[]): Set<number> {
  const out = new Set<number>();
  if (!lines.some((line) => line.includes("PRIVATE KEY"))) return out;
  let inside = false;
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i] ?? "";
    if (inside) {
      inside = !END_LINE.test(line);
      if (inside && line.trim() !== "") out.add(i);
    } else if (BEGIN_KEY.test(line)) {
      inside = i + 1 < lines.length;
    } else if (END_KEY.test(line)) {
      base64Above(lines, i, out);
    }
  }
  return out;
}

/** The base64 lines right above line `end` (blank lines between them skipped). */
function base64Above(lines: readonly string[], end: number, out: Set<number>): void {
  for (let j = end - 1; j >= 0; j -= 1) {
    const t = (lines[j] ?? "").trim();
    if (!(t === "" || (t.length >= 8 && /^[A-Za-z0-9+/=]+$/.test(t)))) break;
    if (t !== "") out.add(j);
  }
}

/** Lines (0-based) of public blocks (certificates, public keys), also a part read without its BEGIN: the mirror of `public_block_lines` (Rust). */
function publicBlockLines(lines: readonly string[]): Set<number> {
  const out = new Set<number>();
  if (!lines.some((line) => line.includes("-----"))) return out;
  let inside = false;
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i] ?? "";
    if (inside) {
      inside = !END_LINE.test(line);
      if (inside) out.add(i);
    } else if (isPublicMarker(line, "BEGIN")) {
      inside = true;
    } else if (isPublicMarker(line, "END")) {
      base64Above(lines, i, out);
    }
  }
  return out;
}

/** An address with a token as the user (`https://<token>@host`): the mirror of `url_password` (Rust). */
const ADDRESS_USER = /:\/\/([^\s/?#"'\\@:]+)@/g;

/** Ranges of the words of a line (split at spaces | tabs). */
function wordRanges(line: string): Span[] {
  const out: Span[] = [];
  for (const m of line.matchAll(/[^ \t]+/g)) out.push({ from: m.index, to: m.index + m[0].length });
  return out;
}

/** A value nothing hides: empty, already hidden, a reference, a command. */
function isOpenValue(value: string): boolean {
  return value === "" || value === HIDDEN || isReference(value) || value.startsWith("$(");
}

/** The end of a quoted text that opens at `open`: its closing quote | the line end. */
function quoteEnd(line: string, open: number): number {
  const quote = line[open];
  for (let i = open + 1; i < line.length; i += 1) {
    if (line[i] === "\\") i += 1;
    else if (line[i] === quote) return i;
  }
  return line.length;
}

/** Short curl flags that take no value and may come before `u` | `U` in one word (`-su`, `-sSu`). */
const CURL_BARE_FLAGS = "sSkvLiIfgGNnqZ";

/** Where the value of a user flag starts in a curl word (`-u`, `-uadmin:pw`, `-su`, `-U`): `curl_short_user`. */
function curlShortUser(word: string): number | null {
  if (!word.startsWith("-") || word.startsWith("--")) return null;
  for (let i = 1; i < word.length; i += 1) {
    const c = word[i] as string;
    if (c === "u" || c === "U") return i + 1;
    if (!CURL_BARE_FLAGS.includes(c)) return null;
  }
  return null;
}

/** `curl -u user:password` and the other user flags (`--user=`, `-uuser:pw`, `-su`, `-U`, `--proxy-user`): `curl_spans`. */
function curlSpans(line: string): Span[] {
  const words = wordRanges(line);
  const text = (r: Span): string => line.slice(r.from, r.to);
  const first = words.findIndex((r) => {
    const w = text(r).toLowerCase();
    return w === "curl" || w === "curl.exe" || w.endsWith("/curl");
  });
  if (first < 0) return [];
  const out: Span[] = [];
  let i = first + 1;
  while (i < words.length) {
    const word = words[i] as Span;
    const w = text(word);
    let start: number;
    const prefix = ["--user=", "--proxy-user="].find((p) => w.startsWith(p));
    const short = curlShortUser(w);
    const attached =
      prefix !== undefined ? prefix.length : short !== null && short < w.length ? short : null;
    if (attached !== null) {
      start = word.from + attached;
    } else if (w === "--user" || w === "--proxy-user" || short !== null) {
      const next = words[i + 1];
      if (next === undefined) break;
      start = next.from;
    } else {
      i += 1;
      continue;
    }
    let from = start;
    let to: number;
    if (line[start] === '"' || line[start] === "'") {
      from = start + 1;
      to = quoteEnd(line, start);
    } else {
      // up to a space; a quote inside (`admin:"a b"`) runs to its closing quote
      let at = start;
      while (at < line.length) {
        const c = line[at];
        if (c === " " || c === "\t") break;
        at = c === '"' || c === "'" ? Math.min(quoteEnd(line, at) + 1, line.length) : at + 1;
      }
      to = at;
    }
    const colon = line.slice(from, to).indexOf(":");
    if (colon >= 0) {
      let pFrom = from + colon + 1;
      let pTo = to;
      const quote = line[pFrom];
      if (pTo > pFrom && (quote === '"' || quote === "'")) {
        const closed = pTo - pFrom >= 2 && line[pTo - 1] === quote;
        pFrom += 1;
        if (closed) pTo -= 1;
      }
      if (!isOpenValue(line.slice(pFrom, pTo))) out.push({ from: pFrom, to: pTo });
    }
    while (i < words.length && (words[i] as Span).from <= to) i += 1;
  }
  return out;
}

/** A `.netrc` line (`machine <host> login <name> password <value>`, or `password <value>`): the mirror of `netrc_spans`. */
function netrcSpans(line: string): Span[] {
  const words = wordRanges(line);
  const word = (i: number): string | undefined => {
    const r = words[i];
    return r === undefined ? undefined : line.slice(r.from, r.to);
  };
  type Value = { span: Span; next: number; quoted: boolean };
  // the value at word `i`: a quoted one inside its quotes
  const value = (i: number): Value | null => {
    const r = words[i];
    if (r === undefined) return null;
    if (line[r.from] !== '"' && line[r.from] !== "'")
      return { span: r, next: i + 1, quoted: false };
    const end = quoteEnd(line, r.from);
    const next = words.findIndex((w) => w.from > end);
    return {
      span: { from: r.from + 1, to: end },
      next: next < 0 ? words.length : next,
      quoted: true,
    };
  };
  // on a line that may be prose (`password reset`) a plain word is no value
  const hides = (v: Value, prose: boolean): boolean => {
    const text = line.slice(v.span.from, v.span.to);
    return !isOpenValue(text) && (v.quoted || !prose || !isPlainWord(text));
  };
  if (word(0) === "password") {
    const v = value(1);
    return v !== null && v.next === words.length && hides(v, true) ? [v.span] : [];
  }
  if (word(0) !== "machine" && word(0) !== "default") return [];
  const prose = word(0) === "default";
  const out: Span[] = [];
  let i = 0;
  for (let keyword = word(i); keyword !== undefined; keyword = word(i)) {
    if (keyword === "default") {
      i += 1;
    } else if (["machine", "login", "password", "account", "port"].includes(keyword)) {
      const found = value(i + 1);
      if (found === null) break;
      if (keyword === "password" && hides(found, prose)) out.push(found.span);
      i = found.next;
    } else if (keyword === "macdef") {
      break;
    } else {
      // not the netrc shape: prose that starts with "machine"
      return [];
    }
  }
  return out;
}

/** `setPassword("v")`, `set_api_key('v')`: the quoted arguments of a setter named after a secret key (`setter_spans`). */
function setterSpans(line: string): Span[] {
  const out: Span[] = [];
  for (const m of line.matchAll(/([\p{L}\p{N}_]+)\(/gu)) {
    const name = m[1] as string;
    const rest = name.startsWith("set") || name.startsWith("Set") ? name.slice(3) : null;
    if (rest === null) continue;
    let key: string;
    if (rest.startsWith("_")) key = rest.slice(1);
    else if (/^\p{Lu}/u.test(rest)) key = rest;
    // `setpassword(`: an all-lowercase name, the key whole
    else if (/^[a-z]+$/.test(rest)) key = rest;
    else continue;
    const cls = keyClass(key);
    if (cls === null) continue;
    let depth = 0;
    let at = m.index + m[0].length;
    while (at < line.length) {
      const c = line[at];
      if (c === "(") depth += 1;
      else if (c === ")") {
        if (depth === 0) break;
        depth -= 1;
      } else if (c === '"' || c === "'" || c === "`") {
        const end = quoteEnd(line, at);
        const value = line.slice(at + 1, end);
        if (depth === 0 && looksSecret(cls, key, value, true, line.slice(end + 1), line)) {
          out.push({ from: at + 1, to: end });
        }
        at = Math.max(end, at) + 1;
        continue;
      }
      at += 1;
    }
  }
  return out;
}

/** The cells (trimmed) of a Markdown table row (a line that starts with `|`); `null`: no table row. */
function tableCells(line: string): Span[] | null {
  const lead = line.length - line.trimStart().length;
  if (line[lead] !== "|") return null;
  const cells: Span[] = [];
  let start = lead + 1;
  for (let i = lead + 1; i < line.length; i += 1) {
    if (line[i] === "|" && line[i - 1] !== "\\") {
      cells.push({ from: start, to: i });
      start = i + 1;
    }
  }
  if (line.slice(start).trim() !== "") cells.push({ from: start, to: line.length });
  return cells.map((r) => {
    const cell = line.slice(r.from, r.to);
    const from = r.from + (cell.length - cell.trimStart().length);
    return { from, to: Math.max(r.from + cell.trimEnd().length, from) };
  });
}

/** A table's `|---|:-:|` row. */
function isSeparatorRow(line: string): boolean {
  const cells = tableCells(line);
  return (
    cells !== null &&
    cells.length > 0 &&
    cells.every((r) => /^-+$/.test(line.slice(r.from, r.to).replace(/^:+|:+$/g, "")))
  );
}

type CellKey = { cls: KeyClass; key: string };

/** A cell that names a secret (`DB_PASSWORD`, `**Password**`, `API key`, `PIN`): one or two words. */
function cellKey(cell: string): CellKey | null {
  const name = cell.replace(/^[`*\s]+|[`*\s]+$/gu, "");
  if (name === "" || name.split(/\s+/u).length > 2) return null;
  // `PIN` as written in a table (`pin` keys elsewhere pin versions)
  if (name.split(/\s+/u).at(-1) === "PIN") return { cls: "strong", key: name };
  const cls = keyClass(name);
  return cls === null ? null : { cls, key: name };
}

/** `label`: where a row's label may stand (right of a secret name, the first column under a secret header). */
type CellRule = { index: number; label: boolean } & CellKey;

/** Table rows (line index) with the cells to judge: the mirror of `table_rules` (Rust). */
function tableRules(lines: readonly string[]): Map<number, CellRule[]> {
  const out = new Map<number, CellRule[]>();
  if (!lines.some((l) => l.includes("|"))) return out;
  let header: (CellKey | null)[] | null = null;
  lines.forEach((line, i) => {
    const cells = tableCells(line);
    if (cells === null) {
      header = null;
      return;
    }
    if (isSeparatorRow(line)) return;
    const next = lines[i + 1];
    if (next !== undefined && isSeparatorRow(next)) {
      header = cells.map((r) => cellKey(line.slice(r.from, r.to)));
      return;
    }
    const rules: CellRule[] = [];
    cells.forEach((_, k) => {
      const column: CellKey | null = (header as (CellKey | null)[] | null)?.[k] ?? null;
      const prev = k > 0 ? cells[k - 1] : undefined;
      const left = prev === undefined ? null : cellKey(line.slice(prev.from, prev.to));
      const found = column ?? left;
      const label = column === null || (k === 0 && cells.length > 1);
      if (found !== null) rules.push({ index: k, label, ...found });
    });
    if (rules.length > 0) out.set(i, rules);
  });
  return out;
}

/** The cells of a table row that hold a secret value; backticks around a value stay (`cell_spans`). */
function cellSpans(line: string, rules: readonly CellRule[]): Span[] {
  const cells = tableCells(line);
  if (cells === null) return [];
  const out: Span[] = [];
  for (const rule of rules) {
    const cell = cells[rule.index];
    if (cell === undefined) continue;
    let { from, to } = cell;
    const text = line.slice(from, to);
    if (text.length >= 2 && text.startsWith("`") && text.endsWith("`")) {
      from += 1;
      to -= 1;
    }
    const value = line.slice(from, to);
    const typed = PRIMITIVE_TYPES.has(value.replace(/\?+$/, "").toLowerCase());
    if (
      /[\p{L}\p{N}]/u.test(value) &&
      !typed &&
      !(rule.label && isPlainWord(value)) &&
      looksSecret(rule.cls, rule.key, value, true, "", line)
    ) {
      out.push({ from, to });
    }
  }
  return out;
}

/** A YAML block scalar under a secret key (`password: |`, `- "api_key": >-  # note`). */
function isSecretBlockKey(line: string): boolean {
  let t = line.trim();
  if (t.startsWith("- ")) t = t.slice(2).trimStart();
  const colon = t.indexOf(":");
  if (colon < 0) return false;
  const key = t
    .slice(0, colon)
    .trim()
    .replace(/^["']+|["']+$/g, "");
  let rest = t.slice(colon + 1);
  const hash = rest.indexOf("#");
  if (hash >= 0) rest = rest.slice(0, hash);
  rest = rest.trim();
  return key !== "" && !/\s/u.test(key) && BLOCK_MARK.test(rest) && keyClass(key) !== null;
}

/** Lines of the body of a block scalar under a secret key: the mirror of `block_lines` (Rust). */
function blockLines(lines: readonly string[]): Set<number> {
  const out = new Set<number>();
  let open: number | null = null;
  lines.forEach((line, i) => {
    const indent = line.length - line.replace(/^[ \t]+/, "").length;
    if (open !== null) {
      if (line.trim() === "") return;
      if (indent > open) {
        out.add(i);
        return;
      }
      open = null;
    }
    if (isSecretBlockKey(line)) open = indent;
  });
  return out;
}

/** Whether a text can hold a secret at all (most text cannot: it is passed over at once). */
const HINT =
  /key|curl|secret|token|pass|pwd|credential|auth|:\/\/|sk-|sk_|rk_|gh[pousr]_|github_pat_|glpat-|akia|asia|aiza|xox|hf_|npm_|eyj/;

/** Spans (in `text`) of the secret values, in order, not overlapping. */
export function findCodeSecrets(text: string): Span[] {
  const rawLines = text.split("\n");
  const lines = rawLines.map((l) => (l.endsWith("\r") ? l.slice(0, -1) : l));
  // `PIN` only as written (a table header): `pin` is in too many words
  if (!HINT.test(text.toLowerCase()) && !text.includes("PIN") && !lines.some(isPemLine)) return [];
  const bodies = keyBodyLines(lines);
  const blocks = blockLines(lines);
  const tables = tableRules(lines);
  const publics = publicBlockLines(lines);
  const found: Span[] = [];
  const named = { value: false };
  let start = 0;
  lines.forEach((line, i) => {
    const base = start;
    start += (rawLines[i] ?? "").length + 1;
    if (bodies.has(i) || blocks.has(i) || (isPemLine(line) && !publics.has(i))) {
      named.value = false;
      const t = line.trim();
      if (t !== "" && t !== HIDDEN) {
        found.push({ from: base + line.length - line.trimStart().length, to: base + line.length });
      }
      return;
    }
    const rules = tables.get(i);
    for (const span of [
      ...pairSpans(line, named),
      ...curlSpans(line),
      ...netrcSpans(line),
      ...setterSpans(line),
      ...(rules === undefined ? [] : cellSpans(line, rules)),
    ]) {
      if (span.to > span.from && line.slice(span.from, span.to) !== HIDDEN) {
        found.push({ from: base + span.from, to: base + span.to });
      }
    }
    for (const match of line.matchAll(ADDRESS_USER)) {
      const user = match[1];
      if (user === undefined || !looksRandom(user)) continue;
      const from = base + match.index + 3;
      found.push({ from, to: from + user.length });
    }
    for (const match of line.matchAll(ADDRESS)) {
      const password = match[1];
      if (password === undefined || password === HIDDEN) continue;
      const from = base + match.index + match[0].length - 1 - password.length;
      found.push({ from, to: from + password.length });
    }
    for (const match of line.matchAll(TOKEN)) {
      const [, prefix, body] = match;
      if (prefix !== undefined && body !== undefined && !isRandom(prefix, body)) continue;
      found.push({ from: base + match.index, to: base + match.index + match[0].length });
    }
  });
  found.sort((a, b) => a.from - b.from || b.to - a.to);
  const out: Span[] = [];
  for (const span of found) {
    const last = out[out.length - 1];
    if (last === undefined || span.from >= last.to) out.push(span);
  }
  return out;
}

/** Lines (0-based) of certificates | public keys of a text held as lines: no mask there (their 64-char lines look like key lines). */
export function publicKeyLines(lines: readonly string[]): Set<number> {
  return publicBlockLines(lines.map((l) => (l.endsWith("\r") ? l.slice(0, -1) : l)));
}

/** Lines (0-based) of block scalar bodies under a secret key, of a text held as lines (masked whole, like a key body). */
export function secretBlockLines(lines: readonly string[]): Set<number> {
  return blockLines(lines.map((l) => (l.endsWith("\r") ? l.slice(0, -1) : l)));
}

/** Secret cell spans of the Markdown table rows of a text held as lines (line 0-based -> spans in that line). */
export function tableSecretSpans(lines: readonly string[]): Map<number, Span[]> {
  const held = lines.map((l) => (l.endsWith("\r") ? l.slice(0, -1) : l));
  const out = new Map<number, Span[]>();
  for (const [i, rules] of tableRules(held)) {
    const spans = cellSpans(held[i] ?? "", rules);
    if (spans.length > 0) out.set(i, spans);
  }
  return out;
}

/** Lines (0-based) inside private key blocks of a text held as lines (a viewer that masks line by line). */
export function privateKeyLines(lines: readonly string[]): Set<number> {
  return keyBodyLines(lines.map((l) => (l.endsWith("\r") ? l.slice(0, -1) : l)));
}
