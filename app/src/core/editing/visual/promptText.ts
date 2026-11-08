// `{{name}}` template variables and `@path/to/file.ext` file references in a text, found by the rules the Markdown
// renderer applies when the prompt-blocks plugin is on (`format/markdown/inline.rs`), so Edit marks what View marks.
// Pure: a reference is a candidate here; it is marked only when the file it names exists (the caller asks). The
// shared fixture `promptText.fixture.json` runs the same texts through both, so the two cannot drift.
//
// Lengths are counted in UTF-8 bytes and characters are Unicode code points, as the renderer counts them.

export type PromptPiece =
  | { kind: "variable"; from: number; to: number; name: string }
  /** `path`: the project path the reference names (`/` separators, no leading `/`). */
  | { kind: "file"; from: number; to: number; path: string };

/** The longest `{{ ... }}` body that is a variable name (bytes). */
const MAX_VARIABLE = 80;
/** The longest `@path` that is looked up (bytes). */
const MAX_REF = 400;

const ALPHANUMERIC = /^[\p{Alphabetic}\p{N}]$/u;
const ALPHABETIC = /^\p{Alphabetic}$/u;
const EDGE_SPACE = /^\p{White_Space}+|\p{White_Space}+$/gu;
/** What ends a sentence after a path, not part of it. */
const TRAILING = /[.\-_/\\]+$/;

const bytes = (text: string): number => new TextEncoder().encode(text).length;

function isPathChar(c: string): boolean {
  return ALPHANUMERIC.test(c) || "_./\\-@+~".includes(c);
}

function isVariableName(name: string): boolean {
  return name !== "" && [...name].every((c) => ALPHANUMERIC.test(c) || "_.-".includes(c));
}

/** An extension of 1..12 characters with at least one letter after the last dot of the last segment. */
function hasExtension(path: string): boolean {
  const name = path.split(/[/\\]/).at(-1) ?? path;
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return false;
  const ext = [...name.slice(dot + 1)];
  return (
    ext.length >= 1 &&
    ext.length <= 12 &&
    ext.every((c) => ALPHANUMERIC.test(c)) &&
    ext.some((c) => ALPHABETIC.test(c))
  );
}

/** The `@path` at the start of `rest` (after the `@`): as written and as a project path. */
function referenceAt(rest: string): { written: string; path: string } | null {
  let run = 0;
  for (const c of rest) {
    if (!isPathChar(c)) break;
    run += c.length;
  }
  const written = rest.slice(0, run).replace(TRAILING, "");
  if (written === "" || bytes(written) > MAX_REF || !hasExtension(written)) return null;
  const path = written.replace(/\\/g, "/").replace(/^\/+/, "");
  if (path.split("/").some((part) => part === ".." || part === "")) return null;
  return { written, path };
}

/** The variables and file reference candidates of `text`, in order. */
export function promptPieces(text: string): PromptPiece[] {
  if (!text.includes("{{") && !text.includes("@")) return [];
  const found: PromptPiece[] = [];
  let previous: string | null = null;
  let at = 0;
  while (at < text.length) {
    const c = String.fromCodePoint(text.codePointAt(at) ?? 0);
    let end: number | null = null;
    if (c === "{" && text.startsWith("{{", at)) {
      const close = text.indexOf("}}", at + 2);
      const body = close < 0 ? "" : text.slice(at + 2, close);
      const name = body.replace(EDGE_SPACE, "");
      if (close >= 0 && bytes(body) <= MAX_VARIABLE && isVariableName(name)) {
        end = close + 2;
        found.push({ kind: "variable", from: at, to: end, name });
      }
    } else if (c === "@" && (previous === null || !isPathChar(previous))) {
      const reference = referenceAt(text.slice(at + 1));
      if (reference !== null) {
        end = at + 1 + reference.written.length;
        found.push({ kind: "file", from: at, to: end, path: reference.path });
      }
    }
    if (end === null) {
      previous = c;
      at += c.length;
    } else {
      // the last character of the piece: `}` or a path character
      previous = [...text.slice(at, end)].at(-1) ?? null;
      at = end;
    }
  }
  return found;
}
