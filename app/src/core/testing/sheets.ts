// Made-up config sheets for tests that need sizes no fixture file has.
import type { Group, Row, Scalar, Sheet } from "../../api";

export function scalar(path: string): Scalar {
  return { path, kind: "string", text: "v", edit: { editable: "surgical" } };
}

export function scalarRow(path: string): Row {
  return {
    path,
    key: path,
    line: 1,
    end: 1,
    value: { type: "scalar", ...scalar(path) },
    comment: null,
  };
}

/** A sheet of `cards` titled cards (`t0`, `t1`, …) with `rows` plain keys each. */
export function bigSheet(cards: number, rows: number): Sheet {
  const groups: Group[] = Array.from({ length: cards }, (_, c) => ({
    path: `t${c}`,
    title: `t${c}`,
    line: c * (rows + 1) + 1,
    end: (c + 1) * (rows + 1),
    comment: null,
    rows: Array.from({ length: rows }, (_, r) => scalarRow(`t${c}.k${r}`)),
  }));
  return { kind: "config", groups };
}
