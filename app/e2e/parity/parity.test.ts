// The parity probe's pure parts: value tolerance, block alignment, element diffs, Edit entry checks, known-gap triage.
// The browser half runs with `npm run ux:parity`.

import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  align,
  compareSides,
  entryDiffs,
  groupDiffs,
  parseKnownGaps,
  same,
  triage,
  type Block,
  type Diff,
  type Item,
  type Side,
} from "./compare.ts";

const item = (type: string, props: Record<string, string>, label = type): Item => ({
  type,
  label,
  props,
});

const block = (kind: string, text: string, o: Partial<Block> = {}): Block => ({
  kind,
  key: `${kind}:${text}`,
  top: 0,
  bottom: 20,
  props: { height: "20px" },
  items: [],
  locked: null,
  ...o,
});

const side = (blocks: Block[], column = { left: "300px", width: "600px" }): Side => ({
  column,
  blocks,
});

const ctx = { doc: "README.md", theme: "light" as const };

describe("same", () => {
  it("allows 1 px on lengths and nothing on colors or words", () => {
    expect(same("24px", "24.9px")).toBe(true);
    expect(same("24px", "25.5px")).toBe(false);
    expect(same("8px 14px", "8.5px 14px")).toBe(true);
    expect(same("rgb(10, 20, 30)", "rgb(10, 20, 31)")).toBe(false);
    expect(same("600", "400")).toBe(false);
    expect(same("100px", "102px", 2)).toBe(true);
  });
});

describe("align", () => {
  it("pairs blocks by kind and text, leaving what Edit locks unpaired", () => {
    const view = [block("h1", "a"), block("p", "b"), block("alert", "c"), block("p", "d")];
    const edit = [
      block("locked", "fm"),
      block("h1", "a"),
      block("p", "b"),
      block("locked", "x"),
      block("p", "d"),
    ];
    expect(align(view, edit)).toEqual([
      [0, 1],
      [1, 2],
      [3, 4],
    ]);
  });
});

describe("compareSides", () => {
  it("reports a taller list item and nothing for equal styles", () => {
    const li = (h: string) => item("li.task.tight", { height: h, color: "rgb(0, 0, 0)" });
    const view = side([block("ul", "x", { items: [li("28px"), li("28px")] })]);
    const edit = side([block("ul", "x", { items: [li("44.5px"), li("28.4px")] })]);
    expect(compareSides(ctx, view, edit)).toEqual([
      {
        ...ctx,
        element: "li.task.tight",
        index: 1,
        property: "height",
        view: "28px",
        edit: "44.5px",
      },
    ]);
  });

  it("reports locked blocks once each by kind, and unexplained unpaired blocks", () => {
    const lockedFm = block("locked", "fm", {
      locked: { kind: "frontMatter", reason: "frontMatter" },
    });
    const view = side([block("h1", "a"), block("p", "only in view")]);
    const edit = side([lockedFm, block("h1", "a")]);
    const diffs = compareSides(ctx, view, edit);
    expect(diffs.map((d) => `${d.element} ${d.property} ${d.view} ${d.edit}`)).toEqual([
      "locked(frontMatter) reason - frontMatter",
      "block(p) only-in yes no",
    ]);
  });

  it("measures the gap between consecutive paired blocks and counts missing elements", () => {
    const view = side([
      block("code", "a", { top: 0, bottom: 100, items: [item("code-label", { color: "x" })] }),
      block("code", "b", { top: 116, bottom: 200 }),
    ]);
    const edit = side([
      block("code", "a", { top: 0, bottom: 100 }),
      block("code", "b", { top: 100, bottom: 184 }),
    ]);
    const rows = compareSides(ctx, view, edit).map((d) => `${d.element}#${d.index} ${d.property}`);
    expect(rows).toEqual(["block(code)#1 gap-after", "code-label#1 count"]);
  });

  it("compares the content column", () => {
    const view = side([], { left: "300px", width: "600px" });
    const edit = side([], { left: "300px", width: "640px" });
    expect(compareSides(ctx, view, edit).map((d) => d.property)).toEqual(["width"]);
  });
});

describe("entryDiffs", () => {
  it("flags a selected node, a scroll jump and a wrapped toolbar", () => {
    const rows = entryDiffs(ctx, { viewScroll: 0, editScroll: 102, selected: 1, toolbarRows: 2 });
    expect(rows.map((d) => d.property)).toEqual(["selected-node", "scroll-top", "toolbar-rows"]);
    expect(
      entryDiffs(ctx, { viewScroll: 10, editScroll: 11.5, selected: 0, toolbarRows: 1 }),
    ).toEqual([]);
  });
});

describe("known gaps", () => {
  const diff = (element: string, property: string): Diff => ({
    ...ctx,
    element,
    index: 1,
    property,
    view: "a",
    edit: "b",
  });

  it("covers matching diffs and names the entries that no longer occur", () => {
    const gaps = parseKnownGaps({
      gaps: [
        { area: "locked-blocks", element: "locked(*)", why: "w" },
        { area: "code-colors", element: "pre", property: "token-colors", why: "w" },
      ],
    });
    const result = triage([diff("locked(alert)", "reason"), diff("li.task.tight", "height")], gaps);
    expect(result.uncovered.map((d) => d.element)).toEqual(["li.task.tight"]);
    expect(result.covered).toHaveLength(1);
    expect(result.stale.map((g) => g.area)).toEqual(["code-colors"]);
  });

  it("refuses an entry without a known area", () => {
    expect(() => parseKnownGaps({ gaps: [{ area: "B", element: "pre", why: "w" }] })).toThrow();
  });

  it("the shipped list parses and holds no gap for the styles Edit already matches", () => {
    const here = path.dirname(fileURLToPath(import.meta.url));
    const gaps = parseKnownGaps(
      JSON.parse(readFileSync(path.join(here, "known-gaps.json"), "utf8")),
    );
    const styleGaps = [
      diff("li.bullet.tight", "height"),
      diff("tr", "height"),
      diff("block(table)", "frame-border"),
      diff("code-label", "count"),
      diff("block(code)", "gap-after"),
      diff("entry", "selected-node"),
      diff("entry", "scroll-top"),
      diff("entry", "toolbar-rows"),
    ];
    expect(triage(styleGaps, gaps).covered).toEqual([]);
  });
});

describe("groupDiffs", () => {
  it("folds themes and indexes of the same difference into one row", () => {
    const d = (theme: "light" | "dark", index: number, view: string, edit: string): Diff => ({
      doc: "README.md",
      theme,
      element: "li.task.tight",
      index,
      property: "height",
      view,
      edit,
    });
    const groups = groupDiffs([
      d("light", 1, "28px", "44.5px"),
      d("light", 2, "30px", "46.5px"),
      d("dark", 1, "28px", "44.5px"),
    ]);
    expect(groups).toHaveLength(1);
    expect(groups[0]).toMatchObject({ themes: ["light", "dark"], indexes: [1, 2], count: 3 });
  });
});
