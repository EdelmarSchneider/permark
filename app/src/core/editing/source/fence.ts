// The diagram fence the caret is in (Source, for the optional preview panel beside the editor). Pure.

/**
 * A fenced diagram block: its language and its code. `from`: first line of the code (0-based in `lines`); `closed`:
 * its closing fence was among the lines read (else the code runs to the last of them).
 */
export type Fence = { language: string; code: string; from: number; closed: boolean };

const DIAGRAMS = new Set(["mermaid", "plantuml", "puml", "dot", "graphviz"]);
const FENCE = /^ {0,3}(`{3,}|~{3,})[ \t]*([^\s`]*)/;
/** Lines read each side of the caret: a long document is not split on every key. */
const AROUND = 400;
/**
 * Most characters read beyond those lines to reach a block's other fence: more than any diagram engine draws, so a
 * longer block reaches the engine's own "too large" check instead of a cut source the engine would call broken.
 */
const READ_PAST = 60_000;

/** Whether `text` closes a block opened with `marker`: same character, at least as long, no info. */
function closes(text: string, marker: string): boolean {
  const match = FENCE.exec(text);
  const fence = match?.[1] ?? "";
  return match !== null && !match[2] && fence[0] === marker[0] && fence.length >= marker.length;
}

/** The fenced block around line `at` (1-based, strictly inside the fences), when its language is a diagram. */
export function fenceAround(lines: readonly string[], at: number): Fence | null {
  const caret = at - 1;
  let open = -1;
  let marker = "";
  let language = "";
  for (let i = 0; i < lines.length; i++) {
    const text = lines[i] ?? "";
    if (open < 0) {
      const match = FENCE.exec(text);
      if (match) {
        open = i;
        marker = match[1] ?? "";
        language = (match[2] ?? "").toLowerCase();
      }
      if (i >= caret) return null;
      continue;
    }
    if (closes(text, marker)) {
      if (caret > open && caret < i) {
        return DIAGRAMS.has(language)
          ? { language, code: lines.slice(open + 1, i).join("\n"), from: open + 1, closed: true }
          : null;
      }
      open = -1;
      if (i >= caret) return null;
    }
  }
  if (open >= 0 && caret > open && DIAGRAMS.has(language)) {
    return { language, code: lines.slice(open + 1).join("\n"), from: open + 1, closed: false };
  }
  return null;
}

/**
 * The diagram fence around line `at` (1-based) of a document of `count` lines (`line(n)`: line n). Reads the lines
 * around the caret; a block opening above them | closing below them is read on, at most `READ_PAST` characters each
 * way. `from`: first line of the code, 0-based in the document.
 */
export function fenceIn(line: (n: number) => string, count: number, at: number): Fence | null {
  let first = Math.max(1, at - AROUND);
  let last = Math.min(count, at + AROUND);
  let lines: string[] = [];
  for (let n = first; n <= last; n++) lines.push(line(n));
  // no fence line from the top of the window down to the caret: the block around it may open above the window
  if (first > 1 && !lines.slice(0, at - first + 1).some((text) => FENCE.test(text))) {
    const above: string[] = [];
    for (let n = first - 1, read = 0; n >= 1 && read <= READ_PAST; n--) {
      const text = line(n);
      above.push(text);
      read += text.length + 1;
      if (FENCE.test(text)) {
        lines = above.reverse().concat(lines);
        first = n;
        break;
      }
    }
  }
  let fence = fenceAround(lines, at - first + 1);
  if (fence !== null && !fence.closed && last < count) {
    const marker = FENCE.exec(lines[fence.from - 1] ?? "")?.[1] ?? "";
    for (let read = 0; last < count && read <= READ_PAST;) {
      last += 1;
      const text = line(last);
      lines.push(text);
      read += text.length + 1;
      if (closes(text, marker)) break;
    }
    fence = fenceAround(lines, at - first + 1);
  }
  return fence && { ...fence, from: fence.from + first - 1 };
}
