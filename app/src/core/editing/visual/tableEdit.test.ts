// @vitest-environment jsdom
// Table rows | columns added in Edit: above | left as well as below | right; only the table's lines change on save.
import "./jsdomShims";
import { undo } from "@milkdown/kit/prose/history";
import { TextSelection } from "@milkdown/kit/prose/state";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import type { Segment } from "../../../api/generated/Segment";
import { formatOf, run } from "./commands";
import { buildDoc, compose, type Built } from "./compose";
import { createCore, type Core } from "./core";

let core: Core;

beforeAll(async () => {
  const host = document.createElement("div");
  document.body.append(host);
  core = await createCore({ host });
});

afterAll(async () => {
  await core.destroy();
});

const BEFORE = "# Plan\r\n\r\nSome  text, two spaces.\r\n\r\n";
const LINES = ["| Name | Note |", "| :--- | ---: |", "| first | a \\| b |", "| second | zeta |"];
const TABLE = LINES.map((line) => `${line}\r\n`).join("");
const AFTER = "\r\n* star bullet\r\n";
const TEXT = BEFORE + TABLE + AFTER;
const SEGMENTS: Segment[] = [
  { lineStart: 1, lineEnd: 1, kind: "heading", lock: null },
  { lineStart: 3, lineEnd: 3, kind: "paragraph", lock: null },
  { lineStart: 5, lineEnd: 8, kind: "table", lock: null },
  { lineStart: 10, lineEnd: 10, kind: "list", lock: null },
];

async function load(): Promise<Built> {
  const built = await buildDoc(core, TEXT, SEGMENTS);
  core.setDoc(built.doc);
  return built;
}

/** Puts the caret inside the cell holding `text`. */
function caretIn(text: string): void {
  const view = core.view;
  let at = -1;
  view.state.doc.descendants((node, pos) => {
    if (at >= 0) return false;
    if (node.isText && (node.text ?? "").includes(text)) at = pos + 1;
    return true;
  });
  if (at < 0) throw new Error(`no ${text}`);
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, at)));
}

/** The table rows as their cell texts. */
function rows(): string[][] {
  const out: string[][] = [];
  core.view.state.doc.descendants((node) => {
    if (node.type.name === "table_row" || node.type.name === "table_header_row") {
      const cells: string[] = [];
      node.forEach((cell) => cells.push(cell.textContent));
      out.push(cells);
      return false;
    }
    return true;
  });
  return out;
}

/** The saved table's lines; the text around the table keeps its bytes. */
function savedTable(built: Built): string[] {
  const text = compose(core, built, core.view.state.doc).text;
  expect(text.startsWith(BEFORE)).toBe(true);
  expect(text.endsWith(AFTER)).toBe(true);
  // edited lines come as LF here; the save gives them the file's line ending
  return text
    .slice(BEFORE.length, text.length - AFTER.length)
    .split(/\r?\n/)
    .filter((line) => line !== "");
}

