// Secrets on screen: what a hidden value looks like, and which values of a loose text (a code block, a code file) are
// secret. Config files get their secret spans from the backend (the structure of the format); text without a format
// uses the rule here, the mirror of `secrets::mask_text` (Rust): a key whose name ends in a secret word, followed by
// `=` | `:` and a value that looks like a secret; the password of an address; known token formats wherever they are;
// the body of a private key block. Both read the cases of `testdata/secrets/cases.json`.

/** What a hidden value shows. Fixed: the length of the value is not given away. */
export const MASK = "••••••";

/** What the backend writes for a hidden value; never hidden twice. */
const HIDDEN = "<hidden>";

/** Last words of a key that name a secret (`DB_PASSWORD`, `clientSecret`, `GITHUB_TOKEN`, `basic_auth`). */
const SECRET_WORDS = new Set([
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
]);
/** Ends of a run-together last word that name a secret (`dbpassword`, `apitoken`, `clientsecret`). */
const SECRET_ENDINGS = ["password", "passwd", "secret", "token", "tokens"];
/** Words before `key` that make it a secret (`api_key`, `privateKey`, `SIGNING_KEY`). */
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
/** Words before `KEY` in an env-style name (`STRIPE_KEY` is secret) that keep it ordinary (`PUBLIC_KEY`, `CACHE_KEY`). */
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
/** Words after a secret word that only say which one (`SECRET_KEY_BASE`, `DB_PASSWORD_PROD`, `password_hash`). */
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
]);
/** Keys that hold a count when their value is a number (`tokens: 1200`); `api_tokens` is never one. */
const COUNT_KEYS = ["tokens", "ntokens"];
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

/** A number under a count key (`tokens: 1200`): a size, not a secret. */
export function isCount(key: string, value: string): boolean {
  const flat = key.toLowerCase().replace(/[^\p{L}\p{N}]/gu, "");
  const v = value.trim();
  return COUNT_KEYS.includes(flat) && v !== "" && Number.isFinite(Number(v));
}

/**
 * Whether a key names a secret: its last word (after `_prod`, `_base`, ... are set aside) is a secret word
 * (`DB_PASSWORD`, `clientSecret`, `authToken`, `basic_auth`), or it is `key` after a word that makes it one
 * (`api_key`, `privateKey`; env-style `STRIPE_KEY`). `keys`, `keydown`, `keyJump`, `KeyW`, `monkey`, `tokenizer`,
 * `max_tokens`, `token_count`, `authorName`, `secretName`, `PASSWORD_FILE` are not.
 */
export function isSecretKey(name: string): boolean {
  const words = keyWords(name);
  while (words.length > 1 && TAIL_WORDS.has(words[words.length - 1] ?? "")) words.pop();
  const last = words[words.length - 1];
  const prev = words[words.length - 2];
  if (last === undefined) return false;
  if (last === "key" || last === "keys") {
    if (prev === undefined) return false;
    if (KEY_QUALIFIERS.has(prev)) return true;
    return last === "key" && !/\p{Lowercase}/u.test(name) && !PLAIN_KEY_QUALIFIERS.has(prev);
  }
  if (prev !== undefined && SECRET_WORDS.has(prev + last)) return true;
  if (last === "token" || last === "tokens") return prev === undefined || !COUNT_WORDS.has(prev);
  if (SECRET_WORDS.has(last)) return true;
  for (const ending of SECRET_ENDINGS) {
    if (last.length > ending.length && last.endsWith(ending)) {
      return !(ending.startsWith("token") && COUNT_WORDS.has(last.slice(0, -ending.length)));
    }
  }
  return false;
}

export type Span = { from: number; to: number };

/** `${VAR}`, `$VAR`, `{{ secrets.NAME }}`: a pointer to a secret, not the secret. */
function isReference(value: string): boolean {
  const t = value.trim();
  return t.startsWith("$") || (t.startsWith("{{") && t.endsWith("}}"));
}

/** Letters, digits and nothing else, lowercase: `API key` and `apiKey` read the same. */
function flat(text: string): string {
  return text.toLowerCase().replace(/[^\p{L}\p{N}]/gu, "");
}

