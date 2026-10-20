// The differences between two texts as rows a view draws: unchanged runs fold to a few lines of context.

import { diff, splitLines } from "./lines";

export type DiffRow =
  | { kind: "same" | "add" | "del"; text: string; key: string }
  | { kind: "fold"; count: number; key: string };

/** Lines of context kept around a change. */
export const CONTEXT = 3;

/** `before` -> `after`: removed lines as `del`, new ones as `add`, long unchanged runs folded. */
export function diffRows(before: string, after: string, context = CONTEXT): DiffRow[] {
  const a = splitLines(before);
  const b = splitLines(after);
  const out: DiffRow[] = [];
  for (const op of diff(a, b)) {
    if (op.kind === "same") {
      const lines = a.slice(op.a, op.a + op.count);
      if (lines.length > context * 2 + 1) {
        lines
          .slice(0, context)
          .forEach((text, i) => out.push({ kind: "same", text, key: `s${op.a + i}` }));
        out.push({ kind: "fold", count: lines.length - context * 2, key: `f${op.a}` });
        lines
          .slice(-context)
          .forEach((text, i) =>
            out.push({ kind: "same", text, key: `s${op.a + op.count - context + i}` }),
          );
      } else lines.forEach((text, i) => out.push({ kind: "same", text, key: `s${op.a + i}` }));
    } else {
      a.slice(op.a, op.a + op.remove).forEach((text, i) =>
        out.push({ kind: "del", text, key: `d${op.a + i}` }),
      );
      b.slice(op.b, op.b + op.add).forEach((text, i) =>
        out.push({ kind: "add", text, key: `a${op.b + i}` }),
      );
    }
  }
  return out;
}
