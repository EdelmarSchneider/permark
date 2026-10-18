// The Mermaid fence the caret is in, so the Source editor can preview that diagram and no other.

export type MermaidFence = {
  /** The diagram text, without the fence lines. */
  source: string;
  /** The opening line, from 1, so a move to another diagram is a different fence. */
  startLine: number;
};

const OPEN = /^ {0,3}(`{3,}|~{3,})[ \t]*mermaid(?:[ \t].*)?$/i;
const CLOSE = /^ {0,3}(`{3,}|~{3,})[ \t]*$/;

/**
 * The Mermaid block that contains `line` (from 1, as the editor numbers lines), or null when the caret is not in one.
 * An unclosed fence still counts: the preview shows whatever has been typed so far.
 */
export function mermaidAt(text: string, line: number): MermaidFence | null {
  if (line < 1) return null;
  const rows = text.split("\n");
  let openAt = -1;
  let mark = "";
  for (let i = 0; i < rows.length; i++) {
    if (openAt < 0) {
      const opener = OPEN.exec(rows[i]);
      if (opener) {
        openAt = i;
        mark = opener[1];
      }
      continue;
    }
    const closer = CLOSE.exec(rows[i]);
    if (closer && closer[1][0] === mark[0] && closer[1].length >= mark.length) {
      if (line >= openAt + 1 && line <= i + 1) {
        return { source: rows.slice(openAt + 1, i).join("\n"), startLine: openAt + 1 };
      }
      openAt = -1;
      mark = "";
    }
  }
  if (openAt >= 0 && line >= openAt + 1) {
    return { source: rows.slice(openAt + 1).join("\n"), startLine: openAt + 1 };
  }
  return null;
}