const NUMBER = /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/;
const COLOR = /^#([0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;
/** Words with an end mark: a value made only of these, with a space, is a sentence (`Password is required.`). */
const PROSE_WORD = /^[\p{L}'’]+[.,!?:;]?$/u;
/** `process.env.TOKEN`, `this.key`, `options.auth`: where a value is, not the value. */
const MEMBER = /^[\p{L}_$]+(\.[\p{L}_$]+)+$/u;
const IDENT = /^[\p{L}_$][\p{L}\p{N}_$]*$/u;

/** Secret only as an env-style `<WORD>_KEY` (`STRIPE_KEY`): such a key may name a slot (`STORAGE_KEY = "app.theme"`). */
function isWeakKey(key: string): boolean {
  const words = keyWords(key);
  while (words.length > 1 && TAIL_WORDS.has(words[words.length - 1] ?? "")) words.pop();
  return words[words.length - 1] === "key" && !KEY_QUALIFIERS.has(words[words.length - 2] ?? "");
}

/**
 * A variable named like a secret (`accessToken`, `API_KEY`, `newPassword`): two words or more, no digits. A single
 * word (`mysecretpassword`) or one with digits (`abc123secret`) is a value.
 */
function isVariable(value: string): boolean {
  return !/\p{N}/u.test(value) && keyWords(value).length > 1 && isSecretKey(value);
}

/** Lowercase words joined by `.`, `-`, `_`, `:` or `/`: a name (`app.theme`, `user-cache`), not a secret. */
const SLUG = /^[a-z][a-z0-9]*([._:/-][a-z0-9]+)+$/;
/** A YAML block scalar mark (`key: |`): the value is on the next lines. */
const BLOCK_MARK = /^[|>][+-]?[0-9]?$/;

/**
 * Whether the value of a secret key looks like a secret: not empty, hidden, a reference, a color, a sentence, the
 * key's own name (`password: "Password"`), a slot name under an env-style key; unquoted, also not a bool | null |
 * number, a block mark, a YAML tag (`!Ref`), an expression (`getToken()`, `new X`, `env["X"]`, `process.env.X`), a
 * variable named like a secret (`token: accessToken`) or ending a statement (`token = t;`), or a word starting a
 * sentence.
 */
function looksSecret(key: string, value: string, quoted: boolean, after: string): boolean {
  if (value === "" || value === HIDDEN || isReference(value) || COLOR.test(value)) return false;
  if (flat(value) === flat(key) || (SLUG.test(value) && isWeakKey(key))) return false;
  if (quoted) {
    const parts = value.split(/\s+/u).filter((p) => p !== "");
    return !(parts.length > 1 && parts.every((p) => PROSE_WORD.test(p)));
  }
  if (CODE_WORDS.has(value.toLowerCase()) || NUMBER.test(value) || BLOCK_MARK.test(value))
    return false;
  // `!Ref x`, `&anchor`, `*alias`: YAML tags and pointers
  if (/[(["']/.test(value) || /^[!&*]/.test(value) || MEMBER.test(value)) return false;
  if (IDENT.test(value) && (isVariable(value) || after.startsWith(";"))) return false;
  // a word followed by more words: a sentence (`The password: must be long`); `hunter2 is rotated` stays secret
  return !(/^\p{Alphabetic}+$/u.test(value) && /^[ \t]+[\p{L}\p{N}]/u.test(after));
}

// key, optional closing quote and spaces, the separator (`==`, `===`, `=>`, `::` are not one), spaces, the value
const PAIR =
  /(?<![\p{L}\p{N}_.-])([\p{L}\p{N}_.-]+)(["']?[ \t]*)(:=|=(?![=>])|:(?!:))([ \t]*)(\$?\{\{[^}\n]*\}\}|\$\{[^}\n]*\}|"(?:[^"\\\r\n]|\\.)*"?|'(?:[^'\\\r\n]|\\.)*'?|`(?:[^`\\\r\n]|\\.)*`?|\[[^\]\r\n]*\]?|[^\s,;)\]}]+)/gu;

/** Keys whose value names a variable (`- name: DB_PASSWORD`), and keys that then hold its value (`value: x`). */
const NAME_KEYS = new Set(["name", "key", "env", "var", "variable"]);
const VALUE_KEYS = new Set(["value", "val", "default"]);

/** The value of a quoted | unquoted token, quotes off. */
function unquote(value: string): string {
  return /^["'`]/.test(value)
    ? value.slice(1, value.endsWith(value[0] ?? "") ? -1 : undefined)
    : value;
}

/** Whether a quoted value has its closing quote (one not escaped by a backslash). */
function isClosed(value: string): boolean {
  if (value.length < 2 || !value.endsWith(value[0] ?? "")) return false;
  let slashes = 0;
  while (value[value.length - 2 - slashes] === "\\" && value.length - 2 - slashes > 0) slashes += 1;
  return slashes % 2 === 0;
}

/** Spans of the values of secret keys: `key <sep> value` pairs, a key's own value or the `value` after its `name`. */
function pairSpans(text: string): Span[] {
  const found: Span[] = [];
  let line = 0; // line of the last match
  let counted = 0; // offset up to which `line` counts the line breaks
  let named = -2; // line of the last `name: <secret key>`
  PAIR.lastIndex = 0;
  for (let match = PAIR.exec(text); match !== null; match = PAIR.exec(text)) {
    const [whole, key, , , gap, value] = match;
    if (key === undefined || value === undefined || gap === undefined) continue;
    for (; counted < match.index; counted += 1) if (text[counted] === "\n") line += 1;
    const lowered = key.toLowerCase();
    if (NAME_KEYS.has(lowered) && isSecretKey(unquote(value))) named = line;
    const keySecret = isSecretKey(key) || (VALUE_KEYS.has(lowered) && line - named <= 1);
    if (!keySecret) {
      // only the key is used up: a value may hold a pair of its own (`x: password=abc`)
      PAIR.lastIndex = match.index + key.length;
      continue;
    }
    const start = match.index + whole.length - value.length;
    if (value.startsWith("{")) {
      // a nested table: its own keys are looked at one by one
      PAIR.lastIndex = start + 1;
      continue;
    }
    let from = start;
    let to = start + value.length;
    const quoted = /^["'`]/.test(value);
    if (quoted) {
      from += 1;
      if (isClosed(value)) to -= 1;
      if (value.startsWith("`") && value.includes("${")) continue;
    } else if (value.startsWith("[")) {
      from += 1;
      if (value.endsWith("]")) to -= 1;
    }
    const inner = text.slice(from, to);
    if (isCount(key, inner)) continue;
    const lineEnd = text.indexOf("\n", start);
    const after = text.slice(start + value.length, lineEnd < 0 ? text.length : lineEnd);
    const list = value.startsWith("[");
    if (list ? inner.trim() !== "" && inner !== HIDDEN : looksSecret(key, inner, quoted, after)) {
      found.push({ from, to });
    }
  }
  return found;
}

/** The address `scheme://user:password@host`: where the password is. */
const ADDRESS = /:\/\/[^\s/?#"'\\@]*?:([^\s/?#"'\\@]+)@/g;

/** Known token formats, secret whatever their key: OpenAI | Anthropic, Stripe, GitHub, GitLab, AWS, Google, Slack, Hugging Face, npm, JWT. */
const TOKEN =
  /(?<![\p{L}\p{N}_-])(?:(sk-|sk_live_|sk_test_|rk_live_|rk_test_|ghp_|gho_|ghu_|ghs_|ghr_|github_pat_|glpat-|hf_|npm_)([A-Za-z0-9_-]+)|(?:AKIA|ASIA)[A-Z0-9]{16}|AIza[A-Za-z0-9_-]{35}|xox[abprs]-[A-Za-z0-9-]{10,}|eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,})(?![A-Za-z0-9_-])/gu;

/** The random part of a prefixed token: 20+ characters with upper, lower case and digits (a slug is not one). */
function isRandom(body: string): boolean {
  return body.length >= 20 && /[A-Z]/.test(body) && /[a-z]/.test(body) && /[0-9]/.test(body);
}

/** Lines of a `-----BEGIN ... PRIVATE KEY-----` block: from the line after it to the `-----END` line (or the end). */
const PRIVATE_KEY =
  /-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----[^\n]*\n([\s\S]*?)(?=^[ \t]*-----END|(?![\s\S]))/gm;

/** Spans of the body lines of private key blocks: each line from its first non-space character, `\r` left out. */
function privateKeySpans(text: string): Span[] {
  const out: Span[] = [];
  for (const match of text.matchAll(PRIVATE_KEY)) {
    const body = match[1] ?? "";
    let at = match.index + match[0].length - body.length;
    for (const line of body.split("\n")) {
      const content = line.endsWith("\r") ? line.slice(0, -1) : line;
      const lead = content.length - content.trimStart().length;
      if (content.trim() !== "" && content.trim() !== HIDDEN) {
        out.push({ from: at + lead, to: at + content.length });
      }
      at += line.length + 1;
    }
  }
  return out;
}

/** Whether a text can hold a secret at all (most text cannot: it is passed over at once). */
const HINT =
  /key|secret|token|pass|pwd|credential|auth|:\/\/|sk-|sk_|rk_|gh[pousr]_|github_pat_|glpat-|akia|asia|aiza|xox|hf_|npm_|eyj/;

/** Spans (in `text`) of the secret values, in order, not overlapping. */
export function findCodeSecrets(text: string): Span[] {
  if (!HINT.test(text.toLowerCase())) return [];
  const found = pairSpans(text);
  for (const match of text.matchAll(ADDRESS)) {
    const password = match[1];
    if (password === undefined || password === HIDDEN) continue;
    const from = match.index + match[0].length - 1 - password.length;
    found.push({ from, to: from + password.length });
  }
  for (const match of text.matchAll(TOKEN)) {
    const body = match[2];
    if (body !== undefined && !isRandom(body)) continue;
    found.push({ from: match.index, to: match.index + match[0].length });
  }
  if (text.includes("PRIVATE KEY-----")) found.push(...privateKeySpans(text));
  found.sort((a, b) => a.from - b.from || b.to - a.to);
  const out: Span[] = [];
  for (const span of found) {
    const last = out[out.length - 1];
    if (last === undefined || span.from >= last.to) out.push(span);
  }
  return out;
}

/** Lines (0-based) inside private key blocks of a text held as lines (a viewer that masks line by line). */
export function privateKeyLines(lines: readonly string[]): Set<number> {
  const out = new Set<number>();
  let inside = false;
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i] ?? "";
    if (inside) {
      if (/^[ \t]*-----END/.test(line)) inside = false;
      else if (line.trim() !== "") out.add(i);
    } else if (
      line.includes("PRIVATE KEY-----") &&
      /-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----/.test(line)
    ) {
      inside = true;
    }
  }
  return out;
}
