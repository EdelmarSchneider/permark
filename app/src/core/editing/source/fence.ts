// The diagram fence the caret is in (Source, for the optional preview panel beside the editor). Pure.

/** A fenced diagram block: its language and its code. `from`: first line of the code (0-based in `lines`). */
export type Fence = { language: string; code: string; from: number };

const DIAGRAMS = new Set(["mermaid", "plantuml", "puml"]);
const FENCE = /^ {0,3}(`{3,}|~{3,})[ \t]*([^\s`]*)/;

/** The fenced block around line `at` (1-based, strictly inside the fences), when its language is a diagram. */
export function fenceAround(lines: readonly string[], at: number): Fence | null {
  const caret = at - 1;
  let open = -1;
  let marker = "";
  let language = "";
  for (let i = 0; i < lines.length; i++) {
    const match = FENCE.exec(lines[i] ?? "");
    if (open < 0) {
      if (match) {
        open = i;
        marker = match[1] ?? "";
        language = (match[2] ?? "").toLowerCase();
      }
      if (i >= caret) return null;
      continue;
    }
    const fence = match?.[1] ?? "";
    const closes =
      match !== null && !match[2] && fence[0] === marker[0] && fence.length >= marker.length;
    if (closes) {
      if (caret > open && caret < i) {
        return DIAGRAMS.has(language)
          ? { language, code: lines.slice(open + 1, i).join("\n"), from: open + 1 }
          : null;
      }
      open = -1;
      if (i >= caret) return null;
    }
  }
  if (open >= 0 && caret > open && DIAGRAMS.has(language)) {
    return { language, code: lines.slice(open + 1).join("\n"), from: open + 1 };
  }
  return null;
}
