// Mirror of the `word-count` crate (same steps, same fixtures: `crates/plugins/word-count/fixtures.json`): live counts
// while typing, without a round trip. See the crate's header for the rules.

export type Counts = { words: number; cjk: number; characters: number };

const WORDS_PER_MINUTE = 230;
const CJK_PER_MINUTE = 500;

export function minutes(counts: Counts): number {
  const latin = counts.words - counts.cjk;
  const scaled = latin * CJK_PER_MINUTE + counts.cjk * WORDS_PER_MINUTE;
  return Math.ceil(scaled / (WORDS_PER_MINUTE * CJK_PER_MINUTE));
}

function isCjk(code: number): boolean {
  return (
    (code >= 0x3040 && code <= 0x30ff) ||
    (code >= 0x31f0 && code <= 0x31ff) ||
    (code >= 0x3400 && code <= 0x4dbf) ||
    (code >= 0x4e00 && code <= 0x9fff) ||
    (code >= 0xac00 && code <= 0xd7af) ||
    (code >= 0xf900 && code <= 0xfaff) ||
    (code >= 0x20000 && code <= 0x2fa1f)
  );
}

// Rust `char::is_alphanumeric` = Alphabetic | Numeric
const WORD = /^[\p{Alphabetic}\p{N}̀-ͯ]$/u;

function isWordChar(ch: string): boolean {
  return WORD.test(ch);
}

export function count(text: string): Counts {
  const counts: Counts = { words: 0, cjk: 0, characters: 0 };
  const chars = Array.from(text);
  let inWord = false;
  for (let i = 0; i < chars.length; i++) {
    const ch = chars[i] ?? "";
    if (ch !== "\n" && ch !== "\r") counts.characters += 1;
    const code = ch.codePointAt(0) ?? 0;
    if (isCjk(code)) {
      counts.words += 1;
      counts.cjk += 1;
      inWord = false;
    } else if (isWordChar(ch)) {
      if (!inWord) {
        counts.words += 1;
        inWord = true;
      }
    } else if ((ch === "'" || ch === "’") && inWord) {
      const next = chars[i + 1];
      if (!(next !== undefined && isWordChar(next) && !isCjk(next.codePointAt(0) ?? 0)))
        inWord = false;
    } else {
      inWord = false;
    }
  }
  return counts;
}

function fenceOf(line: string): { ch: string; n: number } | null {
  const indent = line.length - line.replace(/^ +/, "").length;
  if (indent > 3) return null;
  const rest = line.slice(indent);
  const ch = rest[0];
  if (ch !== "`" && ch !== "~") return null;
  let n = 0;
  while (rest[n] === ch) n += 1;
  return n >= 3 ? { ch, n } : null;
}

function closesFence(line: string, open: { ch: string; n: number }): boolean {
  const fence = fenceOf(line);
  if (!fence || fence.ch !== open.ch || fence.n < open.n) return false;
  return line.replace(/^ +/, "").slice(fence.n).trim() === "";
}

function isDefinition(line: string): boolean {
  const indent = line.length - line.replace(/^ +/, "").length;
  if (indent > 3) return false;
  const rest = line.slice(indent);
  const at = rest.indexOf("]:");
  return rest.startsWith("[") && at > 1;
}

const trimStart = (s: string): string => s.replace(/^[ \t]+/, "");

function stripLineStart(line: string): string {
  let s = trimStart(line);
  for (;;) {
    if (s.startsWith(">")) {
      s = trimStart(s.slice(1));
      continue;
    }
    let hashes = 0;
    while (s[hashes] === "#") hashes += 1;
    if (hashes >= 1 && hashes <= 6) {
      const rest = s.slice(hashes);
      if (rest === "" || rest.startsWith(" ") || rest.startsWith("\t")) {
        s = trimStart(rest);
        continue;
      }
    }
    if ((s[0] === "-" || s[0] === "*" || s[0] === "+") && (s[1] === " " || s[1] === "\t")) {
      s = trimStart(s.slice(1));
      continue;
    }
    let digits = 0;
    while (digits < s.length && s.charCodeAt(digits) >= 48 && s.charCodeAt(digits) <= 57)
      digits += 1;
    if (digits >= 1 && digits <= 9) {
      const rest = s.slice(digits);
      if ((rest[0] === "." || rest[0] === ")") && (rest[1] === " " || rest[1] === "\t")) {
        s = trimStart(rest.slice(1));
        continue;
      }
    }
    for (const box of ["[ ]", "[x]", "[X]"]) {
      if (s.startsWith(box)) {
        const rest = s.slice(box.length);
        if (rest.startsWith(" ") || rest.startsWith("\t")) return trimStart(rest);
      }
    }
    return s;
  }
}

function stripInline(line: string, out: string[]): void {
  const chars = Array.from(line);
  let i = 0;
  const indexOf = (ch: string, from: number): number => {
    for (let j = from; j < chars.length; j++) if (chars[j] === ch) return j;
    return -1;
  };
  while (i < chars.length) {
    const c = chars[i] ?? "";
    if (c === "`") {
      let n = 0;
      while (chars[i + n] === "`") n += 1;
      let close = -1;
      for (let j = i + n; j < chars.length; j++) {
        if (chars[j] !== "`") continue;
        let m = 0;
        while (chars[j + m] === "`") m += 1;
        if (m === n && (j === 0 || chars[j - 1] !== "`")) {
          close = j;
          break;
        }
      }
      if (close >= 0) i = close + n;
      else {
        out.push(...chars.slice(i, i + n));
        i += n;
      }
    } else if (c === "<") {
      const close = indexOf(">", i + 1);
      const next = chars[i + 1];
      const tagLike =
        next !== undefined && (/^[A-Za-z]$/.test(next) || next === "/" || next === "!");
      if (close >= 0 && tagLike) i = close + 1;
      else {
        out.push(c);
        i += 1;
      }
    } else if (c === "[") {
      const end = indexOf("]", i + 1);
      if (end < 0) {
        out.push(c);
        i += 1;
        continue;
      }
      const inner = chars.slice(i + 1, end).join("");
      if (inner.startsWith("^")) {
        i = end + 1;
        continue;
      }
      const after = chars[end + 1];
      const closer = after === "(" ? ")" : after === "[" ? "]" : null;
      const close = closer === null ? -1 : indexOf(closer, end + 2);
      if (closer !== null && close >= 0) {
        stripInline(inner, out);
        i = close + 1;
      } else {
        out.push(c);
        i += 1;
      }
    } else {
      out.push(c);
      i += 1;
    }
  }
}

export function stripMarkdown(text: string): string {
  const lines = text.split("\n").map((l) => (l.endsWith("\r") ? l.slice(0, -1) : l));
  let start = 0;
  const first = lines[0]?.trimEnd();
  if (first === "---" || first === "+++") {
    const close = lines
      .slice(1)
      .findIndex((l) => l.trimEnd() === first || (first === "---" && l.trimEnd() === "..."));
    if (close >= 0) start = close + 2;
  }
  const out: string[] = [];
  let fence: { ch: string; n: number } | null = null;
  for (const line of lines.slice(start)) {
    if (fence) {
      if (closesFence(line, fence)) fence = null;
      continue;
    }
    const open = fenceOf(line);
    if (open) {
      fence = open;
      continue;
    }
    if (isDefinition(line)) continue;
    stripInline(stripLineStart(line), out);
    out.push("\n");
  }
  return out.join("");
}

export function measure(text: string, kind: "markdown" | "plain"): Counts {
  return count(kind === "markdown" ? stripMarkdown(text) : text);
}
