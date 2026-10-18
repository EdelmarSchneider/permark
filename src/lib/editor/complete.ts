// Completions that need no model: files as link paths, headings as fragments, and words already
// written in the document (plus names of files). The search index keeps stemmed forms, which would
// insert the wrong spelling, so words come from the text itself.

import { basename, dirname, isMarkdown, resolveRelative, splitWorktree } from "../paths";

const LIMIT = 8;

export type CompleteItem = {
  kind: "file" | "heading" | "word";
  /** What the list shows. */
  label: string;
  /** What replaces the range being completed. */
  insert: string;
  /** A quieter second column, or "" when the label says enough. */
  detail: string;
};

export type Context =
  | { kind: "link"; from: number; to: number; query: string; bracketed: boolean }
  | { kind: "word"; from: number; to: number; query: string };

const SCHEME = /^[a-z][a-z0-9+.-]*:/i;

/** A path the open folder uses (not a loose file's absolute path, and not a web address). */
export function isVaultPath(path: string): boolean {
  return (
    path !== "" && !/^[a-z]:[\\/]/i.test(path) && !path.startsWith("/") && !path.startsWith("\\")
  );
}

function fold(text: string): string {
  return text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

function isCjk(char: string): boolean {
  const code = char.codePointAt(0) ?? 0;
  return (
    (code >= 0x3005 && code <= 0x3007) ||
    (code >= 0x3041 && code <= 0x3096) ||
    (code >= 0x30a1 && code <= 0x30ff) ||
    (code >= 0x31f0 && code <= 0x31ff) ||
    (code >= 0x3400 && code <= 0x4dbf) ||
    (code >= 0x4e00 && code <= 0x9fff) ||
    (code >= 0xf900 && code <= 0xfaff)
  );
}

/**
 * The fragment a heading gets in the rendered page, so a completed link lands on it.
 * "Café & Chá" is "café--chá": spaces become hyphens and other punctuation drops out.
 */
export function headingSlug(text: string): string {
  return text
    .trim()
    .toLowerCase()
    .replace(/\s+/g, "-")
    .replace(/[^\p{L}\p{N}_-]/gu, "");
}

/** Headings in order, with the suffix the page adds when two would share a fragment. */
export function headingIds(headings: readonly string[]): { text: string; id: string }[] {
  const seen = new Map<string, number>();
  const out: { text: string; id: string }[] = [];
  for (const text of headings) {
    const base = headingSlug(text);
    if (base === "") continue;
    const n = seen.get(base) ?? 0;
    seen.set(base, n + 1);
    out.push({ text, id: n === 0 ? base : `${base}-${n}` });
  }
  return out;
}

/** The link path from `fromFile` to `toFile`, or null when they are not in the same tree. */
export function relativeLink(fromFile: string, toFile: string): string | null {
  if (!isVaultPath(fromFile) || !isVaultPath(toFile)) return null;
  const from = splitWorktree(fromFile);
  const to = splitWorktree(toFile);
  if (from.worktree !== to.worktree || to.rel === "" || to.rel === from.rel) return null;
  const fromParts = dirname(from.rel).split("/").filter(Boolean);
  const toParts = to.rel.split("/").filter(Boolean);
  let shared = 0;
  while (
    shared < fromParts.length &&
    shared < toParts.length &&
    fromParts[shared] === toParts[shared]
  ) {
    shared++;
  }
  const parts = [...Array<string>(fromParts.length - shared).fill(".."), ...toParts.slice(shared)];
  const rel = parts.join("/");
  return rel === "" ? null : rel;
}

function needsBrackets(path: string): boolean {
  return /[\s()<>]/.test(path);
}

function remember(surface: string, into: Map<string, string>): void {
  const key = fold(surface);
  if (key !== "" && !into.has(key)) into.set(key, surface);
}

function addToken(token: string, into: Map<string, string>): void {
  const chars = [...token];
  if (chars.length >= 2 && chars.every(isCjk)) {
    for (let i = 0; i < chars.length - 1; i++) remember(chars.slice(i, i + 2).join(""), into);
    return;
  }
  if (chars.length >= 2 && chars.some((char) => /\p{L}/u.test(char))) remember(token, into);
}

/** Words of a Markdown text, in first-seen order, skipping fenced and inline code. */
export function wordsOf(markdown: string): string[] {
  const into = new Map<string, string>();
  let fence: string | null = null;
  for (const line of markdown.split("\n")) {
    const marker = /^ {0,3}(`{3,}|~{3,})/.exec(line);
    if (marker) {
      const ch = marker[1][0];
      fence = fence === null ? ch : fence === ch ? null : fence;
      continue;
    }
    if (fence !== null) continue;
    const prose = stripInlineCode(line)
      .replace(/\]\(<[^>\n]*>\)/g, " ")
      .replace(/\]\([^)\s\n]*\)/g, " ");
    for (const match of prose.matchAll(/[\p{L}\p{N}_]+/gu)) addToken(match[0], into);
  }
  return [...into.values()];
}

/** Words of plain text (a visual document already left its code blocks out). */
export function collectWords(text: string): string[] {
  const into = new Map<string, string>();
  for (const match of text.matchAll(/[\p{L}\p{N}_]+/gu)) addToken(match[0], into);
  return [...into.values()];
}

function stripInlineCode(line: string): string {
  let out = "";
  let i = 0;
  while (i < line.length) {
    if (line[i] === "`") {
      const ticks = /^`+/.exec(line.slice(i))?.[0] ?? "`";
      const close = line.indexOf(ticks, i + ticks.length);
      if (close < 0) break;
      out += " ";
      i = close + ticks.length;
      continue;
    }
    out += line[i];
    i++;
  }
  return out;
}

/** Pieces of file names, so a document can complete a name it has not used yet. */
export function fileWords(paths: readonly string[]): string[] {
  const into = new Map<string, string>();
  for (const path of paths) {
    const stem = basename(path).replace(/\.[^.]+$/, "");
    for (const part of stem.split(/[-_\s]+/)) {
      if ([...part].length >= 2 && /\p{L}/u.test(part)) remember(part, into);
    }
  }
  return [...into.values()];
}

function enough(query: string, explicit: boolean): boolean {
  const chars = [...query];
  if (chars.length >= 2) return true;
  return chars.length === 1 && (explicit || isCjk(chars[0]));
}

/**
 * The word touching the caret. `before` is the text immediately before it and `after` immediately
 * after. CJK text has no spaces, so only the last few characters are the query.
 */
export function wordAt(
  before: string,
  after: string,
  explicit = false,
): { query: string; before: number; after: number } | null {
  const match = /[\p{L}\p{N}_]+$/u.exec(before);
  if (!match) return null;
  const chars = [...match[0]];
  const cjk = chars.some(isCjk);
  const query = cjk ? chars.slice(-8).join("") : match[0];
  if (!enough(query, explicit)) return null;
  let takeAfter = 0;
  if (!cjk) {
    const rest = /^[\p{L}\p{N}_]+/u.exec(after);
    if (rest && ![...rest[0]].some(isCjk)) takeAfter = rest[0].length;
  }
  return { query, before: query.length, after: takeAfter };
}

function inFence(text: string, pos: number): boolean {
  let fence: string | null = null;
  let i = 0;
  while (i < pos) {
    const end = text.indexOf("\n", i);
    if (end < 0 || end >= pos) break;
    const line = text.slice(i, end);
    const marker = /^ {0,3}(`{3,}|~{3,})/.exec(line);
    if (marker) {
      const ch = marker[1][0];
      fence = fence === null ? ch : fence === ch ? null : fence;
    }
    i = end + 1;
  }
  return fence !== null;
}

function inlineCode(line: string, col: number): boolean {
  let open = false;
  for (let i = 0; i < col; i++) {
    if (line[i] === "\\" && line[i + 1] === "`") {
      i++;
      continue;
    }
    if (line[i] === "`") open = !open;
  }
  return open;
}

type Dest = { from: number; to: number; bracketed: boolean; openerAt: number };

function endOf(line: string, contentFrom: number, bracketed: boolean): number {
  if (bracketed) {
    const close = line.indexOf(">", contentFrom);
    return close < 0 ? line.length : close;
  }
  let i = contentFrom;
  while (i < line.length && line[i] !== ")" && !/\s/.test(line[i])) i++;
  return i;
}

function inlineDest(line: string, col: number): Dest | null {
  let searchTo = col;
  while (searchTo > 0) {
    const before = line.slice(0, searchTo);
    const angle = before.lastIndexOf("](<");
    const plain = before.lastIndexOf("](");
    const at = Math.max(angle, plain);
    if (at < 0) return null;
    if (at > 0 && line[at - 1] === "\\") {
      searchTo = at;
      continue;
    }
    const bracketed = line.startsWith("](<", at);
    const contentFrom = at + (bracketed ? 3 : 2);
    const between = line.slice(contentFrom, col);
    if (bracketed ? between.includes(">") : /[)\s]/.test(between)) {
      searchTo = at;
      continue;
    }
    if (inlineCode(line, at)) {
      searchTo = at;
      continue;
    }
    const to = endOf(line, contentFrom, bracketed);
    if (col > to) {
      searchTo = at;
      continue;
    }
    return { from: contentFrom, to, bracketed, openerAt: at };
  }
  return null;
}

