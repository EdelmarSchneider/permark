// Code references: `src/main.rs:42` in a document becomes a link to that line of that file. This module is the pure part:
// reading a reference out of text, and finding the project file it names. The DOM side is `dom/codeRefsDom.ts`.
//
// A reference is a path with a file extension and an optional line: `src/main.rs`, `:42`, `:42:7`, `:10-20`, `#L10`,
// `#L10-L20`. In inline code the line is optional; in plain text it is required, so prose is never turned into links.
// Nothing becomes a link unless it names a file the project has, so what the grammar lets through is checked again by
// the resolver.

import { WT_PREFIX, basename, dirname, resolveRelative, splitWorktree } from "./paths";

export type CodeRef = {
  /** The path as written, with "/" separators. */
  path: string;
  /** The first line (from 1), when the reference has one. */
  line: number | null;
  /** The last line of a range (`:10-20`, `#L10-L20`). */
  endLine: number | null;
  /** The column (from 1), when the reference has one. */
  col: number | null;
};

export type TextRef = { start: number; end: number; ref: CodeRef };

// Characters of a file or folder name. No space (so prose around a path ends it), no `:` (so `C:\x` and URLs are no paths).
const NAME = "[A-Za-z0-9_.@+~-]";
const SEP = "[\\\\/]";
// The extension needs a letter: `1.2` and `192.168.1.1` are numbers, not file names.
const EXT = "\\.(?=[A-Za-z0-9]*[A-Za-z])[A-Za-z0-9]{1,12}";
const PATH = `${SEP}?(?:${NAME}+${SEP})*${NAME}*${EXT}`;
// `:42`, `:42:7` (line and column), `:10-20` (a range of lines), `#L10`, `#L10C5`, `#L10-L20`.
const SUFFIX =
  "(?::(\\d{1,7})(?:-(\\d{1,7})|:(\\d{1,5}))?|#L(\\d{1,7})(?:C(\\d{1,5}))?(?:-L(\\d{1,7})(?:C\\d{1,5})?)?)";

const INLINE = new RegExp(`^(${PATH})${SUFFIX}?$`);
// Groups: 1 path; 2 line, 3 last line, 4 column for the `:` form; 5 line, 6 column, 7 last line for the `#L` form.
const IN_TEXT = new RegExp(`(${PATH})${SUFFIX}(?![A-Za-z0-9_])`, "g");

/** A reference is not read from the middle of a longer token: URLs, drive paths, `a:b/c.rs`... */
const TOKEN_CHAR = /[A-Za-z0-9_./\\:@%#?&=~+-]/;

/** Text longer than this is not scanned (a pasted blob, not prose). */
const MAX_TEXT = 50_000;

function number(value: string | undefined): number | null {
  if (value === undefined) return null;
  const n = Number(value);
  return n >= 1 ? n : null;
}

function build(match: RegExpExecArray): CodeRef | null {
  let path = match[1].replace(/\\/g, "/");
  // `@src/main.rs`: a mention as "Copy reference" writes it (a worktree path starts with `@wt/` for real)
  if (path.startsWith("@") && !path.startsWith(WT_PREFIX)) path = path.slice(1);
  const colonForm = match[2] !== undefined;
  const hashForm = match[5] !== undefined;
  if (!colonForm && !hashForm) return { path, line: null, endLine: null, col: null };
  const line = number(colonForm ? match[2] : match[5]);
  if (line === null) return null; // `file.rs:0` is not a line
  const endLine = number(colonForm ? match[3] : match[7]);
  return {
    path,
    line,
    endLine: endLine !== null && endLine >= line ? endLine : null,
    col: number(colonForm ? match[4] : match[6]),
  };
}

/** The reference that is the whole of an inline code span, or null when the text is anything else. */
export function parseInlineRef(text: string): CodeRef | null {
  const trimmed = text.trim();
  if (trimmed === "" || trimmed.length > 400) return null;
  const match = INLINE.exec(trimmed);
  return match ? build(match) : null;
}

/** The references in plain text. Only a path with a line counts here. */
export function findTextRefs(text: string): TextRef[] {
  if (text.length > MAX_TEXT || !/[:#]/.test(text)) return [];
  const found: TextRef[] = [];
  IN_TEXT.lastIndex = 0;
  for (let match = IN_TEXT.exec(text); match; match = IN_TEXT.exec(text)) {
    const start = match.index;
    const end = start + match[0].length;
    const before = start > 0 ? text[start - 1] : "";
    const ref = before !== "" && TOKEN_CHAR.test(before) ? null : build(match);
    if (ref) found.push({ start, end, ref });
  }
  return found;
}

// ---- finding the file --------------------------------------------------------------------------------------------

export type FileIndex = {
  readonly files: ReadonlySet<string>;
  /** Project paths by file name. */
  readonly byName: ReadonlyMap<string, readonly string[]>;
};

/** A name that matches more files than this is too common to guess from (`index.ts`, `mod.rs`): it stays plain text. */
export const MAX_CANDIDATES = 12;

export function buildFileIndex(files: readonly string[]): FileIndex {
  const byName = new Map<string, string[]>();
  for (const file of files) {
    const name = basename(file);
    const list = byName.get(name);
    if (list) list.push(file);
    else byName.set(name, [file]);
  }
  return { files: new Set(files), byName };
}

/** How many leading folders two paths share (the closer a candidate is to the document, the earlier it is listed). */
function sharedFolders(a: string, b: string): number {
  const left = dirname(a).split("/");
  const right = dirname(b).split("/");
  let n = 0;
  while (n < left.length && n < right.length && left[n] === right[n]) n++;
  return n;
}

/**
 * The project files a written path can mean, for a reference in the document `docPath`. One entry: found (relative to
 * the project root, else to the document's folder). Several: the path matches the end of more than one file (the
 * closest to the document first). None: not a file of the project.
 *
 * A document inside a linked worktree looks in that worktree only, and one in the main folder never looks into the
 * worktrees (which hold a copy of every file).
 */
export function resolveCodeRef(index: FileIndex, docPath: string, written: string): string[] {
  const { worktree } = splitWorktree(docPath);
  const prefix = worktree === null ? "" : `${WT_PREFIX}${worktree}/`;
  const clean = written.replace(/\\/g, "/").replace(/^\/+/, "");
  if (clean === "") return [];

  const fromRoot = resolveRelative(`${prefix}_`, `/${clean}`);
  const fromDoc = resolveRelative(docPath, written.replace(/\\/g, "/"));
  for (const candidate of [fromRoot, fromDoc]) {
    if (candidate !== null && index.files.has(candidate)) return [candidate];
  }

  // The end of the path, without the `./` and `../` that only mean something from a folder.
  const tail = clean
    .split("/")
    .filter((segment, i, all) => !((segment === "." || segment === "..") && i < all.length - 1))
    .join("/");
  const matches = (index.byName.get(basename(tail)) ?? []).filter(
    (file) =>
      splitWorktree(file).worktree === worktree &&
      (file === prefix + tail || file.endsWith(`/${tail}`)),
  );
  if (matches.length === 0 || matches.length > MAX_CANDIDATES) return [];
  return matches.sort(
    (a, b) => sharedFolders(b, docPath) - sharedFolders(a, docPath) || a.localeCompare(b),
  );
}

/** A reference as it is copied: `src/main.rs:42`. */
export function refLabel(path: string, line: number | null): string {
  return line === null ? path : `${path}:${line}`;
}
