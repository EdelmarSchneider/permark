// Lines inside a fenced block. Prose wraps (the pre is `pre-wrap`). A table row or a line of
// diagram source must stay intact, so it is marked and the pre scrolls to show it.

const BOX_DRAWING = /[│┌┐└┘├┤┬┴┼─═║╔╗╚╝╠╣╦╩╬╭╮╯╰▲▼◀▶←→↔]/;
const DIAGRAM_KEYWORD =
  /^\s*(?:graph|flowchart|sequenceDiagram|classDiagram|stateDiagram(?:-v2)?|erDiagram|gantt|pie|gitGraph|journey|mindmap|timeline|C4Context|C4Container)\b/i;
const ASCII_FRAME = /^[\s+\-_=|./\\<>^v:*#]+$/;

/** A source line whose spaces are structure, so wrapping it would break the row or the drawing. */
export function lineStaysUnbroken(line: string): boolean {
  const text = line.replace(/\s+$/g, "");
  if (text.trim() === "") return false;
  if (isTableRow(text)) return true;
  return isDiagramLine(text);
}

/** A GFM row: at least two pipes with a cell between them. `a || b` is an operator, not a row. */
function isTableRow(line: string): boolean {
  return /\|[^|]+\|/.test(line);
}

function isDiagramLine(line: string): boolean {
  if (BOX_DRAWING.test(line)) return true;
  if (/-->|<--|==>|<==|@startuml|@enduml|@startmindmap|@endmindmap/i.test(line)) return true;
  if (DIAGRAM_KEYWORD.test(line)) return true;
  // "Alice -> Bob" or "Alice -> Bob: hello". `foo->bar` and `return foo -> bar` do not match.
  if (/^\s*[A-Za-z_][\w]*\s+-+>\s+[A-Za-z_"]/.test(line)) return true;
  const drawn = line.trim();
  return drawn.length >= 8 && ASCII_FRAME.test(drawn) && /[+\-|]/.test(drawn);
}

function locate(root: Node, target: number): { node: Text; offset: number } | null {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  let seen = 0;
  let current = walker.nextNode() as Text | null;
  while (current) {
    const next = seen + current.length;
    if (target <= next) return { node: current, offset: target - seen };
    seen = next;
    current = walker.nextNode() as Text | null;
  }
  return null;
}

function unwrapSolid(code: Element): void {
  const spans = code.querySelectorAll(".code-line--unbroken");
  if (spans.length === 0) return;
  for (const span of Array.from(spans)) span.replaceWith(...span.childNodes);
  code.normalize();
}

/** Wrap one source line. `end` is exclusive and does not include the line's trailing newline. */
function wrapRange(code: Element, start: number, end: number): void {
  if (end <= start) return;
  const from = locate(code, start);
  const to = locate(code, end);
  if (!from || !to) return;
  const range = document.createRange();
  range.setStart(from.node, from.offset);
  range.setEnd(to.node, to.offset);
  const span = document.createElement("span");
  span.className = "code-line--unbroken";
  span.appendChild(range.extractContents());
  range.insertNode(span);
}

/**
 * Marks table rows and diagram lines inside a fence so they stay on one line.
 * Highlighting may already have split the text into spans; the characters are unchanged,
 * which is what Copy reads (`textContent`). Safe to call again after a redraw.
 */
export function markSolidLines(code: Element): void {
  unwrapSolid(code);
  const text = code.textContent ?? "";
  const lines = text.split("\n");
  const spans: { start: number; end: number }[] = [];
  let offset = 0;
  for (const line of lines) {
    if (lineStaysUnbroken(line)) spans.push({ start: offset, end: offset + line.length });
    offset += line.length + 1;
  }
  for (let i = spans.length - 1; i >= 0; i--) wrapRange(code, spans[i].start, spans[i].end);
}
