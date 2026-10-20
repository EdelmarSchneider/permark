// Code references: `src/main.rs:42` in a document becomes a link to that line of that file. This module is the pure
// part: reading a reference out of text. Finding the project files they name is the backend's (`code_refs_resolve`).
//
// A reference is a path with a file extension and an optional line: `src/main.rs`, `:42`, `:42:7`, `:10-20`, `#L10`,
// `#L10-L20`. In inline code the line is optional; in plain text it is required, so prose is never turned into links.
// Nothing becomes a link unless it names a file the project has, so what the grammar lets through is checked again.

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
// Sticky: tried only where a path can start, see `findTextRefs`.
const IN_TEXT = new RegExp(`(${PATH})${SUFFIX}(?![A-Za-z0-9_])`, "y");

/** Where a line suffix starts (`:42`, `#L10`): the only places a reference in plain text can end its path. */
const SUFFIX_START = /:\d|#L\d/g;
/** A character a path is made of (a name character or a separator). */
const PATH_CHAR = /[A-Za-z0-9_.@+~\\/-]/;
/** A path longer than this is not a reference (and bounds the work done per candidate). */
const MAX_PATH = 400;

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
  const written = match[1];
  if (written === undefined) return null;
  // `@src/main.rs`: a mention as "Copy reference" writes it
  let path = written.replace(/\\/g, "/");
  if (path.startsWith("@")) path = path.slice(1);
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
  if (trimmed === "" || trimmed.length > MAX_PATH) return null;
  const match = INLINE.exec(trimmed);
  return match ? build(match) : null;
}

/** The references in plain text. Only a path with a line counts here. */
export function findTextRefs(text: string): TextRef[] {
  if (text.length > MAX_TEXT || !/[:#]/.test(text)) return [];
  // A path holds no `:` or `#`, so a reference's path runs back from its suffix over path characters to the start of
  // the token. Only that start can begin a reference (any later start follows a path character, which is a token
  // character), so one anchored try per suffix finds what a scan from every position would, in linear time.
  const found: TextRef[] = [];
  let done = 0;
  SUFFIX_START.lastIndex = 0;
  for (let at = SUFFIX_START.exec(text); at; at = SUFFIX_START.exec(text)) {
    const suffix = at.index;
    if (suffix < done) continue;
    let start = suffix;
    while (start > 0 && suffix - start <= MAX_PATH && PATH_CHAR.test(text.charAt(start - 1)))
      start--;
    if (start === suffix || suffix - start > MAX_PATH) continue;
    if (start > 0 && TOKEN_CHAR.test(text.charAt(start - 1))) continue;
    IN_TEXT.lastIndex = start;
    const match = IN_TEXT.exec(text);
    if (!match) continue;
    const end = start + match[0].length;
    const ref = build(match);
    if (ref) {
      found.push({ start, end, ref });
      done = end;
    }
  }
  return found;
}

/** A reference as it is written back: `src/main.rs:42`. */
export function refLabel(path: string, line: number | null): string {
  return line === null ? path : `${path}:${line}`;
}
