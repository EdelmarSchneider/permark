// @vitest-environment jsdom
// Table rows | columns added in Edit: above | left as well as below | right; only the table's lines change on save.
import "./jsdomShims";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { TextSelection } from "@milkdown/kit/prose/state";
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
const TABLE = "| Name | Note |\r\n| --- | --- |\r\n| first | a \\| b |\r\n| second | zeta |\r\n";
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

function saved(built: Built): string {
  return compose(core, built, core.view.state.doc).text;
}

describe("table rows above and columns to the left", () => {
  it("adds an empty row above the caret's row; text around the table keeps its bytes", async () => {
    const built = await load();
    caretIn("second");
    expect(run(core, { kind: "addRowAbove" })).toBe(true);
    expect(rows()).toEqual([
      ["Name", "Note"],
      ["first", "a | b"],
      ["", ""],
      ["second", "zeta"],
    ]);
    const text = saved(built);
    expect(text.startsWith(BEFORE)).toBe(true);
    expect(text.endsWith(AFTER)).toBe(true);
    const table = text.slice(BEFORE.length, text.length - AFTER.length);
    // edited lines come as LF here; the save gives them the file's ending (`edit_save`, editing.md)
    expect(table.split(/\r?\n/).filter((l) => l !== "")).toHaveLength(5);
    expect(table).toContain("a \\| b");
    expect(table).not.toMatch(/\n\r?\n/);
  });

  it("adds an empty column to the left of the caret's column", async () => {
    const built = await load();
    caretIn("zeta");
    expect(run(core, { kind: "addColumnLeft" })).toBe(true);
    expect(rows()).toEqual([
      ["Name", "", "Note"],
      ["first", "", "a | b"],
      ["second", "", "zeta"],
    ]);
    const text = saved(built);
    expect(text.startsWith(BEFORE) && text.endsWith(AFTER)).toBe(true);
  });

  it("a column to the left of the header's first cell becomes the first column", async () => {
    await load();
    caretIn("Name");
    expect(run(core, { kind: "addColumnLeft" })).toBe(true);
    expect(rows()[0]).toEqual(["", "Name", "Note"]);
    expect(rows()[1]).toEqual(["", "first", "a | b"]);
  });

  it("no row goes above the header row (the menu item is off there)", async () => {
    const built = await load();
    caretIn("Name");
    expect(formatOf(core.view.state).tableHeader).toBe(true);
    expect(run(core, { kind: "addRowAbove" })).toBe(false);
    expect(saved(built)).toBe(TEXT);
    caretIn("first");
    expect(formatOf(core.view.state).tableHeader).toBe(false);
  });

  it("outside a table both commands do nothing; untouched load saves byte for byte", async () => {
    const built = await load();
    caretIn("Some");
    expect(run(core, { kind: "addRowAbove" })).toBe(false);
    expect(run(core, { kind: "addColumnLeft" })).toBe(false);
    expect(saved(built)).toBe(TEXT);
  });

  it("rapid adds then undo all of them restores the bytes", async () => {
    const built = await load();
    caretIn("second");
    for (let i = 0; i < 5; i++) {
      run(core, { kind: "addRowAbove" });
      run(core, { kind: "addColumnLeft" });
    }
    expect(rows()).toHaveLength(8);
    const { undo } = await import("@milkdown/kit/prose/history");
    while (undo(core.view.state, core.view.dispatch));
    expect(saved(built)).toBe(TEXT);
  });
});