function defDest(line: string, col: number): Dest | null {
  const def = /^ {0,3}\[[^\]\n]+\]:[ \t]*/.exec(line);
  if (!def || col < def[0].length) return null;
  const start = def[0].length;
  if (line[start] === "<") {
    const close = line.indexOf(">", start + 1);
    const to = close < 0 ? line.length : close;
    if (col < start + 1 || col > to) return null;
    return { from: start + 1, to, bracketed: true, openerAt: start };
  }
  const to = endOf(line, start, false);
  if (col < start || col > to) return null;
  return { from: start, to, bracketed: false, openerAt: start };
}

/**
 * What the caret is in the middle of completing, or null when nothing should be offered.
 * `explicit` is Ctrl+Space: one letter is enough.
 */
export function contextAt(text: string, pos: number, explicit = false): Context | null {
  if (pos < 0 || pos > text.length || inFence(text, pos)) return null;
  const start = text.lastIndexOf("\n", Math.max(0, pos - 1)) + 1;
  const end = text.indexOf("\n", pos);
  const line = text.slice(start, end < 0 ? text.length : end);
  const col = pos - start;
  if (/^ {0,3}(`{3,}|~{3,})/.test(line)) return null;
  const dest = inlineDest(line, col) ?? defDest(line, col);
  if (dest && !inlineCode(line, dest.openerAt)) {
    return {
      kind: "link",
      from: start + dest.from,
      to: start + dest.to,
      query: line.slice(dest.from, col),
      bracketed: dest.bracketed,
    };
  }
  if (inlineCode(line, col)) return null;
  const word = wordAt(line.slice(0, col), line.slice(col), explicit);
  if (!word) return null;
  return {
    kind: "word",
    from: start + col - word.before,
    to: start + col + word.after,
    query: word.query,
  };
}

function sameDocument(pathQuery: string, fromFile: string): boolean {
  const path = pathQuery.trim();
  if (path === "" || path === ".") return true;
  if (!isVaultPath(fromFile)) return false;
  return resolveRelative(fromFile, path) === fromFile;
}

function finish(path: string, rest: string, bracketed: boolean, wrap: boolean): string {
  const body = rest.startsWith("#") ? `${path}${rest}` : rest;
  if (!wrap || bracketed || !needsBrackets(body)) return body;
  return `<${body}>`;
}

function scorePath(rel: string, query: string): number {
  const folded = fold(rel);
  const base = fold(basename(rel));
  let score = 0;
  if (folded.startsWith(query)) score = 4;
  else if (base.startsWith(query)) score = 3;
  else if (folded.includes(query) || base.includes(query)) score = 1;
  else return 0;
  if (!rel.includes("/")) score += 2;
  return score;
}

export type LinkQuery = {
  fromFile: string;
  files: readonly string[];
  headings: readonly string[];
  /** Source mode wraps a path that contains spaces. The link box stores the path bare. */
  wrap: boolean;
  bracketed?: boolean;
};

/** Files or headings for the link text typed so far (up to the caret). */
export function linkSuggestions(typed: string, opts: LinkQuery): CompleteItem[] {
  let query = typed;
  let bracketed = opts.bracketed ?? false;
  if (query.startsWith("<")) {
    bracketed = true;
    query = query.slice(1);
  }
  const hash = query.indexOf("#");
  const pathQuery = (hash < 0 ? query : query.slice(0, hash)).trim();
  const fragment = hash < 0 ? null : query.slice(hash + 1);
  if (SCHEME.test(pathQuery) || pathQuery.startsWith("//")) return [];
  if (fragment !== null) {
    if (!sameDocument(pathQuery, opts.fromFile)) return [];
    const q = fold(fragment);
    return headingIds(opts.headings)
      .map((heading) => ({ heading, score: headingScore(heading, q) }))
      .filter((row) => row.score > 0)
      .sort((a, b) => b.score - a.score)
      .slice(0, LIMIT)
      .map(({ heading }) => ({
        kind: "heading" as const,
        label: heading.text,
        detail: `#${heading.id}`,
        insert: finish(pathQuery, `#${heading.id}`, bracketed, opts.wrap),
      }));
  }
  if (!isVaultPath(opts.fromFile)) return [];
  const q = fold(pathQuery);
  const rows: { rel: string; score: number }[] = [];
  for (const file of opts.files) {
    if (file === opts.fromFile) continue;
    const rel = relativeLink(opts.fromFile, file);
    if (!rel) continue;
    const score = q === "" ? (rel.includes("/") ? 1 : 3) : scorePath(rel, q);
    if (score > 0) rows.push({ rel, score });
  }
  rows.sort(
    (a, b) =>
      b.score - a.score ||
      Number(isMarkdown(b.rel)) - Number(isMarkdown(a.rel)) ||
      a.rel.length - b.rel.length ||
      a.rel.localeCompare(b.rel),
  );
  return rows.slice(0, LIMIT).map(({ rel }) => ({
    kind: "file" as const,
    label: rel,
    detail: "",
    insert: finish("", rel, bracketed, opts.wrap),
  }));
}

function headingScore(heading: { text: string; id: string }, query: string): number {
  if (query === "") return 1;
  const text = fold(heading.text);
  const id = fold(heading.id);
  if (text.startsWith(query) || id.startsWith(query)) return 3;
  if (text.includes(query) || id.includes(query)) return 1;
  return 0;
}

/** Words that continue `query`. What is already typed in full is left out. */
export function suggestWords(
  query: string,
  words: readonly string[],
  limit = LIMIT,
): CompleteItem[] {
  const q = fold(query);
  if (q === "") return [];
  const seen = new Set<string>();
  const hits: string[] = [];
  for (const word of words) {
    const key = fold(word);
    if (seen.has(key) || key === q || !key.startsWith(q)) continue;
    seen.add(key);
    hits.push(word);
  }
  hits.sort((a, b) => a.length - b.length || a.localeCompare(b));
  return hits.slice(0, limit).map((word) => ({
    kind: "word",
    label: word,
    insert: word,
    detail: "",
  }));
}
