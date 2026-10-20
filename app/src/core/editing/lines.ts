// Line diff of the edit text: what a save sends (changed lines only) and what the conflict view shows. The text is
// split at `\n` (the edit text has no other break): `n` breaks give `n + 1` lines, the last one empty when the text
// ends with a break. Myers' greedy diff after the common prefix and suffix; past `MAX_D` differences the middle is
// one replacement (still only the changed region).

import type { LineEdit } from "../../api/generated/LineEdit";

export type { LineEdit };

/** Differences past which the middle is replaced as a whole. */
export const MAX_D = 1000;

export function splitLines(text: string): string[] {
  return text.split("\n");
}

/** One run of the diff: lines kept, removed from `a`, added from `b`. */
export type DiffOp =
  | { kind: "same"; a: number; b: number; count: number }
  | { kind: "change"; a: number; remove: number; b: number; add: number };

/** Edit script turning `a` into `b`, as runs (ordered, `change` runs never adjacent). */
export function diff(a: readonly string[], b: readonly string[]): DiffOp[] {
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start += 1;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
    endA -= 1;
    endB -= 1;
  }
  const ops: DiffOp[] = [];
  if (start > 0) ops.push({ kind: "same", a: 0, b: 0, count: start });
  const middle = myers(a, b, start, endA, start, endB);
  for (const op of middle) push(ops, op);
  const tail = a.length - endA;
  if (tail > 0) push(ops, { kind: "same", a: endA, b: endB, count: tail });
  return ops;
}

function push(ops: DiffOp[], op: DiffOp): void {
  const last = ops.at(-1);
  if (last && last.kind === op.kind) {
    if (op.kind === "same" && last.kind === "same") {
      last.count += op.count;
      return;
    }
    if (op.kind === "change" && last.kind === "change") {
      last.remove += op.remove;
      last.add += op.add;
      return;
    }
  }
  if (op.kind === "same" && op.count === 0) return;
  if (op.kind === "change" && op.remove === 0 && op.add === 0) return;
  ops.push({ ...op });
}

/** Myers over `a[a0..a1)` vs `b[b0..b1)`; null trace past `MAX_D`. */
function myers(
  a: readonly string[],
  b: readonly string[],
  a0: number,
  a1: number,
  b0: number,
  b1: number,
): DiffOp[] {
  const n = a1 - a0;
  const m = b1 - b0;
  if (n === 0 && m === 0) return [];
  if (n === 0 || m === 0) return [{ kind: "change", a: a0, remove: n, b: b0, add: m }];
  const max = Math.min(n + m, MAX_D);
  const offset = max + 1;
  const v = new Int32Array(2 * max + 3);
  // V before each round, only the part the walk back reads (k - 1 .. k + 1 for |k| <= d)
  const trace: { base: number; v: Int32Array }[] = [];
  let found = -1;
  for (let d = 0; d <= max; d++) {
    trace.push({ base: offset - d - 1, v: v.slice(offset - d - 1, offset + d + 2) });
    for (let k = -d; k <= d; k += 2) {
      let x: number;
      const left = v[offset + k - 1] ?? 0;
      const right = v[offset + k + 1] ?? 0;
      x = k === -d || (k !== d && left < right) ? right : left + 1;
      let y = x - k;
      while (x < n && y < m && a[a0 + x] === b[b0 + y]) {
        x += 1;
        y += 1;
      }
      v[offset + k] = x;
      if (x >= n && y >= m) {
        found = d;
        break;
      }
    }
    if (found >= 0) break;
  }
  if (found < 0) return [{ kind: "change", a: a0, remove: n, b: b0, add: m }];
  // walk back from (n, m): each round = a snake (diagonal) after one step down (insert) or right (delete)
  const ops: DiffOp[] = [];
  let x = n;
  let y = m;
  for (let d = found; d > 0; d--) {
    const round = trace[d];
    if (round === undefined) break;
    const at = (index: number): number => round.v[index - round.base] ?? 0;
    const k = x - y;
    const down = k === -d || (k !== d && at(offset + k - 1) < at(offset + k + 1));
    const prevK = down ? k + 1 : k - 1;
    const prevX = at(offset + prevK);
    const prevY = prevX - prevK;
    const stepX = down ? prevX : prevX + 1;
    const stepY = down ? prevY + 1 : prevY;
    if (x > stepX) ops.push({ kind: "same", a: a0 + stepX, b: b0 + stepY, count: x - stepX });
    if (down) ops.push({ kind: "change", a: a0 + prevX, remove: 0, b: b0 + prevY, add: 1 });
    else ops.push({ kind: "change", a: a0 + prevX, remove: 1, b: b0 + prevY, add: 0 });
    x = prevX;
    y = prevY;
  }
  if (x > 0) ops.push({ kind: "same", a: a0, b: b0, count: x });
  ops.reverse();
  const merged: DiffOp[] = [];
  for (const op of ops) push(merged, op);
  return merged;
}

/** Line edits turning `base` into `next` (both edit texts). Empty when equal. */
export function lineEdits(base: string, next: string): LineEdit[] {
  if (base === next) return [];
  const a = splitLines(base);
  const b = splitLines(next);
  const edits: LineEdit[] = [];
  for (const op of diff(a, b)) {
    if (op.kind === "change") {
      edits.push({ at: op.a, remove: op.remove, insert: b.slice(op.b, op.b + op.add) });
    }
  }
  return edits;
}

/** `base` with `edits` applied (the backend's rule, endings aside): checks and tests. */
export function applyEdits(base: string, edits: readonly LineEdit[]): string {
  const lines = splitLines(base);
  const out: string[] = [];
  let next = 0;
  for (const edit of edits) {
    out.push(...lines.slice(next, edit.at), ...edit.insert);
    next = edit.at + edit.remove;
  }
  out.push(...lines.slice(next));
  return out.join("\n");
}