describe("table rows above and columns to the left", () => {
  it("adds an empty row above the caret's row; the table's other lines keep their bytes", async () => {
    const built = await load();
    caretIn("second");
    expect(run(core, { kind: "addRowAbove" })).toBe(true);
    expect(rows()).toEqual([
      ["Name", "Note"],
      ["first", "a | b"],
      ["", ""],
      ["second", "zeta"],
    ]);
    const lines = savedTable(built);
    expect(lines).toHaveLength(5);
    expect([...lines.slice(0, 3), lines[4]]).toEqual(LINES);
    expect(lines[3]).toMatch(/^\|\s*\|\s*\|$/);
  });

  it("writes the new row as adding a row below writes it", async () => {
    const above = await load();
    caretIn("second");
    run(core, { kind: "addRowAbove" });
    const newAbove = savedTable(above)[3];
    const below = await load();
    caretIn("first");
    run(core, { kind: "addRow" });
    expect(savedTable(below)[3]).toBe(newAbove);
  });

  it("writes a column to the left as adding one to the right of the column before writes it", async () => {
    const left = await load();
    caretIn("zeta");
    run(core, { kind: "addColumnLeft" });
    const withLeft = savedTable(left);
    const right = await load();
    caretIn("Name");
    run(core, { kind: "addColumn" });
    expect(savedTable(right)).toEqual(withLeft);
  });

  it("adds an empty column to the left of the caret's column, keeping each column's alignment", async () => {
    const built = await load();
    caretIn("zeta");
    expect(run(core, { kind: "addColumnLeft" })).toBe(true);
    expect(rows()).toEqual([
      ["Name", "", "Note"],
      ["first", "", "a | b"],
      ["second", "", "zeta"],
    ]);
    const lines = savedTable(built);
    expect(lines).toHaveLength(4);
    expect(lines[1]).toMatch(/^\| :-+ \|.*\| -+: \|$/);
    expect(lines[2]).toContain("a \\| b");
  });

  it("a column to the left of the header's first cell becomes the first column", async () => {
    await load();
    caretIn("Name");
    expect(run(core, { kind: "addColumnLeft" })).toBe(true);
    expect(rows()[0]).toEqual(["", "Name", "Note"]);
    expect(rows()[1]).toEqual(["", "first", "a | b"]);
  });

  it("no row goes above the header row (a table's header stays first)", async () => {
    const built = await load();
    caretIn("Name");
    expect(formatOf(core.view.state).tableHeader).toBe(true);
    expect(run(core, { kind: "addRowAbove" })).toBe(false);
    expect(compose(core, built, core.view.state.doc).text).toBe(TEXT);
    caretIn("first");
    expect(formatOf(core.view.state).tableHeader).toBe(false);
  });

  it("outside a table both commands do nothing; an untouched load saves byte for byte", async () => {
    const built = await load();
    caretIn("Some");
    expect(formatOf(core.view.state).tableHeader).toBe(false);
    expect(run(core, { kind: "addRowAbove" })).toBe(false);
    expect(run(core, { kind: "addColumnLeft" })).toBe(false);
    expect(compose(core, built, core.view.state.doc).text).toBe(TEXT);
  });

  it("one undo takes back one added row or column", async () => {
    const built = await load();
    caretIn("second");
    run(core, { kind: "addRowAbove" });
    expect(undo(core.view.state, core.view.dispatch)).toBe(true);
    expect(compose(core, built, core.view.state.doc).text).toBe(TEXT);
    caretIn("zeta");
    run(core, { kind: "addColumnLeft" });
    expect(undo(core.view.state, core.view.dispatch)).toBe(true);
    expect(compose(core, built, core.view.state.doc).text).toBe(TEXT);
  });

  it("a column added to a table written without padding: the other cells keep their bytes, the new one has no alignment", async () => {
    const lines = [
      "| Ingredient | For 4 | For 12 | Note |",
      "|---|---:|---:|---|",
      "| Limes | 2 | 6 | Juice |",
    ];
    const text = `${lines.join("\n")}\n`;
    const segments: Segment[] = [{ lineStart: 1, lineEnd: 3, kind: "table", lock: null }];
    for (const [kind, at] of [
      ["addColumnLeft", "Limes"],
      ["addColumn", "Limes"],
      ["addColumnLeft", "Juice"],
    ] as const) {
      const built = await buildDoc(core, text, segments);
      core.setDoc(built.doc);
      caretIn(at);
      expect(run(core, { kind })).toBe(true);
      const saved = compose(core, built, core.view.state.doc).text.split("\n").filter(Boolean);
      // each saved line is its old line with one cell put in: take that cell out, the old bytes are left
      const column = kind === "addColumn" ? 1 : at === "Juice" ? 3 : 0;
      saved.forEach((line, row) => {
        const parts = line.split("|");
        const added = parts.splice(column + 1, 1)[0] ?? "";
        expect(parts.join("|"), `${kind} ${at} row ${row}: ${line}`).toBe(lines[row]);
        // a new column has no alignment of its own: plain dashes, no colon
        expect(added.trim(), `${kind} ${at} row ${row}`).toMatch(row === 1 ? /^-+$/ : /^$/);
      });
    }
  });

  it("a row above, then a column to the left: the old rows keep their bytes after the new cell", async () => {
    const lines = ["| A | B |", "|---|--:|", "| a1 | b1 |", "| a2 | b2 |"];
    const built = await buildDoc(core, `${lines.join("\n")}\n`, [
      { lineStart: 1, lineEnd: 4, kind: "table", lock: null },
    ]);
    core.setDoc(built.doc);
    caretIn("a2");
    run(core, { kind: "addRowAbove" });
    caretIn("a2");
    run(core, { kind: "addColumnLeft" });
    const saved = compose(core, built, core.view.state.doc).text.split("\n").filter(Boolean);
    expect(saved).toHaveLength(5);
    const old = saved.filter((_, row) => row !== 3).map((line) => line.replace(/^\|[^|]*/, ""));
    expect(old).toEqual(lines);
    expect(saved[1]).toBe("| --- |---|--:|");
  });

  it("rapid adds then undo all of them restores the bytes", async () => {
    const built = await load();
    caretIn("second");
    for (let i = 0; i < 5; i++) {
      run(core, { kind: "addRowAbove" });
      run(core, { kind: "addColumnLeft" });
    }
    expect(rows()).toHaveLength(8);
    while (undo(core.view.state, core.view.dispatch));
    expect(compose(core, built, core.view.state.doc).text).toBe(TEXT);
  });
});
