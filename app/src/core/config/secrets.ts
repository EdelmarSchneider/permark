// Secrets on screen: what a hidden value looks like, and which values of a loose text (a code block) are secret.
// Config files get their secret spans from the backend (the structure of the format); text without a format
// (code in a Markdown document) uses the same key rule here: a secret key word followed by `=` | `:` hides the value.

/** What a hidden value shows. Fixed: the length of the value is not given away. */
export const MASK = "••••••";

const SECRET_WORDS = [
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
const HARMLESS = [
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

/** Whether a key names a secret (`API_KEY`, `client-secret`, `authToken`, `db.password`); `authors` does not. */
export function isSecretKey(key: string): boolean {
  let flat = key.toLowerCase().replace(/[^\p{L}\p{N}]/gu, "");
  for (const word of HARMLESS) flat = flat.split(word).join("");
  return SECRET_WORDS.some((word) => flat.includes(word));
}

export type Span = { from: number; to: number };

/** `${VAR}`, `$VAR`, `{{ secrets.NAME }}`: a pointer to a secret, not the secret. */
function isReference(value: string): boolean {
  const t = value.trim();
  return (
    /^\$\{\{?[\w .:-]+\}?\}$/.test(t) ||
    /^\{\{[\w .:-]+\}\}$/.test(t) ||
    /^\$[\p{L}\p{N}_]+$/u.test(t)
  );
}

// key, optional closing quote and spaces, the separator, spaces, then the value
const PAIR =
  /(?<![\w.-])((?:[\p{L}\p{N}_.-])+)(["']?[ \t]*)(:=|=|:)(?!:)([ \t]*)(\$?\{\{[^}\n]*\}\}|\$\{[^}\n]*\}|"(?:[^"\\\n]|\\.)*"?|'[^'\n]*'?|`[^`\n]*`?|\[[^\]\n]*\]?|[^\s,;]+)/gu;

/** The address `scheme://user:password@host`: where the password is. */
const ADDRESS = /:\/\/[^\s/?#"'\\@]*?:([^\s/?#"'\\@]+)@/g;

/** Spans (in `text`) of the values of secret keys and of the passwords in addresses, in order, not overlapping. */
export function findCodeSecrets(text: string): Span[] {
  const lower = text.toLowerCase();
  if (!SECRET_WORDS.some((w) => lower.includes(w)) && !text.includes("://")) return [];
  const found: Span[] = [];
  for (const match of text.matchAll(PAIR)) {
    const [whole, key, , , gap, value] = match;
    if (key === undefined || value === undefined || gap === undefined) continue;
    if (!isSecretKey(key) || value.startsWith("{")) continue;
    const start = match.index + whole.length - value.length;
    let from = start;
    let to = start + value.length;
    if (/^["'`]/.test(value)) {
      from += 1;
      if (value.length > 1 && value.endsWith(value[0] ?? "")) to -= 1;
    } else if (value.startsWith("[")) {
      from += 1;
      if (value.endsWith("]")) to -= 1;
    }
    const inner = text.slice(from, to);
    if (to > from && inner !== "<hidden>" && !isReference(inner)) found.push({ from, to });
  }
  for (const match of text.matchAll(ADDRESS)) {
    const password = match[1];
    if (password === undefined) continue;
    const from = match.index + match[0].length - 1 - password.length;
    found.push({ from, to: from + password.length });
  }
  found.sort((a, b) => a.from - b.from);
  const out: Span[] = [];
  for (const span of found) {
    const last = out[out.length - 1];
    if (last === undefined || span.from >= last.to) out.push(span);
  }
  return out;
}
